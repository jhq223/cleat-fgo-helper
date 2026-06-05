//! FGO story script parser using PEG for line-level tokenizing.
//!
//! Strategy: PEG tokenizes each line into a Tag. Then post-processing
//! groups related lines into semantic blocks (dialogue, choice, command).
//!
//! Script format is line-based with inline tags like:
//!   [color]text[-]  [r]  [line N]  [#ruby:reading]  [&m:f]  [%1]

use std::fmt;

// ── Line-level tags ──

/// A classified line of the script.
#[derive(Debug, Clone, PartialEq)]
enum Tag {
    /// `＄...` header
    Header(String),
    /// `＠Speaker` or `＠[color]Speaker[-]` or `＠Slot：Speaker`
    Speaker(String),
    /// `[k]` — end of dialogue
    KeyWait,
    /// `？！` — choice separator
    ChoiceSep,
    /// `？1：text` or `？2：text` — choice option
    ChoiceOpt(String),
    /// `[...]` — standalone command
    Command(String),
    /// Any other text line
    Text(String),
    /// Empty / whitespace-only line
    Blank,
}

peg::parser! {
    grammar line_grammar() for str {
        pub rule tag() -> Tag
            = header_tag()
            / keywait_tag()
            / choicesep_tag()
            / choiceopt_tag()
            / speaker_tag()
            / command_tag()
            / blank_tag()
            / text_tag()

        rule header_tag() -> Tag
            = "＄" t:$([^'\n']*) { Tag::Header(t.to_string()) }

        rule keywait_tag() -> Tag
            = "[" "k" "]" { Tag::KeyWait }

        rule choicesep_tag() -> Tag
            = "？！" { Tag::ChoiceSep }

        rule choiceopt_tag() -> Tag
            = "？" ['1' | '2'] "：" t:$([^'\n']*) { Tag::ChoiceOpt(t.to_string()) }

        rule speaker_tag() -> Tag
            = "＠" t:$([^'\n']*) { Tag::Speaker(t.to_string()) }

        rule command_tag() -> Tag
            = "[" c:$((!"]" [_])*) "]" ![_] { Tag::Command(c.to_string()) }

        rule blank_tag() -> Tag
            = [' ' | '\t']* { Tag::Blank }

        rule text_tag() -> Tag
            = t:$([^'\n']+) { Tag::Text(t.to_string()) }
    }
}

// ── Semantic blocks (post-processed from Tags) ──

/// A semantic block in the script.
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Header(String),
    Dialogue {
        speaker_raw: String,
        speaker_name: String,
        lines: Vec<String>,
    },
    Choice {
        options: Vec<String>,
    },
    Command(String),
}

impl fmt::Display for Block {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Block::Header(h) => write!(f, "Header: {}", h),
            Block::Dialogue { speaker_name, lines, .. } => {
                write!(f, "Dialogue[{}]: {}", speaker_name, lines.join(" | "))
            }
            Block::Choice { options } => {
                write!(f, "Choice[{}]", options.join(" | "))
            }
            Block::Command(c) => write!(f, "Command: {}", c),
        }
    }
}

/// Parse a single line into a Tag, consuming exactly one line of input.
fn parse_tag(line: &str) -> Option<Tag> {
    line_grammar::tag(line.trim_end_matches('\r')).ok()
}

