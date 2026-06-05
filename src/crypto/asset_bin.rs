//! AssetBundle .bin encryption/decryption (MouseGame4 / CatGame4).
//!
//! Normal path:
//!   Decrypt: Rijndael-256-CBC Decrypt → Byte Swap + XOR → AssetBundle bytes
//!   Encrypt: AssetBundle bytes → Byte Swap + XOR → Rijndael-256-CBC Encrypt
//!
//! Extra key path:
//!   Decrypt: Rijndael-256-CBC Decrypt (with derived key) → AssetBundle bytes
//!   (no byte swap/xor post-processing)

use crate::crypto::{keys, rijndael};
use crate::error::Result;

/// Decrypt an encrypted AssetBundle .bin file.
///
/// `extra_key`: if Some, uses the extra key derivation path (no byte swap/xor).
pub fn decrypt(
    data: &[u8],
    base_data: &[u8; 32],
    base_top: &[u8; 32],
    extra_key: Option<&str>,
) -> Result<Vec<u8>> {
    if let Some(ek) = extra_key {
        return decrypt_with_extra_key(data, ek);
    }
    decrypt_normal(data, base_data, base_top)
}

/// Normal decrypt: Rijndael CBC → Byte Swap + XOR.
fn decrypt_normal(data: &[u8], base_data: &[u8; 32], base_top: &[u8; 32]) -> Result<Vec<u8>> {
    let decrypted =
        rijndael::decrypt(data, base_data, base_top).map_err(|e| crate::error::Error::Crypto(e))?;

    // Byte swap + XOR
    let mut result = decrypted;
    for i in (0..result.len().saturating_sub(1)).step_by(2) {
        let b1 = result[i];
        let b2 = result[i + 1];
        result[i] = b2 ^ 0xD2; // 210
        result[i + 1] = b1 ^ 0xCE; // 206
    }

    Ok(result)
}

/// Encrypt AssetBundle bytes → .bin format.
pub fn encrypt(data: &[u8], base_data: &[u8; 32], base_top: &[u8; 32]) -> Result<Vec<u8>> {
    // Byte swap + XOR (symmetric operation, different constants for encrypt)
    let mut xored = data.to_vec();
    for i in (0..xored.len().saturating_sub(1)).step_by(2) {
        let b1 = xored[i];
        let b2 = xored[i + 1];
        xored[i] = b2 ^ 0xCE;
        xored[i + 1] = b1 ^ 0xD2;
    }

    rijndael::encrypt(&xored, base_data, base_top).map_err(|e| crate::error::Error::Crypto(e))
}

/// Decrypt using extra key: split key → Rijndael CBC (no post-processing).
fn decrypt_with_extra_key(data: &[u8], key_str: &str) -> Result<Vec<u8>> {
    let (home, info) = keys::split_extra_key(key_str);
    // C# MouseHomeMain(data, home=KEY, info=IV)
    rijndael::decrypt(data, &home, &info).map_err(|e| crate::error::Error::Crypto(e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_byte_swap_xor_roundtrip() {
        let original = b"Hello, World! This is a test.";
        let base_data = [0x42u8; 32];
        let base_top = [0x17u8; 32];

        let encrypted = encrypt(original, &base_data, &base_top).unwrap();
        let decrypted = decrypt(&encrypted, &base_data, &base_top, None).unwrap();
        assert_eq!(&original[..], &decrypted[..original.len()]);
    }
}
