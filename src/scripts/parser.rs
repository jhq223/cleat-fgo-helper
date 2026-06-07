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
            Block::Dialogue {
                speaker_name,
                lines,
                ..
            } => {
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
                            if lines.is_empty() {
                                blocks.push(Block::Command(cmd.clone()));
                                i += 1;
                            } else {
                                lines.push(format!("[{}]", cmd));
                                i += 1;
                            }
                        }
                        Tag::Blank => {
                            i += 1;
                        }
                        Tag::Header(_) => {
                            break;
                        }
                    }
                }

                let lines: Vec<String> =
                    lines.into_iter().filter(|l| !l.trim().is_empty()).collect();

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
                        }
                        _ => {
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
                i += 1;
            }
            Tag::KeyWait => {
                i += 1;
            }
            Tag::Blank => {
                i += 1;
            }
            Tag::Text(t) => {
                blocks.push(Block::Command(format!("__text__:{}", t)));
                i += 1;
            }
        }
    }

    Ok(blocks)
}

/// Extract the display speaker name from a raw speaker line.
pub fn extract_speaker_name(raw: &str) -> String {
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
                if i < chars.len() {
                    i += 1;
                }
            } else {
                name.push(chars[i]);
                i += 1;
            }
        }
        let name = name.trim().to_string();
        if !name.is_empty() {
            return name;
        }
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

/// Extract the display name from a `charaSet` command.
/// Format: `charaSet SLOT ID NUM NAME`
/// e.g. `charaSet A 98001000 0 マシュ` → `マシュ`
pub fn extract_chara_name(cmd: &str) -> Option<&str> {
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
        if let Block::Dialogue {
            speaker_name,
            lines,
            ..
        } = &result[0]
        {
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
        if let Block::Dialogue {
            speaker_name,
            lines,
            ..
        } = &result[0]
        {
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
}
