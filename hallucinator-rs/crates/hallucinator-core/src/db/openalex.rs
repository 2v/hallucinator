use super::{DatabaseBackend, DbQueryError, DbQueryResult};
use crate::matching::titles_match;
use crate::rate_limit::check_rate_limit_response;
use crate::text_utils::get_query_words;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

pub struct OpenAlex {
    pub api_key: String,
}

impl DatabaseBackend for OpenAlex {
    fn name(&self) -> &str {
        "OpenAlex"
    }

    fn query<'a>(
        &'a self,
        title: &'a str,
        client: &'a reqwest::Client,
        timeout: Duration,
    ) -> Pin<Box<dyn Future<Output = Result<DbQueryResult, DbQueryError>> + Send + 'a>> {
        Box::pin(async move {
            let words = get_query_words(title, 6);
            let query = words.join(" ");
            let url = format!(
                "https://api.openalex.org/works?filter=title.search:{}&api_key={}",
                urlencoding::encode(&query),
                urlencoding::encode(&self.api_key)
            );

            let resp = client
                .get(&url)
                .header("User-Agent", "Academic Reference Parser")
                .timeout(timeout)
                .send()
                .await
                .map_err(|e| DbQueryError::Other(e.to_string()))?;

            check_rate_limit_response(&resp)?;
            if !resp.status().is_success() {
                return Err(DbQueryError::Other(format!("HTTP {}", resp.status())));
            }

            let data: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| DbQueryError::Other(e.to_string()))?;
            let results = data["results"].as_array().cloned().unwrap_or_default();

            for item in results.iter().take(5) {
                let found_title = item["title"].as_str().unwrap_or("");
                if !found_title.is_empty() && titles_match(title, found_title) {
                    let authors: Vec<String> = item["authorships"]
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|a| {
                                    a["author"]["display_name"].as_str().map(String::from)
                                })
                                .collect()
                        })
                        .unwrap_or_default();

                    // Skip results with empty authors - let other DBs verify
                    if authors.is_empty() {
                        continue;
                    }

                    let paper_url = item["doi"]
                        .as_str()
                        .map(String::from)
                        .or_else(|| item["id"].as_str().map(String::from));

                    let journal = item["primary_location"]["source"]["display_name"]
                        .as_str()
                        .map(String::from);
                    let year = item["publication_year"].as_u64().and_then(|y| u16::try_from(y).ok());
                    let volume = item["biblio"]["volume"].as_str().map(String::from);
                    let issue = item["biblio"]["issue"].as_str().map(String::from);
                    let first = item["biblio"]["first_page"].as_str();
                    let last = item["biblio"]["last_page"].as_str();
                    let pages = match (first, last) {
                        (Some(f), Some(l)) if f != l => Some(format!("{}-{}", f, l)),
                        (Some(f), _) => Some(f.to_string()),
                        _ => None,
                    };
                    // OpenAlex DOI URL like "https://doi.org/10.xxxx" — strip prefix.
                    let doi = item["doi"]
                        .as_str()
                        .map(|d| d.trim_start_matches("https://doi.org/").to_string());

                    return Ok(DbQueryResult {
                        found_title: Some(found_title.to_string()),
                        authors,
                        paper_url,
                        journal,
                        year,
                        volume,
                        issue,
                        pages,
                        doi,
                        ..DbQueryResult::default()
                    });
                }
            }

            Ok(DbQueryResult::not_found())
        })
    }
}
