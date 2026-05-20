//! PDF upload + async-job endpoints.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::body::Body;
use axum::extract::{Multipart, Path, State};
use axum::http::{StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::Response;
use futures_util::stream::{self, Stream, StreamExt};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;
use tokio_stream::wrappers::BroadcastStream;

use crate::error::ApiError;
use crate::jobs::{JobEvent, JobState, PdfJob, process_job};
use crate::state::AppState;
use crate::types::ApiValidationResult;

#[derive(Debug, Serialize)]
pub struct UploadResponse {
    pub job_id: String,
    pub filename: String,
}

pub async fn upload(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Json<UploadResponse>, ApiError> {
    let mut received: Option<(String, PdfJob)> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::BadRequest(format!("multipart parse: {e}")))?
    {
        // Accept either field name; first non-empty file wins.
        let filename = field
            .file_name()
            .unwrap_or("upload.pdf")
            .to_string();
        let bytes = field
            .bytes()
            .await
            .map_err(|e| ApiError::BadRequest(format!("read upload bytes: {e}")))?;
        if bytes.is_empty() {
            continue;
        }

        let tmp = tempfile::tempdir().map_err(|e| ApiError::Other(e.into()))?;
        let pdf_path = tmp.path().join("input.pdf");
        let mut f = tokio::fs::File::create(&pdf_path)
            .await
            .map_err(|e| ApiError::Other(e.into()))?;
        f.write_all(&bytes)
            .await
            .map_err(|e| ApiError::Other(e.into()))?;
        f.sync_all()
            .await
            .map_err(|e| ApiError::Other(e.into()))?;

        let job = PdfJob::new(filename.clone(), tmp, pdf_path);
        received = Some((filename, job));
        break;
    }

    let (filename, job) = received.ok_or_else(|| {
        ApiError::BadRequest("no file field in multipart upload".into())
    })?;

    let job = Arc::new(job);
    let job_id = job.id.clone();
    state.jobs.insert(job_id.clone(), job.clone());

    // Spawn the pipeline.
    let st = state.clone();
    let job_for_task = job.clone();
    tokio::spawn(async move {
        process_job(st, job_for_task).await;
    });

    Ok(Json(UploadResponse {
        job_id,
        filename,
    }))
}

pub async fn status(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let job = state
        .jobs
        .get(&job_id)
        .ok_or(ApiError::NotFound)?
        .clone();
    let st = job.state.lock().unwrap().clone();
    Ok(Json(status_to_json(&job, &st)))
}

pub async fn stream(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let job = state
        .jobs
        .get(&job_id)
        .ok_or(ApiError::NotFound)?
        .clone();

    // Send a snapshot first so late subscribers see the current state.
    let snapshot = {
        let st = job.state.lock().unwrap().clone();
        status_to_json(&job, &st)
    };
    let snapshot_event = Event::default()
        .event("snapshot")
        .json_data(snapshot)
        .unwrap();

    let rx = job.events.subscribe();
    let live = BroadcastStream::new(rx).filter_map(|res| async move {
        let ev = res.ok()?;
        let event_name = match &ev {
            JobEvent::Extracted { .. } => "extracted",
            JobEvent::ReferenceComplete { .. } => "reference_complete",
            JobEvent::Done => "done",
            JobEvent::Failed { .. } => "failed",
        };
        Some(Ok(Event::default()
            .event(event_name)
            .json_data(ev)
            .unwrap()))
    });

    let combined = stream::iter(vec![Ok(snapshot_event)]).chain(live);
    Ok(Sse::new(combined).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

pub async fn serve_pdf(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
) -> Result<Response, ApiError> {
    let job = state
        .jobs
        .get(&job_id)
        .ok_or(ApiError::NotFound)?
        .clone();
    let bytes = tokio::fs::read(&job.pdf_path)
        .await
        .map_err(|e| ApiError::Other(e.into()))?;
    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/pdf")
        .header(
            header::CONTENT_DISPOSITION,
            format!("inline; filename=\"{}\"", sanitize(&job.original_filename)),
        )
        .body(Body::from(bytes))
        .map_err(|e| ApiError::Other(anyhow::anyhow!(e)))?;
    Ok(response)
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '_' })
        .collect()
}

fn status_to_json(job: &PdfJob, state: &JobState) -> Value {
    fn refs_json(refs: &[ApiValidationResult]) -> Value {
        serde_json::to_value(refs).unwrap_or_else(|_| Value::Null)
    }
    match state {
        JobState::Pending => json!({
            "job_id": job.id,
            "filename": job.original_filename,
            "state": "pending",
        }),
        JobState::Running {
            page_count,
            references,
            completed,
        } => json!({
            "job_id": job.id,
            "filename": job.original_filename,
            "state": "running",
            "page_count": page_count,
            "completed": completed,
            "total": references.len(),
            "references": refs_json(references),
        }),
        JobState::Done {
            page_count,
            references,
        } => json!({
            "job_id": job.id,
            "filename": job.original_filename,
            "state": "done",
            "page_count": page_count,
            "total": references.len(),
            "references": refs_json(references),
        }),
        JobState::Failed(err) => json!({
            "job_id": job.id,
            "filename": job.original_filename,
            "state": "failed",
            "error": err,
        }),
    }
}
