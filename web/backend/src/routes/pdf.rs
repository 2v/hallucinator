//! PDF upload + async-job endpoints. Stubs for now; filled in by Phase 2c.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use serde_json::{Value, json};

use crate::error::ApiError;
use crate::state::AppState;

pub async fn upload(State(_state): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    Err(ApiError::Other(anyhow::anyhow!("not implemented")))
}

pub async fn status(
    State(_state): State<Arc<AppState>>,
    Path(_job_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    Err(ApiError::Other(anyhow::anyhow!("not implemented")))
}

pub async fn stream(
    State(_state): State<Arc<AppState>>,
    Path(_job_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    Err(ApiError::Other(anyhow::anyhow!("not implemented")))
}

pub async fn serve_pdf(
    State(_state): State<Arc<AppState>>,
    Path(_job_id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let resp: Json<Value> = Json(json!({ "error": "not implemented" }));
    Ok(resp)
}
