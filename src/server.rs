use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use anyhow::{Context as _, Result};
use axum::{
    Router,
    extract::{Path, Query, State},
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
        .route("/files/{name}", get(file))
        .merge(crate::admin::routes())
        .with_state(state);

    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;
    info!("Server is running at {addr}");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
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

#[derive(Deserialize)]
struct FileParams {
    key: Option<String>,
}

/// Serve one of the token's hosted files, e.g. `/files/google.mrs` or
/// `/files/geoip`. The file set is private per token: names never collide
/// across tokens.
async fn file(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Query(params): Query<FileParams>,
) -> Result<Response, AppError> {
    let provided = params.key.as_deref().unwrap_or_default();
    let Some(record) = state.db.verify_file_key(provided).await else {
        return Err(AppError::new(StatusCode::UNAUTHORIZED, "Unauthorized"));
    };

    let lookup_name = name.strip_suffix(".mrs").unwrap_or(&name);
    let file = state
        .db
        .get_hosted_file(record.id, lookup_name)
        .await
        .map_err(|err| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?
        .ok_or_else(|| AppError::new(StatusCode::NOT_FOUND, "no such file"))?;

    let disposition = format!("attachment; filename={name}");
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
