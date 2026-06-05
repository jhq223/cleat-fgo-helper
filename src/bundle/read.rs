//! .script Bundle binary reader — parses FGO TextBundle format.
//!
//! ## Binary format (Big Endian)
//!
//! ```text
//! ┌─ Header
//! │   magic:      i32 = 0x7A684244
//! │   file_count: i32
//! ├─ File entries × file_count
//! │   index:  u32   (byte offset to encrypted data)
//! │   id:     1-byte length + UTF-8 bytes
//! │   size:   u32   (encrypted data length)
//! │   crc:    u32   (unused)
//! └─ Data section
//!     encrypted per entry → Rijndael-256-CBC → zlib → UTF-16LE text
//! ```

use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::path::Path;

use simple_rijndael::impls::RijndaelCbc;
use simple_rijndael::paddings::Pkcs7Padding;

const MAGIC: i32 = 0x7A684244;
const KEY: &[u8; 32] = b"mYq3t6v9y$B&E)H@McQfTjWnZr4u7x!z";
const IV: &[u8; 32] = b"TjWnZr4u7x!A%D*G-KaNdRgUkXp2s5v8";
const BLOCK_SIZE: usize = 32;

/// Loaded script entries: asset ID → plaintext content.
pub struct ScriptBundle {
    pub entries: HashMap<String, String>,
}

impl ScriptBundle {
    /// Load the newest .script file from a directory.
    pub fn load_dir(dir: &Path) -> Result<Self, String> {
        let mut entries = HashMap::new();

        let chosen = pick_newest(dir)?;
        if let Some(path) = chosen {
            log::info!("bundle: loading {}", path.display());
            let data = std::fs::read(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
            parse_bundle(&data, &mut entries)?;
        }

        log::info!("bundle: {} entries", entries.len());
        Ok(Self { entries })
    }

    /// Load from a single .script file path.
    pub fn load_file(path: &Path) -> Result<Self, String> {
        let mut entries = HashMap::new();
        let data = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        parse_bundle(&data, &mut entries)?;
        Ok(Self { entries })
    }

    pub fn get(&self, id: &str) -> Option<&str> {
        self.entries.get(id).map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Pick the .script file with the largest numeric stem.
fn pick_newest(dir: &Path) -> Result<Option<std::path::PathBuf>, String> {
    let read_dir = match std::fs::read_dir(dir) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("read_dir: {e}")),
    };

    let mut best: Option<(u64, std::path::PathBuf)> = None;
    for entry in read_dir {
        let entry = entry.map_err(|e| format!("dir entry: {e}"))?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("script") {
            continue;
        }
        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s,
            None => continue,
        };
        let num: u64 = match stem.parse() {
            Ok(n) => n,
            Err(_) => continue,
        };
        match &best {
            Some((prev, _)) if num <= *prev => {}
            _ => best = Some((num, path)),
        }
    }

    Ok(best.map(|(_, p)| p))
}

struct FileInfo {
    index: u32,
    id: String,
    size: u32,
}

fn parse_bundle(data: &[u8], out: &mut HashMap<String, String>) -> Result<(), String> {
    let mut cur = Cursor::new(data);

    // Header
    let mut buf = [0u8; 4];
    cur.read_exact(&mut buf)
        .map_err(|e| format!("magic: {e}"))?;
    if i32::from_be_bytes(buf) != MAGIC {
        return Err(format!("bad magic: 0x{:08X}", i32::from_be_bytes(buf)));
    }

    cur.read_exact(&mut buf)
        .map_err(|e| format!("count: {e}"))?;
    let file_count = i32::from_be_bytes(buf) as usize;

    // Read file info entries
    let mut infos = Vec::with_capacity(file_count);
    for _ in 0..file_count {
        // index
        cur.read_exact(&mut buf)
            .map_err(|e| format!("index: {e}"))?;
        let index = u32::from_be_bytes(buf);

        // id (1-byte length prefix + UTF-8 bytes)
        let mut len_buf = [0u8; 1];
        cur.read_exact(&mut len_buf)
            .map_err(|e| format!("id len: {e}"))?;
        let id_len = len_buf[0] as usize;
        let mut id_bytes = vec![0u8; id_len];
        cur.read_exact(&mut id_bytes)
            .map_err(|e| format!("id: {e}"))?;
        let id = String::from_utf8(id_bytes).map_err(|e| format!("id utf8: {e}"))?;

        // size
        cur.read_exact(&mut buf).map_err(|e| format!("size: {e}"))?;
        let size = u32::from_be_bytes(buf);

        // crc — read but unused
        cur.read_exact(&mut buf).map_err(|e| format!("crc: {e}"))?;

        infos.push(FileInfo { index, id, size });
    }

    // Decrypt + decompress each entry
    for info in &infos {
        let start = info.index as usize;
        let end = start + info.size as usize;
        if end > data.len() {
            log::warn!("bundle: {} — size {end} exceeds file", info.id);
            continue;
        }

        let encrypted = &data[start..end];

        let cipher = RijndaelCbc::<Pkcs7Padding>::new(KEY, BLOCK_SIZE)
            .map_err(|e| format!("rijndael init: {e:?}"))?;

        let decrypted = cipher
            .decrypt(IV, encrypted.to_vec())
            .map_err(|e| format!("{}: decrypt: {e:?}", info.id))?;

        let mut decompressed = Vec::new();
        flate2::read::ZlibDecoder::new(&decrypted[..])
            .read_to_end(&mut decompressed)
            .map_err(|e| format!("{}: zlib: {e}", info.id))?;

        let plain = decode_utf16le(&decompressed).map_err(|e| format!("{}: {e}", info.id))?;

        out.insert(info.id.clone(), plain);
    }

    Ok(())
}

/// Decode UTF-16LE bytes → Rust String.
fn decode_utf16le(data: &[u8]) -> Result<String, String> {
    if !data.len().is_multiple_of(2) {
        return Err("odd byte length for UTF-16".into());
    }
    let u16s: Vec<u16> = data
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16(&u16s).map_err(|e| format!("utf-16: {e:?}"))
}
