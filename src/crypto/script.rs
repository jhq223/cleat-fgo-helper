//! Script codecs. The caller resolves each script's key ID from its bundle.
use crate::crypto::{keys, rijndael};
use base64::Engine;
use std::io::Read;

pub const MAX_SCRIPT: usize = 8 * 1024 * 1024;

pub fn read_bounded(reader: impl Read, limit: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("decompress: {e}"))?;
    if bytes.len() > limit {
        return Err("decompressed resource exceeds size limit".into());
    }
    Ok(bytes)
}

pub fn decompress(plain: Vec<u8>, limit: usize) -> Result<Vec<u8>, String> {
    if plain.starts_with(b"BZh") {
        read_bounded(bzip2::read::BzDecoder::new(&plain[..]), limit)
    } else if plain.starts_with(&[0x1f, 0x8b, 0x08]) {
        read_bounded(flate2::read::GzDecoder::new(&plain[..]), limit)
    } else if plain.len() <= limit {
        Ok(plain)
    } else {
        Err("resource exceeds size limit".into())
    }
}

pub fn decode_base64(text: &str, limit: usize) -> Result<Vec<u8>, String> {
    if text.len() > limit.saturating_mul(4) / 3 + 4 {
        return Err("encoded resource exceeds size limit".into());
    }
    base64::engine::general_purpose::STANDARD
        .decode(text.trim())
        .map_err(|e| format!("base64: {e}"))
}

pub fn decrypt_with_key(text: &str, seed: &str) -> Result<String, String> {
    if seed.is_empty() {
        return Err("empty script key".into());
    }
    let (key, iv) = keys::split_extra_key(seed);
    decode(text, &key, &iv, false, MAX_SCRIPT)
}

/// Legacy scripts invert bytes after decompression. Detect compression by header.
pub fn decrypt(
    text: &str,
    key: &[u8; 32],
    iv: &[u8; 32],
    _use_bzip2: bool,
) -> Result<String, String> {
    decode(text, key, iv, true, MAX_SCRIPT)
}

pub fn decode(
    text: &str,
    key: &[u8; 32],
    iv: &[u8; 32],
    invert: bool,
    limit: usize,
) -> Result<String, String> {
    let encrypted = decode_base64(text, limit)?;
    let mut bytes = decompress(rijndael::decrypt(&encrypted, key, iv)?, limit)?;
    if invert {
        for byte in &mut bytes {
            *byte = !*byte;
        }
    }
    String::from_utf8(bytes)
        .map(|s| s.trim_end_matches('\0').to_owned())
        .map_err(|e| format!("UTF-8: {e}"))
}

pub fn validate(name: &str, text: &str) -> Result<(), String> {
    let body = text.trim_start_matches('\u{feff}').trim();
    if body.is_empty() || text.contains('\0') {
        return Err(format!("{name}: empty or binary plaintext"));
    }
    let valid = if name == "ScriptFileList" {
        body.lines().all(|line| {
            let line = line.trim();
            line.ends_with(".txt") && !line.contains(['/', '\\', ':']) && !line.starts_with('.')
        })
    } else {
        (9..=10).contains(&name.len())
            && name.bytes().all(|b| b.is_ascii_digit())
            && (body.starts_with(['＄', '$']) || body.starts_with("[soundStopAll]"))
    };
    if valid {
        Ok(())
    } else {
        Err(format!("{name}: plaintext is not a valid script"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use simple_rijndael::{impls::RijndaelCbc, paddings::Pkcs7Padding};
    use std::io::Write;
    #[test]
    fn legacy_and_new_scripts_use_explicit_codecs() {
        let text = "＄test\n＠测试\n译文[k]\n";
        for invert in [false, true] {
            let key = [7; 32];
            let iv = [3; 32];
            let bytes: Vec<u8> = text.bytes().map(|b| if invert { !b } else { b }).collect();
            let mut gzip =
                flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            gzip.write_all(&bytes).unwrap();
            let encrypted = RijndaelCbc::<Pkcs7Padding>::new(&key, 32)
                .unwrap()
                .encrypt(&iv, gzip.finish().unwrap())
                .unwrap();
            let base64 = base64::engine::general_purpose::STANDARD.encode(encrypted);
            assert_eq!(
                decode(&base64, &key, &iv, invert, MAX_SCRIPT).unwrap(),
                text
            );
            assert!(decode(&base64, &key, &iv, invert, 2).is_err());
        }
        assert!(validate("0100010010", "abcdefghijklmnop").is_err());
        assert!(validate("0100010010", text).is_ok());
        assert!(validate("ScriptFileList", "../escape.txt").is_err());
    }
}
