use once_cell::sync::Lazy;
use std::collections::HashSet;
use unicode_normalization::UnicodeNormalization;

use crate::matching::fold_special_letters;

/// Common surname prefixes (case-insensitive).
static SURNAME_PREFIXES: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        "van", "von", "de", "del", "della", "di", "da", "al", "el", "la", "le", "ben", "ibn",
        "mac", "mc", "o",
    ]
    .into_iter()
    .collect()
});

/// Name suffixes to strip.
static NAME_SUFFIXES: Lazy<HashSet<&'static str>> =
    Lazy::new(|| ["jr", "sr", "ii", "iii", "iv", "v"].into_iter().collect());

/// Known organizational author names. When a ref_author matches one of these,
/// we check if the org name appears anywhere in the found_authors list instead
/// of doing normal name matching.
static ORG_AUTHOR_NAMES: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        "openai",
        "meta",
        "google",
        "deepmind",
        "anthropic",
        "microsoft",
        "deepseek",
        "deepseekai",
        "alibaba",
        "baidu",
        "tencent",
        "nvidia",
        "apple",
        "darpa",
        "ftc",
        "nasa",
        "nist",
        "ieee",
        "acm",
        "who",
        "oecd",
        "unesco",
        "european commission",
        "mistralai",
        "mistral",
        // Government departments/agencies (returned by GovInfo)
        "commerce department",
        "department of commerce",
        "department of defense",
        "department of energy",
        "department of homeland security",
        "department of justice",
        "congress",
        "senate",
        "gao",
        "cisa",
        "fda",
        "epa",
        "fcc",
        "sec",
    ]
    .into_iter()
    .collect()
});

/// Validate that the cited authors plausibly belong to the paper whose
/// authors are `found_authors`. Returns `false` for an author mismatch
/// (including look-alike phantoms like "Isaac Shi" vs "Ivy Shi").
pub fn validate_authors(ref_authors: &[String], found_authors: &[String]) -> bool {
    if ref_authors.is_empty() || found_authors.is_empty() {
        return false;
    }

    // Org names (e.g. "OpenAI", "Qwen Team") match by org match, not per-author.
    for author_list in [ref_authors, found_authors] {
        for author in author_list {
            let lower = author.trim().to_lowercase();
            let dehyphen = lower.replace('-', "");
            if ORG_AUTHOR_NAMES.contains(lower.as_str())
                || ORG_AUTHOR_NAMES.contains(dehyphen.as_str())
            {
                return true;
            }
            let words: Vec<&str> = lower.split_whitespace().collect();
            if words.last() == Some(&"team") && words.len() <= 3 {
                return true;
            }
        }
    }

    let ref_clean: Vec<&str> = ref_authors
        .iter()
        .map(|a| a.trim())
        .filter(|a| !a.is_empty())
        .collect();

    let last_name_only_count = ref_clean
        .iter()
        .filter(|a| !has_first_name_or_initial(a))
        .count();
    let ref_are_last_name_only = last_name_only_count > ref_clean.len() / 2;

    if ref_are_last_name_only {
        let ref_surnames: Vec<String> = ref_authors
            .iter()
            .filter_map(|a| {
                let s = get_last_name(a);
                if s.is_empty() { None } else { Some(s) }
            })
            .collect();

        let found_surnames: Vec<String> = found_authors
            .iter()
            .filter_map(|a| {
                let s = get_last_name(a);
                if s.is_empty() { None } else { Some(s) }
            })
            .collect();

        for rn in &ref_surnames {
            for fn_ in &found_surnames {
                if rn == fn_ {
                    return true;
                }
                if fn_.ends_with(rn.as_str()) || rn.ends_with(fn_.as_str()) {
                    return true;
                }
            }
        }
        false
    } else {
        // Phantom-author guard: a citation that pads several unrelated
        // names onto a real paper's author list (common LLM behaviour)
        // should be flagged even though the genuine authors still overlap.
        // Uses cache::author_fingerprint to handle particle-prefixed
        // surnames and to ignore "et al." tokens.
        if ref_authors.len() > found_authors.len() {
            let found_fps: HashSet<String> = found_authors
                .iter()
                .filter_map(|a| crate::cache::author_fingerprint(a))
                .collect();
            let phantom_count = ref_authors
                .iter()
                .filter(|a| {
                    crate::cache::author_fingerprint(a).is_some_and(|fp| !found_fps.contains(&fp))
                })
                .count();
            if phantom_count >= 3 && phantom_count * 4 > ref_authors.len() {
                return false;
            }
        }

        // Per-author classification: `compat` = surname collision + compat
        // match, `lookalike` = surname collision but full first name
        // differs ("Isaac Shi" vs "Ivy Shi"), `unknown` = surname not in
        // found list. If any compat match exists alongside any lookalike,
        // it's a confirmed paper with a swapped author — flag it.
        let ref_keys: Vec<AuthorKey> =
            ref_authors.iter().filter_map(|a| make_author_key(a)).collect();
        let found_keys: Vec<AuthorKey> = found_authors
            .iter()
            .filter_map(|a| make_author_key(a))
            .collect();

        let mut compat_count = 0usize;
        let mut lookalike_count = 0usize;
        for r in &ref_keys {
            if r.surname.is_empty() {
                continue;
            }
            let mut had_collision = false;
            let mut had_compat = false;
            for f in &found_keys {
                if f.surname == r.surname {
                    had_collision = true;
                    if keys_compat(r, f) {
                        had_compat = true;
                        break;
                    }
                }
            }
            if had_compat {
                compat_count += 1;
            } else if had_collision {
                lookalike_count += 1;
            }
        }

        if compat_count > 0 && lookalike_count > 0 {
            return false;
        }
        if compat_count > 0 {
            return true;
        }

        let ref_surnames: HashSet<String> = ref_authors
            .iter()
            .filter_map(|a| {
                let s = get_last_name(a);
                if s.is_empty() { None } else { Some(s) }
            })
            .collect();
        let found_surnames: HashSet<String> = found_authors
            .iter()
            .filter_map(|a| {
                let s = get_last_name(a);
                if s.is_empty() { None } else { Some(s) }
            })
            .collect();

        // et-al truncation: "Gentry et al." ⊂ DB's full author list.
        if ref_authors.len() < found_authors.len()
            && ref_authors.len() <= 5
            && !ref_surnames.is_empty()
            && ref_surnames.is_subset(&found_surnames)
        {
            return true;
        }

        // Last-name-first style ("Ekparinya Parinya" vs DBLP's "Parinya
        // Ekparinya"): when names are ambiguous two-token shapes, also
        // accept first-token-vs-surname overlap. Only reached if the
        // stricter checks above failed.
        if ref_authors.iter().all(|a| is_ambiguous_two_token(a))
            && found_authors.iter().any(|a| is_ambiguous_two_token(a))
        {
            let ref_first_tokens: HashSet<String> = ref_authors
                .iter()
                .filter_map(|a| first_token_lower(a))
                .collect();
            if !ref_first_tokens.is_disjoint(&found_surnames) {
                return true;
            }
        }

        false
    }
}

