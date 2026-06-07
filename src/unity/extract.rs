//! Unity AssetBundle TextAsset extraction via the unity-asset Rust crate.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use unity_asset::load_bundle_from_memory;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleTexts {
    pub bundle: String,
    pub texts: Vec<TextAssetEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextAssetEntry {
    pub name: String,
    pub script_text: String,
}

// ── public API ────────────────────────────────────────────────────────

/// Extract TextAssets from all `.unity3d` files in a directory.
///
/// Each file is parsed in-process via `unity-asset`; no Python subprocess
/// is needed.
pub fn extract_batch(ab_dir: &Path) -> Result<Vec<BundleTexts>> {
    let entries: Vec<_> = std::fs::read_dir(ab_dir)
        .map_err(Error::Io)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "unity3d"))
        .collect();

    let total = entries.len();
    if total == 0 {
        return Ok(Vec::new());
    }

    log::info!(
        "Extracting TextAssets from {} bundles via unity-asset...",
        total
    );

    let mut results = Vec::new();
    let mut error_count = 0usize;

    for entry in &entries {
        match extract_bundle(&entry.path()) {
            Ok(bt) => {
                if !bt.texts.is_empty() {
                    results.push(bt);
                }
            }
            Err(e) => {
                log::warn!(
                    "Failed to extract {}: {e}",
                    entry.file_name().to_string_lossy()
                );
                error_count += 1;
            }
        }
    }

    let total_texts: usize = results.iter().map(|b| b.texts.len()).sum();
    log::info!(
        "Extracted {} TextAssets from {}/{} bundles ({} errors)",
        total_texts,
        results.len(),
        total,
        error_count
    );

    Ok(results)
}

// ── single-bundle extraction ─────────────────────────────────────────

/// Extract TextAssets from a single `.unity3d` bundle file.
fn extract_bundle(path: &Path) -> Result<BundleTexts> {
    let data = std::fs::read(path).map_err(Error::Io)?;
    let bundle = load_bundle_from_memory(data)
        .map_err(|e| Error::Parse(format!("Failed to load bundle {}: {e}", path.display())))?;

    let bundle_name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let mut texts = Vec::new();

    for asset in &bundle.assets {
        for handle in asset.object_handles() {
            if handle.class_id() != unity_asset::class_ids::TEXT_ASSET {
                continue;
            }

            // 1) TypeTree-based parsing (works when `enable_type_tree` is
            //    true or when an external registry is supplied).
            if let Ok(obj) = handle.read() {
                let name = obj
                    .get("m_Name")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let script = obj.get("m_Script").and_then(|v| match v {
                    unity_asset::UnityValue::String(s) => Some(s.clone()),
                    unity_asset::UnityValue::Bytes(b) => String::from_utf8(b.clone()).ok(),
                    _ => None,
                });

                if let (Some(name), Some(script)) = (name, script) {
                    if is_valid_script_name(&name) {
                        texts.push(TextAssetEntry {
                            name,
                            script_text: script,
                        });
                    }
                }
                continue;
            }

            // 2) Fallback: raw-byte parsing for stripped IL2CPP bundles.
            if let Ok(raw) = handle.raw_data() {
                if let Some((name, script)) = parse_textasset_raw(raw) {
                    if is_valid_script_name(&name) {
                        texts.push(TextAssetEntry {
                            name,
                            script_text: script,
                        });
                    }
                }
            }
        }
    }

    Ok(BundleTexts {
        bundle: bundle_name,
        texts,
    })
}

// ── helpers ───────────────────────────────────────────────────────────

/// Only keep TextAssets whose name is all digits or the special
/// `ScriptFileList` entry (matching the original Python behaviour).
fn is_valid_script_name(name: &str) -> bool {
    !name.is_empty() && (name.chars().all(|c| c.is_ascii_digit()) || name == "ScriptFileList")
}

// ── raw-byte fallback (IL2CPP without TypeTree) ──────────────────────

/// Parse a TextAsset from raw object bytes.
///
/// Unity serializes `TextAsset` (class id 49) as two [`AlignedString`]s:
///
/// ```text
/// m_Name:   int32 byte_count (LE)  |  UTF-8 bytes  |  pad to 4
/// m_Script: int32 byte_count (LE)  |  UTF-8 bytes  |  pad to 4
/// ```
///
/// This fallback is used when the TypeTree is unavailable (stripped
/// IL2CPP builds).  Little-endian is assumed because all modern Unity
/// target platforms are LE.
fn parse_textasset_raw(raw: &[u8]) -> Option<(String, String)> {
    if raw.len() < 8 {
        return None;
    }
    let mut off = 0usize;
    let name = read_aligned_string(raw, &mut off)?;
    let script = read_aligned_string(raw, &mut off)?;
    Some((name, script))
}

fn read_aligned_string(data: &[u8], offset: &mut usize) -> Option<String> {
    if *offset + 4 > data.len() {
        return None;
    }
    let size = i32::from_le_bytes([
        data[*offset],
        data[*offset + 1],
        data[*offset + 2],
        data[*offset + 3],
    ]) as usize;
    *offset += 4;

    // reject obviously-wrong sizes
    if size > data.len().saturating_sub(*offset) {
        return None;
    }

    let bytes = &data[*offset..*offset + size];
    *offset += size;

    // align to 4-byte boundary
    let rem = *offset % 4;
    if rem != 0 {
        *offset += 4 - rem;
    }

    // strip trailing NULs
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8(bytes[..end].to_vec()).ok()
}

// ── tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse the real FGO script bundle and verify we get the expected
    /// `ScriptFileList` TextAsset with encrypted content.
    #[test]
    fn test_extract_scriptfilelist_bundle() {
        let path = Path::new(
            "data/jp/decrypted_ab/2b3c7a4706632dd61f9e7e840fedd8e368a81633.bin.unity3d",
        );
        assert!(
            path.exists(),
            "Test bundle not found: {} – run `cargo test` from the project root",
            path.display()
        );

        let result = extract_bundle(path).expect("Failed to extract bundle");

        assert_eq!(
            result.bundle,
            "2b3c7a4706632dd61f9e7e840fedd8e368a81633.bin.unity3d"
        );
        assert!(!result.texts.is_empty(), "Should have at least one TextAsset");

        // The bundle contains one TextAsset: "ScriptFileList"
        let sfl = result
            .texts
            .iter()
            .find(|t| t.name == "ScriptFileList")
            .expect("Should contain a TextAsset named 'ScriptFileList'");

        assert!(
            !sfl.script_text.is_empty(),
            "ScriptFileList m_Script should not be empty"
        );

        // The content is base64-encoded encrypted text.
        // Just verify it's printable ASCII (base64 charset).
        assert!(
            sfl.script_text
                .chars()
                .all(|c| c.is_ascii_graphic() || c == '+' || c == '/' || c == '='),
            "ScriptFileList content should be base64-encoded"
        );

        // Sanity: m_Name matches the property
        assert_eq!(sfl.name, "ScriptFileList");
    }
}
