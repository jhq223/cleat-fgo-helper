//! Full download & decrypt pipeline orchestration.

use crate::config::derive_keys;
use crate::crypto::asset_bin;
use crate::crypto::script;
use crate::error::Result;
use crate::res::{asset_storage, download, parser, version};
use crate::unity;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Data directory for a server.
fn data_dir(server: &str) -> PathBuf {
    PathBuf::from("data").join(server.to_lowercase())
}

/// Fetch version info only.
pub async fn cmd_info(server: &str) -> Result<()> {
    let client = build_client()?;
    let info = version::fetch(&client, server).await?;
    println!("Server: {}", server.to_uppercase());
    println!("  appVer:      {}", info.app_ver);
    println!("  folderName:  {}", info.folder_name);
    println!("  dataVer:     {}", info.data_ver);
    if !info.date_ver.is_empty() {
        println!("  dateVer:     {}", info.date_ver);
    }
    if let Some(cdn) = &info.cdn_addr {
        println!("  cdn:         {}", cdn);
    }
    if let Some(asv) = &info.asset_storage_version {
        println!("  assetStorage: {}", asv);
    }
    Ok(())
}

/// List script assets from cached AssetStorage.
pub async fn cmd_list(server: &str) -> Result<()> {
    let dir = data_dir(server);
    let storage_path = dir.join("AssetStorage_dec.txt");

    if !storage_path.exists() {
        eprintln!("No cached AssetStorage. Run 'download' first.");
        return Ok(());
    }

    let config = crate::config::get_config(server);
    let entries =
        parser::parse(&storage_path, config.server_type).map_err(crate::error::Error::Parse)?;

    let scripts = parser::find_script_assets(&entries);

    println!(
        "Server: {} — {} script assets",
        server.to_uppercase(),
        scripts.len()
    );
    for s in scripts {
        let ek = s.extra_key.as_deref().unwrap_or("-");
        println!("  {:45}  {:>8} KB  ek={}", s.asset_path, s.size / 1024, ek);
    }

    Ok(())
}

/// Full pipeline: version → AssetStorage → download → decrypt → extract.
pub async fn cmd_download(server: &str, force: bool, no_scripts: bool) -> Result<()> {
    let (stage_data, stage_top, base_data, base_top) = derive_keys(server);
    let config = crate::config::get_config(server);
    let dir = data_dir(server);
    std::fs::create_dir_all(&dir)?;

    let client = build_client()?;

    // Step 1: Version info
    let ver_info = version::fetch(&client, server).await?;

    // Step 2: AssetStorage
    let storage_path = asset_storage::fetch(
        &client,
        server,
        &ver_info.folder_name,
        ver_info.cdn_addr.as_deref(),
        ver_info.asset_storage_version.as_deref(),
        &stage_data,
        &stage_top,
        &dir,
        force,
    )
    .await?;

    // Step 3: Parse assets
    let entries =
        parser::parse(&storage_path, config.server_type).map_err(crate::error::Error::Parse)?;

    let script_assets = parser::find_script_assets(&entries);
    log::info!("[{}] Found {} script assets", server, script_assets.len());

    if no_scripts {
        log::info!("[{}] Skipping script download (--no-scripts)", server);
        return Ok(());
    }

    // Step 4: Load extra keys (JP only)
    let extra_keys: HashMap<String, String> = if config.server_type == "jp" {
        load_or_fetch_extra_keys(&client, server, &ver_info, &dir, force).await?
    } else {
        HashMap::new()
    };

    // Step 5: Parallel download + decrypt all bundles
    log::info!("[{}] Downloading & decrypting bundles...", server);

    let downloaded = download::download_bundles(
        &client,
        server,
        &ver_info.folder_name,
        ver_info.cdn_addr.as_deref(),
        &script_assets,
        &dir,
        force,
    )
    .await?;

    // Decrypt each bundle
    let ab_dir = dir.join("decrypted_ab");
    std::fs::create_dir_all(&ab_dir)?;

    let mut decrypted_paths = Vec::new();
    for (_cache_path, enc_data) in &downloaded {
        // Find the matching asset entry
        let file_name = _cache_path.file_name().unwrap().to_str().unwrap_or("");

        let asset = script_assets.iter().find(|a| a.file_name == file_name);

        let real_key = asset
            .and_then(|a| a.extra_key.as_deref())
            .and_then(|ek| extra_keys.get(ek).map(String::as_str));

        let ab_data = asset_bin::decrypt(enc_data, &base_data, &base_top, real_key)?;

        let unity3d_name = format!("{}.unity3d", file_name);
        let ab_path = ab_dir.join(&unity3d_name);
        std::fs::write(&ab_path, &ab_data)?;
        decrypted_paths.push(ab_path);
    }

    log::info!(
        "[{}] Decrypted {} bundles to {}",
        server,
        decrypted_paths.len(),
        ab_dir.display()
    );

    // Step 6: Extract TextAsset via Python (UnityPy)
    if decrypted_paths.is_empty() {
        log::warn!("[{}] No bundles to extract", server);
        return Ok(());
    }

    let scripts_dir = dir.join("scripts");
    std::fs::create_dir_all(&scripts_dir)?;

    let extracted = unity::extract::extract_batch(&ab_dir)?;

    // Step 7: Decrypt scripts in parallel (rayon)
    use rayon::prelude::*;

    let all_texts: Vec<_> = extracted
        .iter()
        .flat_map(|b| {
            b.texts
                .iter()
                .map(move |t| (t.name.clone(), t.script_text.clone()))
        })
        .collect();

    log::info!("[{}] Decrypting {} scripts...", server, all_texts.len());

    let use_bzip2 = config.use_bzip2;
    let results: Vec<Result<PathBuf>> = all_texts
        .par_iter()
        .filter(|(name, _)| {
            name.is_empty() || name.chars().all(|c| c.is_ascii_digit()) || name == "ScriptFileList"
        })
        .map(|(name, text)| {
            let path = scripts_dir.join(format!("{name}.txt"));
            // Try MouseGame3 decrypt; if it fails, save raw text as-is
            match script::decrypt(text, &stage_data, &stage_top, use_bzip2) {
                Ok(plain) => {
                    std::fs::write(&path, &plain)?;
                }
                Err(_) => {
                    // Not encrypted — save the raw text directly
                    std::fs::write(&path, text)?;
                }
            }
            Ok(path)
        })
        .collect();

    let mut count = 0;
    for r in results {
        match r {
            Ok(_) => count += 1,
            Err(e) => log::error!("Decrypt failed: {e}"),
        }
    }

    log::info!(
        "[{}] Done! {} script .txt files → {}",
        server,
        count,
        scripts_dir.display()
    );

    Ok(())
}

