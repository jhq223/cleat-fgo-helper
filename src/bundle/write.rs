//! .script Bundle binary writer — produces FGO TextBundle format.
//!
//! Reverse of read.rs: UTF-16LE → zlib → Rijndael-256-CBC encrypt → binary layout.

use std::io::Write;
use std::path::Path;

use simple_rijndael::impls::RijndaelCbc;
use simple_rijndael::paddings::Pkcs7Padding;

const MAGIC: i32 = 0x7A684244;
const KEY: &[u8; 32] = b"mYq3t6v9y$B&E)H@McQfTjWnZr4u7x!z";
const IV: &[u8; 32] = b"TjWnZr4u7x!A%D*G-KaNdRgUkXp2s5v8";
const BLOCK_SIZE: usize = 32;

/// Pack a directory of .txt files into a .script bundle.
/// Each file stem becomes the entry ID (must be numeric or "ScriptFileList").
pub fn pack_dir(input_dir: &Path, output_path: &Path) -> Result<(), String> {
    let mut entries: Vec<(String, String)> = Vec::new();

    for entry in std::fs::read_dir(input_dir).map_err(|e| format!("read_dir: {e}"))? {
        let entry = entry.map_err(|e| format!("dir entry: {e}"))?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("txt") {
            continue;
        }
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();

        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;

        entries.push((id, text));
    }

    if entries.is_empty() {
        return Err("No .txt files found".into());
    }

    // Sort by ID for deterministic output
    entries.sort_by(|a, b| {
        let a_num: u64 = a.0.parse().unwrap_or(0);
        let b_num: u64 = b.0.parse().unwrap_or(0);
        a_num.cmp(&b_num)
    });

    log::info!(
        "Packing {} entries → {}",
        entries.len(),
        output_path.display()
    );

    let data = pack_entries(&entries)?;
    std::fs::write(output_path, &data)
        .map_err(|e| format!("write {}: {e}", output_path.display()))?;

    log::info!("Written {} bytes", data.len());
    Ok(())
}

struct EncryptedEntry {
    id: String,
    data: Vec<u8>,
}

/// Pack entries into the binary format.
fn pack_entries(entries: &[(String, String)]) -> Result<Vec<u8>, String> {
    // Phase 1: Encrypt each entry and collect info
    let mut encrypted = Vec::new();
    for (id, text) in entries {
        let enc = encrypt_entry(id, text)?;
        encrypted.push(enc);
    }

    // Phase 2: Calculate layout
    // Header: magic(4) + file_count(4)
    let header_size = 8;
    // Each file entry: index(4) + id_len(1) + id + size(4) + crc(4)
    let mut file_entry_size = 0usize;
    for e in &encrypted {
        file_entry_size += 4 + 1 + e.id.len() + 4 + 4;
    }
    let data_offset = header_size + file_entry_size;

    // Phase 3: Build the binary
    let mut buf = Vec::new();

    // Header
    buf.extend_from_slice(&MAGIC.to_be_bytes());
    buf.extend_from_slice(&(entries.len() as i32).to_be_bytes());

    // File entries + data
    let mut current_offset = data_offset as u32;
    for e in &encrypted {
        // index (offset to data)
        buf.extend_from_slice(&current_offset.to_be_bytes());
        // id (1-byte length + UTF-8)
        buf.push(e.id.len() as u8);
        buf.extend_from_slice(e.id.as_bytes());
        // size
        buf.extend_from_slice(&(e.data.len() as u32).to_be_bytes());
        // crc (zero for now)
        buf.extend_from_slice(&0u32.to_be_bytes());

        current_offset += e.data.len() as u32;
    }

    // Data section
    for e in &encrypted {
        buf.extend_from_slice(&e.data);
    }

    Ok(buf)
}

/// Encrypt a single entry: UTF-16LE → zlib → Rijndael-256-CBC.
fn encrypt_entry(id: &str, text: &str) -> Result<EncryptedEntry, String> {
    // UTF-16LE encode
    let utf16: Vec<u8> = text.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();

    // Zlib compress
    let mut compressed = Vec::new();
    let mut encoder = flate2::write::ZlibEncoder::new(&mut compressed, flate2::Compression::best());
    encoder
        .write_all(&utf16)
        .map_err(|e| format!("{id}: zlib write: {e}"))?;
    encoder
        .finish()
        .map_err(|e| format!("{id}: zlib finish: {e}"))?;

    // Rijndael-256-CBC encrypt
    let cipher = RijndaelCbc::<Pkcs7Padding>::new(KEY, BLOCK_SIZE)
        .map_err(|e| format!("rijndael init: {e:?}"))?;
    let encrypted = cipher
        .encrypt(IV, compressed)
        .map_err(|e| format!("{id}: encrypt: {e:?}"))?;

    Ok(EncryptedEntry {
        id: id.to_string(),
        data: encrypted,
    })
}