/// Parse a full script string into semantic blocks.
pub fn parse_script(input: &str) -> Result<Vec<Block>, String> {
    // Phase 1: Tag every line
    let mut tags: Vec<Tag> = Vec::new();
    for raw_line in input.lines() {
        let line = raw_line.trim_end_matches('\r');
        match parse_tag(line) {
            Some(tag) => tags.push(tag),
            None => {
                // If PEG can't classify, treat as text
                if line.trim().is_empty() {
                    tags.push(Tag::Blank);
                } else {
                    tags.push(Tag::Text(line.to_string()));
                }
            }
        }
    }

    // Phase 2: Group tags into blocks
    let mut blocks: Vec<Block> = Vec::new();
    let mut i = 0;
    
    while i < tags.len() {
        match &tags[i] {
            Tag::Header(h) => {
                blocks.push(Block::Header(h.clone()));
                i += 1;
            }
            Tag::Speaker(_) => {
                // Dialogue block: speaker → text lines → [k]
                let speaker = match &tags[i] {
                    Tag::Speaker(s) => s.clone(),
                    _ => unreachable!(),
                };
                let speaker_name = extract_speaker_name(&speaker);
                let mut lines = Vec::new();
                i += 1;
                
                // Collect text lines until [k] or end
                while i < tags.len() {
                    match &tags[i] {
                        Tag::KeyWait => {
                            i += 1;
                            break;
                        }
                        Tag::Speaker(_) => {
                            // New speaker before [k] — treat as end of current dialogue
                            break;
                        }
                        Tag::ChoiceOpt(_) | Tag::ChoiceSep => {
                            // Choice starting before [k] — break and let choice handler take over
                            break;
                        }
                        Tag::Text(t) => {
                            lines.push(t.clone());
                            i += 1;
                        }
                        Tag::Command(cmd) => {
                            // Commands between speaker and [k] in real scripts
                            // e.g., [charaFadein A 0.1 1] between speaker and text
                            // These are inline commands that come before the text
                            if lines.is_empty() {
                                // Inline command before any text — add as command block
                                blocks.push(Block::Command(cmd.clone()));
                                i += 1;
                            } else {
                                // Command after text has started — might be inline, add as text
                                lines.push(format!("[{}]", cmd));
                                i += 1;
                            }
                        }
                        Tag::Blank => {
                            i += 1;
                        }
                        Tag::Header(_) => {
                            // Shouldn't happen but handle gracefully
                            break;
                        }
                    }
                }
                
                // Filter empty lines
                let lines: Vec<String> = lines.into_iter()
                    .filter(|l| !l.trim().is_empty())
                    .collect();
                
                blocks.push(Block::Dialogue {
                    speaker_raw: speaker,
                    speaker_name,
                    lines,
                });
            }
            Tag::ChoiceOpt(_) => {
                // Choice block: consecutive choice options → ？！
                let mut options = Vec::new();
                while i < tags.len() {
                    match &tags[i] {
                        Tag::ChoiceOpt(t) => {
                            options.push(t.clone());
                            i += 1;
                        }
                        Tag::ChoiceSep => {
                            i += 1;
                            break;
                        }
                        Tag::Blank => {
                            i += 1;
                            // Allow blank lines between options
                        }
                        _ => {
                            // End of choice block without separator
                            break;
                        }
                    }
                }
                if !options.is_empty() {
                    blocks.push(Block::Choice { options });
                }
            }
            Tag::Command(cmd) => {
                blocks.push(Block::Command(cmd.clone()));
                i += 1;
            }
            Tag::ChoiceSep => {
                // Stray separator, skip
                i += 1;
            }
            Tag::KeyWait => {
                // Stray [k], skip
                i += 1;
            }
            Tag::Blank => {
                i += 1;
            }
            Tag::Text(t) => {
                // Stray text (not part of any dialogue) — treat as orphan text
                // This happens with lines like "？！" appearing alone or stray text
                blocks.push(Block::Command(format!("__text__:{}", t)));
                i += 1;
            }
        }
    }

    Ok(blocks)
}

/// Extract the display speaker name from a raw speaker line.
fn extract_speaker_name(raw: &str) -> String {
    // Case 1: Contains color tag [hex]name[-]
    if raw.contains("[-]") {
        let mut name = String::new();
        let chars: Vec<char> = raw.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '[' {
                i += 1;
                while i < chars.len() && chars[i] != ']' {
                    i += 1;
                }
                if i < chars.len() { i += 1; }
            } else {
                name.push(chars[i]);
                i += 1;
            }
        }
        let name = name.trim().to_string();
        if !name.is_empty() { return name; }
    }
    
    // Case 2: Slot prefix like "C：？？？"
    if let Some(pos) = raw.find('：') {
        return raw[pos + '：'.len_utf8()..].trim().to_string();
    }
    if let Some(pos) = raw.find(':') {
        return raw[pos + 1..].trim().to_string();
    }
    
    raw.trim().to_string()
}

// ── Export / Import ──

use serde::{Deserialize, Serialize};

/// Flat message entry for the new JSON format.
/// `name` is the speaker name (empty for choices and char-name entries).
/// `original` is the source text — never modified; used as anchor for import.
/// `message` is the translatable text — translator modifies this field.
#[derive(Debug, Serialize, Deserialize, Clone)]
struct MessageEntry {
    name: String,
    #[serde(default = "default_empty_string")]
    original: String,
    message: String,
}

fn default_empty_string() -> String {
    String::new()
}

impl MessageEntry {
    /// Create an entry. Both `original` and `message` have `[#ruby]` stripped —
    /// Chinese doesn't need ruby annotations, so they're noise for translation.
    fn new(name: &str, original_text: &str) -> Self {
        let stripped = strip_ruby(original_text);
        Self {
            name: name.to_string(),
            original: stripped.clone(),
            message: stripped,
        }
    }

    fn is_translated(&self) -> bool {
        self.original != self.message
    }
}

// ── Ruby annotation handling ──

