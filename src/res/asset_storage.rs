//! AssetStorage download & decryption.

use crate::config::get_config;
use crate::crypto::asset_storage as crypto;
use crate::error::Result;
use std::path::{Path, PathBuf};

/// Fetch and decrypt AssetStorage.txt from server.
///
/// JP: {cdn_base}/{folder_name}Android/AssetStorage.txt
/// CN: {cdn_addr}/NewResources/Android/AssetStorage.{ver}.txt
#[allow(clippy::too_many_arguments)]
pub async fn fetch(
    client: &reqwest::Client,
    server: &str,
    folder_name: &str,
    cdn_addr: Option<&str>,
    asset_storage_version: Option<&str>,
    stage_data: &[u8; 32],
    stage_top: &[u8; 32],
    data_dir: &Path,
    force: bool,
) -> Result<PathBuf> {
    let out_path = data_dir.join("AssetStorage_dec.txt");
    if out_path.exists() && !force {
        log::info!(
            "[{}] AssetStorage_dec.txt cached, skipping download",
            server
        );
        return Ok(out_path);
    }

    let config = get_config(server);
    let is_cn = config.server_type == "cn";

    let url = if is_cn {
        // CN format: {cdn_addr}/NewResources/Android/AssetStorage.{ver}.txt
        let cdn =
            cdn_addr.ok_or_else(|| crate::error::Error::NotFound("CN cdn_addr not set".into()))?;
        let ver = asset_storage_version.unwrap_or("");
        format!("{cdn}/NewResources/Android/AssetStorage.{ver}.txt")
    } else {
        // JP format
        format!(
            "{}/{}{}/AssetStorage.txt",
            config.cdn_base, folder_name, config.platform
        )
    };

    log::info!("[{}] Downloading AssetStorage...", server);
    log::info!("  URL: {url}");

    let enc_text = client.get(&url).send().await?.text().await?;

    // Save encrypted text for debugging
    let enc_path = data_dir.join("AssetStorage_enc.txt");
    std::fs::write(&enc_path, &enc_text)?;

    log::info!(
        "[{}] Decrypting AssetStorage... ({} bytes)",
        server,
        enc_text.len()
    );

    let dec_text = crypto::decrypt(&enc_text, stage_data, stage_top)
        .map_err(crate::error::Error::Crypto)?;

    std::fs::write(&out_path, &dec_text)?;
    log::info!(
        "[{}] Saved: {} ({} bytes)",
        server,
        out_path.display(),
        dec_text.len()
    );

    Ok(out_path)
}
