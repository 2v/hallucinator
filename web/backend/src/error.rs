use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("not found")]
    NotFound,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (code, msg) = match &self {
            ApiError::BadRequest(m) => (StatusCode::BAD_REQUEST, m.clone()),
            ApiError::NotFound => (StatusCode::NOT_FOUND, "not found".into()),
            ApiError::Other(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")),
        };
        (code, Json(json!({ "error": msg }))).into_response()
    }
}
