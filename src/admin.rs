use std::{
    collections::{HashMap, HashSet},
    sync::Mutex,
    time::{Duration, Instant},
};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::time::sleep;

use crate::{db, db::NewHostedFile, entity::Token, geo, mrs, server::AppState};

const SESSION_COOKIE: &str = "admin_session";
const SESSION_TTL: Duration = Duration::from_secs(8 * 60 * 60);

/// Download timeout for mrs source fetching.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30);

/// Download timeout for geo database fetching; they are far larger.
const GEO_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);

/// Admin authentication state. The admin page is disabled unless the
/// `ADMIN_PASSWORD` environment variable is set to a non-empty value.
pub struct AdminState {
    password: Option<String>,
    sessions: Mutex<HashMap<String, Instant>>,
}

impl AdminState {
    pub fn from_env() -> Self {
        let password = std::env::var("ADMIN_PASSWORD")
            .ok()
            .filter(|password| !password.is_empty());
        Self {
            password,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    pub fn enabled(&self) -> bool {
        self.password.is_some()
    }
}

pub fn routes() -> Router<AppState> {
    // The admin UI is embedded only in release builds; debug builds expose
    // the API alone so the frontend is served by the Vite dev server.
    let router = Router::new()
        .route("/admin/api/login", post(login))
        .route("/admin/api/logout", post(logout))
        .route("/admin/api/tokens", get(list_tokens).post(add_token))
        .route("/admin/api/tokens/{id}/enable", post(enable_token))
        .route("/admin/api/tokens/{id}/disable", post(disable_token))
        .route("/admin/api/tokens/{id}/config", post(set_token_config))
        .route("/admin/api/tokens/{id}/name", post(set_token_name))
        .route(
            "/admin/api/tokens/{id}/convert-mrs",
            post(convert_token_mrs),
        )
        .route(
            "/admin/api/tokens/{id}/convert-geo",
            post(convert_token_geo),
        )
        .route("/admin/api/tokens/{id}/duplicate", post(duplicate_token))
        .route("/admin/api/tokens/{id}", delete(remove_token));

    #[cfg(not(debug_assertions))]
    let router = router
        .route("/admin", get(release_page))
        .route("/admin/", get(release_page))
        .route("/admin/assets/{*path}", get(crate::assets::asset));

    #[cfg(debug_assertions)]
    let router = router
        .route("/admin", get(debug_page))
        .route("/admin/", get(debug_page));

    router
}

#[cfg(not(debug_assertions))]
async fn release_page(State(state): State<AppState>) -> Response {
    if !state.admin.enabled() {
        return disabled_page().into_response();
    }
    crate::assets::page().await
}

#[cfg(debug_assertions)]
async fn debug_page(State(state): State<AppState>) -> Response {
    if !state.admin.enabled() {
        return disabled_page().into_response();
    }
    (
        StatusCode::OK,
        "admin UI is not served in debug builds; run `npm run dev` in frontend/, \
         or use a release build",
    )
        .into_response()
}

fn disabled_page() -> (StatusCode, &'static str) {
    (
        StatusCode::NOT_FOUND,
        "admin page is disabled; set ADMIN_PASSWORD to enable",
    )
}

#[derive(Deserialize)]
struct LoginPayload {
    password: String,
}

async fn login(State(state): State<AppState>, Json(payload): Json<LoginPayload>) -> Response {
    let Some(password) = state.admin.password.as_ref() else {
        return (StatusCode::NOT_FOUND, "admin page is disabled").into_response();
    };

    if !constant_time_eq(payload.password.as_bytes(), password.as_bytes()) {
        // Slow down brute-force attempts.
        sleep(Duration::from_millis(500)).await;
        return (StatusCode::UNAUTHORIZED, "wrong password").into_response();
    }

    let session = uuid::Uuid::new_v4().simple().to_string();
    if let Ok(mut sessions) = state.admin.sessions.lock() {
        sessions.insert(session.clone(), Instant::now() + SESSION_TTL);
    }

    let mut response = Json(json!({ "ok": true })).into_response();
    set_cookie(
        &mut response,
        &format!(
            "{SESSION_COOKIE}={session}; HttpOnly; SameSite=Strict; Path=/; Max-Age={}",
            SESSION_TTL.as_secs()
        ),
    );
    response
}

async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(session) = session_id_from_headers(&headers)
        && let Ok(mut sessions) = state.admin.sessions.lock()
    {
        sessions.remove(&session);
    }

    let mut response = Json(json!({ "ok": true })).into_response();
    set_cookie(
        &mut response,
        &format!("{SESSION_COOKIE}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0"),
    );
    response
}

#[derive(Serialize)]
struct TokenJson {
    id: i32,
    token: String,
    name: String,
    config: String,
    status: &'static str,
    expires_at: String,
    last_used_at: String,
    created_at: String,
}

impl TokenJson {
    fn from_record(record: &Token) -> Self {
        Self {
            id: record.id,
            token: record.token.clone(),
            name: record.name.clone(),
            config: record.config.clone(),
            status: db::token_status(record, db::now()),
            expires_at: db::fmt_datetime(record.expires_at, "never"),
            last_used_at: db::fmt_datetime(record.last_used_at, "-"),
            created_at: db::fmt_datetime(Some(record.created_at), "-"),
        }
    }
}

async fn list_tokens(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = guard(&state, &headers) {
        return response;
    }

    match state.db.list().await {
        Ok(records) => {
            let tokens: Vec<TokenJson> = records.iter().map(TokenJson::from_record).collect();
            Json(tokens).into_response()
        }
        Err(err) => internal(err),
    }
}

#[derive(Deserialize)]
struct AddTokenPayload {
    name: Option<String>,
    days: Option<i64>,
    config: Option<String>,
}

/// Create a token; the token value is generated server-side and returned.
async fn add_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<AddTokenPayload>,
) -> Response {
    if let Err(response) = guard(&state, &headers) {
        return response;
    }

    if payload.days.is_some_and(|days| days < 0) {
        return (StatusCode::BAD_REQUEST, "days must not be negative").into_response();
    }

    let name = payload.name.unwrap_or_default().trim().to_owned();
    let config = payload.config.unwrap_or_default();
    if let Err(err) = db::validate_config(config.trim()) {
        return (StatusCode::BAD_REQUEST, err.to_string()).into_response();
    }
    match state.db.add(&name, payload.days, &config).await {
        Ok(record) => (StatusCode::CREATED, Json(TokenJson::from_record(&record))).into_response(),
        Err(err) => internal(err),
    }
}

