use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum BackendError {
    #[error("failed to open PDF: {0}")]
    OpenError(String),
    #[error("failed to extract text: {0}")]
    ExtractionError(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Axis-aligned bounding box in PDF user-space coordinates.
/// Origin is top-left; units are PDF points (1/72 inch).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct BBox {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

/// Where on a PDF page (or pages) a piece of text was found.
/// `bboxes` is one rect per line span — a multi-line reference yields
/// multiple rects, all on the same page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PdfLocation {
    pub page: usize,
    pub bboxes: Vec<BBox>,
}

/// Trait for PDF text extraction backends.
pub trait PdfBackend: Send + Sync {
    /// Extract the full text content of a PDF file.
    fn extract_text(&self, path: &Path) -> Result<String, BackendError>;

    /// For each needle, search the PDF and return the first match's
    /// page + line bounding boxes. None entries mean no match found.
    /// Default impl returns all-None for backends that don't support search.
    fn locate_strings(
        &self,
        path: &Path,
        needles: &[&str],
    ) -> Result<Vec<Option<PdfLocation>>, BackendError> {
        let _ = path;
        Ok(vec![None; needles.len()])
    }
}
