//! A single reproducible path from verified server resources to module updates.
use crate::{res, scripts, storage};
use serde::Serialize;
use std::{collections::BTreeMap, path::Path};

#[derive(Serialize)]
struct ResourceFile {
    name: String,
    url: String,
    size: u64,
    sha256: String,
}
#[derive(Serialize)]
struct Manifest {
    schema: u32,
    version: String,
    files: Vec<ResourceFile>,
}
#[derive(Serialize, Default)]
struct Report {
    jp_snapshot: String,
    cn_snapshot: String,
    translated: usize,
    missing_cn: Vec<String>,
    needs_review: BTreeMap<String, String>,
}

pub async fn build(
    offline: bool,
    base_url: &str,
    output: &str,
    font: Option<&str>,
) -> anyhow::Result<()> {
    if !offline {
        res::cmd_download("jp", false, false).await?;
        res::cmd_download("cn", false, false).await?;
    }
    let jp = res::snapshot::active("jp")?;
    let cn = res::snapshot::active("cn")?;
    let output = Path::new(output);
    std::fs::create_dir_all(output)?;
    let stage = tempfile::tempdir_in(output)?;
    let story_dir = stage.path().join("scripts");
    std::fs::create_dir(&story_dir)?;
    let mut report = Report {
        jp_snapshot: snapshot_id(&jp),
        cn_snapshot: snapshot_id(&cn),
        ..Report::default()
    };
    let mut originals: Vec<_> =
        std::fs::read_dir(jp.join("scripts"))?.collect::<std::io::Result<_>>()?;
    originals.sort_by_key(|entry| entry.file_name());
    for entry in originals {
        let path = entry.path();
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        if id == "ScriptFileList" || path.extension().is_none_or(|e| e != "txt") {
            continue;
        }
        let cn_path = cn.join("scripts").join(entry.file_name());
        if !cn_path.is_file() {
            report.missing_cn.push(id.to_owned());
            continue;
        }
        let original = std::fs::read_to_string(&path)?;
        let translation = scripts::names::deharmonize_text(&std::fs::read_to_string(cn_path)?);
        match scripts::align::merge(&original, &translation) {
            Ok(text) if text != original => {
                crate::crypto::script::validate(id, &text).map_err(anyhow::Error::msg)?;
                std::fs::write(story_dir.join(entry.file_name()), text)?;
                report.translated += 1;
            }
            Ok(_) => {}
            Err(reason) => {
                report.needs_review.insert(id.to_owned(), reason);
            }
        }
    }
    let localization = stage.path().join("LocalizationJpn.txt");
    scripts::cmd_localize(
        &jp.to_string_lossy(),
        &cn.to_string_lossy(),
        &localization.to_string_lossy(),
    )?;
    let bundle = stage.path().join("stories.script");
    crate::bundle::write::pack_dir(&story_dir, &bundle).map_err(anyhow::Error::msg)?;
    let mut paths = vec![localization, bundle];
    if let Some(font) = font {
        let font = Path::new(font);
        anyhow::ensure!(
            std::fs::metadata(font)?.len() <= 128 * 1024 * 1024,
            "font exceeds module size limit"
        );
        let bytes = std::fs::read(font)?;
        anyhow::ensure!(
            bytes.starts_with(b"UnityFS\0"),
            "Font must be a Unity AssetBundle"
        );
        let target = stage.path().join("Font");
        std::fs::write(&target, bytes)?;
        paths.push(target);
    }
    let mut files = Vec::new();
    for path in paths {
        let bytes = std::fs::read(&path)?;
        files.push(ResourceFile {
            name: path.file_name().unwrap().to_string_lossy().into_owned(),
            url: String::new(),
            size: bytes.len() as u64,
            sha256: storage::digest(&bytes),
        });
    }
    files.sort_by(|a, b| a.name.cmp(&b.name));
    let version = storage::digest(&serde_json::to_vec(&files)?)[..24].to_owned();
    let base = reqwest::Url::parse(&base_url.replace("{version}", &version))?;
    anyhow::ensure!(
        base.scheme() == "https"
            && base.username().is_empty()
            && base.password().is_none()
            && base.query().is_none()
            && base.fragment().is_none()
            && base.path().ends_with('/'),
        "base URL must be an HTTPS directory without credentials, query or fragment"
    );
    for file in &mut files {
        file.url = base.join(&file.name)?.to_string();
    }
    let manifest = Manifest {
        schema: 1,
        version: version.clone(),
        files,
    };
    let destination = output.join(&version);
    std::fs::create_dir_all(&destination)?;
    for file in &manifest.files {
        storage::atomic_write(
            &destination.join(&file.name),
            &std::fs::read(stage.path().join(&file.name))?,
        )?;
    }
    storage::atomic_write(
        &destination.join("report.json"),
        &serde_json::to_vec_pretty(&report)?,
    )?;
    storage::atomic_write(
        &destination.join("manifest.json"),
        &serde_json::to_vec_pretty(&manifest)?,
    )?;
    storage::atomic_write(&output.join("latest.txt"), version.as_bytes())?;
    log::info!(
        "Resource pack {version}: {} translated stories, {} without CN text, {} require review",
        report.translated,
        report.missing_cn.len(),
        report.needs_review.len()
    );
    log::info!("Manifest: {}", destination.join("manifest.json").display());
    Ok(())
}

fn snapshot_id(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}
