//! Rijndael-256 CBC encrypt/decrypt wrappers.
//!
//! Block size: 256 bits = 32 bytes (NOT standard AES-256 which uses 128-bit blocks).
//! Key size: 256 bits = 32 bytes.
//! Mode: CBC with PKCS7 padding (padded to 32-byte boundary).

use simple_rijndael::impls::RijndaelCbc;
use simple_rijndael::paddings::{Padding, Pkcs7Padding};

const BLOCK_SIZE: usize = 32;

// simple-rijndael 0.3's padding decoder trusts the last byte and can panic on
// malformed ciphertext. Keep its cipher, validate padding before truncating.
struct CheckedPadding(usize);
impl Padding for CheckedPadding {
    fn new(size: usize) -> Self {
        Self(size)
    }
    fn size(&self) -> usize {
        self.0
    }
    fn encode(&self, data: Vec<u8>) -> Vec<u8> {
        Pkcs7Padding::new(self.0).encode(data)
    }
    fn decode(&self, mut data: Vec<u8>) -> Result<Vec<u8>, simple_rijndael::Errors> {
        let count = usize::from(
            *data
                .last()
                .ok_or(simple_rijndael::Errors::InvalidDataSize)?,
        );
        if count == 0
            || count > self.0
            || count > data.len()
            || !data.len().is_multiple_of(self.0)
            || !data[data.len() - count..]
                .iter()
                .all(|b| usize::from(*b) == count)
        {
            return Err(simple_rijndael::Errors::InvalidDataSize);
        }
        data.truncate(data.len() - count);
        Ok(data)
    }
}

/// Rijndael-256 CBC decrypt.
///
/// `key`: 32 bytes
/// `iv`: 32 bytes
/// `data`: ciphertext (must be padded to 32-byte boundary)
pub fn decrypt(data: &[u8], key: &[u8; 32], iv: &[u8; 32]) -> Result<Vec<u8>, String> {
    if data.is_empty() || !data.len().is_multiple_of(BLOCK_SIZE) {
        return Err("invalid ciphertext block length".into());
    }
    let cipher = RijndaelCbc::<CheckedPadding>::new(key, BLOCK_SIZE)
        .map_err(|e| format!("rijndael init: {e:?}"))?;
    cipher
        .decrypt(iv, data.to_vec())
        .map_err(|e| format!("rijndael decrypt: {e:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_padding_is_rejected_without_panicking() {
        for data in [vec![], vec![0; 32], vec![255; 32], vec![1; 31]] {
            assert!(CheckedPadding(32).decode(data).is_err());
        }
        let mut invalid = vec![3; 32];
        invalid[31] = 2;
        assert!(CheckedPadding(32).decode(invalid).is_err());
        assert_eq!(
            CheckedPadding(32).decode(vec![32; 32]).unwrap(),
            Vec::<u8>::new()
        );
        assert!(decrypt(&[], &[0; 32], &[0; 32]).is_err());
    }
}
