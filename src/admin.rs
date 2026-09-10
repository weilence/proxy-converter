use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::time::sleep;

use crate::{db, entity::Token, server::AppState};

const SESSION_COOKIE: &str = "admin_session";
const SESSION_TTL: Duration = Duration::from_secs(8 * 60 * 60);

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
    Router::new()
        .route("/admin", get(page))
        .route("/admin/api/login", post(login))
        .route("/admin/api/logout", post(logout))
        .route("/admin/api/tokens", get(list_tokens).post(add_token))
        .route("/admin/api/tokens/{id}/enable", post(enable_token))
        .route("/admin/api/tokens/{id}/disable", post(disable_token))
        .route("/admin/api/tokens/{id}", delete(remove_token))
}

async fn page(State(state): State<AppState>) -> Response {
    if !state.admin.enabled() {
        return (StatusCode::NOT_FOUND, "admin page is disabled; set ADMIN_PASSWORD to enable")
            .into_response();
    }
    Html(include_str!("../frontend/admin.html")).into_response()
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
        &format!("{SESSION_COOKIE}={session}; HttpOnly; SameSite=Strict; Path=/; Max-Age={}", SESSION_TTL.as_secs()),
    );
    response
}

async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(session) = session_id_from_headers(&headers) {
        if let Ok(mut sessions) = state.admin.sessions.lock() {
            sessions.remove(&session);
        }
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
    token: String,
    name: Option<String>,
    days: Option<i64>,
}

async fn add_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<AddTokenPayload>,
) -> Response {
    if let Err(response) = guard(&state, &headers) {
        return response;
    }

    let token = payload.token.trim().to_owned();
    if token.is_empty() {
        return (StatusCode::BAD_REQUEST, "token must not be empty").into_response();
    }
    if payload.days.is_some_and(|days| days < 0) {
        return (StatusCode::BAD_REQUEST, "days must not be negative").into_response();
    }

    let name = payload.name.unwrap_or_default().trim().to_owned();
    match state.db.add(&token, &name, payload.days).await {
        Ok(true) => (StatusCode::CREATED, "added").into_response(),
        Ok(false) => (StatusCode::CONFLICT, "token already exists").into_response(),
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

async fn disable_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<i32>,
) -> Response {
    update_enabled(state, headers, id, false).await
}

async fn update_enabled(
    state: AppState,
    headers: HeaderMap,
    id: i32,
    enabled: bool,
) -> Response {
    if let Err(response) = guard(&state, &headers) {
        return response;
    }

    match state.db.set_enabled_by_id(id, enabled).await {
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

    match state.db.remove_by_id(id).await {
        Ok(0) => (StatusCode::NOT_FOUND, "token not found").into_response(),
        Ok(_) => (StatusCode::OK, "ok").into_response(),
        Err(err) => internal(err),
    }
}

/// Reject disabled admin pages and unauthenticated requests.
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
    cookie
        .split(';')
        .find_map(|part| {
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
    a.iter()
        .zip(b)
        .fold(0u8, |diff, (x, y)| diff | (x ^ y))
        == 0
}

fn internal(err: anyhow::Error) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("internal error: {err}"),
    )
        .into_response()
}
