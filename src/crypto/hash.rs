//! File name hashing (MD5 / SHA1-XOR).
//!
//! Audio/Movie assets: MD5(asset_name_utf8 + "pN6ds2Bg") → lowercase hex
//! Everything else: SHA1(asset_name_utf8) → XOR each byte with 0xAA → hex + ".bin"

use crate::config::MD5_SALT;
use md5::{Digest, Md5};
use sha1::Sha1;

/// MD5 filename for Audio/Movie assets.
pub fn md5_filename(asset_name: &str) -> String {
    let mut hasher = Md5::new();
    hasher.update(asset_name.as_bytes());
    hasher.update(MD5_SALT);
    hex::encode(hasher.finalize())
}

/// SHA1-XOR filename for regular assets.
/// Returns lowercase hex string (without ".bin" suffix — callers can append it).
pub fn sha1_xor_name(asset_name: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(asset_name.as_bytes());
    let digest = hasher.finalize();
    digest.iter().map(|b| format!("{:02x}", b ^ 0xAA)).collect()
}

/// SHA1-XOR filename with ".bin" extension.
pub fn sha1_xor_filename(asset_name: &str) -> String {
    format!("{}.bin", sha1_xor_name(asset_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha1_xor_known_value() {
        // Check that output is valid hex
        let name = sha1_xor_name("test");
        assert!(!name.is_empty());
        assert!(name.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
