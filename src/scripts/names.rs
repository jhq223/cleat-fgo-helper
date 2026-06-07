//! Speaker name handling: replacement, deharmonization, and scanning.

use crate::scripts::parser::extract_chara_name;
use serde::{Deserialize, Serialize};

/// Replace speaker names in charaSet commands and ＠ speaker lines.
pub fn replace_speaker_names(content: &str, name_map: &[(String, String)]) -> String {
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
            for (orig, trans) in name_map {
                if let Some(name) = extract_chara_name(line) {
                    if name == orig {
                        if let Some(pos) = line.rfind(orig) {
                            new_line =
                                format!("{}{}{}", &line[..pos], trans, &line[pos + orig.len()..]);
                        }
                    }
                }
            }
        } else if line.starts_with('＠') {
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
    result.push_str(remaining);

    result
}

// ── Deharmonize map ──

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

    println!("Deharmonized {} files → {}", processed, output_dir);
    Ok(())
}

// ── Scan Names ──

/// Output entry for the names.json mapping file.
#[derive(Debug, Serialize, Deserialize)]
pub struct NameEntry {
    pub src: String,
    pub dst: String,
    #[serde(default)]
    pub info: String,
}

/// Generate character name mappings from Chaldea svt_names.json only.
pub fn cmd_scan_names(
    mappings_dir: &str,
    output_path: &str,
) -> anyhow::Result<()> {
    use std::collections::BTreeMap;

    let mut name_map: BTreeMap<String, (String, String)> = BTreeMap::new();

    let svt_path = std::path::Path::new(mappings_dir).join("svt_names.json");
    if !svt_path.exists() {
        anyhow::bail!(
            "svt_names.json not found in {} – run `mappings download` first",
            mappings_dir
        );
    }

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

    log::info!("Loaded Chaldea svt_names.json: {} entries", name_map.len());

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