#[derive(Deserialize)]
struct SetConfigPayload {
    config: String,
}

async fn set_token_config(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i32>,
    Json(payload): Json<SetConfigPayload>,
) -> Response {
    if let Err(response) = guard(&state, &headers) {
        return response;
    }

    let config = payload.config.trim();
    if let Err(err) = db::validate_config(config) {
        return (StatusCode::BAD_REQUEST, err.to_string()).into_response();
    }
    match state.db.set_config(id, config).await {
        Ok(0) => (StatusCode::NOT_FOUND, "token not found").into_response(),
        Ok(_) => (StatusCode::OK, "ok").into_response(),
        Err(err) => internal(err),
    }
}

#[derive(Deserialize)]
struct SetNamePayload {
    name: String,
}

async fn set_token_name(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i32>,
    Json(payload): Json<SetNamePayload>,
) -> Response {
    if let Err(response) = guard(&state, &headers) {
        return response;
    }

    match state.db.set_name(id, payload.name.trim()).await {
        Ok(0) => (StatusCode::NOT_FOUND, "token not found").into_response(),
        Ok(_) => (StatusCode::OK, "ok").into_response(),
        Err(err) => internal(err),
    }
}

async fn enable_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i32>,
) -> Response {
    update_enabled(state, headers, id, true).await
}

#[derive(Deserialize)]
struct ConvertMrsPayload {
    base_url: String,
}

#[derive(Serialize)]
struct MrsProviderResult {
    name: String,
    behavior: String,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Serialize)]
struct MrsConvertResponse {
    results: Vec<MrsProviderResult>,
    config: String,
}

