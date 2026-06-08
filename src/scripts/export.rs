//! Export dialogue and choice text to translation-friendly flat JSON.

use crate::scripts::message::MessageEntry;
use crate::scripts::parser::{Block, parse_script};

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
            let mut char_names: Vec<String> = Vec::new();
            let mut char_seen: std::collections::HashSet<String> = std::collections::HashSet::new();
            let add_char = |names: &mut Vec<String>,
                            seen: &mut std::collections::HashSet<String>,
                            name: &str| {
                let trimmed = name.trim();
                if !trimmed.is_empty() && seen.insert(trimmed.to_string()) {
                    names.push(trimmed.to_string());
                }
            };

            for block in &blocks {
                match block {
                    Block::Command(cmd) => {
                        if let Some(name) = crate::scripts::parser::extract_chara_name(cmd) {
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
                    Block::Dialogue {
                        speaker_name,
                        lines,
                        ..
                    } => {
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

            let msg_path = output.join(format!("{}.json", stem));
            let msg_json = serde_json::to_string_pretty(&messages)?;
            std::fs::write(&msg_path, msg_json)?;

            if !char_entries.is_empty() {
                let char_path = output.join(format!("{}_char.json", stem));
                let char_json = serde_json::to_string_pretty(&char_entries)?;
                std::fs::write(&char_path, char_json)?;
            }

            total_files += 1;
        }
    }

    log::info!(
        "Exported {} files, {} message lines to {}",
        total_files,
        total_lines,
        output_dir
    );
    Ok(())
}
