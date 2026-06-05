//! Script text encryption/decryption (MouseGame3 / CatGame3).
//!
//! Decrypt chain:
//!   Base64Decode → Rijndael-256-CBC Decrypt → Decompress(GZip/BZip2) → ByteNOT → UTF-8
//!
//! Encrypt chain:
//!   UTF-8 → ByteNOT → Compress(GZip/BZip2) → Rijndael-256-CBC Encrypt → Base64Encode

use crate::crypto::rijndael;
use std::io::Read;

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

