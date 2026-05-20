use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::routing::{get, post};
use tokio_util::sync::CancellationToken;
use tower_http::cors::CorsLayer;
use tracing::info;

mod error;
mod jobs;
mod routes;
mod state;
mod types;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "hallucinator_api=info,tower_http=info".into()))
        .init();

    let cancel = CancellationToken::new();
    let state = Arc::new(state::AppState::new(cancel.clone()).await?);

    let app = Router::new()
        .route("/health", get(routes::health::health))
        .route("/api/validate-reference", post(routes::reference::validate_one))
        .route("/api/validate-references", post(routes::reference::validate_many))
        .route("/api/validate-pdf", post(routes::pdf::upload))
        .route("/api/validate-pdf/{job_id}", get(routes::pdf::status))
        .route("/api/validate-pdf/{job_id}/stream", get(routes::pdf::stream))
        .route("/api/pdf/{job_id}", get(routes::pdf::serve_pdf))
        .layer(CorsLayer::very_permissive())
        .with_state(state);

    let addr: SocketAddr = std::env::var("API_BIND")
        .unwrap_or_else(|_| "127.0.0.1:8787".into())
        .parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!(%addr, "hallucinator-api listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            cancel.cancel();
        })
        .await?;
    Ok(())
}
