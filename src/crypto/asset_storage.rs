//! AssetStorage encryption/decryption (MouseGame8).
//!
//! Chain: Base64 → Rijndael-256-CBC Decrypt → Auto-Detect Decompress → ByteNOT → UTF-8

use crate::crypto::rijndael;
use std::io::Read;

/// Decrypt AssetStorage.txt content.
///
/// `base64_text`: raw response body from server
/// `stage_data`: 32-byte IV
/// `stage_top`: 32-byte key
pub fn decrypt(
    base64_text: &str,
    stage_data: &[u8; 32],
    stage_top: &[u8; 32],
) -> Result<String, String> {
    use base64::Engine;

    let raw = base64::engine::general_purpose::STANDARD
        .decode(base64_text)
        .map_err(|e| format!("base64 decode: {e}"))?;

    let plain = rijndael::decrypt(&raw, stage_data, stage_top)?;

    // Auto-detect compression
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

    // Byte NOT (bitwise invert)
    let inverted: Vec<u8> = decompressed.iter().map(|&b| !b).collect();

    String::from_utf8(inverted)
        .map(|s| s.trim_end_matches('\0').to_string())
        .map_err(|e| format!("utf-8 decode: {e}"))
}

/// Encrypt text → AssetStorage format (for re-upload / modding).
pub fn encrypt(
    plain_text: &str,
    stage_data: &[u8; 32],
    stage_top: &[u8; 32],
) -> Result<String, String> {
    use base64::Engine;
    use std::io::Write;

    // Byte NOT
    let inverted: Vec<u8> = plain_text.as_bytes().iter().map(|&b| !b).collect();

    // GZip compress
    let mut compressed = Vec::new();
    let mut encoder = flate2::write::GzEncoder::new(&mut compressed, flate2::Compression::best());
    encoder
        .write_all(&inverted)
        .map_err(|e| format!("gzip compress: {e}"))?;
    encoder.finish().map_err(|e| format!("gzip finish: {e}"))?;

    // Rijndael-256-CBC encrypt
    let encrypted = rijndael::encrypt(&compressed, stage_data, stage_top)?;

    Ok(base64::engine::general_purpose::STANDARD.encode(&encrypted))
}
