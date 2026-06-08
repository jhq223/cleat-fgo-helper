//! Merge translated JSON (message-only) into original exported JSON.

use crate::scripts::message::MessageEntry;

/// Merge translated JSON (message-only, from tools like AiNiee) into the
/// original exported JSON (full format with name/original/message).
/// Matching is by index — entries must be in the same order.
pub fn cmd_merge(translated_dir: &str, original_dir: &str, output_dir: &str) -> anyhow::Result<()> {
    let translated = std::path::Path::new(translated_dir);
    let original = std::path::Path::new(original_dir);
    let output = std::path::Path::new(output_dir);
    std::fs::create_dir_all(output)?;

    let mut merged_count = 0u64;
    let mut total_messages = 0u64;

    for entry in std::fs::read_dir(translated)? {
        let entry = entry?;
        let path = entry.path();
        let fname = entry.file_name().to_string_lossy().to_string();

        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }

        let orig_path = original.join(&fname);
        if !orig_path.exists() {
            log::warn!("Original not found for {}, skipping", fname);
            continue;
        }

        let orig_json = std::fs::read_to_string(&orig_path)?;
        let mut orig_entries: Vec<MessageEntry> = serde_json::from_str(&orig_json)?;

        let tr_json = std::fs::read_to_string(&path)?;
        let tr_values: Vec<serde_json::Value> = serde_json::from_str(&tr_json)?;

        if orig_entries.len() != tr_values.len() {
            log::warn!(
                "{}: entry count mismatch (original {} vs translated {}), skipping",
                fname,
                orig_entries.len(),
                tr_values.len()
            );
            continue;
        }

        for (i, tr_val) in tr_values.iter().enumerate() {
            if let Some(msg) = tr_val.get("message").and_then(|v| v.as_str())
                && orig_entries[i].message != msg
            {
                orig_entries[i].message = msg.to_string();
                total_messages += 1;
            }
        }

        let out_path = output.join(&fname);
        let out_json = serde_json::to_string_pretty(&orig_entries)?;
        std::fs::write(&out_path, out_json)?;
        merged_count += 1;
    }

    log::info!(
        "Merged {} files to {} ({} messages updated)",
        merged_count,
        output_dir,
        total_messages
    );
    Ok(())
}
