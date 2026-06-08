//! Localization merge: inject CN translations into JP LocalizationJpn format.
//!
//! Output format (matching the reference that the game actually loads):
//! - UTF-16 LE with BOM
//! - CRLF line endings
//! - 2-space indent, `"key": "value"` (space after colon only)
//! - Trailing comma on all entries except the last
//! - No `//` comments, no blank lines

use indexmap::IndexMap;

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

    // Parse JP → ordered key-value pairs (IndexMap preserves insertion order)
    log::info!("Loading JP: {}", jp_path.display());
    let jp_text = std::fs::read_to_string(&jp_path)?;
    let jp_map = parse_localization_json(&jp_text)?;
    log::info!("  JP: {} keys", jp_map.len());

    // Parse CN → lookup map
    log::info!("Loading CN: {}", cn_path.display());
    let cn_text = std::fs::read_to_string(&cn_path)?;
    let cn_map = parse_localization_json(&cn_text)?;
    log::info!("  CN: {} keys", cn_map.len());

    // Merge: JP order, CN value when available
    let mut overwritten = 0u64;
    let mut entries: Vec<(String, String)> = Vec::with_capacity(jp_map.len());
    for (key, jp_val) in &jp_map {
        if let Some(cn_val) = cn_map.get(key) {
            entries.push((key.clone(), cn_val.clone()));
            overwritten += 1;
        } else {
            entries.push((key.clone(), jp_val.clone()));
        }
    }
    log::info!("Merged: {} keys overwritten with CN values", overwritten);

    let out_text = build_output(&entries);
    let out_bytes = encode_utf16le_crlf(&out_text);

    std::fs::write(out_path, &out_bytes)?;
    log::info!(
        "Saved: {} ({} bytes, UTF-16LE)",
        out_path.display(),
        out_bytes.len()
    );

    Ok(())
}

fn parse_localization_json(text: &str) -> anyhow::Result<IndexMap<String, String>> {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut pos = 0usize;
    let mut map = IndexMap::new();

    skip_ws(&chars, &mut pos, len);
    expect_char(&chars, &mut pos, len, '{')?;

    loop {
        skip_ws(&chars, &mut pos, len);
        if pos < len && chars[pos] == '}' {
            break;
        }

        let key = read_string(&chars, &mut pos, len)?;
        skip_ws(&chars, &mut pos, len);
        expect_char(&chars, &mut pos, len, ':')?;
        skip_ws(&chars, &mut pos, len);
        let value = read_string(&chars, &mut pos, len)?;

        map.insert(key, value);

        skip_ws(&chars, &mut pos, len);
        while pos < len && chars[pos] == ',' {
            pos += 1;
            skip_ws(&chars, &mut pos, len);
        }
    }

    Ok(map)
}

fn skip_ws(chars: &[char], pos: &mut usize, len: usize) {
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

fn read_string(chars: &[char], pos: &mut usize, len: usize) -> anyhow::Result<String> {
    expect_char(chars, pos, len, '"')?;

    let mut s = String::new();

    while *pos < len {
        let c = chars[*pos];
        *pos += 1;
        match c {
            '"' => return Ok(s),
            '\\' if *pos < len => {
                let next = chars[*pos];
                *pos += 1;
                match next {
                    '"' => s.push('"'),
                    '\\' => s.push('\\'),
                    '/' => s.push('/'),
                    'b' => s.push('\x08'),
                    'f' => s.push('\x0C'),
                    'n' => s.push('\n'),
                    'r' => s.push('\r'),
                    't' => s.push('\t'),
                    'u' => {
                        if *pos + 4 > len {
                            anyhow::bail!("Truncated \\u escape");
                        }
                        let hex: String = chars[*pos..*pos + 4].iter().collect();
                        *pos += 4;
                        let cp = u32::from_str_radix(&hex, 16)
                            .map_err(|_| anyhow::anyhow!("Invalid unicode escape \\u{hex}"))?;
                        s.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
                    }
                    other => {
                        // Unknown escape: keep both chars literally
                        s.push('\\');
                        s.push(other);
                    }
                }
            }
            other => s.push(other),
        }
    }

    anyhow::bail!("Unterminated string at pos {pos}");
}

fn build_output(entries: &[(String, String)]) -> String {
    let mut out = String::with_capacity(entries.len() * 64);
    out.push_str("{\r\n");

    let last = entries.len().saturating_sub(1);
    for (i, (key, value)) in entries.iter().enumerate() {
        out.push_str("  \"");
        out.push_str(&json_escape(key));
        out.push_str("\": \"");
        out.push_str(&json_escape_value(value));
        out.push('"');
        if i < last {
            out.push(',');
        }
        out.push_str("\r\n");
    }

    out.push('}');
    out
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            other => out.push(other),
        }
    }
    out
}