/// Convert the token's `rule-providers` to hosted mrs files and return the
/// rewritten config for review. The token's own config is left untouched.
async fn convert_token_mrs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i32>,
    Json(payload): Json<ConvertMrsPayload>,
) -> Response {
    if let Err(response) = guard(&state, &headers) {
        return response;
    }

    let base_url = match normalize_base_url(&payload.base_url) {
        Ok(base_url) => base_url,
        Err(message) => return (StatusCode::BAD_REQUEST, message).into_response(),
    };

    let Some(record) = (match state.db.get(id).await {
        Ok(record) => record,
        Err(err) => return internal(err),
    }) else {
        return (StatusCode::NOT_FOUND, "token not found").into_response();
    };
    if record.config.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "the token has no config").into_response();
    }
    let providers = match mrs::parse_rule_providers(&record.config) {
        Ok(providers) => providers,
        Err(err) => return (StatusCode::BAD_REQUEST, err.to_string()).into_response(),
    };
    if providers.is_empty() {
        return (StatusCode::BAD_REQUEST, "the config has no rule-providers").into_response();
    }

    let client = match reqwest::Client::builder()
        .timeout(DOWNLOAD_TIMEOUT)
        .user_agent(concat!("proxy-converter/", env!("CARGO_PKG_VERSION")))
        .build()
    {
        Ok(client) => client,
        Err(err) => return internal(anyhow::anyhow!("failed to build the HTTP client: {err}")),
    };

    let mut results = Vec::with_capacity(providers.len());
    let mut converted = HashSet::new();
    let mut files = Vec::new();
    // Hosted geo files must survive mrs sync: the two conversions share one
    // file store per token.
    let mut keep = geo::geo_names();
    keep.extend(providers.iter().map(|p| p.name.clone()));
    for provider in providers {
        let outcome = mrs::convert_provider(&client, &record.token, &base_url, &provider).await;
        let (status, size, url, reason, error) = match &outcome {
            mrs::ProviderOutcome::Converted { size, url, .. } => {
                ("converted", Some(*size), Some(url.clone()), None, None)
            }
            mrs::ProviderOutcome::Skipped { reason } => {
                ("skipped", None, None, Some(*reason), None)
            }
            mrs::ProviderOutcome::Failed { error } => {
                ("failed", None, None, None, Some(error.to_string()))
            }
        };
        // A provider that was not re-converted keeps its previously hosted
        // file, so its download link stays valid; surface that link.
        let (status, size, url) = if matches!(outcome, mrs::ProviderOutcome::Converted { .. }) {
            (status, size, url)
        } else if let Ok(Some(existing)) = state.db.get_hosted_file(id, &provider.name).await {
            (
                status,
                Some(existing.content.len()),
                Some(mrs::download_url(&base_url, &record.token, &provider.name)),
            )
        } else {
            (status, size, url)
        };
        if let mrs::ProviderOutcome::Converted { content, .. } = outcome {
            files.push(NewHostedFile {
                name: provider.name.clone(),
                source_url: provider.url.clone(),
                content,
            });
            converted.insert(provider.name.clone());
        }
        results.push(MrsProviderResult {
            name: provider.name,
            behavior: provider.behavior,
            status,
            size,
            url,
            reason,
            error,
        });
    }

    if let Err(err) = state.db.set_hosted_files(id, files, &keep).await {
        return internal(err);
    }
    let config = match mrs::rewrite_config(&record.config, &base_url, &record.token, &converted) {
        Ok(config) => config,
        Err(err) => return internal(err),
    };

    Json(MrsConvertResponse { results, config }).into_response()
}

#[derive(Deserialize)]
struct ConvertGeoPayload {
    base_url: String,
}

#[derive(Serialize)]
struct GeoFileResult {
    name: String,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Serialize)]
struct GeoConvertResponse {
    results: Vec<GeoFileResult>,
    config: String,
}

/// Download the token's geo databases (geox-url, falling back to mihomo's
/// built-in sources) and host them as-is. The token's own config is left
/// untouched; the rewritten config is returned for review.
async fn convert_token_geo(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i32>,
    Json(payload): Json<ConvertGeoPayload>,
) -> Response {
    if let Err(response) = guard(&state, &headers) {
        return response;
    }

    let base_url = match normalize_base_url(&payload.base_url) {
        Ok(base_url) => base_url,
        Err(message) => return (StatusCode::BAD_REQUEST, message).into_response(),
    };

    let Some(record) = (match state.db.get(id).await {
        Ok(record) => record,
        Err(err) => return internal(err),
    }) else {
        return (StatusCode::NOT_FOUND, "token not found").into_response();
    };
    if record.config.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "the token has no config").into_response();
    }
    let sources = match geo::resolve_sources(&record.config) {
        Ok(sources) => sources,
        Err(err) => return (StatusCode::BAD_REQUEST, err.to_string()).into_response(),
    };

    // Geo databases are much larger than rule lists, so allow more time.
    let client = match reqwest::Client::builder()
        .timeout(GEO_DOWNLOAD_TIMEOUT)
        .user_agent(concat!("proxy-converter/", env!("CARGO_PKG_VERSION")))
        .build()
    {
        Ok(client) => client,
        Err(err) => return internal(anyhow::anyhow!("failed to build the HTTP client: {err}")),
    };

    let mut results = Vec::with_capacity(sources.len());
    let mut converted = HashSet::new();
    let mut files = Vec::new();
    for (name, source_url) in &sources {
        let outcome = mrs::download(&client, source_url, geo::MAX_GEO_SOURCE_BYTES).await;
        let (status, mut size, error) = match &outcome {
            Ok(content) => ("converted", Some(content.len()), None),
            Err(error) => ("failed", None, Some(error.to_string())),
        };
        if let Ok(content) = outcome {
            files.push(NewHostedFile {
                name: (*name).to_owned(),
                source_url: source_url.clone(),
                content,
            });
            converted.insert((*name).to_owned());
        } else if let Ok(Some(existing)) = state.db.get_hosted_file(id, name).await {
            // The stale file is still hosted and still served; surface it.
            size = Some(existing.content.len());
        }
        results.push(GeoFileResult {
            name: (*name).to_owned(),
            status,
            size,
            url: (status == "converted" || size.is_some())
                .then(|| geo::download_url(&base_url, &record.token, name)),
            error,
        });
    }

    // Failed keys keep their previously hosted file (upsert only touches
    // what converted), so only the config rewrite depends on this run.
    if let Err(err) = state.db.upsert_hosted_files(id, files).await {
        return internal(err);
    }
    let config = match geo::rewrite_config(&record.config, &base_url, &record.token, &converted) {
        Ok(config) => config,
        Err(err) => return internal(err),
    };

    Json(GeoConvertResponse { results, config }).into_response()
}

