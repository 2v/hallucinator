//! Per-field validation report.
//!
//! Unlike [`crate::Status`], which collapses a citation to a single verdict,
//! `ValidationReport` emits one signal per field (each author, journal,
//! year, etc.) so a downstream UI can highlight potential hallucinations
//! and let the user veto false positives.

use serde::{Deserialize, Serialize};

/// Per-field breakdown of how a cited reference compares to the DB record
/// we matched it against. Populated only when at least one DB returned a
/// title match; if `title_status` is `NotFoundInAnyDB`, the rest is empty.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationReport {
    pub title_status: TitleStatus,
    /// Which DB the field comparisons came from (None when title wasn't matched).
    pub source_db: Option<String>,
    /// One entry per cited author (in citation order).
    pub authors: Vec<AuthorFieldStatus>,
    pub journal: Option<FieldStatus>,
    pub year: Option<FieldStatus>,
    pub volume: Option<FieldStatus>,
    pub issue: Option<FieldStatus>,
    pub pages: Option<FieldStatus>,
    pub doi: Option<FieldStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TitleStatus {
    Matched,
    NotFoundInAnyDb,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorFieldStatus {
    pub cited: String,
    pub status: AuthorStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AuthorStatus {
    /// Cited author compat-matches a found author.
    Matched { db_match: String },
    /// Same surname in DB but full first name differs — classic LLM swap.
    PotentialLookalike { db_candidate: String },
    /// Surname not present in DB record — fabricated co-author or DB is incomplete.
    NotInDb,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FieldStatus {
    Matched {
        value: String,
    },
    PotentialMismatch {
        cited: String,
        db_value: String,
    },
    /// Citation has a value but the DB record doesn't carry this field.
    Unverifiable {
        cited: String,
    },
}

/// Compare a single optional field, returning None if the citation didn't
/// include it. Equality is case-insensitive on the trimmed strings — used
/// for both string and numeric fields after stringification.
pub fn classify_field(cited: Option<&str>, db_value: Option<&str>) -> Option<FieldStatus> {
    let cited = cited?.trim();
    if cited.is_empty() {
        return None;
    }
    match db_value.map(|s| s.trim()) {
        None => Some(FieldStatus::Unverifiable {
            cited: cited.to_string(),
        }),
        Some(db) if db.is_empty() => Some(FieldStatus::Unverifiable {
            cited: cited.to_string(),
        }),
        Some(db) if cited.eq_ignore_ascii_case(db) => Some(FieldStatus::Matched {
            value: db.to_string(),
        }),
        Some(db) => Some(FieldStatus::PotentialMismatch {
            cited: cited.to_string(),
            db_value: db.to_string(),
        }),
    }
}

/// Compare page-number fields. Citations often give a range ("989-1000")
/// while DBs may report only the start page; match on start page when the
/// DB value is shorter.
pub fn classify_pages(cited: Option<&str>, db_value: Option<&str>) -> Option<FieldStatus> {
    fn start_page(s: &str) -> &str {
        s.split(['-', '\u{2013}', '\u{2014}']).next().unwrap_or(s).trim()
    }
    let cited = cited?.trim();
    if cited.is_empty() {
        return None;
    }
    match db_value.map(|s| s.trim()) {
        None => Some(FieldStatus::Unverifiable {
            cited: cited.to_string(),
        }),
        Some(db) if db.is_empty() => Some(FieldStatus::Unverifiable {
            cited: cited.to_string(),
        }),
        Some(db) => {
            if cited.eq_ignore_ascii_case(db) || start_page(cited).eq_ignore_ascii_case(start_page(db)) {
                Some(FieldStatus::Matched {
                    value: db.to_string(),
                })
            } else {
                Some(FieldStatus::PotentialMismatch {
                    cited: cited.to_string(),
                    db_value: db.to_string(),
                })
            }
        }
    }
}

/// Compare journal names. Tries exact match first, then a normalised
/// match that strips common abbreviation noise (periods, lowercased,
/// "the " prefix, common stop-words like "of"/"and") so "Ann Intern Med"
/// and "Annals of Internal Medicine" don't false-mismatch.
pub fn classify_journal(cited: Option<&str>, db_value: Option<&str>) -> Option<FieldStatus> {
    fn normalize(s: &str) -> String {
        let lower = s.to_ascii_lowercase();
        let no_punct: String = lower
            .chars()
            .map(|c| if c.is_ascii_punctuation() { ' ' } else { c })
            .collect();
        no_punct
            .split_whitespace()
            .filter(|w| !matches!(*w, "the" | "of" | "and" | "for" | "a" | "an"))
            .map(|w| {
                // Strip common journal-abbrev plural/suffix endings
                w.trim_end_matches('.')
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn abbreviates(short: &str, full: &str) -> bool {
        // Treat as abbreviation if every short token is a prefix of some full token,
        // taken in order (common pattern: "Ann Intern Med" → "Annals Internal Medicine").
        let short_toks: Vec<&str> = short.split_whitespace().collect();
        let full_toks: Vec<&str> = full.split_whitespace().collect();
        if short_toks.is_empty() || short_toks.len() > full_toks.len() {
            return false;
        }
        let mut fi = 0;
        for s in &short_toks {
            let mut matched = false;
            while fi < full_toks.len() {
                if full_toks[fi].starts_with(s) {
                    matched = true;
                    fi += 1;
                    break;
                }
                fi += 1;
            }
            if !matched {
                return false;
            }
        }
        true
    }

    let cited = cited?.trim();
    if cited.is_empty() {
        return None;
    }
    match db_value.map(|s| s.trim()) {
        None => Some(FieldStatus::Unverifiable {
            cited: cited.to_string(),
        }),
        Some(db) if db.is_empty() => Some(FieldStatus::Unverifiable {
            cited: cited.to_string(),
        }),
        Some(db) => {
            let n_cited = normalize(cited);
            let n_db = normalize(db);
            if cited.eq_ignore_ascii_case(db)
                || n_cited == n_db
                || abbreviates(&n_cited, &n_db)
                || abbreviates(&n_db, &n_cited)
            {
                Some(FieldStatus::Matched {
                    value: db.to_string(),
                })
            } else {
                Some(FieldStatus::PotentialMismatch {
                    cited: cited.to_string(),
                    db_value: db.to_string(),
                })
            }
        }
    }
}

/// Assemble a per-field [`ValidationReport`] from a cited reference and
/// the DB record that matched it.
pub fn build_validation_report(
    cited: &crate::Reference,
    db: Option<(&str, &crate::db::DbQueryResult)>,
) -> ValidationReport {
    let Some((db_name, db_result)) = db else {
        return ValidationReport {
            title_status: TitleStatus::NotFoundInAnyDb,
            source_db: None,
            authors: Vec::new(),
            journal: None,
            year: None,
            volume: None,
            issue: None,
            pages: None,
            doi: None,
        };
    };

    let authors = crate::authors::classify_authors(&cited.authors, &db_result.authors);

    let year_cited = cited.year.map(|y| y.to_string());
    let year_db = db_result.year.map(|y| y.to_string());

    ValidationReport {
        title_status: TitleStatus::Matched,
        source_db: Some(db_name.to_string()),
        authors,
        journal: classify_journal(cited.journal.as_deref(), db_result.journal.as_deref()),
        year: classify_field(year_cited.as_deref(), year_db.as_deref()),
        volume: classify_field(cited.volume.as_deref(), db_result.volume.as_deref()),
        issue: classify_field(cited.issue.as_deref(), db_result.issue.as_deref()),
        pages: classify_pages(cited.pages.as_deref(), db_result.pages.as_deref()),
        doi: classify_field(cited.doi.as_deref(), db_result.doi.as_deref()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_field_basic() {
        assert!(matches!(
            classify_field(Some("2024"), Some("2024")),
            Some(FieldStatus::Matched { .. })
        ));
        assert!(matches!(
            classify_field(Some("2023"), Some("2024")),
            Some(FieldStatus::PotentialMismatch { .. })
        ));
        assert!(matches!(
            classify_field(Some("2024"), None),
            Some(FieldStatus::Unverifiable { .. })
        ));
        assert!(classify_field(None, Some("2024")).is_none());
        assert!(classify_field(Some(""), Some("2024")).is_none());
    }

    #[test]
    fn classify_pages_handles_range_vs_start() {
        // DB has just start page, citation has range — still a match.
        assert!(matches!(
            classify_pages(Some("989-1000"), Some("989")),
            Some(FieldStatus::Matched { .. })
        ));
        // Both have range, exact equality.
        assert!(matches!(
            classify_pages(Some("989-1000"), Some("989-1000")),
            Some(FieldStatus::Matched { .. })
        ));
        // Different start pages → mismatch.
        assert!(matches!(
            classify_pages(Some("1989-2000"), Some("989-1000")),
            Some(FieldStatus::PotentialMismatch { .. })
        ));
        // En-dash splitter.
        assert!(matches!(
            classify_pages(Some("989\u{2013}1000"), Some("989")),
            Some(FieldStatus::Matched { .. })
        ));
    }

    #[test]
    fn classify_journal_handles_abbreviation() {
        assert!(matches!(
            classify_journal(Some("Ann Intern Med"), Some("Annals of Internal Medicine")),
            Some(FieldStatus::Matched { .. })
        ));
        assert!(matches!(
            classify_journal(Some("JAMA"), Some("JAMA")),
            Some(FieldStatus::Matched { .. })
        ));
        assert!(matches!(
            classify_journal(Some("New England Journal of Medicine"), Some("JAMA")),
            Some(FieldStatus::PotentialMismatch { .. })
        ));
    }
}
