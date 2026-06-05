//! Version info fetching (JP: gamedata/top, CN: multi-step).

use crate::config::{get_config, CN_CONFIG, JP_CONFIG};
use crate::crypto::gamedata::{self, GameData};
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// JP server data cache (mirrors C# ServerDataJP.json).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct JpCache {
    #[serde(rename = "AppVer", default)]
    app_ver: String,
    #[serde(rename = "DataVer", default)]
    data_ver: String,
    #[serde(rename = "DateVer", default)]
    date_ver: String,
    #[serde(rename = "AssetBundleFolder", default)]
    asset_bundle_folder: String,
}

/// Fetched version info.
#[derive(Debug, Clone)]
pub struct VersionInfo {
    pub app_ver: String,
    pub folder_name: String,
    pub data_ver: String,
    /// JP only: date version (Unix timestamp)
    pub date_ver: String,
    /// CN only: CDN address
    pub cdn_addr: Option<String>,
    /// CN only: Server address
    pub ser_addr: Option<String>,
    /// CN only: assetStorageVersion
    pub asset_storage_version: Option<String>,
}

/// Fetch version info for a server.
pub async fn fetch(client: &reqwest::Client, server: &str) -> Result<VersionInfo> {
    match get_config(server).server_type {
        "jp" => fetch_jp(client).await,
        "cn" => fetch_cn(client).await,
        _ => Err(Error::UnsupportedServer(server.to_string())),
    }
}

fn jp_cache_path() -> PathBuf {
    PathBuf::from("data").join("jp").join("ServerDataJP.json")
}

fn load_jp_cache() -> JpCache {
    let path = jp_cache_path();
    if path.exists() {
        match std::fs::read_to_string(&path) {
            Ok(json) => match serde_json::from_str::<JpCache>(&json) {
                Ok(cache) => {
                    log::info!(
                        "[JP] Loaded cache: appVer={}, dataVer={}, dateVer={}",
                        cache.app_ver,
                        cache.data_ver,
                        cache.date_ver
                    );
                    return cache;
                }
                Err(e) => log::warn!("[JP] Failed to parse cache: {e}"),
            },
            Err(e) => log::warn!("[JP] Failed to read cache: {e}"),
        }
    }
    JpCache::default()
}

fn save_jp_cache(cache: &JpCache) {
    let path = jp_cache_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(cache) {
        let _ = std::fs::write(&path, json);
        log::debug!("[JP] Saved cache");
    }
}

async fn fetch_jp(client: &reqwest::Client) -> Result<VersionInfo> {
    log::info!("[JP] Fetching version info...");

    let mut cache = load_jp_cache();
    let base_url = JP_CONFIG
        .version_url
        .split('?')
        .next()
        .unwrap_or(JP_CONFIG.version_url);

    // Build URL: include cached dataVer/dateVer to avoid slow CDN redirect
    let app_ver = if cache.app_ver.is_empty() {
        "0.0".to_string()
    } else {
        cache.app_ver.clone()
    };
    let mut url = format!("{base_url}?appVer={app_ver}");
    if !cache.data_ver.is_empty() || !cache.date_ver.is_empty() {
        url.push_str(&format!(
            "&dataVer={}&dateVer={}",
            cache.data_ver, cache.date_ver
        ));
    }

    log::debug!("[JP] Request: {url}");

    // Use a client with longer timeout for JP (CDN redirect can be slow)
    let resp = client.get(&url).send().await?;
    let mut data: serde_json::Value = resp.json().await?;

    let mut response = &data["response"][0];

    // Handle app_version_up
    if let Some(fail) = response.get("fail") {
        if fail
            .get("action")
            .map_or(false, |a| a == "app_version_up")
        {
            let detail = fail["detail"].as_str().unwrap_or("");
            let new_ver = if let Some(re) = JP_CONFIG.version_regex {
                let re = regex::Regex::new(re).unwrap();
                re.captures(detail)
                    .and_then(|caps| caps.get(1))
                    .map(|m| m.as_str().to_string())
                    .ok_or_else(|| {
                        Error::Version(format!("Cannot parse version from: {detail}"))
                    })?
            } else {
                return Err(Error::Version("No version regex configured".into()));
            };

            log::info!("[JP] Version upgrade: {new_ver}");
            cache.app_ver = new_ver;
            cache.data_ver.clear(); // force re-download
            cache.date_ver.clear();
            save_jp_cache(&cache);

            let new_url = format!("{base_url}?appVer={}", cache.app_ver);
            data = client.get(&new_url).send().await?.json().await?;
            response = &data["response"][0];
        }
    }

    // Check success or handle redirect-based response
    if response.get("success").is_none() {
        return Err(Error::Version(format!(
            "Unexpected JP response keys: {:?}",
            response
                .as_object()
                .map(|o| o.keys().collect::<Vec<_>>())
        )));
    }

    let success = response["success"]
        .as_object()
        .ok_or_else(|| Error::Version("No success object".into()))?;

    // Extract server-side dataVer/dateVer (numbers)
    let sv_data_ver = success["dataVer"]
        .as_i64()
        .map(|n| n.to_string())
        .unwrap_or_default();
    let sv_date_ver = success["dateVer"]
        .as_i64()
        .map(|n| n.to_string())
        .unwrap_or_default();

    // Check if data changed — if same, use cached folderName (fast path)
    if sv_data_ver == cache.data_ver
        && sv_date_ver == cache.date_ver
        && !cache.asset_bundle_folder.is_empty()
    {
        log::info!(
            "[JP] Data unchanged (dataVer={}, dateVer={}), using cached folderName: {}",
            sv_data_ver,
            sv_date_ver,
            cache.asset_bundle_folder
        );
        return Ok(VersionInfo {
            app_ver: cache.app_ver,
            folder_name: cache.asset_bundle_folder.clone(),
            data_ver: sv_data_ver,
            date_ver: sv_date_ver,
            cdn_addr: None,
            ser_addr: None,
            asset_storage_version: None,
        });
    }

    // Data changed — decrypt assetbundle to get folderName
    log::info!(
        "[JP] Data changed: dataVer {}→{}, dateVer {}→{}",
        cache.data_ver,
        sv_data_ver,
        cache.date_ver,
        sv_date_ver
    );

    let assetbundle_b64 = success["assetbundle"]
        .as_str()
        .ok_or_else(|| Error::Version("No assetbundle field".into()))?;

    let gd: GameData =
        gamedata::unpack(assetbundle_b64).map_err(|e| Error::Crypto(e))?;

    log::info!("[JP] folderName: {}", gd.folder_name);

    // Update cache
    cache.data_ver = sv_data_ver;
    cache.date_ver = sv_date_ver;
    cache.asset_bundle_folder = gd.folder_name.clone();
    save_jp_cache(&cache);

    Ok(VersionInfo {
        app_ver: cache.app_ver,
        folder_name: gd.folder_name,
        data_ver: gd.data_ver,
        date_ver: cache.date_ver,
        cdn_addr: None,
        ser_addr: None,
        asset_storage_version: None,
    })
}

