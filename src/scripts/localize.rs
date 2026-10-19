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
        if let Some(cn_val) = cn_map
            .get(key)
            .filter(|cn| !cn.trim().is_empty() && placeholders_match(jp_val, cn))
        {
            entries.push((key.clone(), cn_val.clone()));
            overwritten += 1;
        } else {
            entries.push((key.clone(), jp_val.clone()));
        }
    }
    log::info!("Merged: {} keys overwritten with CN values", overwritten);

    let out_text = build_output(&entries);
    let out_bytes = encode_utf16le_crlf(&out_text);

    crate::storage::atomic_write(out_path, &out_bytes)?;
    log::info!(
        "Saved: {} ({} bytes, UTF-16LE)",
        out_path.display(),
        out_bytes.len()
    );

    Ok(())
}

fn placeholders_match(source: &str, translated: &str) -> bool {
    static PATTERN: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r"\{\d+(?:[^{}]*)\}|%(?:\d+\$)?[-+0 #]*\d*(?:\.\d+)?[sdfi]|\[[a-zA-Z_][a-zA-Z_0-9]*\]",
        )
        .unwrap()
    });
    let tokens = |text: &str| {
        let mut values: Vec<_> = PATTERN
            .find_iter(text)
            .map(|m| m.as_str().to_owned())
            .collect();
        values.sort();
        values
    };
    tokens(source) == tokens(translated)
}

fn parse_localization_json(text: &str) -> anyhow::Result<IndexMap<String, String>> {
    // The client also accepts full-width whitespace outside JSON strings.
    // Keep strings/comments intact and leave JSON parsing to the parser crate.
    static TOKENS: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r#""(?:\\.|[^"\\])*"|//[^\r\n]*|/\*[\s\S]*?\*/|[^\S\x00-\x7F]"#).unwrap()
    });
    let normalized = TOKENS.replace_all(text, |token: &regex::Captures<'_>| {
        if token[0].chars().all(char::is_whitespace) {
            " ".to_owned()
        } else {
            token[0].to_owned()
        }
    });
    // Some shipped JP files omit the comma between consecutive string entries.
    // Only repair adjacent string tokens; malformed values still fail parsing.
    let mut repaired = String::with_capacity(normalized.len());
    static COMMAS: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r",(?:\s*,)+").unwrap());
    let mut end = 0;
    let mut previous_string = false;
    for token in TOKENS.find_iter(&normalized) {
        let gap = &normalized[end..token.start()];
        if !gap.trim().is_empty() {
            previous_string = false;
        }
        let is_string = token.as_str().starts_with('"');
        repaired.push_str(&COMMAS.replace_all(gap, ","));
        if previous_string && is_string && gap.trim().is_empty() {
            repaired.push(',');
        }
        repaired.push_str(token.as_str());
        if is_string {
            previous_string = true;
        } else if !token.as_str().starts_with("//") && !token.as_str().starts_with("/*") {
            previous_string = false;
        }
        end = token.end();
    }
    repaired.push_str(&COMMAS.replace_all(&normalized[end..], ","));
    let mut parser = serde_json_lenient::Deserializer::from_slice_with_options(
        repaired.trim_start_matches('\u{feff}').as_bytes(),
        false,
        true,
        true,
        false,
        false,
    );
    let values = <IndexMap<String, String> as serde::Deserialize>::deserialize(&mut parser)?;
    parser.end()?;
    Ok(values)
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
    json_escape_value(s)
}
fn json_escape_value(s: &str) -> String {
    let quoted = serde_json::to_string(s).expect("string serialization");
    quoted[1..quoted.len() - 1].to_owned()
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
    fn preserves_unicode_and_format_arguments() {
        assert_eq!(
            parse_localization_json("{\"a\":\"A,,B\",,\n\"b\":\"B\",,}").unwrap()["a"],
            "A,,B"
        );
        assert_eq!(
            parse_localization_json("{\"a\":\"A\"\n// comment\n\"b\":\"B\"}")
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            parse_localization_json("{\"a\":\"A\"\n\"b\":\"B\"}")
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            parse_localization_json("{\"key\":　\"文字　文字\"}").unwrap()["key"],
            "文字　文字"
        );
        assert_eq!(
            parse_localization_json(r#"{"emoji":"\uD83D\uDE00",}"#).unwrap()["emoji"],
            "😀"
        );
        assert!(!placeholders_match("{0} {1}", "{0}"));
        assert!(placeholders_match("{0} {1}", "{1} {0}"));
        assert!(!placeholders_match("[servantName]", "名字"));
    }

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
                .as_chunks::<2>()
                .0
                .iter()
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
