//! Public JSON request/response shapes.

use serde::{Deserialize, Serialize};

use hallucinator_core::report::ValidationReport;
use hallucinator_core::{BBox, Reference, ValidationResult};

/// Input shape for a single-reference validation. Matches the
/// `hallucinator_core::Reference` shape — title is required, the rest
/// are optional metadata that improves the per-field report.
#[derive(Debug, Clone, Deserialize)]
pub struct ValidateOneRequest {
    pub title: String,
    #[serde(default)]
    pub authors: Vec<String>,
    pub doi: Option<String>,
    pub arxiv_id: Option<String>,
    pub journal: Option<String>,
    pub year: Option<u16>,
    pub volume: Option<String>,
    pub issue: Option<String>,
    pub pages: Option<String>,
    /// Echoed back so a batching client can correlate results.
    pub client_ref: Option<String>,
}

impl ValidateOneRequest {
    pub fn into_reference(self) -> Reference {
        Reference {
            raw_citation: format!(
                "{} {}",
                self.authors.join(", "),
                self.title.trim()
            ).trim().to_string(),
            title: Some(self.title),
            authors: self.authors,
            doi: self.doi,
            arxiv_id: self.arxiv_id,
            journal: self.journal,
            year: self.year,
            volume: self.volume,
            issue: self.issue,
            pages: self.pages,
            ..Reference::default()
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidateOneResponse {
    pub client_ref: Option<String>,
    pub result: ApiValidationResult,
}

/// JSON-friendly mirror of `hallucinator_core::ValidationResult`. We
/// keep this in the API crate so the public schema is independent of
/// internal Rust type layout — handy for stable MCP contracts later.
#[derive(Debug, Clone, Serialize)]
pub struct ApiValidationResult {
    pub title: String,
    pub raw_citation: String,
    pub ref_authors: Vec<String>,
    pub status: String,
    pub mismatch_kinds: Vec<String>,
    pub source: Option<String>,
    pub found_authors: Vec<String>,
    pub paper_url: Option<String>,
    pub failed_dbs: Vec<String>,
    pub doi: Option<String>,
    pub arxiv_id: Option<String>,
    pub retraction: Option<RetractionJson>,
    pub report: Option<ValidationReport>,
    /// PDF location: present only when validated from an uploaded PDF.
    pub page_number: Option<usize>,
    #[serde(default)]
    pub bboxes: Vec<BBox>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RetractionJson {
    pub is_retracted: bool,
    pub retraction_doi: Option<String>,
    pub retraction_source: Option<String>,
}

impl ApiValidationResult {
    pub fn from_core(r: ValidationResult, page: Option<usize>, bboxes: Vec<BBox>) -> Self {
        use hallucinator_core::{MismatchKind, Status};

        let (status, kinds) = match &r.status {
            Status::Verified => ("verified".to_string(), vec![]),
            Status::NotFound => ("not_found".to_string(), vec![]),
            Status::Mismatch(k) => {
                let mut ks = Vec::new();
                if k.contains(MismatchKind::AUTHOR) { ks.push("author".into()); }
                if k.contains(MismatchKind::DOI) { ks.push("doi".into()); }
                if k.contains(MismatchKind::ARXIV_ID) { ks.push("arxiv_id".into()); }
                ("mismatch".to_string(), ks)
            }
        };

        Self {
            title: r.title,
            raw_citation: r.raw_citation,
            ref_authors: r.ref_authors,
            status,
            mismatch_kinds: kinds,
            source: r.source,
            found_authors: r.found_authors,
            paper_url: r.paper_url,
            failed_dbs: r.failed_dbs,
            doi: r.doi_info.map(|d| d.doi),
            arxiv_id: r.arxiv_info.map(|a| a.arxiv_id),
            retraction: r.retraction_info.map(|ri| RetractionJson {
                is_retracted: ri.is_retracted,
                retraction_doi: ri.retraction_doi,
                retraction_source: ri.retraction_source,
            }),
            report: r.report,
            page_number: page,
            bboxes,
        }
    }
}
