//! Import translated JSON back into .txt scripts.

use crate::scripts::message::{json_entry_line_starts, strip_ruby, validate_tags, MessageEntry};
use crate::scripts::names::replace_speaker_names;
use crate::scripts::parser::{extract_chara_name, parse_script, Block};

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
        let json_line_starts = json_entry_line_starts(&json_content);
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
        let orig_content = strip_ruby(&orig_content);

        let blocks = parse_script(&orig_content)
            .map_err(|e| anyhow::anyhow!("Parse error in original {}: {}", stem, e))?;

        // ── Step 1: Build name map from char_entries ──
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

        struct TagError {
            json_line: usize,
            original: String,
            translated: String,
        }
        let mut tag_errors: Vec<TagError> = Vec::new();

        let mut entry_consumed: std::collections::HashSet<usize> = std::collections::HashSet::new();
        let mut block_subs: std::collections::BTreeMap<usize, Vec<(usize, String)>> =
            std::collections::BTreeMap::new();

        for anchor in &anchors {
            if let Some(translations) = translated_map.get(&anchor.original) {
                for (entry_idx, translated) in translations {
                    if !entry_consumed.contains(entry_idx) {
                        entry_consumed.insert(*entry_idx);

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
                            let json_line = json_line_starts.get(*entry_idx).copied().unwrap_or(0);
                            tag_errors.push(TagError {
                                json_line,
                                original: anchor.original.clone(),
                                translated: translated.clone(),
                            });
                            total_skipped += 1;
                        }
                        break;
                    }
                }
            }
        }

        // Report tag errors grouped by file, showing JSON line number
        if !tag_errors.is_empty() {
            eprintln!("{}:", stem);
            for err in &tag_errors {
                eprintln!("  L{}: TAG MISMATCH", err.json_line);
                eprintln!("    original  : {}", err.original);
                eprintln!("    translated: {}", err.translated);
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
        total_files,
        output_dir.display(),
        total_applied,
        total_skipped
    );
    Ok(())
}

/// Replace text content within a block in the original script content.
fn replace_block_text(content: &mut String, block: &Block, new_lines: &[String]) {
    match block {
        Block::Dialogue { lines, .. } => {
            if lines.len() != new_lines.len() {
                log::warn!(
                    "Line count changed ({} → {}), skipping replacement",
                    lines.len(),
                    new_lines.len()
                );
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
                log::warn!(
                    "Choice count changed ({} → {}), skipping replacement",
                    options.len(),
                    new_lines.len()
                );
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
