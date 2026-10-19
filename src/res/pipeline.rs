//! Download & decrypt pipeline orchestration.

use crate::config::derive_keys;
use crate::crypto::asset_bin;
use crate::crypto::script;
use crate::error::Result;
use crate::res::parser::AssetEntry;
use crate::res::{asset_storage, download, parser, version};
use crate::unity;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn data_dir(server: &str) -> PathBuf {
    PathBuf::from("data").join(server.to_lowercase())
}

pub async fn cmd_info(server: &str) -> Result<()> {
    let client = build_client()?;
    let info = version::fetch(&client, server).await?;
    log::info!("Server: {}", server.to_uppercase());
    log::info!("  appVer:      {}", info.app_ver);
    log::info!("  folderName:  {}", info.folder_name);
    log::info!("  dataVer:     {}", info.data_ver);
    if !info.date_ver.is_empty() {
        log::info!("  dateVer:     {}", info.date_ver);
    }
    if let Some(cdn) = &info.cdn_addr {
        log::info!("  cdn:         {}", cdn);
    }
    if let Some(asv) = &info.asset_storage_version {
        log::info!("  assetStorage: {}", asv);
    }
    Ok(())
}

pub async fn cmd_list(server: &str) -> Result<()> {
    let dir = data_dir(server);
    let storage_path = dir.join("AssetStorage_dec.txt");

    if !storage_path.exists() {
        log::warn!("No cached AssetStorage. Run 'download' first.");
        return Ok(());
    }

    let config = crate::config::get_config(server);
    let entries =
        parser::parse(&storage_path, config.server_type).map_err(crate::error::Error::Parse)?;

    let scripts = parser::find_script_assets(&entries);

    log::info!(
        "Server: {} — {} script assets",
        server.to_uppercase(),
        scripts.len()
    );
    for s in scripts {
        let ek = s.extra_key.as_deref().unwrap_or("-");
        log::info!("  {:45}  {:>8} KB  ek={}", s.asset_path, s.size / 1024, ek);
    }

    Ok(())
}

