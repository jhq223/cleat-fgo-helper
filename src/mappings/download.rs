//! Chaldea Center translation mappings download.

use crate::error::{Error, Result};
use futures_util::StreamExt;
use std::path::PathBuf;

const MAPPINGS_URL: &str =
    "https://raw.githubusercontent.com/chaldea-center/chaldea-data/main/mappings";

/// Translation-relevant mapping files (22 core categories).
const MAPPING_FILES: &[&str] = &[
    "svt_names.json",
    "ce_names.json",
    "cc_names.json",
    "costume_names.json",
    "mc_names.json",
    "mc_detail.json",
    "skill_names.json",
    "skill_detail.json",
    "td_names.json",
    "td_ruby.json",
    "td_types.json",
    "td_detail.json",
    "item_names.json",
    "entity_names.json",
    "quest_names.json",
    "spot_names.json",
    "event_names.json",
    "war_names.json",
    "buff_names.json",
    "buff_detail.json",
    "event_mission.json",
    "mission_names.json",
];

/// Download all Chaldea translation mapping JSON files.
pub async fn cmd_download(output: &str) -> Result<()> {
    let dir = PathBuf::from(output);
    std::fs::create_dir_all(&dir)?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .user_agent("cleat-fgo-helper/0.1")
        .build()?;

    let total = MAPPING_FILES.len();
    log::info!("Downloading {total} translation mapping files...");

    let results = futures_util::stream::iter(MAPPING_FILES.iter())
        .map(|name| {
            let client = client.clone();
            let dir = dir.clone();
            async move {
                let url = format!("{MAPPINGS_URL}/{name}");
                log::debug!("  GET {}", name);

                let resp = client.get(&url).send().await?;
                if !resp.status().is_success() {
                    log::error!("  {} — HTTP {}", name, resp.status());
                    return Ok::<_, Error>(None);
                }

                let bytes = resp.bytes().await?;
                let path = dir.join(name);
                std::fs::write(&path, &bytes)?;
                log::info!("  ✓ {} ({} KB)", name, bytes.len() / 1024);
                Ok(Some(name))
            }
        })
        .buffer_unordered(8)
        .collect::<Vec<_>>()
        .await;

    let mut count = 0;
    for r in results {
        match r {
            Ok(Some(_)) => count += 1,
            Ok(None) => {}
            Err(e) => log::error!("Download error: {e}"),
        }
    }

    log::info!("Downloaded {}/{} mapping files to {}", count, total, dir.display());
    Ok(())
}
