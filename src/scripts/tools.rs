//! Directory comparison, deduplication, and localization utilities.

use std::collections::HashMap;

// Localization JSON parser.
// Handles non-standard formatting: `//` comments, full-width spaces (U+3000, U+A0),
// missing spaces, double commas, multi-line strings, trailing commas.

fn parse_localization_json(text: &str) -> anyhow::Result<HashMap<String, String>> {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut pos = 0usize;
    let mut map = HashMap::new();

    // Skip to opening brace
    skip_ws_and_comments(&chars, &mut pos, len);
    expect_char(&chars, &mut pos, len, '{')?;

    loop {
        skip_ws_and_comments(&chars, &mut pos, len);

        if pos < len && chars[pos] == '}' {
            break;
        }

        let key = read_string(&chars, &mut pos, len)?;

        skip_ws_and_comments(&chars, &mut pos, len);
        expect_char(&chars, &mut pos, len, ':')?;

        skip_ws_and_comments(&chars, &mut pos, len);
        let value = read_string(&chars, &mut pos, len)?;

        map.insert(key, value);

        skip_ws_and_comments(&chars, &mut pos, len);
        while pos < len && chars[pos] == ',' {
            pos += 1;
            skip_ws_and_comments(&chars, &mut pos, len);
        }
    }

    Ok(map)
}

/// Skip whitespace (including U+3000/U+A0) and `//` comments.
fn skip_ws_and_comments(chars: &[char], pos: &mut usize, len: usize) {
    while *pos < len {
        let c = chars[*pos];
        if c == ' ' || c == '\t' || c == '\r' || c == '\n' || c == '\u{3000}' || c == '\u{A0}' {
            *pos += 1;
        } else if c == '/' && *pos + 1 < len && chars[*pos + 1] == '/' {
            *pos += 2;
            while *pos < len && chars[*pos] != '\n' {
                *pos += 1;
            }
        } else {
            break;
        }
    }
}

/// Expect a specific character.
fn expect_char(chars: &[char], pos: &mut usize, len: usize, expected: char) -> anyhow::Result<()> {
    if *pos >= len {
        anyhow::bail!("Unexpected EOF, expected '{expected}'");
    }
    if chars[*pos] != expected {
        let start = (*pos).saturating_sub(10);
        let end = (*pos + 10).min(len);
        let ctx: String = chars[start..end].iter().collect();
        anyhow::bail!(
            "Expected '{expected}' at pos {pos}, got '{}'. Context: ...{ctx}...",
            chars[*pos]
        );
    }
    *pos += 1;
    Ok(())
}

/// Read a double-quoted string. Escaped quotes and other backslash
/// sequences are passed through, and literal newlines are allowed.
fn read_string(chars: &[char], pos: &mut usize, len: usize) -> anyhow::Result<String> {
    expect_char(chars, pos, len, '"')?;

    let mut s = String::new();

    while *pos < len {
        let c = chars[*pos];
        if c == '"' {
            *pos += 1;
            return Ok(s);
        } else if c == '\\' && *pos + 1 < len && chars[*pos + 1] == '"' {
            s.push('\\');
            s.push('"');
            *pos += 2;
        } else if c == '\\' && *pos + 1 < len {
            s.push('\\');
            s.push(chars[*pos + 1]);
            *pos += 2;
        } else {
            s.push(c);
            *pos += 1;
        }
    }

    anyhow::bail!("Unterminated string at pos {pos}");
}

fn merge_cn_into_jp(jp: &mut HashMap<String, String>, cn: &HashMap<String, String>) -> usize {
    let mut count = 0;
    for (key, cn_val) in cn {
        if jp.contains_key(key) {
            jp.insert(key.clone(), cn_val.clone());
            count += 1;
        }
    }
    count
}

pub fn cmd_localize(jp_base: &str, cn_ref: &str, output: &str) -> anyhow::Result<()> {
    let jp_path = std::path::Path::new(jp_base).join("LocalizationJpn.txt");
    let cn_path = std::path::Path::new(cn_ref).join("LocalizationJpn1.txt");
    let out_path = std::path::Path::new(output);

    if !jp_path.exists() {
        anyhow::bail!("JP localization not found: {}", jp_path.display());
    }
    if !cn_path.exists() {
        anyhow::bail!("CN localization not found: {}", cn_path.display());
    }

    log::info!("Loading JP: {}", jp_path.display());
    let jp_text = std::fs::read_to_string(&jp_path)?;
    let mut jp_map = parse_localization_json(&jp_text)?;
    log::info!("  JP: {} keys", jp_map.len());

    log::info!("Loading CN: {}", cn_path.display());
    let cn_text = std::fs::read_to_string(&cn_path)?;
    let cn_map = parse_localization_json(&cn_text)?;
    log::info!("  CN: {} keys", cn_map.len());

    let overwritten = merge_cn_into_jp(&mut jp_map, &cn_map);
    log::info!("Merged: {} keys overwritten with CN values", overwritten);

    let out_json = serde_json::to_string_pretty(&jp_map)?;
    std::fs::write(out_path, &out_json)?;
    log::info!("Saved: {} ({} bytes)", out_path.display(), out_json.len());

    Ok(())
}

/// Copy JP-only scripts (missing from CN) to output directory.
pub fn cmd_compare(jp_dir: &str, cn_dir: &str, output_dir: &str) -> anyhow::Result<()> {
    let jp = std::path::Path::new(jp_dir);
    let cn = std::path::Path::new(cn_dir);
    let out = std::path::Path::new(output_dir);
    std::fs::create_dir_all(out)?;

    let cn_files: std::collections::HashSet<String> = std::fs::read_dir(cn)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "txt"))
        .filter_map(|e| e.file_name().to_str().map(String::from))
        .collect();

    let mut copied = 0u64;
    for entry in std::fs::read_dir(jp)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "txt") {
            let fname = entry.file_name().to_string_lossy().to_string();
            if !cn_files.contains(&fname) {
                std::fs::copy(&path, out.join(&fname))?;
                copied += 1;
            }
        }
    }

    log::info!("Copied {} JP-only scripts to {}", copied, output_dir);
    Ok(())
}

/// Remove files from translated directory that also exist in CN directory.
pub fn cmd_dedup(cn_dir: &str, translated_dir: &str) -> anyhow::Result<()> {
    let cn = std::path::Path::new(cn_dir);
    let translated = std::path::Path::new(translated_dir);

    let cn_files: std::collections::HashSet<String> = std::fs::read_dir(cn)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "txt"))
        .filter_map(|e| e.file_name().to_str().map(String::from))
        .collect();

    let mut removed = 0u64;
    for entry in std::fs::read_dir(translated)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "txt") {
            let fname = entry.file_name().to_string_lossy().to_string();
            if cn_files.contains(&fname) {
                std::fs::remove_file(&path)?;
                removed += 1;
            }
        }
    }

    log::info!(
        "Removed {} files from {} (already exist in CN)",
        removed,
        translated_dir
    );
    Ok(())
}
