use anyhow::{Result, bail};
use oai_pmh::{ListRecordsRequest, OaiClient};
use paper_store::PaperStore;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConfig {
    pub base_url: Option<String>,
    pub metadata_prefix: String,
    pub from: Option<String>,
    pub until: Option<String>,
    pub set: Option<String>,
    pub limit: Option<usize>,
    pub state_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    pub fetched_records: usize,
    pub last_token: Option<String>,
}

pub async fn sync_oai_records(store: &PaperStore, config: SyncConfig) -> Result<SyncResult> {
    if config.metadata_prefix.trim().is_empty() {
        bail!("metadata_prefix 不能为空");
    }

    let client = OaiClient::new(config.base_url.clone())?;
    let start_token = store.get_sync_state(&config.state_key)?;

    let request = ListRecordsRequest {
        metadata_prefix: config.metadata_prefix,
        from: config.from,
        until: config.until,
        set: config.set,
        resumption_token: start_token.clone(),
    };

    let page = client.list_records(&request).await?;

    let mut count = 0usize;
    for record in &page.records {
        if let Some(identifier) = record.identifier.as_deref() {
            store.set_sync_state(
                &format!("last_record:{}", config.state_key),
                identifier,
            )?;
        }
        count += 1;
        if let Some(limit) = config.limit && count >= limit {
            break;
        }
    }

    if let Some(token) = page.resumption_token.as_deref() {
        store.set_sync_state(&config.state_key, token)?;
    } else {
        store.set_sync_state(&config.state_key, "")?;
    }

    Ok(SyncResult {
        fetched_records: count,
        last_token: page.resumption_token,
    })
}
