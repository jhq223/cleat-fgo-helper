//! Unity AssetBundle TextAsset extraction via Python UnityPy subprocess.
//!
//! FGO uses IL2CPP bundles where the unity-asset crate can't reliably
//! resolve TextAsset objects. Python UnityPy handles this correctly.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

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

/// Extract TextAssets from all .unity3d files using Python UnityPy.
pub fn extract_batch(ab_dir: &Path) -> Result<Vec<BundleTexts>> {
    let entries: Vec<_> = std::fs::read_dir(ab_dir)
        .map_err(|e| Error::Io(e))?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .map_or(false, |ext| ext == "unity3d" || ext == "bin")
        })
        .collect();

    let total = entries.len();
    if total == 0 {
        return Ok(Vec::new());
    }

    log::info!("Extracting TextAssets from {} bundles via UnityPy...", total);

    let script = r#"
import sys, json, base64, os
import UnityPy

ab_dir = sys.argv[1]
results = []
errors = []

for fname in sorted(os.listdir(ab_dir)):
    if not (fname.endswith('.unity3d') or fname.endswith('.bin')):
        continue
    path = os.path.join(ab_dir, fname)
    try:
        env = UnityPy.load(path)
        texts = []
        for obj in env.objects:
            if obj.type.name != 'TextAsset':
                continue
            data = obj.read()
            name = data.m_Name
            script = data.m_Script
            if not script or not name:
                continue
            if not (name.isdigit() or name == 'ScriptFileList'):
                continue
            # UnityPy may return m_Script as str or bytes.
            # Output the raw content as-is (don't double-encode).
            if isinstance(script, str):
                script_str = script
            elif isinstance(script, bytes):
                script_str = script.decode('utf-8', errors='replace').rstrip('\0')
            else:
                continue
            texts.append({
                'name': name,
                'script_text': script_str,
            })
        if texts:
            results.append({'bundle': fname, 'texts': texts})
    except Exception as e:
        errors.append({'bundle': fname, 'error': str(e)})

print(json.dumps({'results': results, 'errors': errors}))
"#;

    // Find python with UnityPy installed
    let python = if cfg!(windows) {
        // Use the known venv path
        let venv_python = r"D:\Users\jhq223\Documents\Code\python\cleat-fgo-builder\.venv\Scripts\python.exe";
        if std::path::Path::new(venv_python).exists() {
            venv_python
        } else {
            "python"
        }
    } else {
        "python3"
    };

    let output = Command::new(python)
        .arg("-c")
        .arg(script)
        .arg(ab_dir.to_str().unwrap_or("."))
        .output()
        .map_err(|e| Error::Parse(format!("UnityPy not found: {e}. Install: pip install UnityPy")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Parse(format!("UnityPy failed: {}", stderr)));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .map_err(|e| Error::Parse(format!("UnityPy JSON parse: {e} (stdout was: {}...)", &stdout[..200.min(stdout.len())])))?;

    let mut results = Vec::new();
    if let Some(arr) = parsed["results"].as_array() {
        for item in arr {
            if let Ok(bt) = serde_json::from_value::<BundleTexts>(item.clone()) {
                results.push(bt);
            }
        }
    }

    let total_texts: usize = results.iter().map(|b| b.texts.len()).sum();
    let err_count = parsed["errors"].as_array().map(|a| a.len()).unwrap_or(0);

    log::info!(
        "Extracted {} TextAssets from {}/{} bundles ({} errors)",
        total_texts,
        results.len(),
        total,
        err_count
    );

    Ok(results)
}