/// Strip `[#kanji:reading]` → `kanji` (ruby is unnecessary for Chinese, just noise).
fn strip_ruby(text: &str) -> String {
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
                // Extract just the kanji part (before :)
                if let Some(cp) = colon_pos {
                    let kanji: String = chars[start..cp].iter().collect();
                    result.push_str(&kanji);
                } else {
                    // No colon — keep everything inside [#...]
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
/// Excludes `[#...]` ruby annotations — they are content, not control tags.
fn collect_tags(text: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '[' {
            // Skip [#ruby] annotations — they are content, not control tags
            if i + 1 < chars.len() && chars[i + 1] == '#' {
                i += 1;
                while i < chars.len() && chars[i] != ']' {
                    i += 1;
                }
                if i < chars.len() { i += 1; }
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
fn validate_tags(original: &[String], translated: &[String]) -> Result<(), Vec<String>> {
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

    if errors.is_empty() { Ok(()) } else { Err(errors) }
}

// ── Speaker name extraction from charaSet commands ──

/// Extract the display name from a `charaSet` command.
/// Format: `charaSet SLOT ID NUM NAME`
/// e.g. `charaSet A 98001000 0 マシュ` → `マシュ`
fn extract_chara_name(cmd: &str) -> Option<&str> {
    // Strip leading '[' if present
    let inner = cmd.strip_prefix('[').unwrap_or(cmd);
    if !inner.starts_with("charaSet ") {
        return None;
    }
    // Split into 5 parts: charaSet, slot, id, num, name
    let parts: Vec<&str> = inner.splitn(5, ' ').collect();
    if parts.len() >= 5 {
        // Strip trailing ']' and '\r' (Windows line endings may leave \r)
        let name = parts[4].trim_end_matches([']', '\r']);
        Some(name)
    } else {
        None
    }
}

// ── Speaker name replacement in script content ──

/// Replace speaker names in charaSet commands and ＠ speaker lines.
fn replace_speaker_names(content: &str, name_map: &[(String, String)]) -> String {
    if name_map.is_empty() {
        return content.to_string();
    }

    let mut result = String::with_capacity(content.len());
    let mut remaining = content;

    while let Some(line_end) = remaining.find('\n') {
        let (line, rest) = remaining.split_at(line_end);
        let newline = &rest[..1]; // "\n"
        remaining = &rest[1..];

        let mut new_line = line.to_string();

        if line.starts_with("[charaSet ") || line.starts_with("[charaSet") {
            // Replace the name at the end of the charaSet command
            for (orig, trans) in name_map {
                if let Some(name) = extract_chara_name(line) {
                    if name == orig {
                        // Replace the last occurrence of the name
                        if let Some(pos) = line.rfind(orig) {
                            new_line = format!("{}{}{}", &line[..pos], trans, &line[pos + orig.len()..]);
                        }
                    }
                }
            }
        } else if line.starts_with('＠') {
            // Replace speaker name in the ＠ line
            // Handle: ＠Name, ＠Slot：Name, ＠[color]Name[-]
            let name_part = &line['＠'.len_utf8()..].trim();
            for (orig, trans) in name_map {
                if name_part.contains(orig.as_str()) {
                    new_line = line.replace(orig.as_str(), trans.as_str());
                    break;
                }
            }
        }

        result.push_str(&new_line);
        result.push_str(newline);
    }
    // Last line (no trailing newline)
    result.push_str(remaining);

    result
}

// ── CLI: Compare ──

/// Compare JP and CN script directories, copy JP-only scripts to output.
pub fn cmd_compare(jp_dir: &str, cn_dir: &str, output_dir: &str) -> anyhow::Result<()> {
    let jp = std::path::Path::new(jp_dir);
    let cn = std::path::Path::new(cn_dir);
    let out = std::path::Path::new(output_dir);
    std::fs::create_dir_all(out)?;

    // Build set of CN script filenames
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

    println!(
        "Copied {} JP-only scripts to {}",
        copied, output_dir
    );
    Ok(())
}

// ── CLI: Dedup ──

/// Remove files from translated directory that also exist in CN directory.
pub fn cmd_dedup(cn_dir: &str, translated_dir: &str) -> anyhow::Result<()> {
    let cn = std::path::Path::new(cn_dir);
    let translated = std::path::Path::new(translated_dir);

    // Build set of CN script filenames
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

    println!(
        "Removed {} files from {} (already exist in CN)",
        removed, translated_dir
    );
    Ok(())
}

// ── CLI: Deharmonize ──

/// Mapping of CN harmonized names → original names.
/// Used to revert bilibili content changes in exported text.
static DEHARMONIZE_MAP: &[(&str, &str)] = &[
    ("匕见", "荆轲"),
    ("虎狼", "吕布"),
    ("周照", "武则天"),
    ("莲偶", "哪吒"),
    ("重瞳", "项羽"),
    ("忠贞", "秦良玉"),
    ("祖政", "始皇帝"),
    ("雏罂", "虞美人"),
    ("丹驹", "赤兔马"),
    ("晋帝", "司马懿"),
    ("琰女", "杨贵妃"),
    ("瞑生院", "杀生院"),
    ("歌果", "美杜莎"),
    ("爱迪·萨奇", "爱德华·蒂奇"),
    ("雾都弃子", "开膛手杰克"),
    ("西行者", "玄奘三藏"),
    ("方巿", "徐福"),
    ("吾绰", "呼延灼"),
    ("暗匿者", "暗杀者"),
];

/// Apply anti-harmonization replacements to CN script txt files.
pub fn cmd_deharmonize(input_dir: &str, output_dir: &str) -> anyhow::Result<()> {
    let input = std::path::Path::new(input_dir);
    let output = std::path::Path::new(output_dir);
    std::fs::create_dir_all(output)?;

    let mut processed = 0u64;
    for entry in std::fs::read_dir(input)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "txt") {
            let fname = entry.file_name().to_string_lossy().to_string();
            let mut content = std::fs::read_to_string(&path)?;

            for &(harmonized, original) in DEHARMONIZE_MAP {
                content = content.replace(harmonized, original);
            }

            let out_path = output.join(&fname);
            std::fs::write(&out_path, &content)?;
            processed += 1;
        }
    }

    println!(
        "Deharmonized {} files → {}",
        processed, output_dir
    );
    Ok(())
}

// ── CLI: Scan Names ──

/// Output entry for the names.json mapping file.
#[derive(Debug, Serialize, Deserialize)]
struct NameEntry {
    src: String,
    dst: String,
    #[serde(default)]
    info: String,
}

/// Scan JP+CN scripts for character name mappings, merge with Chaldea svt_names.json.
pub fn cmd_scan_names(jp_dir: &str, cn_dir: &str, mappings_dir: Option<&str>, output_path: &str) -> anyhow::Result<()> {
    let jp = std::path::Path::new(jp_dir);
    let cn = std::path::Path::new(cn_dir);

    // Helper: extract charaSet entries (slot, id, num, name) from a script.
    #[derive(Debug, Clone)]
    struct CharaEntry {
        key: String, // "SLOT ID NUM" (without name)
        name: String,
    }

    fn extract_chara_entries(content: &str) -> Vec<CharaEntry> {
        let mut entries = Vec::new();
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with("[charaSet ") {
                // Parse: [charaSet SLOT ID NUM NAME]
                let inner = &line[1..]; // strip '['
                if let Some(end) = inner.rfind(']') {
                    let inner = &inner[..end];
                    let parts: Vec<&str> = inner.splitn(5, ' ').collect();
                    if parts.len() >= 5 {
                        let key = format!("{} {} {}", parts[1], parts[2], parts[3]);
                        let name = parts[4].trim().to_string();
                        if !name.is_empty() {
                            entries.push(CharaEntry { key, name });
                        }
                    }
                }
            }
        }
        entries
    }

    // Helper: extract dialogue speaker names (in order)
    fn extract_speakers(content: &str) -> Vec<String> {
        let mut names = Vec::new();
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('＠') {
                let raw = &line['＠'.len_utf8()..];
                let name = extract_speaker_name(raw);
                if !name.is_empty() {
                    names.push(name);
                }
            }
        }
        names
    }

    let mut name_map: std::collections::BTreeMap<String, (String, String)> = std::collections::BTreeMap::new();
    // value = (dst, info)

    let mut matched_scripts = 0u64;
    let mut total_pairs = 0u64;

    // Scan all script pairs
    for entry in std::fs::read_dir(jp)? {
        let entry = entry?;
        let fname = entry.file_name().to_string_lossy().to_string();
        if !fname.ends_with(".txt") {
            continue;
        }
        let cn_path = cn.join(&fname);
        if !cn_path.exists() {
            continue;
        }

        let jp_content = std::fs::read_to_string(entry.path())?;
        let cn_content = std::fs::read_to_string(&cn_path)?;

        // Match charaSet entries by (slot, id, num) key
        let jp_chara = extract_chara_entries(&jp_content);
        let cn_chara = extract_chara_entries(&cn_content);

        let cn_chara_map: std::collections::HashMap<String, String> = cn_chara
            .iter()
            .map(|e| (e.key.clone(), e.name.clone()))
            .collect();

        for je in &jp_chara {
            if let Some(cn_name) = cn_chara_map.get(&je.key) {
                if je.name != *cn_name && !je.name.is_empty() && !cn_name.is_empty() {
                    name_map
                        .entry(je.name.clone())
                        .or_insert_with(|| (cn_name.clone(), format!("script:{fname}")));
                    total_pairs += 1;
                }
            }
        }

        // Match speaker names by position
        let jp_speakers = extract_speakers(&jp_content);
        let cn_speakers = extract_speakers(&cn_content);
        let n = jp_speakers.len().min(cn_speakers.len());
        for i in 0..n {
            if jp_speakers[i] != cn_speakers[i]
                && !jp_speakers[i].is_empty()
                && !cn_speakers[i].is_empty()
            {
                name_map
                    .entry(jp_speakers[i].clone())
                    .or_insert_with(|| (cn_speakers[i].clone(), format!("script:{fname}")));
                total_pairs += 1;
            }
        }

        matched_scripts += 1;
    }

    log::info!(
        "Script scan: {} scripts, {} name pairs",
        matched_scripts, total_pairs
    );

    // Load Chaldea svt_names.json if available
    if let Some(mappings) = mappings_dir {
        let svt_path = std::path::Path::new(mappings).join("svt_names.json");
        if svt_path.exists() {
            let svt_json = std::fs::read_to_string(&svt_path)?;
            let svt_data: serde_json::Value = serde_json::from_str(&svt_json)?;
            if let Some(obj) = svt_data.as_object() {
                for (jp_name, lang_obj) in obj {
                    if let Some(cn_name) = lang_obj.get("CN").and_then(|v| v.as_str()) {
                        if !cn_name.is_empty() && jp_name != cn_name {
                            name_map
                                .entry(jp_name.clone())
                                .or_insert_with(|| (cn_name.to_string(), "Chaldea".to_string()));
                        }
                    }
                }
            }
            log::info!("Loaded Chaldea svt_names.json");
        }
    }

    // Convert to sorted output list
    let entries: Vec<NameEntry> = name_map
        .into_iter()
        .map(|(src, (dst, info))| NameEntry { src, dst, info })
        .collect();

    log::info!("Total unique name mappings: {}", entries.len());

    let json = serde_json::to_string_pretty(&entries)?;
    std::fs::write(output_path, &json)?;

    println!(
        "Exported {} name mappings to {}",
        entries.len(),
        output_path
    );
    Ok(())
}

// ── CLI: Export ──

/// Export scripts to translation-friendly flat JSON.
/// Each .txt produces two files:
///   xxx.json       — dialogue lines + choice options  [{name, original, message}]
///   xxx_char.json  — unique character names           [{name:"", original, message}]
pub fn cmd_export(input_dir: &str, output_dir: &str) -> anyhow::Result<()> {
    let input = std::path::Path::new(input_dir);
    let output = std::path::Path::new(output_dir);
    std::fs::create_dir_all(output)?;

    let mut total_files = 0u64;
    let mut total_lines = 0u64;

    for entry in std::fs::read_dir(input)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "txt") {
            let stem = path.file_stem().unwrap().to_string_lossy();
            let content = std::fs::read_to_string(&path)?;
            let blocks = parse_script(&content)
                .map_err(|e| anyhow::anyhow!("Parse error in {}: {}", stem, e))?;

            // Collect unique character names from charaSet commands and dialogue blocks.
            // Use IndexSet-like ordering: first appearance wins.
            let mut char_names: Vec<String> = Vec::new();
            let mut char_seen: std::collections::HashSet<String> = std::collections::HashSet::new();
            let add_char = |names: &mut Vec<String>, seen: &mut std::collections::HashSet<String>, name: &str| {
                let trimmed = name.trim();
                if !trimmed.is_empty() && seen.insert(trimmed.to_string()) {
                    names.push(trimmed.to_string());
                }
            };

            for block in &blocks {
                match block {
                    Block::Command(cmd) => {
                        if let Some(name) = extract_chara_name(cmd) {
                            add_char(&mut char_names, &mut char_seen, name);
                        }
                    }
                    Block::Dialogue { speaker_name, .. } => {
                        add_char(&mut char_names, &mut char_seen, speaker_name);
                    }
                    _ => {}
                }
            }

            // Build xxx.json: all dialogue lines + choice options
            let mut messages: Vec<MessageEntry> = Vec::new();

            for block in &blocks {
                match block {
                    Block::Dialogue { speaker_name, lines, .. } => {
                        for line in lines {
                            messages.push(MessageEntry::new(speaker_name, line));
                        }
                    }
                    Block::Choice { options } => {
                        for opt in options {
                            messages.push(MessageEntry::new("", opt));
                        }
                    }
                    _ => {}
                }
            }

            // Build xxx_char.json: unique character names
            let char_entries: Vec<MessageEntry> = char_names
                .iter()
                .map(|n| MessageEntry::new("", n))
                .collect();

            total_lines += messages.len() as u64;

            // Write xxx.json
            let msg_path = output.join(format!("{}.json", stem));
            let msg_json = serde_json::to_string_pretty(&messages)?;
            std::fs::write(&msg_path, msg_json)?;

            // Write xxx_char.json
            if !char_entries.is_empty() {
                let char_path = output.join(format!("{}_char.json", stem));
                let char_json = serde_json::to_string_pretty(&char_entries)?;
                std::fs::write(&char_path, char_json)?;
            }

            total_files += 1;
        }
    }

    println!(
        "Exported {} files, {} message lines to {}",
        total_files, total_lines, output_dir
    );
    Ok(())
}

