use std::{net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Context as _, Result};
use axum::{
    extract::{Query, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use serde::Deserialize;
use tokio::{net::TcpListener, signal};
use tracing::{info, warn};

use crate::{admin::AdminState, db::Db, js::JsTransformer};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) type AppState = Arc<AppStateInner>;

pub(crate) struct AppStateInner {
    pub(crate) http: reqwest::Client,
    pub(crate) db: Db,
    pub(crate) admin: AdminState,
    pub(crate) transformer: Option<JsTransformer>,
}

pub async fn run(addr: SocketAddr, script: Option<PathBuf>, database: PathBuf) -> Result<()> {
    let transformer = match &script {
        Some(path) => Some(JsTransformer::start(path)?),
        None => None,
    };

    let db = Db::open(&database).await?;

    let admin = AdminState::from_env();
    if !admin.enabled() {
        warn!("Admin page is disabled; set ADMIN_PASSWORD to enable it");
    }

    let http = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .user_agent(concat!("proxy-converter/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("failed to build the HTTP client")?;

    let state: AppState = Arc::new(AppStateInner {
        http,
        db,
        admin,
        transformer,
    });

    let app = Router::new()
        .route("/convert", get(convert))
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
struct ConvertParams {
    url: String,
    token: Option<String>,
}

async fn convert(
    State(state): State<AppState>,
    Query(params): Query<ConvertParams>,
) -> Result<Response, AppError> {
    let provided = params.token.as_deref().unwrap_or_default();
    if !state.db.verify(provided).await {
        return Err(AppError::new(StatusCode::UNAUTHORIZED, "Unauthorized"));
    }

    let body = state
        .http
        .get(&params.url)
        .send()
        .await
        .and_then(|response| response.error_for_status())
        .map_err(|err| {
            AppError::new(
                StatusCode::BAD_GATEWAY,
                format!("failed to fetch the upstream config: {err}"),
            )
        })?
        .text()
        .await
        .map_err(|err| {
            AppError::new(
                StatusCode::BAD_GATEWAY,
                format!("failed to read the upstream config: {err}"),
            )
        })?;

    let mut data: serde_json::Value = serde_yaml::from_str(&body)
        .map_err(|err| {
            AppError::new(
                StatusCode::BAD_GATEWAY,
                format!("failed to parse the upstream YAML: {err}"),
            )
        })?;

    if let Some(transformer) = &state.transformer {
        data = transformer
            .transform(data)
            .await
            .map_err(AppError::internal)?;
    }

    let yaml = serde_yaml::to_string(&data)
        .map_err(|err| AppError::internal(format!("failed to serialize the YAML: {err}")))?;
    // serde_yaml emits a document start marker; drop it to match the old output.
    let yaml = yaml.strip_prefix("---\n").unwrap_or(&yaml).to_owned();

    let mut response = Response::new(axum::body::Body::from(yaml));
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=config.yaml"),
    );
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-yaml"),
    );
    Ok(response)
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

    fn internal(message: impl std::fmt::Display) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, message.to_string())
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
