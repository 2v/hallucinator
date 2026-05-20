use std::sync::Arc;

use anyhow::Context;
use dashmap::DashMap;
use hallucinator_core::Config;
use hallucinator_core::pool::ValidationPool;
use tokio_util::sync::CancellationToken;

use crate::jobs::PdfJob;

pub struct AppState {
    pub config: Arc<Config>,
    pub cancel: CancellationToken,
    pub pool: ValidationPool,
    pub jobs: DashMap<String, Arc<PdfJob>>,
}

impl AppState {
    pub async fn new(cancel: CancellationToken) -> anyhow::Result<Self> {
        let config = Arc::new(load_config()?);
        // 4 concurrent ref-validation workers — same default the CLI uses.
        let pool = ValidationPool::new(config.clone(), cancel.clone(), 4);
        Ok(Self {
            config,
            cancel,
            pool,
            jobs: DashMap::new(),
        })
    }
}

fn load_config() -> anyhow::Result<Config> {
    // Reuse the same config-loading rules as the CLI/TUI: hallucinator.toml in
    // CWD, then ~/.config/hallucinator/config.toml, then defaults.
    let file = hallucinator_core::config_file::load_config();
    let mut cfg = Config::default();
    if let Some(api) = file.api_keys {
        cfg.openalex_key = api.openalex_key;
        cfg.s2_api_key = api.s2_api_key;
        cfg.crossref_mailto = api.crossref_mailto;
    }
    if let Some(db) = file.databases {
        cfg.dblp_offline_path = db.dblp_offline_path.map(Into::into);
        cfg.acl_offline_path = db.acl_offline_path.map(Into::into);
        cfg.arxiv_offline_path = db.arxiv_offline_path.map(Into::into);
        cfg.iacr_eprint_offline_path = db.iacr_eprint_offline_path.map(Into::into);
        cfg.openalex_offline_path = db.openalex_offline_path.map(Into::into);
        if let Some(cache_path) = db.cache_path {
            cfg.query_cache = Some(Arc::new(
                hallucinator_core::cache::QueryCache::open(
                    std::path::Path::new(&cache_path),
                    std::time::Duration::from_secs(30 * 86400),
                    std::time::Duration::from_secs(86400),
                )
                .map_err(|e| anyhow::anyhow!(e))
                .context("opening cache")?,
            ));
        }
        if let Some(disabled) = db.disabled {
            cfg.disabled_dbs = disabled.into_iter().collect();
        }
    }
    cfg.rate_limiters = Arc::new(hallucinator_core::rate_limit::RateLimiters::new(
        cfg.crossref_mailto.is_some(),
        cfg.s2_api_key.is_some(),
    ));
    Ok(cfg)
}
