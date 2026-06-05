//! AssetStorage CSV parser.
//!
//! Parses AssetStorage_dec.txt into structured asset entries.
//!
//! JP format (comma-separated):
//!   type, hash, size, flag, asset_path [, extra_key]
//!
//! CN format (7 columns):
//!   hash, name, size, state, path, ?, ?

use crate::crypto::hash;
use std::collections::HashMap;
use std::path::Path;

/// A single asset entry from AssetStorage.
#[derive(Debug, Clone)]
pub struct AssetEntry {
    pub asset_path: String,
    pub file_name: String,
    pub size: u64,
    pub extra_key: Option<String>,
}

/// Parse AssetStorage CSV file.
/// `server_type`: "jp" or "cn"
pub fn parse(path: &Path, server_type: &str) -> Result<Vec<AssetEntry>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("read: {e}"))?;
    let entries = parse_text(&text, server_type)?;
    Ok(entries)
}

/// Parse AssetStorage text content.
pub fn parse_text(text: &str, server_type: &str) -> Result<Vec<AssetEntry>, String> {
    let mut assets = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.split(',').collect();

        if server_type == "cn" {
            // CN: hash, name, size, state, path, ?, ?
            if parts.len() < 5 {
                continue;
            }
            let asset_path = parts[4].to_string();
            let file_name = format!("{}.bin", parts[0]);
            let size = parts[2].parse::<u64>().unwrap_or(0);

            assets.push(AssetEntry {
                asset_path,
                file_name,
                size,
                extra_key: None,
            });
        } else {
            // JP: type, hash, size, flag, asset_path [, extra_key]
            if parts.len() < 5 {
                continue;
            }
            let asset_type = parts[0];
            if asset_type != "1" {
                continue; // Only type=1 assets
            }

            let asset_path = parts[4].to_string();
            let extra_key = if parts.len() >= 6 && !parts[5].is_empty() {
                Some(parts[5].to_string())
            } else {
                None
            };

            let asset_name = asset_path.replace('/', "@");
            let file_name = if asset_path.contains("Audio") || asset_path.contains("Movie") {
                hash::md5_filename(&asset_name)
            } else {
                hash::sha1_xor_filename(&format!("{asset_name}.unity3d"))
            };

            assets.push(AssetEntry {
                asset_path,
                file_name,
                size: parts[2].parse::<u64>().unwrap_or(0),
                extra_key,
            });
        }
    }

    Ok(assets)
}

/// Filter assets to only script-relevant entries.
pub fn find_script_assets(assets: &[AssetEntry]) -> Vec<&AssetEntry> {
    assets
        .iter()
        .filter(|a| {
            a.asset_path.starts_with("Script/") || a.asset_path.starts_with("ScriptActionEncrypt/")
        })
        .collect()
}

/// Build extra_key mapping from assetbundleKey.json.
pub fn load_extra_keys(path: &Path) -> Result<HashMap<String, String>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("read: {e}"))?;

    #[derive(serde::Deserialize)]
    struct KeyEntry {
        id: String,
        #[serde(rename = "decryptKey")]
        decrypt_key: String,
    }

    let list: Vec<KeyEntry> = serde_json::from_str(&text).map_err(|e| format!("json: {e}"))?;

    Ok(list.into_iter().map(|k| (k.id, k.decrypt_key)).collect())
}
