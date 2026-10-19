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

    if !force && crate::storage::read_cache(&out_path, &url, 64 * 1024 * 1024).is_some() {
        log::info!("[{server}] AssetStorage cache matches the current version");
        return Ok(out_path);
    }
    log::info!("[{}] Downloading AssetStorage...", server);
    log::info!("  URL: {url}");

    let enc_bytes = crate::storage::get(client, &url, 64 * 1024 * 1024).await?;
    let enc_text =
        String::from_utf8(enc_bytes).map_err(|e| crate::error::Error::Parse(e.to_string()))?;

    // Save encrypted text for debugging
    let enc_path = data_dir.join("AssetStorage_enc.txt");

    log::info!(
        "[{}] Decrypting AssetStorage... ({} bytes)",
        server,
        enc_text.len()
    );

    let dec_text =
        crypto::decrypt(&enc_text, stage_data, stage_top).map_err(crate::error::Error::Crypto)?;

    let entries = super::parser::parse_text(&dec_text, config.server_type)
        .map_err(crate::error::Error::Parse)?;
    if entries.is_empty() {
        return Err(crate::error::Error::Parse("empty AssetStorage".into()));
    }
    crate::storage::atomic_write(&enc_path, enc_text.as_bytes())?;
    crate::storage::write_cache(&out_path, &url, dec_text.as_bytes())?;
    log::info!(
        "[{}] Saved: {} ({} bytes)",
        server,
        out_path.display(),
        dec_text.len()
    );

    Ok(out_path)
}
