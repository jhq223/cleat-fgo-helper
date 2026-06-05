//! Rijndael-256 CBC encrypt/decrypt wrappers.
//!
//! Block size: 256 bits = 32 bytes (NOT standard AES-256 which uses 128-bit blocks).
//! Key size: 256 bits = 32 bytes.
//! Mode: CBC with PKCS7 padding (padded to 32-byte boundary).

use simple_rijndael::impls::RijndaelCbc;
use simple_rijndael::paddings::Pkcs7Padding;

const BLOCK_SIZE: usize = 32;

/// Rijndael-256 CBC decrypt.
///
/// `key`: 32 bytes
/// `iv`: 32 bytes
/// `data`: ciphertext (must be padded to 32-byte boundary)
pub fn decrypt(data: &[u8], key: &[u8; 32], iv: &[u8; 32]) -> Result<Vec<u8>, String> {
    let cipher = RijndaelCbc::<Pkcs7Padding>::new(key, BLOCK_SIZE)
        .map_err(|e| format!("rijndael init: {e:?}"))?;
    cipher
        .decrypt(iv, data.to_vec())
        .map_err(|e| format!("rijndael decrypt: {e:?}"))
}