fn build_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(90)) // JP CDN redirect can take 40-50s
        .user_agent("Dalvik/2.1.0 (Linux; U; Android 13; Pixel 6 Build/TQ2A.230505.002)")
        .build()
        .map_err(crate::error::Error::Http)
}

async fn load_or_fetch_extra_keys(
    client: &reqwest::Client,
    server: &str,
    ver_info: &version::VersionInfo,
    data_dir: &Path,
    force: bool,
) -> Result<HashMap<String, String>> {
    let key_path = data_dir.join("assetbundleKey.json");

    if key_path.exists() && !force {
        match parser::load_extra_keys(&key_path) {
            Ok(keys) => {
                log::info!("[{}] Loaded {} extra keys", server, keys.len());
                return Ok(keys);
            }
            Err(e) => log::warn!("Failed to load extra keys: {e}"),
        }
    }

    // Fetch extra keys from gamedata API (JP only)
    log::info!("[{}] Fetching assetbundleKey from gamedata...", server);

    let base_url = crate::config::JP_CONFIG
        .version_url
        .split('?')
        .next()
        .unwrap();
    let url = format!(
        "{base_url}?appVer={}&dataVer={}&dateVer={}",
        ver_info.app_ver, ver_info.data_ver, ver_info.date_ver
    );

    let resp = client.get(&url).send().await?;
    let data: serde_json::Value = resp.json().await?;

    let success = data["response"][0]["success"]
        .as_object()
        .ok_or_else(|| crate::error::Error::Version("No success in response".into()))?;

    let ab_key_b64 = success
        .get("assetbundleKey")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if ab_key_b64.is_empty() {
        log::info!("[{}] No assetbundleKey in response", server);
        return Ok(HashMap::new());
    }

    let key_list = crate::crypto::gamedata::unpack_key_list(ab_key_b64)
        .map_err(crate::error::Error::Crypto)?;

    let json = serde_json::to_string_pretty(&key_list).map_err(crate::error::Error::Json)?;
    std::fs::write(&key_path, &json)?;

    let keys: HashMap<String, String> = key_list
        .into_iter()
        .map(|k| (k.id, k.decrypt_key))
        .collect();

    log::info!(
        "[{}] Saved {} extra keys to {:?}",
        server,
        keys.len(),
        key_path
    );
    Ok(keys)
}