/// Per-author breakdown of how cited authors compare to a DB record's
/// author list. Same matching machinery as [`validate_authors`] but
/// returns one [`crate::report::AuthorFieldStatus`] per cited author
/// instead of collapsing to a single bool.
pub fn classify_authors(
    ref_authors: &[String],
    found_authors: &[String],
) -> Vec<crate::report::AuthorFieldStatus> {
    use crate::report::{AuthorFieldStatus, AuthorStatus};

    let found_keys: Vec<(usize, AuthorKey)> = found_authors
        .iter()
        .enumerate()
        .filter_map(|(i, a)| make_author_key(a).map(|k| (i, k)))
        .collect();

    ref_authors
        .iter()
        .map(|cited| {
            let cited_str = cited.clone();
            let key = match make_author_key(cited) {
                Some(k) => k,
                None => {
                    return AuthorFieldStatus {
                        cited: cited_str,
                        status: AuthorStatus::NotInDb,
                    };
                }
            };

            let mut surname_collision: Option<String> = None;
            for (i, f) in &found_keys {
                if f.surname == key.surname {
                    if keys_compat(&key, f) {
                        return AuthorFieldStatus {
                            cited: cited_str,
                            status: AuthorStatus::Matched {
                                db_match: found_authors[*i].clone(),
                            },
                        };
                    }
                    if surname_collision.is_none() {
                        surname_collision = Some(found_authors[*i].clone());
                    }
                }
            }

            let status = match surname_collision {
                Some(db_candidate) => AuthorStatus::PotentialLookalike { db_candidate },
                None => AuthorStatus::NotInDb,
            };
            AuthorFieldStatus {
                cited: cited_str,
                status,
            }
        })
        .collect()
}

/// A name is "ambiguous two-token" if it's exactly two whitespace-separated
/// tokens, both begin with an uppercase letter, and neither is an initial or
/// carries a period (i.e. we cannot tell Given-Family from Family-Given).
fn is_ambiguous_two_token(name: &str) -> bool {
    // AAAI form ("Smith, Jane") is unambiguous — the comma marks the
    // surname — so it should never enter the LNF fallback.
    if name.contains(',') {
        return false;
    }
    // split_whitespace already handles leading/trailing whitespace.
    let parts: Vec<&str> = name.split_whitespace().collect();
    if parts.len() != 2 {
        return false;
    }
    for p in &parts {
        if p.contains('.') || p.len() < 2 {
            return false;
        }
        let first = p.chars().next().unwrap();
        if !first.is_uppercase() {
            return false;
        }
    }
    true
}

/// Lowercase first whitespace-separated token (with trailing punctuation
/// trimmed). Used only by the LNF fallback above.
fn first_token_lower(name: &str) -> Option<String> {
    let first = name.split_whitespace().next()?;
    let first = first.trim_end_matches(|c: char| !c.is_alphanumeric());
    if first.is_empty() {
        None
    } else {
        Some(strip_diacritics(first).to_lowercase())
    }
}

/// Drop a trailing DBLP-style 4-digit disambiguation suffix
/// (`"Wenbo Guo 0001"` → `["Wenbo", "Guo"]`). Without this, downstream
/// surname extraction would treat `0001` as the surname.
fn strip_dblp_suffix<'a>(parts: &[&'a str]) -> Vec<&'a str> {
    if parts.len() >= 2 {
        let last = *parts.last().unwrap();
        if last.len() == 4 && last.bytes().all(|b| b.is_ascii_digit()) {
            return parts[..parts.len() - 1].to_vec();
        }
    }
    parts.to_vec()
}

