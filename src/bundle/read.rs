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

const MAGIC: i32 = 0x7A684244;
const KEY: &[u8; 32] = b"mYq3t6v9y$B&E)H@McQfTjWnZr4u7x!z";
const IV: &[u8; 32] = b"TjWnZr4u7x!A%D*G-KaNdRgUkXp2s5v8";

/// Loaded script entries: asset ID → plaintext content.
pub struct ScriptBundle {
    pub entries: HashMap<String, String>,
}

impl ScriptBundle {
    /// Load from a single .script file path.
    pub fn load_file(path: &Path) -> Result<Self, String> {
        let mut entries = HashMap::new();
        if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > 128 * 1024 * 1024 {
            return Err("bundle exceeds size limit".into());
        }
        let data = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        parse_bundle(&data, &mut entries)?;
        Ok(Self { entries })
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
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
    let file_count =
        usize::try_from(i32::from_be_bytes(buf)).map_err(|_| "negative bundle count")?;
    if file_count > 100_000 || file_count > data.len().saturating_sub(8) / 14 {
        return Err("invalid bundle entry count".into());
    }

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
        if id != "ScriptFileList" && (id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit())) {
            return Err("invalid script ID".into());
        }

        // size
        cur.read_exact(&mut buf).map_err(|e| format!("size: {e}"))?;
        let size = u32::from_be_bytes(buf);

        // crc — read but unused
        cur.read_exact(&mut buf).map_err(|e| format!("crc: {e}"))?;

        infos.push(FileInfo { index, id, size });
    }

    // Decrypt + decompress each entry
    let header_end = cur.position() as usize;
    let mut total = 0usize;
    for info in &infos {
        let start = info.index as usize;
        let end = start
            .checked_add(info.size as usize)
            .ok_or("bundle entry overflow")?;
        if start < header_end || end > data.len() {
            return Err(format!("{}: invalid entry bounds", info.id));
        }

        let encrypted = &data[start..end];

        let decrypted = crate::crypto::rijndael::decrypt(encrypted, KEY, IV)?;
        let decompressed = crate::crypto::script::read_bounded(
            flate2::read::ZlibDecoder::new(&decrypted[..]),
            8 * 1024 * 1024,
        )?;

        let plain = decode_utf16le(&decompressed).map_err(|e| format!("{}: {e}", info.id))?;

        total = total
            .checked_add(plain.len())
            .ok_or("bundle text size overflow")?;
        if total > 256 * 1024 * 1024 {
            return Err("bundle text exceeds size limit".into());
        }
        if out.insert(info.id.clone(), plain).is_some() {
            return Err("duplicate story ID".into());
        }
    }

    Ok(())
}

/// Decode UTF-16LE bytes → Rust String.
fn decode_utf16le(data: &[u8]) -> Result<String, String> {
    if !data.len().is_multiple_of(2) {
        return Err("odd byte length for UTF-16".into());
    }
    let u16s: Vec<u16> = data
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16(&u16s).map_err(|e| format!("utf-16: {e:?}"))
}
