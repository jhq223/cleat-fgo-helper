//! Script text encryption/decryption (MouseGame3 / CatGame3).
//!
//! Decrypt chain:
//!   Base64Decode → Rijndael-256-CBC Decrypt → Decompress(GZip/BZip2) → ByteNOT → UTF-8
//!
//! Encrypt chain:
//!   UTF-8 → ByteNOT → Compress(GZip/BZip2) → Rijndael-256-CBC Encrypt → Base64Encode

use crate::crypto::rijndael;
use serde::Deserialize;
use std::io::Read;

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct KeyEntry {
    pub id: String,
    #[serde(alias = "decryptKey")]
    pub decrypt_key: String,
}

pub fn derive_key_iv(seed_str: &str) -> ([u8; 32], [u8; 32]) {
    let seed_bytes = seed_str.as_bytes();
    let mut key = [0u8; 32];
    let mut iv = [0u8; 32];

    if !seed_bytes.is_empty() {
        key[0] = seed_bytes[0];
    }

    let limit = std::cmp::min(32, seed_bytes.len());
    iv[1..limit].copy_from_slice(&seed_bytes[1..limit]);

    (key, iv)
}

pub fn decrypt_new(base64_text: &str, key: &[u8; 32], iv: &[u8; 32]) -> Result<String, String> {
    use base64::Engine;

    let raw = base64::engine::general_purpose::STANDARD
        .decode(base64_text.trim())
        .map_err(|e| format!("base64 decode: {e}"))?;

    let plain = rijndael::decrypt(&raw, key, iv)?;

    let decompressed = if plain.starts_with(b"BZh") {
        let mut dec = Vec::new();
        let mut decoder = bzip2::read::BzDecoder::new(&plain[..]);
        decoder
            .read_to_end(&mut dec)
            .map_err(|e| format!("bzip2 decompress: {e}"))?;
        dec
    } else if plain.starts_with(&[0x1f, 0x8b, 0x08]) {
        let mut dec = Vec::new();
        let mut decoder = flate2::read::GzDecoder::new(&plain[..]);
        decoder
            .read_to_end(&mut dec)
            .map_err(|e| format!("gzip decompress: {e}"))?;
        dec
    } else {
        plain
    };

    String::from_utf8(decompressed)
        .map(|s| s.trim_end_matches('\0').to_string())
        .map_err(|e| format!("utf-8 decode: {e}"))
}

pub fn decrypt_with_keys(base64_text: &str, keys: &[KeyEntry]) -> Option<String> {
    for entry in keys {
        let (key, iv) = derive_key_iv(&entry.decrypt_key);
        if let Ok(decrypted) = decrypt_new(base64_text, &key, &iv) {
            if !decrypted.is_empty()
                && (decrypted.contains('{')
                    || decrypted.contains('"')
                    || decrypted.lines().count() > 1
                    || decrypted.chars().any(|c| c.is_alphabetic()))
            {
                return Some(decrypted);
            }
        }
    }
    None
}

/// Decrypt a base64-encoded script text.
pub fn decrypt(
    base64_text: &str,
    stage_data: &[u8; 32],
    stage_top: &[u8; 32],
    use_bzip2: bool,
) -> Result<String, String> {
    use base64::Engine;

    let raw = base64::engine::general_purpose::STANDARD
        .decode(base64_text)
        .map_err(|e| format!("base64 decode: {e}"))?;

    let plain = rijndael::decrypt(&raw, stage_data, stage_top)?;

    // Auto-detect or forced compression
    let decompressed = if use_bzip2 || plain.starts_with(b"BZh") {
        let mut dec = Vec::new();
        let mut decoder = bzip2::read::BzDecoder::new(&plain[..]);
        decoder
            .read_to_end(&mut dec)
            .map_err(|e| format!("bzip2 decompress: {e}"))?;
        dec
    } else if plain.starts_with(&[0x1f, 0x8b, 0x08]) {
        let mut dec = Vec::new();
        let mut decoder = flate2::read::GzDecoder::new(&plain[..]);
        decoder
            .read_to_end(&mut dec)
            .map_err(|e| format!("gzip decompress: {e}"))?;
        dec
    } else {
        plain
    };

    // Byte NOT
    let inverted: Vec<u8> = decompressed.iter().map(|&b| !b).collect();

    String::from_utf8(inverted)
        .map(|s| s.trim_end_matches('\0').to_string())
        .map_err(|e| format!("utf-8 decode: {e}"))
}
