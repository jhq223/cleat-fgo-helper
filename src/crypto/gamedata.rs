//! Game metadata decryption (MsgPack format).
//!
//! Used for the `assetbundle` field in version responses.
//!
//! data layout: [IV: 32 bytes][body: encrypted bytes]
//! key = ASSET_BUNDLE_KEY (32 bytes UTF-8)
//! Chain: Base64Decode → Rijndael-256-CBC Decrypt → GZip → MsgPack

use crate::config::ASSET_BUNDLE_KEY;
use crate::crypto::rijndael;
use serde::{Deserialize, Serialize};
use std::io::Read;

/// Parsed version info from gamedata MsgPack.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct GameData {
    #[serde(rename = "appVer", default)]
    pub app_ver: String,
    #[serde(rename = "folderName", default)]
    pub folder_name: String,
    #[serde(rename = "dataVer", default)]
    pub data_ver: String,
}

/// Decrypt and unpack base64-encoded gamedata (MsgPack).
pub fn unpack(base64_data: &str) -> Result<GameData, String> {
    use base64::Engine;

    let raw = base64::engine::general_purpose::STANDARD
        .decode(base64_data)
        .map_err(|e| format!("base64 decode: {e}"))?;

    if raw.len() < 32 {
        return Err("gamedata too short".into());
    }

    let info_top: [u8; 32] = raw[..32].try_into().unwrap();
    let body = &raw[32..];

    let decrypted = rijndael::decrypt(body, ASSET_BUNDLE_KEY, &info_top)?;

    let mut decompressed = Vec::new();
    let mut decoder = flate2::read::GzDecoder::new(&decrypted[..]);
    decoder
        .read_to_end(&mut decompressed)
        .map_err(|e| format!("gzip decompress: {e}"))?;

    rmp_serde::from_slice(&decompressed).map_err(|e| format!("msgpack decode: {e}"))
}

/// Unpack assetbundleKey list (array of {id, decryptKey}).
pub fn unpack_key_list(base64_data: &str) -> Result<Vec<AssetBundleKey>, String> {
    use base64::Engine;

    let raw = base64::engine::general_purpose::STANDARD
        .decode(base64_data)
        .map_err(|e| format!("base64 decode: {e}"))?;

    if raw.len() < 32 {
        return Err("key list too short".into());
    }

    let info_top: [u8; 32] = raw[..32].try_into().unwrap();
    let body = &raw[32..];

    let decrypted = rijndael::decrypt(body, ASSET_BUNDLE_KEY, &info_top)?;

    let mut decompressed = Vec::new();
    let mut decoder = flate2::read::GzDecoder::new(&decrypted[..]);
    decoder
        .read_to_end(&mut decompressed)
        .map_err(|e| format!("gzip decompress: {e}"))?;

    rmp_serde::from_slice(&decompressed).map_err(|e| format!("msgpack decode: {e}"))
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AssetBundleKey {
    pub id: String,
    #[serde(rename = "decryptKey")]
    pub decrypt_key: String,
}
