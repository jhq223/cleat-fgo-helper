//! Message entry type, ruby stripping, tag collection, JSON utilities.

use serde::{Deserialize, Serialize};

/// Flat message entry for the translation JSON format.
/// `name` is the speaker name (empty for choices and char-name entries).
/// `original` is the source text — never modified; used as anchor for import.
/// `message` is the translatable text — translator modifies this field.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MessageEntry {
    pub name: String,
    #[serde(default = "default_empty_string")]
    pub original: String,
    pub message: String,
}

fn default_empty_string() -> String {
    String::new()
}

impl MessageEntry {
    /// Create an entry. Both `original` and `message` have `[#ruby]` stripped —
    /// Chinese doesn't need ruby annotations, so they're noise for translation.
    pub fn new(name: &str, original_text: &str) -> Self {
        let stripped = strip_ruby(original_text);
        Self {
            name: name.to_string(),
            original: stripped.clone(),
            message: stripped,
        }
    }

    pub fn is_translated(&self) -> bool {
        self.original != self.message
    }
}

// ── Ruby annotation handling ──

/// Strip `[#kanji:reading]` → `kanji` (ruby is unnecessary for Chinese, just noise).
pub fn strip_ruby(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '[' && i + 1 < chars.len() && chars[i + 1] == '#' {
            let start = i + 2;
            let mut j = i + 2;
            let mut colon_pos: Option<usize> = None;
            while j < chars.len() && chars[j] != ']' {
                if chars[j] == ':' && colon_pos.is_none() {
                    colon_pos = Some(j);
                }
                j += 1;
            }
            if j < chars.len() {
                if let Some(cp) = colon_pos {
                    let kanji: String = chars[start..cp].iter().collect();
                    result.push_str(&kanji);
                } else {
                    let inner: String = chars[start..j].iter().collect();
                    result.push_str(&inner);
                }
                i = j + 1;
                continue;
            }
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}

// ── Tag utilities ──

/// Collect all bracket tags from a text string (in order).
/// Excludes `[#...]` ruby annotations and `[&...]` gender/politeness variants — they are content, not control tags.
pub fn collect_tags(text: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '[' {
            if i + 1 < chars.len() && (chars[i + 1] == '#' || chars[i + 1] == '&') {
                // Skip ruby [#...], and gender/politeness variants [&...]
                i += 1;
                while i < chars.len() && chars[i] != ']' {
                    i += 1;
                }
                if i < chars.len() {
                    i += 1;
                }
                continue;
            }
            let start = i;
            i += 1;
            while i < chars.len() && chars[i] != ']' {
                i += 1;
            }
            if i < chars.len() {
                i += 1;
                let tag: String = chars[start..i].iter().collect();
                tags.push(tag);
            }
        } else {
            i += 1;
        }
    }
    tags
}

/// Strict validation: translated text must preserve ALL bracket tags from original.
/// Returns Ok(()) if tags match, or Err with per-line error messages.
pub fn validate_tags(original: &[String], translated: &[String]) -> Result<(), Vec<String>> {
    if original.len() != translated.len() {
        return Err(vec![format!(
            "Line count mismatch: original {} vs translated {}",
            original.len(),
            translated.len()
        )]);
    }

    let mut errors = Vec::new();
    for (i, (orig, trans)) in original.iter().zip(translated.iter()).enumerate() {
        let orig_tags = collect_tags(orig);
        let trans_tags = collect_tags(trans);

        for tag in &orig_tags {
            if !trans_tags.contains(tag) {
                errors.push(format!(
                    "Line {}: missing tag '{}'\n  original: {}\n  translated: {}",
                    i, tag, orig, trans
                ));
            }
        }
        for tag in &trans_tags {
            if !orig_tags.contains(tag) {
                errors.push(format!(
                    "Line {}: extra tag '{}'\n  original: {}\n  translated: {}",
                    i, tag, orig, trans
                ));
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Scan a pretty-printed JSON array string and return the starting line number
/// (1-indexed) for each top-level array element. Uses a simple char-by-char
/// state machine that tracks string literals and nesting depth.
pub fn json_entry_line_starts(json: &str) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut depth = 0u32;
    let mut in_string = false;
    let mut escape = false;
    let mut line = 1usize;

    for ch in json.chars() {
        if ch == '\n' {
            line += 1;
            escape = false;
            continue;
        }
        if escape {
            escape = false;
            continue;
        }
        if ch == '\\' && in_string {
            escape = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match ch {
            '{' => {
                if depth == 1 {
                    starts.push(line);
                }
                depth += 1;
            }
            '}' => {
                depth = depth.saturating_sub(1);
            }
            '[' => {
                depth += 1;
            }
            ']' => {
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
    }

    starts
}
