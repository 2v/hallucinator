//! PDF validation job state. Filled in by Phase 2c.

use std::path::PathBuf;
use std::sync::Mutex;

use hallucinator_core::ValidationResult;

/// A single PDF validation job.
pub struct PdfJob {
    pub id: String,
    pub original_filename: String,
    pub pdf_path: PathBuf,
    pub state: Mutex<JobState>,
}

pub enum JobState {
    Pending,
    Running { progress: f32 },
    Done {
        references: Vec<ValidationResult>,
        page_count: usize,
    },
    Failed(String),
}