/// Extract surname from name parts, handling multi-word surnames and suffixes.
fn get_surname_from_parts(parts: &[&str]) -> String {
    if parts.is_empty() {
        return String::new();
    }

    // Strip DBLP disambiguation suffix and name suffixes
    let mut parts = strip_dblp_suffix(parts);
    while parts.len() >= 2
        && NAME_SUFFIXES.contains(parts.last().unwrap().to_lowercase().trim_end_matches('.'))
    {
        parts.pop();
    }

    if parts.is_empty() {
        return String::new();
    }

    // Check for three-part surnames like "De La Cruz"
    if parts.len() >= 3
        && SURNAME_PREFIXES.contains(parts[parts.len() - 3].to_lowercase().trim_end_matches('.'))
    {
        return parts[parts.len() - 3..].join(" ");
    }

    // Check for two-part surnames like "Van Bavel"
    if parts.len() >= 2
        && SURNAME_PREFIXES.contains(parts[parts.len() - 2].to_lowercase().trim_end_matches('.'))
    {
        return parts[parts.len() - 2..].join(" ");
    }

    parts.last().unwrap().to_string()
}

/// Strip diacritics and normalize typographic characters for comparison.
/// "Müller" → "Muller", "Crépeau" → "Crepeau", "Müßig" → "Mussig",
/// "Adıgüzel" → "Adiguzel", "O'Brien" → "O'Brien"
fn strip_diacritics(s: &str) -> String {
    // Normalize curly quotes/apostrophes to ASCII before NFKD
    // (NFKD doesn't decompose U+2019 RIGHT SINGLE QUOTATION MARK)
    let s = s
        .replace(['\u{2019}', '\u{2018}'], "'") // curly single quotes → apostrophe
        .replace(['\u{201C}', '\u{201D}'], "\""); // curly double quotes → straight
    // Fold ß/ı/ø/… to their DBLP/arXiv-style ASCII transliteration; NFKD alone
    // would leave these untouched and the ASCII filter would then drop them.
    let s = fold_special_letters(&s);
    s.nfkd().filter(|c| c.is_ascii()).collect()
}

/// Normalize an author name to "FirstInitial surname" format for comparison.
///
/// Kept for the unit tests that pin the legacy fingerprint format. The
/// live matching path uses [`make_author_key`] / [`keys_compat`] instead,
/// which preserves full first names so "Isaac Shi" no longer collides
/// with "Ivy Shi".
#[cfg(test)]
fn normalize_author(name: &str) -> String {
    // Strip diacritics first so comparisons are accent-insensitive
    let name = strip_diacritics(name.trim());
    let name = name.trim();

    // AAAI "Surname, Initials" format
    if name.contains(',') {
        let parts: Vec<&str> = name.splitn(2, ',').collect();
        let surname = parts[0].trim();
        let initials = if parts.len() > 1 { parts[1].trim() } else { "" };
        let first_initial = initials.chars().next().unwrap_or(' ');
        return format!("{} {}", first_initial, surname.to_lowercase());
    }

    let raw_parts: Vec<&str> = name.split_whitespace().collect();
    if raw_parts.is_empty() {
        return String::new();
    }
    let parts = strip_dblp_suffix(&raw_parts);
    if parts.is_empty() {
        return String::new();
    }

    // Springer "Surname Initial" format: last part is 1-2 uppercase letters
    if parts.len() >= 2 {
        let last = *parts.last().unwrap();
        if last.len() <= 2 && last.chars().all(|c| c.is_uppercase()) {
            let surname = parts[..parts.len() - 1].join(" ");
            let initial = last.chars().next().unwrap();
            return format!("{} {}", initial, surname.to_lowercase());
        }
    }

    // Standard: "FirstName LastName"
    let surname = get_surname_from_parts(&parts);
    let first_initial = parts[0].chars().next().unwrap_or(' ');
    format!("{} {}", first_initial, surname.to_lowercase())
}

/// Get the last name from an author name string (public API for orchestrator).
pub fn get_last_name_public(name: &str) -> String {
    get_last_name(name)
}

/// Get the last name from an author name string.
fn get_last_name(name: &str) -> String {
    // Strip diacritics for accent-insensitive comparison
    let name = strip_diacritics(name.trim());
    let name = name.trim();

    // AAAI "Surname, Initials" format
    if name.contains(',') {
        return name.split(',').next().unwrap().trim().to_lowercase();
    }

    let parts: Vec<&str> = name.split_whitespace().collect();
    if parts.is_empty() {
        return String::new();
    }

    get_surname_from_parts(&parts).to_lowercase()
}