pub async fn cmd_download(server: &str, force: bool, no_scripts: bool) -> Result<()> {
    let (stage_data, stage_top, base_data, base_top) = derive_keys(server);
    let config = crate::config::get_config(server);
    let dir = data_dir(server);
    std::fs::create_dir_all(&dir)?;

    let client = build_client()?;

    let ver_info = version::fetch(&client, server).await?;

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

    let entries =
        parser::parse(&storage_path, config.server_type).map_err(crate::error::Error::Parse)?;

    let script_assets = parser::find_script_assets(&entries);
    log::info!("[{}] Found {} script assets", server, script_assets.len());

    let extra_keys: HashMap<String, String> = ver_info
        .resource_keys
        .iter()
        .map(|key| (key.id.clone(), key.decrypt_key.clone()))
        .collect();
    crate::storage::atomic_write(
        &dir.join("assetbundleKey.json"),
        &serde_json::to_vec(&ver_info.resource_keys)?,
    )?;
    let snapshot_id = crate::storage::digest(&serde_json::to_vec(&ver_info)?);
    if !force && let Ok(snapshot) = super::snapshot::load(&dir, &snapshot_id, no_scripts) {
        log::info!(
            "[{server}] verified complete snapshot: {}",
            snapshot.display()
        );
        super::snapshot::activate(&dir, &snapshot_id)?;
        return Ok(());
    }
    let staging = tempfile::tempdir_in(&dir)?;
    let output = staging.path();

    match download_localization(
        &ResourceContext {
            client: &client,
            server,
            ver_info: &ver_info,
            extra_keys: &extra_keys,
            base_data: &base_data,
            base_top: &base_top,
            data_dir: &dir,
            force,
        },
        &entries,
        output,
    )
    .await
    {
        Ok(paths) if paths.is_empty() => {
            return Err(crate::error::Error::NotFound("Localization asset".into()));
        }
        Ok(paths) => {
            let names: Vec<_> = paths.iter().map(|p| p.display().to_string()).collect();
            log::info!("[{}] Localization saved: {}", server, names.join(", "));
        }
        Err(e) => return Err(e),
    }

    if no_scripts {
        log::info!("[{}] Skipping script download (--no-scripts)", server);
        super::snapshot::publish(&dir, &snapshot_id, staging, &ver_info, false)?;
        return Ok(());
    }

    log::info!("[{}] Downloading bundles...", server);

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

    let ab_dir = dir.join("decrypted_ab");
    std::fs::create_dir_all(&ab_dir)?;

    let mut decrypted_paths = Vec::new();
    for (_cache_path, enc_data) in &downloaded {
        let file_name = _cache_path.file_name().unwrap().to_str().unwrap_or("");

        let asset = script_assets.iter().find(|a| a.file_name == file_name);

        let real_key = resource_key(asset.and_then(|a| a.extra_key.as_deref()), &extra_keys)?;

        let ab_data = asset_bin::decrypt(enc_data, &base_data, &base_top, real_key)?;

        let ab_path = ab_dir.join(format!("{}.unity3d", file_name));
        validate_bundle(&ab_data)?;
        crate::storage::atomic_write(&ab_path, &ab_data)?;
        decrypted_paths.push(ab_path);
    }

    log::info!(
        "[{}] Decrypted {} bundles to {}",
        server,
        decrypted_paths.len(),
        ab_dir.display()
    );

    if decrypted_paths.is_empty() {
        log::warn!("[{}] No bundles to extract", server);
        return Ok(());
    }

    let scripts_dir = output.join("scripts");
    std::fs::create_dir_all(&scripts_dir)?;
    let mut key_ids = std::collections::BTreeMap::new();
    let mut all_texts = std::collections::BTreeMap::new();
    for path in &decrypted_paths {
        for (name, key) in unity::extract::script_keys(path)? {
            if key_ids
                .insert(name.clone(), key.clone())
                .is_some_and(|previous| previous != key)
            {
                return Err(crate::error::Error::Parse(format!(
                    "{name}: conflicting key IDs"
                )));
            }
        }
        for text in unity::extract::extract_bundle(path)?.texts {
            if let Some(previous) = all_texts.insert(text.name.clone(), text.script_text.clone())
                && previous != text.script_text
            {
                return Err(crate::error::Error::Parse(format!(
                    "{}: conflicting scripts",
                    text.name
                )));
            }
        }
    }
    log::info!(
        "[{server}] Decrypting {} scripts with {} explicit key references",
        all_texts.len(),
        key_ids.len()
    );
    use rayon::prelude::*;
    let results: Vec<Result<()>> = all_texts
        .par_iter()
        .map(|(name, text)| {
            // GetKey returns null for stale key IDs; the client then uses the
            // legacy codec. This is explicit dispatch, never a trial of other keys.
            let plain = if let Some(key) = key_ids.get(name).and_then(|id| extra_keys.get(id)) {
                script::decrypt_with_key(text, key)
            } else {
                script::decrypt(text, &stage_data, &stage_top, config.use_bzip2)
            }
            .map_err(|e| crate::error::Error::Crypto(format!("{name}: {e}")))?;
            script::validate(name, &plain).map_err(crate::error::Error::Parse)?;
            std::fs::write(scripts_dir.join(format!("{name}.txt")), plain)?;
            Ok(())
        })
        .collect();
    let failures: Vec<_> = results.into_iter().filter_map(Result::err).collect();
    if !failures.is_empty() {
        for error in failures.iter().take(10) {
            log::error!("{error}");
        }
        return Err(crate::error::Error::Crypto(format!(
            "{}/{} script decryptions failed; previous snapshot preserved",
            failures.len(),
            all_texts.len()
        )));
    }
    if all_texts.is_empty() {
        return Err(crate::error::Error::Parse("no scripts extracted".into()));
    }
    super::snapshot::publish(&dir, &snapshot_id, staging, &ver_info, true)?;
    log::info!(
        "[{server}] Verified {} scripts; snapshot {snapshot_id}",
        all_texts.len()
    );
    Ok(())
}

