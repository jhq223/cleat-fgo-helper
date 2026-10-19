//! Asset bundle download (parallel HTTP).

use crate::config::get_config;
use crate::error::Result;
use crate::res::parser::AssetEntry;
use std::path::{Path, PathBuf};

/// Download a single encrypted .bin file.
pub async fn download_bundle(
    client: &reqwest::Client,
    server: &str,
    folder_name: &str,
    cdn_addr: Option<&str>,
    asset: &AssetEntry,
    data_dir: &Path,
    force: bool,
) -> Result<Vec<u8>> {
    let cache_path = data_dir.join("cache").join(&asset.file_name);

    let config = get_config(server);
    let is_cn = config.server_type == "cn";

    let url = if is_cn {
        let cdn =
            cdn_addr.ok_or_else(|| crate::error::Error::NotFound("CN cdn_addr not set".into()))?;
        // CN: {cdn}/NewResources/Android/{hash[0:2]}/{file_name}
        let prefix = &asset.file_name[..2.min(asset.file_name.len())];
        format!("{cdn}/NewResources/Android/{prefix}/{}", asset.file_name)
    } else {
        // JP: {cdn_base}/{folder_name}Android/{file_name}
        format!(
            "{}/{}{}/{}",
            config.cdn_base, folder_name, config.platform, asset.file_name
        )
    };

    log::debug!("  GET {}", url);
    const LIMIT: usize = 128 * 1024 * 1024;
    let identity = format!("{url}|{}|{}", asset.revision, asset.size);
    if !force && let Some(bytes) = crate::storage::read_cache(&cache_path, &identity, LIMIT) {
        return Ok(bytes);
    }
    let data = crate::storage::get(client, &url, LIMIT).await?;
    if data.is_empty() || (asset.size != 0 && data.len() as u64 != asset.size) {
        return Err(crate::error::Error::Parse(format!(
            "{}: downloaded {} bytes, expected {}",
            asset.asset_path,
            data.len(),
            asset.size
        )));
    }
    crate::storage::write_cache(&cache_path, &identity, &data)?;

    Ok(data)
}

/// Download multiple bundles in parallel.
pub async fn download_bundles(
    client: &reqwest::Client,
    server: &str,
    folder_name: &str,
    cdn_addr: Option<&str>,
    assets: &[&AssetEntry],
    data_dir: &Path,
    force: bool,
) -> Result<Vec<(PathBuf, Vec<u8>)>> {
    use futures_util::StreamExt;

    let total = assets.len();
    log::info!("[{}] Downloading {} bundles...", server, total);

    let results: Vec<Result<(PathBuf, Vec<u8>)>> = futures_util::stream::iter(assets.iter())
        .map(|asset| {
            let client = client.clone();
            let server = server.to_string();
            let folder_name = folder_name.to_string();
            let cdn_addr = cdn_addr.map(String::from);
            let data_dir = data_dir.to_path_buf();
            async move {
                let data = download_bundle(
                    &client,
                    &server,
                    &folder_name,
                    cdn_addr.as_deref(),
                    asset,
                    &data_dir,
                    force,
                )
                .await?;
                let cache_path = data_dir.join("cache").join(&asset.file_name);
                Ok((cache_path, data))
            }
        })
        .buffer_unordered(8) // max concurrent downloads
        .collect()
        .await;

    let mut succeeded = Vec::new();
    let mut failures = Vec::new();
    for result in results {
        match result {
            Ok(v) => succeeded.push(v),
            Err(e) => failures.push(e.to_string()),
        }
    }

    log::info!(
        "[{}] Downloaded {}/{} bundles",
        server,
        succeeded.len(),
        total
    );
    if !failures.is_empty() {
        return Err(crate::error::Error::Parse(format!(
            "{}/{} bundle downloads failed: {}",
            failures.len(),
            total,
            failures.join("; ")
        )));
    }
    Ok(succeeded)
}
