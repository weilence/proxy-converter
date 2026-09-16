use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use anyhow::{Context as _, Result};
use axum::{
    Router,
    extract::{Path, Query, RawQuery, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;
use tokio::{net::TcpListener, signal};
use tracing::{info, warn};

use crate::{admin::AdminState, db::Db};

pub(crate) type AppState = Arc<AppStateInner>;

pub(crate) struct AppStateInner {
    pub(crate) db: Db,
    pub(crate) admin: AdminState,
}

pub async fn run(addr: SocketAddr, database: PathBuf) -> Result<()> {
    let db = Db::open(&database).await?;

    let admin = AdminState::from_env();
    if !admin.enabled() {
        warn!("Admin page is disabled; set ADMIN_PASSWORD to enable it");
    }

    let state: AppState = Arc::new(AppStateInner { db, admin });

    let app = Router::new()
        .route("/config", get(config))
        .route("/convert", get(convert))
        .route("/mrs/{name}", get(mrs))
        .merge(crate::admin::routes())
        .with_state(state);

    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;
    info!("Server is running at {addr}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server error")?;

    info!("Shutting down...");
    Ok(())
}

#[derive(Deserialize)]
struct ConfigParams {
    token: Option<String>,
}

/// Serve the proxy config bound to the token.
async fn config(
    State(state): State<AppState>,
    Query(params): Query<ConfigParams>,
) -> Result<Response, AppError> {
    let provided = params.token.as_deref().unwrap_or_default();
    let Some(record) = state.db.verify(provided).await else {
        return Err(AppError::new(StatusCode::UNAUTHORIZED, "Unauthorized"));
    };

    // A token without bound content yields empty content.
    Ok(config_response(record.config.trim().to_owned()))
}

/// `/convert` is deprecated; redirect to `/config`, preserving the query.
async fn convert(RawQuery(query): RawQuery) -> Response {
    let location = match query.filter(|query| !query.is_empty()) {
        Some(query) => format!("/config?{query}"),
        None => "/config".to_owned(),
    };
    match HeaderValue::from_str(&location) {
        Ok(location) => (StatusCode::FOUND, [(header::LOCATION, location)]).into_response(),
        Err(_) => (StatusCode::FOUND, "moved to /config").into_response(),
    }
}

#[derive(Deserialize)]
struct MrsParams {
    token: Option<String>,
}

/// Serve one of the token's converted rule files, e.g. `/mrs/google.mrs`.
/// The file set is private per token: names never collide across tokens.
async fn mrs(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Query(params): Query<MrsParams>,
) -> Result<Response, AppError> {
    let provided = params.token.as_deref().unwrap_or_default();
    let Some(record) = state.db.verify(provided).await else {
        return Err(AppError::new(StatusCode::UNAUTHORIZED, "Unauthorized"));
    };

    let file_name = name.strip_suffix(".mrs").unwrap_or(&name);
    let file = state
        .db
        .get_mrs_file(record.id, file_name)
        .await
        .map_err(|err| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
        .ok_or_else(|| AppError::new(StatusCode::NOT_FOUND, "no such mrs file"))?;

    let disposition = format!("attachment; filename={file_name}.mrs");
    let mut response = Response::new(axum::body::Body::from(file.content));
    let headers = response.headers_mut();
    if let Ok(value) = HeaderValue::from_str(&disposition) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    Ok(response)
}

/// Build the downloadable `config.yaml` response; an empty body means the
/// token has no config bound.
fn config_response(body: String) -> Response {
    let mut response = Response::new(axum::body::Body::from(body));
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=config.yaml"),
    );
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-yaml"),
    );
    response
}

struct AppError {
    status: StatusCode,
    message: String,
}

impl AppError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (self.status, self.message).into_response()
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c().await.ok();
    };

    #[cfg(unix)]
    let terminate = async {
        match signal::unix::signal(signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
