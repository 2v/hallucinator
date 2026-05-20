//! Synchronous reference-validation endpoints. Each request submits a
//! single Reference to the shared ValidationPool and awaits the
//! ValidationResult.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use futures_util::future::join_all;
use hallucinator_core::ProgressEvent;
use hallucinator_core::pool::RefJob;
use tokio::sync::oneshot;

use crate::error::ApiError;
use crate::state::AppState;
use crate::types::{ApiValidationResult, ValidateOneRequest, ValidateOneResponse};

pub async fn validate_one(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ValidateOneRequest>,
) -> Result<Json<ValidateOneResponse>, ApiError> {
    let client_ref = req.client_ref.clone();
    let result = run_one(&state, req).await?;
    Ok(Json(ValidateOneResponse {
        client_ref,
        result,
    }))
}

pub async fn validate_many(
    State(state): State<Arc<AppState>>,
    Json(reqs): Json<Vec<ValidateOneRequest>>,
) -> Result<Json<Vec<ValidateOneResponse>>, ApiError> {
    if reqs.is_empty() {
        return Ok(Json(Vec::new()));
    }
    if reqs.len() > 200 {
        return Err(ApiError::BadRequest(
            "batch too large (max 200 references per call)".into(),
        ));
    }
    let mut futs = Vec::with_capacity(reqs.len());
    for req in reqs {
        let st = state.clone();
        futs.push(async move {
            let client_ref = req.client_ref.clone();
            let res = run_one(&st, req).await;
            res.map(|r| ValidateOneResponse {
                client_ref,
                result: r,
            })
        });
    }
    let results: Result<Vec<_>, _> = join_all(futs).await.into_iter().collect();
    Ok(Json(results?))
}

async fn run_one(
    state: &Arc<AppState>,
    req: ValidateOneRequest,
) -> Result<ApiValidationResult, ApiError> {
    let reference = req.into_reference();
    let (tx, rx) = oneshot::channel();
    let progress: Arc<dyn Fn(ProgressEvent) + Send + Sync> = Arc::new(|_| {});
    state
        .pool
        .submit(RefJob {
            reference,
            result_tx: tx,
            ref_index: 0,
            total: 1,
            progress,
        })
        .await;
    let core_result = rx.await.map_err(|_| {
        ApiError::Other(anyhow::anyhow!("pool dropped result before sending"))
    })?;
    Ok(ApiValidationResult::from_core(core_result, None, Vec::new()))
}