#[derive(Debug, Deserialize)]
struct BiligameResponse {
    data: BiligameData,
}

#[derive(Debug, Deserialize)]
struct BiligameData {
    android_version: String,
}

#[derive(Debug, Deserialize)]
struct NetworkConfig {
    list: Vec<NetworkConfigList>,
}

#[derive(Debug, Deserialize)]
struct NetworkConfigList {
    #[serde(rename = "androidSer")]
    android_ser: Vec<String>,
    cdn: Vec<String>,
}

async fn fetch_cn(client: &reqwest::Client) -> Result<VersionInfo> {
    log::info!("[CN] Fetching version info...");

    // Step 1: Get android_version from biligame API
    log::info!("[CN] Step 1: Getting android_version...");
    let resp: BiligameResponse = client
        .get(CN_CONFIG.version_url)
        .send()
        .await?
        .json()
        .await?;
    let cn_version = resp.data.android_version;
    log::info!("[CN] android_version: {cn_version}");

    // Step 2: Get network config
    log::info!("[CN] Step 2: Getting network config...");
    let nc_url = CN_CONFIG
        .network_config_tpl
        .as_ref()
        .unwrap()
        .replace("{version}", &cn_version);
    let nc: NetworkConfig = client.get(&nc_url).send().await?.json().await?;
    let ser_addr = &nc.list[0].android_ser[0];
    let cdn_addr = &nc.list[0].cdn[0];
    log::info!(
        "[CN] ser: {}...",
        &ser_addr[..50.min(ser_addr.len())]
    );
    log::info!("[CN] cdn: {cdn_addr}");

    // Step 3: member.php — response is URL-encoded → base64 → JSON
    log::info!("[CN] Step 3: Getting game data...");
    let gd_url =
        format!("{ser_addr}/rongame_beta/rgfate/60_member/member.php?appVer={cn_version}");
    let resp_text = client.get(&gd_url).send().await?.text().await?;

    // URL decode → base64 decode → UTF-8 → JSON
    let decoded =
        urlencoding::decode(&resp_text).map_err(|e| Error::Version(format!("url decode: {e}")))?;

    use base64::Engine;
    let raw = base64::engine::general_purpose::STANDARD
        .decode(decoded.as_bytes())
        .map_err(|e| Error::Version(format!("base64 decode: {e}")))?;

    let raw_str =
        String::from_utf8(raw).map_err(|e| Error::Version(format!("utf-8: {e}")))?;

    let gd: serde_json::Value = serde_json::from_str(&raw_str)
        .map_err(|e| Error::Version(format!("json parse: {e}")))?;

    // CN response: response[0].success — values may be string or number
    let success = gd["response"][0]["success"]
        .as_object()
        .ok_or_else(|| Error::Version("No success object in CN response".into()))?;

    fn val_to_string(v: &serde_json::Value) -> String {
        v.as_str()
            .map(String::from)
            .or_else(|| v.as_i64().map(|n| n.to_string()))
            .or_else(|| v.as_f64().map(|n| n.to_string()))
            .unwrap_or_default()
    }

    let data_ver = success
        .get("version")
        .map(val_to_string)
        .unwrap_or_default();
    let asset_storage_version = success.get("assetStorageVersion").map(val_to_string);

    log::info!(
        "[CN] dataVer: {}, assetStorageVer: {:?}",
        data_ver,
        asset_storage_version
    );

    Ok(VersionInfo {
        app_ver: cn_version.clone(),
        folder_name: String::new(),
        data_ver,
        date_ver: String::new(),
        cdn_addr: Some(cdn_addr.clone()),
        ser_addr: Some(ser_addr.clone()),
        asset_storage_version,
    })
}