fn normalize_base_url(raw: &str) -> Result<String, &'static str> {
    let url = reqwest::Url::parse(raw.trim()).map_err(|_| "base_url is not a valid URL")?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err("base_url must be an http or https URL");
    }
    if url.host_str().is_none() || url.host_str() == Some("") {
        return Err("base_url must include a host");
    }
    Ok(url.origin().ascii_serialization())
}

async fn disable_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i32>,
) -> Response {
    update_enabled(state, headers, id, false).await
}

async fn update_enabled(state: AppState, headers: HeaderMap, id: i32, enabled: bool) -> Response {
    if let Err(response) = guard(&state, &headers) {
        return response;
    }

    match state.db.set_enabled(id, enabled).await {
        Ok(0) => (StatusCode::NOT_FOUND, "token not found").into_response(),
        Ok(_) => (StatusCode::OK, "ok").into_response(),
        Err(err) => internal(err),
    }
}

async fn remove_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i32>,
) -> Response {
    if let Err(response) = guard(&state, &headers) {
        return response;
    }

    match state.db.remove(id).await {
        Ok(0) => (StatusCode::NOT_FOUND, "token not found").into_response(),
        Ok(_) => (StatusCode::OK, "ok").into_response(),
        Err(err) => internal(err),
    }
}

/// Duplicate a token with its config and hosted files; the config's hosted
/// download links are re-pointed at the new token value.
async fn duplicate_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i32>,
) -> Response {
    if let Err(response) = guard(&state, &headers) {
        return response;
    }

    match state.db.duplicate(id).await {
        Ok(Some(record)) => {
            (StatusCode::CREATED, Json(TokenJson::from_record(&record))).into_response()
        }
        Ok(None) => (StatusCode::NOT_FOUND, "token not found").into_response(),
        Err(err) => internal(err),
    }
}

/// Reject disabled admin pages and unauthenticated requests.
#[allow(clippy::result_large_err)] // a boxed error type would not simplify the callers
fn guard(state: &AppState, headers: &HeaderMap) -> Result<(), Response> {
    if !state.admin.enabled() {
        return Err((StatusCode::NOT_FOUND, "admin page is disabled").into_response());
    }
    if !session_valid(&state.admin, headers) {
        return Err((StatusCode::UNAUTHORIZED, "unauthorized").into_response());
    }
    Ok(())
}

fn session_valid(admin: &AdminState, headers: &HeaderMap) -> bool {
    let Some(session) = session_id_from_headers(headers) else {
        return false;
    };
    let Ok(mut sessions) = admin.sessions.lock() else {
        return false;
    };
    let now = Instant::now();
    sessions.retain(|_, expires| *expires > now);
    sessions.contains_key(&session)
}

fn session_id_from_headers(headers: &HeaderMap) -> Option<String> {
    let cookie = headers.get(header::COOKIE)?.to_str().ok()?;
    cookie.split(';').find_map(|part| {
        let part = part.trim();
        part.strip_prefix(SESSION_COOKIE)
            .and_then(|rest| rest.strip_prefix('='))
            .map(str::to_owned)
    })
}

fn set_cookie(response: &mut Response, cookie: &str) {
    if let Ok(value) = HeaderValue::from_str(cookie) {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |diff, (x, y)| diff | (x ^ y)) == 0
}

fn internal(err: anyhow::Error) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("internal error: {err}"),
    )
        .into_response()
}
