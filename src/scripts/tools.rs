//! Directory comparison and deduplication utilities.

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

    println!("Copied {} JP-only scripts to {}", copied, output_dir);
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

    println!(
        "Removed {} files from {} (already exist in CN)",
        removed, translated_dir
    );
    Ok(())
}