fn resource_key<'a>(
    id: Option<&str>,
    keys: &'a HashMap<String, String>,
) -> Result<Option<&'a str>> {
    id.map(|id| {
        keys.get(id).map(String::as_str).ok_or_else(|| {
            crate::error::Error::Crypto(format!("resource key ID {id} absent in this version"))
        })
    })
    .transpose()
}

fn validate_bundle(bytes: &[u8]) -> Result<()> {
    if bytes.starts_with(b"UnityFS\0")
        || bytes.starts_with(b"UnityRaw\0")
        || bytes.starts_with(b"UnityWeb\0")
    {
        Ok(())
    } else {
        Err(crate::error::Error::Parse(
            "decrypted resource is not a Unity bundle".into(),
        ))
    }
}

struct ResourceContext<'a> {
    client: &'a reqwest::Client,
    server: &'a str,
    ver_info: &'a version::VersionInfo,
    extra_keys: &'a HashMap<String, String>,
    base_data: &'a [u8; 32],
    base_top: &'a [u8; 32],
    data_dir: &'a Path,
    force: bool,
}

async fn download_localization(
    context: &ResourceContext<'_>,
    entries: &[AssetEntry],
    output: &Path,
) -> Result<Vec<PathBuf>> {
    let ResourceContext {
        client,
        server,
        ver_info,
        extra_keys,
        base_data,
        base_top,
        data_dir,
        force,
    } = *context;
    let loc_asset = match parser::find_localization_asset(entries) {
        Some(a) => a,
        None => return Ok(Vec::new()),
    };

    log::info!(
        "[{}] Found Localization asset: {} ({} bytes)",
        server,
        loc_asset.asset_path,
        loc_asset.size
    );

    let enc_data = download::download_bundle(
        client,
        server,
        &ver_info.folder_name,
        ver_info.cdn_addr.as_deref(),
        loc_asset,
        data_dir,
        force,
    )
    .await?;

    let real_key = resource_key(loc_asset.extra_key.as_deref(), extra_keys)?;

    let ab_data = crate::crypto::asset_bin::decrypt(&enc_data, base_data, base_top, real_key)
        .map_err(|e| crate::error::Error::Crypto(format!("Localization decrypt: {e}")))?;

    validate_bundle(&ab_data)?;
    let tmp_ab_path = output.join("_loc_temp.unity3d");
    std::fs::write(&tmp_ab_path, &ab_data)?;

    let texts = crate::unity::extract::extract_bundle_all_texts(&tmp_ab_path)?;
    let _ = std::fs::remove_file(&tmp_ab_path);

    if texts.is_empty() {
        return Err(crate::error::Error::Parse(
            "Localization bundle contains no TextAssets".into(),
        ));
    }

    log::info!(
        "[{}] Localization bundle contains {} TextAsset(s): {:?}",
        server,
        texts.len(),
        texts.iter().map(|t| &t.name).collect::<Vec<_>>()
    );

    let mut saved = Vec::new();
    for t in &texts {
        if !t.name.starts_with("Localization") || !t.name.bytes().all(|b| b.is_ascii_alphanumeric())
        {
            continue;
        }
        let out_path = output.join(format!("{}.txt", t.name));
        std::fs::write(&out_path, &t.script_text)?;
        log::info!(
            "[{}]   → {} ({} bytes)",
            server,
            out_path.display(),
            t.script_text.len()
        );
        saved.push(out_path);
    }

    Ok(saved)
}

fn build_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(90))
        .user_agent("Dalvik/2.1.0 (Linux; U; Android 13; Pixel 6 Build/TQ2A.230505.002)")
        .build()
        .map_err(crate::error::Error::Http)
}
