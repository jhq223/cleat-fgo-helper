//! Key derivation functions.
//!
//! Each server has two 64-byte source strings. Each is split into two
//! 32-byte keys (data/IV pair). The split pattern differs by server
//! and key type.
//!
//! JP:
//!   stage keys → byte interleave (odd/even → top/data)
//!   base keys  → 4-byte block interleave
//! CN:
//!   stage keys → 4-byte block interleave
//!   base keys  → byte interleave

/// Byte interleave: even → data, odd → top
pub fn derive_byte_interleave(src: &[u8; 64]) -> ([u8; 32], [u8; 32]) {
    let mut data = [0u8; 32];
    let mut top = [0u8; 32];
    for i in 0..64 {
        if i % 2 == 0 {
            data[i / 2] = src[i];
        } else {
            top[i / 2] = src[i];
        }
    }
    (data, top)
}

/// 4-byte block interleave: even blocks → data, odd blocks → top
pub fn derive_4byte_interleave(src: &[u8; 64]) -> ([u8; 32], [u8; 32]) {
    let mut data = [0u8; 32];
    let mut top = [0u8; 32];
    // 64 bytes = 16 blocks of 4
    for block_idx in 0..16 {
        let start = block_idx * 4;
        let chunk = &src[start..start + 4];
        if block_idx % 2 == 0 {
            let dst_start = (block_idx / 2) * 4;
            data[dst_start..dst_start + 4].copy_from_slice(chunk);
        } else {
            let dst_start = (block_idx / 2) * 4;
            top[dst_start..dst_start + 4].copy_from_slice(chunk);
        }
    }
    (data, top)
}

/// Split extra_key (decryptKey UTF-8 bytes) into home (IV) and info (key).
/// home[0] = first byte, info[1..] = remaining bytes (up to 31).
pub fn split_extra_key(key_str: &str) -> ([u8; 32], [u8; 32]) {
    let key_bytes = key_str.as_bytes();
    let mut home = [0u8; 32];
    let mut info = [0u8; 32];

    if !key_bytes.is_empty() {
        home[0] = key_bytes[0];
    }
    let copy_len = (key_bytes.len() - 1).min(31);
    info[1..1 + copy_len].copy_from_slice(&key_bytes[1..1 + copy_len]);

    (home, info)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_byte_interleave() {
        let src: [u8; 64] = [
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45,
            46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63,
        ];
        let (data, top) = derive_byte_interleave(&src);
        // data gets even indices
        assert_eq!(data[0], 0);
        assert_eq!(data[1], 2);
        // top gets odd indices
        assert_eq!(top[0], 1);
        assert_eq!(top[1], 3);
    }

    #[test]
    fn test_4byte_interleave() {
        let mut src = [0u8; 64];
        for i in 0..64 {
            src[i] = i as u8;
        }
        let (data, top) = derive_4byte_interleave(&src);
        // block 0 (bytes 0-3) → data[0..4]
        assert_eq!(&data[0..4], &[0, 1, 2, 3]);
        // block 1 (bytes 4-7) → top[0..4]
        assert_eq!(&top[0..4], &[4, 5, 6, 7]);
    }
}
