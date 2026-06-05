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

    if cache_path.exists() && !force {
        log::debug!("  cached: {}", asset.file_name);
        return Ok(std::fs::read(&cache_path)?);
    }

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

    let resp = client.get(&url).send().await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let url_str = url.clone();
        return Err(crate::error::Error::Crypto(format!(
            "HTTP {} for {} ({})",
            status, asset.file_name, url_str
        )));
    }

    let data = resp.bytes().await?.to_vec();

    // Cache to disk
    if let Some(parent) = cache_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&cache_path, &data)?;

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
    for result in results {
        match result {
            Ok(v) => succeeded.push(v),
            Err(e) => log::error!("Download failed: {e}"),
        }
    }

    log::info!(
        "[{}] Downloaded {}/{} bundles",
        server,
        succeeded.len(),
        total
    );
    Ok(succeeded)
}
