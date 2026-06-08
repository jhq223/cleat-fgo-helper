//! .script Bundle format — bidirectional conversion between .txt and .script.

pub mod read;
pub mod write;

use std::path::Path;

/// Pack .txt files into a .script bundle.
pub fn cmd_txt_to_bundle(input: &str, output: &str) -> anyhow::Result<()> {
    write::pack_dir(Path::new(input), Path::new(output)).map_err(|e| anyhow::anyhow!("{e}"))?;
    log::info!("Packed to {output}");
    Ok(())
}

/// Unpack a .script bundle to .txt files.
pub fn cmd_bundle_to_txt(input: &str, output: &str) -> anyhow::Result<()> {
    let bundle =
        read::ScriptBundle::load_file(Path::new(input)).map_err(|e| anyhow::anyhow!("{e}"))?;

    let out_dir = Path::new(output);
    std::fs::create_dir_all(out_dir)?;

    for (id, text) in &bundle.entries {
        let path = out_dir.join(format!("{id}.txt"));
        std::fs::write(&path, text)?;
    }

    log::info!("Extracted {} scripts to {}", bundle.len(), output);
    Ok(())
}