// ── CLI: Import ──

/// Import translated JSON back into .txt scripts.
/// Reads the new flat format: compares `original` vs `message` to find changes.
/// Also reads `_char.json` for speaker name translations if present.
pub fn cmd_import(json_dir: &str, original_dir: &str, output_dir: &str) -> anyhow::Result<()> {
    let json_dir = std::path::Path::new(json_dir);
    let original_dir = std::path::Path::new(original_dir);
    let output_dir = std::path::Path::new(output_dir);
    std::fs::create_dir_all(output_dir)?;

    let mut total_files = 0u64;
    let mut total_applied = 0u64;
    let mut total_skipped = 0u64;

    for entry in std::fs::read_dir(json_dir)? {
        let entry = entry?;
        let path = entry.path();
        let fname = path.file_stem().unwrap().to_string_lossy();

        // Skip _char.json files (processed alongside their sibling)
        if fname.ends_with("_char") || path.extension().is_some_and(|e| e != "json") {
            continue;
        }

        let stem = &fname;
        let json_content = std::fs::read_to_string(&path)?;
        let messages: Vec<MessageEntry> = serde_json::from_str(&json_content)?;

        // Read char.json if present
        let char_path = json_dir.join(format!("{}_char.json", stem));
        let char_entries: Vec<MessageEntry> = if char_path.exists() {
            let char_json = std::fs::read_to_string(&char_path)?;
            serde_json::from_str(&char_json).unwrap_or_default()
        } else {
            Vec::new()
        };

        let orig_path = original_dir.join(format!("{}.txt", stem));
        if !orig_path.exists() {
            log::warn!("Original script not found: {}.txt, skipping", stem);
            continue;
        }

        let orig_content = std::fs::read_to_string(&orig_path)?;

        // Strip ruby annotations — Chinese doesn't need them
        let orig_content = strip_ruby(&orig_content);

        let blocks = parse_script(&orig_content)
            .map_err(|e| anyhow::anyhow!("Parse error in original {}: {}", stem, e))?;

        // ── Step 1: Build name map from char_entries ──
        // Collect unique original names (same order as export)
        let mut orig_chars: Vec<String> = Vec::new();
        let mut char_seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        for block in &blocks {
            match block {
                Block::Command(cmd) => {
                    if let Some(name) = extract_chara_name(cmd) {
                        let trimmed = name.trim().to_string();
                        if !trimmed.is_empty() && char_seen.insert(trimmed.clone()) {
                            orig_chars.push(trimmed);
                        }
                    }
                }
                Block::Dialogue { speaker_name, .. } => {
                    let trimmed = speaker_name.trim().to_string();
                    if !trimmed.is_empty() && char_seen.insert(trimmed.clone()) {
                        orig_chars.push(trimmed);
                    }
                }
                _ => {}
            }
        }

        // Build name map from translated char entries
        let name_map: Vec<(String, String)> = orig_chars
            .iter()
            .zip(char_entries.iter())
            .filter(|(orig, entry)| entry.is_translated() && orig.as_str() != entry.message)
            .map(|(orig, entry)| (orig.clone(), entry.message.clone()))
            .collect();

        let mut content = if name_map.is_empty() {
            orig_content.clone()
        } else {
            replace_speaker_names(&orig_content, &name_map)
        };

        // ── Step 2: Walk blocks and entries in parallel ──
        // Build flat list from blocks (same order as export)
        // Each element: (block_idx, line_idx_in_block, original_text)
        struct Anchor {
            block_idx: usize,
            line_idx: usize,
            original: String,
        }
        let mut anchors: Vec<Anchor> = Vec::new();
        for (bi, block) in blocks.iter().enumerate() {
            match block {
                Block::Dialogue { lines, .. } => {
                    for (li, line) in lines.iter().enumerate() {
                        anchors.push(Anchor {
                            block_idx: bi,
                            line_idx: li,
                            original: line.clone(),
                        });
                    }
                }
                Block::Choice { options } => {
                    for (li, opt) in options.iter().enumerate() {
                        anchors.push(Anchor {
                            block_idx: bi,
                            line_idx: li,
                            original: opt.clone(),
                        });
                    }
                }
                _ => {}
            }
        }

        // Match entries with anchors by original text
        // Build a lookup: original_text → Vec<(entry_index, translated_message_in_fgo_format)>
        let translated_map: std::collections::HashMap<String, Vec<(usize, String)>> = {
            let mut m: std::collections::HashMap<String, Vec<(usize, String)>> =
                std::collections::HashMap::new();
            for (i, entry) in messages.iter().enumerate() {
                if entry.is_translated() {
                    m.entry(entry.original.clone())
                        .or_default()
                        .push((i, entry.message.clone()));
                }
            }
            m
        };

        // Collect substitutions: (block_idx, old_lines, new_lines)
        // Strategy: walk anchors in order, consume translated entries by matching original text
        let mut entry_consumed: std::collections::HashSet<usize> = std::collections::HashSet::new();

        // For each block, collect the per-line substitutions
        // Map block_idx → Vec<(line_idx, new_text)>
        let mut block_subs: std::collections::BTreeMap<usize, Vec<(usize, String)>> =
            std::collections::BTreeMap::new();

        for anchor in &anchors {
            if let Some(translations) = translated_map.get(&anchor.original) {
                // Find the first not-yet-consumed translation for this original text
                for (entry_idx, translated) in translations {
                    if !entry_consumed.contains(entry_idx) {
                        entry_consumed.insert(*entry_idx);

                        // Validate tags
                        let ok = {
                            let orig_vec = vec![anchor.original.clone()];
                            let trans_vec = vec![translated.clone()];
                            validate_tags(&orig_vec, &trans_vec).is_ok()
                        };

                        if ok {
                            block_subs
                                .entry(anchor.block_idx)
                                .or_default()
                                .push((anchor.line_idx, translated.clone()));
                            total_applied += 1;
                        } else {
                            log::error!(
                                "[{}] block[{}] line[{}]: TAG MISMATCH — skipped",
                                stem,
                                anchor.block_idx,
                                anchor.line_idx
                            );
                            total_skipped += 1;
                        }
                        break;
                    }
                }
            }
        }

        // ── Step 3: Apply substitutions to the already-name-replaced content ──
        for (block_idx, line_subs) in &block_subs {
            let block = &blocks[*block_idx];
            match block {
                Block::Dialogue { lines, .. } => {
                    let mut new_lines = lines.clone();
                    for (li, new_text) in line_subs {
                        if *li < new_lines.len() {
                            new_lines[*li] = new_text.clone();
                        }
                    }
                    if new_lines != *lines {
                        replace_block_text(&mut content, block, &new_lines);
                    }
                }
                Block::Choice { options } => {
                    let mut new_options = options.clone();
                    for (li, new_text) in line_subs {
                        if *li < new_options.len() {
                            new_options[*li] = new_text.clone();
                        }
                    }
                    if new_options != *options {
                        replace_block_text(&mut content, block, &new_options);
                    }
                }
                _ => {}
            }
        }

        // Write output
        let out_path = output_dir.join(format!("{}.txt", stem));
        std::fs::write(&out_path, &content)?;
        total_files += 1;
    }

    println!(
        "Imported {} files to {} ({} messages applied, {} skipped)",
        total_files, output_dir.display(), total_applied, total_skipped
    );
    Ok(())
}