fn json_escape_value(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04X}", c as u32));
            }
            other => out.push(other),
        }
    }
    out
}

fn encode_utf16le_crlf(text: &str) -> Vec<u8> {
    let utf16: Vec<u16> = text.encode_utf16().collect();
    let mut bytes = Vec::with_capacity(2 + utf16.len() * 2);
    bytes.push(0xFF);
    bytes.push(0xFE);
    for unit in utf16 {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_jp_format() {
        let text = r#"  {
    "KEY_ONE" : "value1",
    // comment
    "KEY_TWO" : "value\n2",
    "KEY_THREE" : "value3",
}"#;
        let map = parse_localization_json(text).unwrap();
        assert_eq!(map.len(), 3);
        assert_eq!(map.get("KEY_ONE").unwrap(), "value1");
        assert_eq!(map.get("KEY_TWO").unwrap(), "value\n2");
        assert_eq!(map.get("KEY_THREE").unwrap(), "value3");
        // Order preserved
        let keys: Vec<&str> = map.keys().map(|s| s.as_str()).collect();
        assert_eq!(keys, vec!["KEY_ONE", "KEY_TWO", "KEY_THREE"]);
    }

    #[test]
    fn test_parse_cn_format() {
        let text = r#"{
    "ACCOUNT_DELETE_CONFIRM_CANCEL": "キャンセル",
    "ACCOUNT_DELETE_CONFIRM_DECIDE": "パスワード発行"
}"#;
        let map = parse_localization_json(text).unwrap();
        assert_eq!(map.len(), 2);
        assert_eq!(
            map.get("ACCOUNT_DELETE_CONFIRM_CANCEL").unwrap(),
            "キャンセル"
        );
    }

    #[test]
    fn test_build_output_format() {
        let entries = vec![
            ("KEY_ONE".into(), "value1".into()),
            ("KEY_TWO".into(), "multi\nline".into()),
        ];
        let out = build_output(&entries);
        assert_eq!(
            out,
            "{\r\n  \"KEY_ONE\": \"value1\",\r\n  \"KEY_TWO\": \"multi\\nline\"\r\n}"
        );
    }

    #[test]
    fn test_utf16le_encoding() {
        let text = "{\r\n  \"A\": \"B\"\r\n}";
        let bytes = encode_utf16le_crlf(text);
        assert_eq!(bytes[0], 0xFF);
        assert_eq!(bytes[1], 0xFE);
        let decoded: String = char::decode_utf16(
            bytes[2..]
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]])),
        )
        .map(|r| r.unwrap())
        .collect();
        assert_eq!(decoded, text);
    }

    #[test]
    fn test_json_escape_value() {
        assert_eq!(json_escape_value("hello"), "hello");
        assert_eq!(json_escape_value("a\"b"), "a\\\"b");
        assert_eq!(json_escape_value("a\\b"), "a\\\\b");
        assert_eq!(json_escape_value("a\nb"), "a\\nb");
        assert_eq!(json_escape_value("a\rb"), "a\\rb");
        assert_eq!(json_escape_value("a\tb"), "a\\tb");
    }
}
