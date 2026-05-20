//! PDF validation job state and the background pipeline that fills it.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use hallucinator_core::ProgressEvent;
use hallucinator_core::pool::RefJob;
use hallucinator_parsing::ReferenceExtractor;
use hallucinator_pdf_mupdf::MupdfBackend;
use serde::Serialize;
use tempfile::TempDir;
use tokio::sync::broadcast;
use tokio::sync::oneshot;

use crate::state::AppState;
use crate::types::ApiValidationResult;

const EVENT_CHANNEL_CAPACITY: usize = 256;

/// A single PDF validation job. Owned by AppState.jobs; cloned via Arc.
/// `_tmp_dir` keeps the upload directory alive for the lifetime of the job.
pub struct PdfJob {
    pub id: String,
    pub original_filename: String,
    pub pdf_path: PathBuf,
    pub state: Mutex<JobState>,
    pub events: broadcast::Sender<JobEvent>,
    _tmp_dir: TempDir,
}

#[derive(Clone)]
pub enum JobState {
    Pending,
    Running {
        page_count: usize,
        references: Vec<ApiValidationResult>,
        completed: usize,
    },
    Done {
        page_count: usize,
        references: Vec<ApiValidationResult>,
    },
    Failed(String),
}

/// Events broadcast to SSE subscribers as the pipeline progresses.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JobEvent {
    /// Initial reference list extracted from the PDF (no validation yet).
    /// Each entry has page_number + bboxes populated; status/source/report are empty.
    Extracted {
        page_count: usize,
        references: Vec<ApiValidationResult>,
    },
    /// A single reference finished validation; replaces the stub at this index.
    ReferenceComplete {
        index: usize,
        result: ApiValidationResult,
    },
    Done,
    Failed {
        error: String,
    },
}

impl PdfJob {
    pub fn new(original_filename: String, tmp_dir: TempDir, pdf_path: PathBuf) -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        let (events, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        Self {
            id,
            original_filename,
            pdf_path,
            state: Mutex::new(JobState::Pending),
            events,
            _tmp_dir: tmp_dir,
        }
    }
}

/// Run the full pipeline: extract references → locate in PDF → validate
/// each via the shared pool → publish events along the way.
pub async fn process_job(state: Arc<AppState>, job: Arc<PdfJob>) {
    let extract = tokio::task::spawn_blocking({
        let path = job.pdf_path.clone();
        move || {
            let extractor = ReferenceExtractor::new();
            let backend = MupdfBackend;
            extractor
                .extract_references_with_locations(&path, &backend)
                .map_err(|e| e.to_string())
        }
    })
    .await;

    let extraction = match extract {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => {
            fail_job(&job, e);
            return;
        }
        Err(e) => {
            fail_job(&job, format!("extraction task panicked: {e}"));
            return;
        }
    };

    // Build stubs — one ApiValidationResult per reference, with no DB result yet.
    let stubs: Vec<ApiValidationResult> = extraction
        .references
        .iter()
        .map(|r| ApiValidationResult {
            title: r.title.clone().unwrap_or_default(),
            raw_citation: r.raw_citation.clone(),
            ref_authors: r.authors.clone(),
            status: "pending".into(),
            mismatch_kinds: vec![],
            source: None,
            found_authors: vec![],
            paper_url: None,
            failed_dbs: vec![],
            doi: r.doi.clone(),
            arxiv_id: r.arxiv_id.clone(),
            retraction: None,
            report: None,
            page_number: r.page_number,
            bboxes: r.bboxes.clone(),
        })
        .collect();

    // Heuristic page count: highest page_number we saw + 1, or 0.
    let page_count = extraction
        .references
        .iter()
        .filter_map(|r| r.page_number)
        .max()
        .map(|p| p + 1)
        .unwrap_or(0);

    *job.state.lock().unwrap() = JobState::Running {
        page_count,
        references: stubs.clone(),
        completed: 0,
    };
    let _ = job.events.send(JobEvent::Extracted {
        page_count,
        references: stubs.clone(),
    });

    // Fan out validation. Submit all to the pool; collect results as
    // each oneshot fires.
    let refs: Vec<_> = extraction.references.into_iter().enumerate().collect();
    let mut handles = Vec::with_capacity(refs.len());
    let total = refs.len();
    let progress: Arc<dyn Fn(ProgressEvent) + Send + Sync> = Arc::new(|_| {});
    for (idx, reference) in refs {
        let (tx, rx) = oneshot::channel();
        // Capture the page/bbox before consuming the Reference.
        let page = reference.page_number;
        let bboxes = reference.bboxes.clone();
        state
            .pool
            .submit(RefJob {
                reference,
                result_tx: tx,
                ref_index: idx,
                total,
                progress: progress.clone(),
            })
            .await;
        handles.push((idx, page, bboxes, rx));
    }

    for (idx, page, bboxes, rx) in handles {
        let core = match rx.await {
            Ok(r) => r,
            Err(_) => continue,
        };
        let api_result = ApiValidationResult::from_core(core, page, bboxes);

        // Update state
        let mut snapshot_for_done: Option<Vec<ApiValidationResult>> = None;
        {
            let mut st = job.state.lock().unwrap();
            if let JobState::Running {
                references,
                completed,
                ..
            } = &mut *st
            {
                if idx < references.len() {
                    references[idx] = api_result.clone();
                }
                *completed += 1;
                if *completed >= total {
                    snapshot_for_done = Some(references.clone());
                }
            }
        }

        let _ = job.events.send(JobEvent::ReferenceComplete {
            index: idx,
            result: api_result,
        });

        if let Some(final_refs) = snapshot_for_done {
            *job.state.lock().unwrap() = JobState::Done {
                page_count,
                references: final_refs,
            };
            let _ = job.events.send(JobEvent::Done);
        }
    }
}

fn fail_job(job: &PdfJob, error: String) {
    *job.state.lock().unwrap() = JobState::Failed(error.clone());
    let _ = job.events.send(JobEvent::Failed { error });
}