/// Parsed author with separated surname and first-name component.
/// Two keys are compatible iff surnames match AND first parts are
/// compatible (see [`keys_compat`]).
#[derive(Debug, Clone, PartialEq, Eq)]
struct AuthorKey {
    surname: String,
    first: FirstPart,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FirstPart {
    Full(String),
    Initial(char),
    None,
}

fn make_author_key(name: &str) -> Option<AuthorKey> {
    let name = strip_diacritics(name.trim());
    let name = name.trim();
    if name.is_empty() {
        return None;
    }

    // AAAI "Surname, Initials" form.
    if name.contains(',') {
        let parts: Vec<&str> = name.splitn(2, ',').collect();
        let surname = parts[0].trim().to_lowercase();
        if surname.is_empty() {
            return None;
        }
        let first_str = parts.get(1).map_or("", |s| s.trim());
        let first = if first_str.is_empty() {
            FirstPart::None
        } else {
            let token = first_str.split_whitespace().next().unwrap_or("");
            parse_first_part(token)
        };
        return Some(AuthorKey { surname, first });
    }

    let raw_parts: Vec<&str> = name.split_whitespace().collect();
    let parts = strip_dblp_suffix(&raw_parts);
    if parts.is_empty() {
        return None;
    }

    if parts.len() == 1 {
        return Some(AuthorKey {
            surname: parts[0].to_lowercase(),
            first: FirstPart::None,
        });
    }

    // Springer "Surname Initial" form.
    let last = *parts.last().unwrap();
    if last.len() <= 2 && last.chars().all(|c| c.is_uppercase()) {
        let surname = parts[..parts.len() - 1].join(" ").to_lowercase();
        let initial = last.chars().next().unwrap().to_ascii_lowercase();
        return Some(AuthorKey {
            surname,
            first: FirstPart::Initial(initial),
        });
    }

    let surname = get_surname_from_parts(&parts).to_lowercase();
    if surname.is_empty() {
        return None;
    }
    Some(AuthorKey {
        surname,
        first: parse_first_part(parts[0]),
    })
}

fn parse_first_part(token: &str) -> FirstPart {
    let token = token.trim();
    if token.is_empty() {
        return FirstPart::None;
    }
    let stripped = token.trim_end_matches('.');

    if token.contains('.') {
        return stripped
            .chars()
            .find(|c| c.is_alphabetic())
            .map(|c| FirstPart::Initial(c.to_ascii_lowercase()))
            .unwrap_or(FirstPart::None);
    }

    if stripped.len() <= 2 && stripped.chars().all(|c| c.is_uppercase()) {
        return stripped
            .chars()
            .next()
            .map(|c| FirstPart::Initial(c.to_ascii_lowercase()))
            .unwrap_or(FirstPart::None);
    }

    if stripped.is_empty() {
        FirstPart::None
    } else {
        FirstPart::Full(stripped.to_lowercase())
    }
}

/// Surname-equal, first-part-compatible. `Full` vs `Full` requires
/// equality (distinguishes Isaac/Ivy); `Full` vs `Initial` only checks
/// the first letter (preserves "I. Shi" ↔ "Isaac Shi"); `None` on either
/// side accepts the surname-only match.
fn keys_compat(a: &AuthorKey, b: &AuthorKey) -> bool {
    if a.surname.is_empty() || a.surname != b.surname {
        return false;
    }
    match (&a.first, &b.first) {
        (FirstPart::Full(x), FirstPart::Full(y)) => x == y,
        (FirstPart::Full(f), FirstPart::Initial(i))
        | (FirstPart::Initial(i), FirstPart::Full(f)) => f
            .chars()
            .next()
            .is_some_and(|c| c.to_ascii_lowercase() == *i),
        (FirstPart::Initial(x), FirstPart::Initial(y)) => x == y,
        (FirstPart::None, _) | (_, FirstPart::None) => true,
    }
}

/// Check if a name contains a first name or initial (not just a surname).
fn has_first_name_or_initial(name: &str) -> bool {
    let name = name.trim();
    if name.is_empty() {
        return false;
    }

    // "Surname, Initial" format
    if name.contains(',') {
        let parts: Vec<&str> = name.splitn(2, ',').collect();
        return parts.len() > 1 && !parts[1].trim().is_empty();
    }

    let parts: Vec<&str> = name.split_whitespace().collect();
    // Strip name suffixes
    let core_parts: Vec<&str> = parts
        .iter()
        .filter(|p| !NAME_SUFFIXES.contains(p.to_lowercase().trim_end_matches('.')))
        .copied()
        .collect();

    if core_parts.len() <= 1 {
        return false;
    }

    // Check for initials in non-last positions
    for part in &core_parts[..core_parts.len() - 1] {
        if part.trim_end_matches('.').len() == 1 {
            return true;
        }
    }

    // Check Springer "Surname Initial" format (last part is 1-2 uppercase)
    let last = *core_parts.last().unwrap();
    if last.len() <= 2 && last.chars().all(|c| c.is_uppercase()) {
        return true;
    }

    // Check if first part is a first name
    let first = core_parts[0].trim_end_matches('.');
    if first.len() >= 2
        && first.chars().next().is_some_and(|c| c.is_uppercase())
        && !SURNAME_PREFIXES.contains(first.to_lowercase().as_str())
        && core_parts.len() >= 2
    {
        let second = core_parts[1].trim_end_matches('.');
        if second.len() >= 2 && second.chars().next().is_some_and(|c| c.is_uppercase()) {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_validate_authors_basic() {
        assert!(validate_authors(
            &s(&["John Smith", "Alice Jones"]),
            &s(&["John Smith", "Bob Brown"]),
        ));
    }

    #[test]
    fn test_validate_authors_no_overlap() {
        assert!(!validate_authors(&s(&["John Smith"]), &s(&["Bob Brown"]),));
    }

    #[test]
    fn test_validate_authors_last_name_only() {
        // Last-name-only mode
        assert!(validate_authors(
            &s(&["Smith", "Jones"]),
            &s(&["John Smith", "Alice Jones"]),
        ));
    }

    #[test]
    fn test_validate_authors_multi_word_surname() {
        assert!(validate_authors(
            &s(&["Jay Van Bavel"]),
            &s(&["J. J. Van Bavel"]),
        ));
    }

    #[test]
    fn test_validate_authors_aaai_format() {
        assert!(validate_authors(
            &s(&["Bail, C. A.", "Jones, M."]),
            &s(&["Christopher Bail", "Michael Jones"]),
        ));
    }

    #[test]
    fn test_normalize_author_springer() {
        assert_eq!(normalize_author("Abrahao S"), "S abrahao");
    }

    #[test]
    fn test_normalize_author_standard() {
        assert_eq!(normalize_author("John Smith"), "J smith");
    }

    #[test]
    fn test_normalize_author_aaai() {
        assert_eq!(normalize_author("Bail, C. A."), "C bail");
    }

    #[test]
    fn test_get_last_name_multi_word() {
        assert_eq!(get_last_name("Jay Van Bavel"), "van bavel");
    }

    #[test]
    fn test_dblp_homonym_suffix_stripped() {
        // DBLP appends 4-digit homonym suffixes ("Wenbo Guo 0001") to
        // disambiguate same-named authors. The suffix must not leak
        // into surname extraction or normalization.
        assert_eq!(get_last_name("Wenbo Guo 0001"), "guo");
        assert_eq!(normalize_author("Wenbo Guo 0001"), "W guo");
        assert!(validate_authors(
            &s(&["Wenbo Guo"]),
            &s(&["Wenbo Guo 0001", "Alice Jones"]),
        ));
    }

    #[test]
    fn test_empty() {
        assert!(!validate_authors(&[], &s(&["Smith"])));
        assert!(!validate_authors(&s(&["Smith"]), &[]));
    }

    #[test]
    fn test_org_author_openai() {
        // "OpenAI" as ref author should match found authors from the OpenAI org
        assert!(validate_authors(
            &s(&["OpenAI"]),
            &s(&["Josh Achiam", "Steven Adler"]),
        ));
    }

    #[test]
    fn test_org_author_team() {
        // "Qwen Team" as org author — skip validation, accept any found authors
        assert!(validate_authors(
            &s(&["Qwen Team"]),
            &s(&["An Yang", "Baosong Yang"]),
        ));
    }

    #[test]
    fn test_org_author_meta() {
        // "Meta" is a known org — skip validation
        assert!(validate_authors(
            &s(&["Meta"]),
            &s(&["Hugo Touvron", "Thibaut Lavril"]),
        ));
    }

    #[test]
    fn test_org_author_deepseek() {
        // "DeepSeek-AI" with hyphen should also match
        assert!(validate_authors(&s(&["DeepSeek-AI"]), &s(&["Some Author"]),));
    }

    #[test]
    fn test_org_author_found_side() {
        // DBLP returns "DeepSeek-AI" as org, but PDF has individual authors
        assert!(validate_authors(
            &s(&["Daya Guo", "Dejian Yang", "Haowei Zhang"]),
            &s(&["DeepSeek-AI"]),
        ));
    }

    #[test]
    fn test_accent_insensitive_muller() {
        // "Müller" from PDF should match "Muller" from DB
        assert!(validate_authors(
            &s(&["Nicolas M. Müller"]),
            &s(&["Nicolas M. Muller"]),
        ));
    }

    #[test]
    fn test_accent_insensitive_crepeau() {
        // "Crépeau" should match "Crepeau"
        assert!(validate_authors(
            &s(&["C. Crépeau", "D. Gottesman"]),
            &s(&["Claude Crepeau", "Daniel Gottesman"]),
        ));
    }

    #[test]
    fn test_accent_insensitive_doupe() {
        // "Doupé" should match "Doupe"
        assert!(validate_authors(
            &s(&["Huahong Tu", "Adam Doupé"]),
            &s(&["Huahong Tu", "Adam Doupe"]),
        ));
    }

    #[test]
    fn test_accent_insensitive_tramer() {
        // "Tramèr" should match "Tramer"
        assert!(validate_authors(
            &s(&["Florian Tramèr"]),
            &s(&["Florian Tramer"]),
        ));
    }

    #[test]
    fn test_sharp_s_folds_to_ss() {
        // PDF: "Müßig", DBLP/arXiv: "Mussig" (ß → ss).
        // NFKD alone would drop ß and produce "Mig", which never matched.
        assert!(validate_authors(&s(&["Hans Müßig"]), &s(&["Hans Mussig"]),));
    }

    #[test]
    fn test_dotless_i_folds_to_i() {
        // PDF: "Adıgüzel" (Turkish dotless i), DBLP: "Adiguzel".
        // NFKD alone would drop ı and produce "Adgzel", which never matched.
        assert!(validate_authors(
            &s(&["Cemal Adıgüzel"]),
            &s(&["Cemal Adiguzel"]),
        ));
    }

    #[test]
    fn test_slashed_o_folds_to_o() {
        // PDF: "Bjørn Østergaard", DBLP: "Bjorn Ostergaard".
        assert!(validate_authors(
            &s(&["Bjørn Østergaard"]),
            &s(&["Bjorn Ostergaard"]),
        ));
    }

    #[test]
    fn test_l_with_stroke_folds_to_l() {
        // PDF: "Wojciech Łukasz", DBLP: "Wojciech Lukasz".
        assert!(validate_authors(
            &s(&["Wojciech Łukasz"]),
            &s(&["Wojciech Lukasz"]),
        ));
    }

    #[test]
    fn test_accent_insensitive_last_name_only() {
        // Last-name-only mode with accents
        assert!(validate_authors(
            &s(&["Müller", "Köbis"]),
            &s(&["Nicolas Muller", "Nils Kobis"]),
        ));
    }

    #[test]
    fn test_curly_quote_obrien() {
        // PDF uses curly quote U+2019, DB uses straight apostrophe
        assert!(validate_authors(
            &s(&["Sean O\u{2019}Brien"]),
            &s(&["Sean O'Brien"]),
        ));
    }

    #[test]
    fn test_et_al_subset_single_author() {
        // PDF says "Gentry" (et al. truncated), DB has "Boneh, Gentry"
        assert!(validate_authors(
            &s(&["Craig Gentry"]),
            &s(&["Dan Boneh", "Craig Gentry"]),
        ));
    }

    #[test]
    fn test_phantom_authors_padded_citation_rejected() {
        // Real USENIX 2022 paper "Are Your Sensitive Attributes Private?"
        // has 5 authors. A hallucinated citation pads in 5 famous security
        // researchers as fake co-authors — the genuine 5 still overlap, so
        // the standard intersection would pass. The phantom-author guard
        // must catch this and flag it as a mismatch.
        assert!(!validate_authors(
            &s(&[
                "Shagufta Mehnaz",
                "Sayanton V Dibbo",
                "Roberta De Viti",
                "Ehsanul Kabir",
                "Björn B Brandenburg",
                "Stefan Mangard",
                "Ninghui Li",
                "Elisa Bertino",
                "Michael Backes",
                "Emiliano De Cristofaro",
            ]),
            &s(&[
                "Shagufta Mehnaz",
                "Sayanton V. Dibbo",
                "Ehsanul Kabir",
                "Ninghui Li",
                "Elisa Bertino",
            ]),
        ));
    }

    #[test]
    fn test_phantom_authors_one_extra_still_passes() {
        // A single unmatched name (e.g., a typo'd or unusual
        // transliteration) shouldn't trip the guard — only sustained
        // padding should.
        assert!(validate_authors(
            &s(&[
                "Alice Author",
                "Bob Author",
                "Carol Author",
                "Dave Author",
                "Eve Author",
                "Frank Typo",
            ]),
            &s(&[
                "Alice Author",
                "Bob Author",
                "Carol Author",
                "Dave Author",
                "Eve Author",
            ]),
        ));
    }

    #[test]
    fn test_phantom_authors_fires_even_with_small_found() {
        // Real failure: DBLP's StackGuard (USENIX 1998) entry has only
        // one author indexed (Crispan Cowan) although the actual paper
        // has 10. A padded citation (10 ref authors, 1 DBLP author, 9
        // surnames not matching the lone DB author) used to pass the
        // standard intersection because the phantom guard was gated on
        // `found.len() >= 3` — DB-completeness is too generous a
        // benefit-of-the-doubt when the citation/DB skew is this
        // extreme. The guard now fires regardless of DB size, so
        // citations that don't match the indexed author list (whatever
        // its size) are flagged as mismatches.
        assert!(!validate_authors(
            &s(&[
                "Crispan Cowan",
                "Calton Pu",
                "Dave Maier",
                "Heather Hintony",
                "Jonathan Walpole",
                "Peat Bakke",
                "Steve Beattie",
                "Aaron Grier",
                "Perry Wagle",
                "Qian Zhang",
            ]),
            &s(&["Crispan Cowan"]),
        ));
    }

    #[test]
    fn test_phantom_authors_use_fingerprint_normalization() {
        // The phantom guard now keys on `<initial>:<surname>`
        // fingerprints (the same form `compute_fp_identity` uses) so
        // particle-prefixed surnames don't inflate the phantom count
        // when one side carries the particle and the other doesn't:
        //   ref:  "Emiliano De Cristofaro"  →  e:cristofaro
        //   db:   "Cristofaro, E."          →  e:cristofaro
        // Surname-only would have produced "de cristofaro" vs
        // "cristofaro" — different — and counted the matched author
        // as a phantom. With fingerprints they collide, so a citation
        // whose authors all match the DB record (even via different
        // surname formats) doesn't trip the guard.
        assert!(validate_authors(
            &s(&[
                "Alice Author",
                "Bob Author",
                "Carol Author",
                "Emiliano De Cristofaro",
            ]),
            &s(&[
                "Alice Author",
                "Bob Author",
                "Carol Author",
                "Cristofaro, E.",
            ]),
        ));
    }

    #[test]
    fn test_et_al_subset_two_authors() {
        // PDF says "Dwork, Roth" (et al. truncated), DB has "Dwork, Roth, Others"
        assert!(validate_authors(
            &s(&["Cynthia Dwork", "Aaron Roth"]),
            &s(&["Cynthia Dwork", "Aaron Roth", "Guy Rothblum"]),
        ));
    }

    #[test]
    fn test_et_al_no_false_positive() {
        // Subset match should NOT match when ref authors are NOT in found authors
        assert!(!validate_authors(
            &s(&["John Smith"]),
            &s(&["Alice Jones", "Bob Brown"]),
        ));
    }

    #[test]
    fn test_et_al_many_ref_authors_no_subset() {
        // When ref has many authors (>3), subset match is disabled,
        // so completely different authors should NOT match
        assert!(!validate_authors(
            &s(&["X. Alpha", "Y. Beta", "Z. Gamma", "W. Delta"]),
            &s(&[
                "A. One", "B. Two", "C. Three", "D. Four", "E. Five", "F. Six"
            ]),
        ));
    }

    // ─── Fix D: last-name-first citation style ───

    #[test]
    fn test_lnf_style_matches_dblp_given_family() {
        // USENIX 2025 case: "The attack of the clones against proof-of-authority"
        // cited as "Ekparinya Parinya, Gramoli Vincent, and Jourjon Guillaume"
        // — family-first — while DBLP has standard "Parinya Ekparinya",
        // "Vincent Gramoli", "Guillaume Jourjon".
        assert!(validate_authors(
            &s(&["Ekparinya Parinya", "Gramoli Vincent", "Jourjon Guillaume"]),
            &s(&["Parinya Ekparinya", "Vincent Gramoli", "Guillaume Jourjon"]),
        ));
    }

    #[test]
    fn test_lnf_style_still_rejects_wrong_paper() {
        // LNF fallback must not turn unrelated authors into a match.
        // "Alice Bob, Charlie Dave" (two-token, ambiguous) against unrelated
        // surnames — no first-token or last-token overlap either way.
        assert!(!validate_authors(
            &s(&["Alice Bob", "Charlie Dave"]),
            &s(&["Eve Foster", "Frank Greene"]),
        ));
    }

    #[test]
    fn test_lnf_fallback_skipped_when_names_have_initials() {
        // When names carry initials, the ordering is unambiguous — the
        // LNF fallback should not fire. This guards against enabling the
        // fallback for normal IEEE-style refs and accidentally matching
        // "J. Smith" to DBLP's "Smith John" just because both contain "smith".
        assert!(!validate_authors(
            &s(&["J. Smith"]),
            &s(&["Alice Kumar", "Robert Chen"]),
        ));
    }

    // ─── Full-first-name comparison (Isaac/Ivy class) ───

    #[test]
    fn test_full_first_names_with_shared_initial_mismatch() {
        // Same surname, same first initial, different full first names.
        // The classic AI-hallucination shape: real paper title, plausible
        // co-author swapped for someone else whose name starts with the
        // same letter.
        assert!(!validate_authors(
            &s(&["Isaac Shi"]),
            &s(&["Ivy Shi"]),
        ));
        assert!(!validate_authors(
            &s(&["Aaron Smith"]),
            &s(&["Alan Smith"]),
        ));
        assert!(!validate_authors(
            &s(&["Michael Chen"]),
            &s(&["Maria Chen"]),
        ));
    }

    #[test]
    fn test_full_first_name_matches_initial_form() {
        // The compatibility check must still accept the common
        // initial-vs-full-name format pair so citations of "I. Shi" still
        // verify against DB records of "Isaac Shi".
        assert!(validate_authors(&s(&["I. Shi"]), &s(&["Isaac Shi"]),));
        assert!(validate_authors(&s(&["Isaac Shi"]), &s(&["I. Shi"]),));
    }

    #[test]
    fn test_full_first_names_equal_match() {
        assert!(validate_authors(
            &s(&["Isaac Shi"]),
            &s(&["Isaac Shi", "Other Person"]),
        ));
    }

    #[test]
    fn test_surname_only_citation_still_matches_full() {
        // If the citation gives only a surname, the strongest available
        // signal is surname overlap — accept it.
        assert!(validate_authors(
            &s(&["Shi", "Jones"]),
            &s(&["Isaac Shi", "Alice Jones"]),
        ));
    }

    #[test]
    fn test_full_first_name_accent_insensitive() {
        // "Sören" vs "Soren" must still equate after diacritic stripping.
        assert!(validate_authors(
            &s(&["Sören Müller"]),
            &s(&["Soren Muller"]),
        ));
        // ...but two different full first names sharing an initial must
        // still mismatch even with accents in play.
        assert!(!validate_authors(
            &s(&["Sören Müller"]),
            &s(&["Stefan Muller"]),
        ));
    }

    #[test]
    fn test_full_first_name_aaai_vs_full() {
        // AAAI "Smith, John" against full "John Smith" still matches.
        assert!(validate_authors(
            &s(&["Smith, John"]),
            &s(&["John Smith"]),
        ));
        // ...but "Smith, Jane" vs "John Smith" must not (same initial 'J',
        // different full first names after disambiguation).
        // Note: "Smith, J." vs "John Smith" should still match (initial
        // form on one side).
        assert!(validate_authors(
            &s(&["Smith, J."]),
            &s(&["John Smith"]),
        ));
        assert!(!validate_authors(
            &s(&["Smith, Jane"]),
            &s(&["John Smith"]),
        ));
    }

    #[test]
    fn test_full_first_name_different_surname_obvious_reject() {
        // Sanity: full-name compat does nothing if surnames differ.
        assert!(!validate_authors(
            &s(&["Isaac Shi"]),
            &s(&["Isaac Wong"]),
        ));
    }

    // ─── Look-alike phantom detection (one bad author among good ones) ───

    #[test]
    fn test_lookalike_phantom_flags_mixed_citation() {
        // Real JAMA 2024 paper "Projected changes in statin and
        // antihypertensive therapy eligibility with the AHA PREVENT
        // cardiovascular risk equations" (Diao et al.). The citation
        // swaps "Ivy Shi" for "Isaac Shi" (same surname, same initial,
        // wrong full first name) — the classic LLM-swap shape. Other
        // cited authors are genuine, so the previous "any overlap →
        // verified" rule would pass it. The look-alike check must trip.
        assert!(!validate_authors(
            &s(&[
                "James A. Diao",
                "Isaac Shi", // ← hallucinated
                "Venkatesh L. Murthy",
                "Puneet Batra",
                "Amit V. Khera",
            ]),
            &s(&[
                "James A. Diao",
                "Ivy Shi",
                "Venkatesh L. Murthy",
                "Thomas A. Buckley",
                "Chirag J. Patel",
                "Emma Pierson",
                "Robert W. Yeh",
                "Dhruv S. Kazi",
                "Rishi K. Wadhera",
                "Arjun K. Manrai",
            ]),
        ));
    }

    #[test]
    fn test_lookalike_phantom_requires_real_compat_match() {
        // If NO cited author compat-matches a found author (just a bunch
        // of surname collisions with all-wrong first names), don't fire
        // the look-alike rule — the citation may simply be for a
        // different paper, and the existing fallbacks handle it.
        // Here: same surnames, all wrong first names, no other signal.
        // (Returns false via "no compat" — covered below.)
        assert!(!validate_authors(
            &s(&["Alice Smith", "Bob Jones"]),
            &s(&["Charlie Smith", "Dave Jones"]),
        ));
    }

    #[test]
    fn test_lookalike_only_unknown_surnames_still_verifies() {
        // Cited authors whose surnames don't appear in the DB at all
        // are "unknown" — not look-alikes. As long as at least one
        // cited author compat-matches, the citation verifies. This
        // preserves the et-al-truncation case where the citation lists
        // only a couple of names from a longer roster.
        assert!(validate_authors(
            &s(&["Craig Gentry", "Some Unknown"]),
            &s(&["Dan Boneh", "Craig Gentry"]),
        ));
    }

    #[test]
    fn test_lookalike_initial_form_does_not_trip() {
        // "I. Shi" should compat-match "Isaac Shi" (initial vs full),
        // not register as a look-alike. The whole citation should
        // verify normally.
        assert!(validate_authors(
            &s(&["J. Diao", "I. Shi", "V. Murthy"]),
            &s(&[
                "James A. Diao",
                "Isaac Shi",
                "Venkatesh L. Murthy",
            ]),
        ));
    }

    #[test]
    fn test_lookalike_full_first_name_typo_one_authoritative() {
        // Just one good compat match plus one look-alike → flag.
        // This is the minimum case the new rule catches.
        assert!(!validate_authors(
            &s(&["James Diao", "Isaac Shi"]),
            &s(&["James Diao", "Ivy Shi"]),
        ));
    }

    #[test]
    fn test_make_author_key_parsing() {
        // Direct unit tests for the new key extractor.
        let k = make_author_key("Isaac Shi").unwrap();
        assert_eq!(k.surname, "shi");
        assert_eq!(k.first, FirstPart::Full("isaac".to_string()));

        let k = make_author_key("I. Shi").unwrap();
        assert_eq!(k.surname, "shi");
        assert_eq!(k.first, FirstPart::Initial('i'));

        let k = make_author_key("Shi").unwrap();
        assert_eq!(k.surname, "shi");
        assert_eq!(k.first, FirstPart::None);

        // Multi-word surname preserved
        let k = make_author_key("Jay Van Bavel").unwrap();
        assert_eq!(k.surname, "van bavel");
        assert_eq!(k.first, FirstPart::Full("jay".to_string()));

        // AAAI form
        let k = make_author_key("Bail, C. A.").unwrap();
        assert_eq!(k.surname, "bail");
        assert_eq!(k.first, FirstPart::Initial('c'));

        // Springer trailing-initial form
        let k = make_author_key("Abrahao S").unwrap();
        assert_eq!(k.surname, "abrahao");
        assert_eq!(k.first, FirstPart::Initial('s'));
    }

    #[test]
    fn test_is_ambiguous_two_token() {
        assert!(is_ambiguous_two_token("Ekparinya Parinya"));
        assert!(is_ambiguous_two_token("Gramoli Vincent"));
        // Three tokens — not ambiguous two-token
        assert!(!is_ambiguous_two_token("John M. Smith"));
        // Initial — not ambiguous
        assert!(!is_ambiguous_two_token("J. Smith"));
        assert!(!is_ambiguous_two_token("Smith J."));
        // One token
        assert!(!is_ambiguous_two_token("Madonna"));
        // Lowercase — not a proper noun
        assert!(!is_ambiguous_two_token("john smith"));
    }
}