/// Replace text content within a block in the original script content.
fn replace_block_text(content: &mut String, block: &Block, new_lines: &[String]) {
    match block {
        Block::Dialogue { lines, .. } => {
            if lines.len() != new_lines.len() {
                log::warn!("Line count changed ({} → {}), skipping replacement", lines.len(), new_lines.len());
                return;
            }
            for (orig, new) in lines.iter().zip(new_lines.iter()) {
                if orig != new {
                    if let Some(pos) = content.find(orig.as_str()) {
                        content.replace_range(pos..pos + orig.len(), new);
                    }
                }
            }
        }
        Block::Choice { options } => {
            if options.len() != new_lines.len() {
                log::warn!("Choice count changed ({} → {}), skipping replacement", options.len(), new_lines.len());
                return;
            }
            for (orig, new) in options.iter().zip(new_lines.iter()) {
                if orig != new {
                    if let Some(pos) = content.find(orig.as_str()) {
                        content.replace_range(pos..pos + orig.len(), new);
                    }
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_header() {
        let input = "＄01-00-00-00-1-0\n[soundStopAll]\n[end]\n";
        let result = parse_script(input).unwrap();
        assert_eq!(result.len(), 3);
        assert!(matches!(&result[0], Block::Header(_)));
        assert!(matches!(&result[1], Block::Command(c) if c == "soundStopAll"));
        assert!(matches!(&result[2], Block::Command(c) if c == "end"));
    }

    #[test]
    fn test_parse_dialogue_simple() {
        let input = "＠マシュ\nこんにちは。\n[k]\n";
        let result = parse_script(input).unwrap();
        assert_eq!(result.len(), 1);
        if let Block::Dialogue { speaker_name, lines, .. } = &result[0] {
            assert_eq!(speaker_name, "マシュ");
            assert_eq!(lines.len(), 1);
            assert_eq!(lines[0], "こんにちは。");
        } else {
            panic!("Expected Dialogue, got {:?}", result[0]);
        }
    }

    #[test]
    fn test_parse_dialogue_color_speaker() {
        let input = "＠[51d4ff]アナウンス[-]\n[51d4ff]ようこそ。[-]\n[k]\n";
        let result = parse_script(input).unwrap();
        if let Block::Dialogue { speaker_name, lines, .. } = &result[0] {
            assert_eq!(speaker_name, "アナウンス");
            assert_eq!(lines[0], "[51d4ff]ようこそ。[-]");
        } else {
            panic!("Expected Dialogue");
        }
    }

    #[test]
    fn test_parse_choices() {
        let input = "？1：はい\n？2：いいえ\n？！\n";
        let result = parse_script(input).unwrap();
        assert_eq!(result.len(), 1);
        if let Block::Choice { options } = &result[0] {
            assert_eq!(options.len(), 2);
            assert_eq!(options[0], "はい");
            assert_eq!(options[1], "いいえ");
        } else {
            panic!("Expected Choice");
        }
    }

    #[test]
    fn test_parse_full_script_0100000010() {
        let path = "data/jp/scripts/0100000010.txt";
        let input = std::fs::read_to_string(path).unwrap();
        let result = parse_script(&input);
        match result {
            Ok(blocks) => {
                println!("0100000010: Parsed {} blocks", blocks.len());
                for block in &blocks {
                    match block {
                        Block::Dialogue { speaker_name, lines, .. } => {
                            println!("  DIALOGUE: {} ({} lines)", speaker_name, lines.len());
                        }
                        Block::Choice { options } => {
                            println!("  CHOICE: {} options", options.len());
                        }
                        Block::Command(cmd) => {
                            println!("  COMMAND: {}", cmd);
                        }
                        Block::Header(h) => {
                            println!("  HEADER: {}", h);
                        }
                    }
                }
            }
            Err(e) => {
                panic!("Parse error: {}", e);
            }
        }
    }

    #[test]
    fn test_parse_full_script_0100000011() {
        let path = "data/jp/scripts/0100000011.txt";
        let input = std::fs::read_to_string(path).unwrap();
        let result = parse_script(&input);
        match result {
            Ok(blocks) => {
                println!("0100000011: Parsed {} blocks", blocks.len());
            }
            Err(e) => {
                panic!("Parse error: {}", e);
            }
        }
    }

    /// Test parsing ALL JP scripts to ensure the parser handles every file.
    #[test]
    fn test_parse_all_jp_scripts() {
        let dir = "data/jp/scripts";
        let mut failures = Vec::new();
        let mut total_dialogues = 0;
        let mut total_choices = 0;
        let mut total_commands = 0;
        
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "txt") {
                let input = std::fs::read_to_string(&path).unwrap();
                match parse_script(&input) {
                    Ok(blocks) => {
                        for block in &blocks {
                            match block {
                                Block::Dialogue { .. } => total_dialogues += 1,
                                Block::Choice { .. } => total_choices += 1,
                                Block::Command(_) => total_commands += 1,
                                _ => {}
                            }
                        }
                    }
                    Err(e) => {
                        failures.push((path.file_name().unwrap().to_string_lossy().to_string(), e));
                    }
                }
            }
        }
        
        println!("Total: {} dialogues, {} choices, {} commands", 
                 total_dialogues, total_choices, total_commands);
        
        if !failures.is_empty() {
            for (name, err) in &failures {
                eprintln!("FAIL {}: {}", name, err);
            }
            panic!("{} scripts failed to parse", failures.len());
        } else {
            println!("All scripts parsed successfully!");
        }
    }
}
