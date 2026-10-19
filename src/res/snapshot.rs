//! Publish only fully validated resource sets; failed runs keep the active set.
use crate::{
    error::{Error, Result},
    storage,
};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

#[derive(Serialize, Deserialize)]
struct File {
    path: String,
    size: u64,
    sha256: String,
}
#[derive(Serialize, Deserialize)]
struct Snapshot {
    schema: u32,
    scripts: bool,
    version: super::version::VersionInfo,
    files: Vec<File>,
}

fn valid_id(id: &str) -> bool {
    id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

pub fn active(server: &str) -> Result<PathBuf> {
    let base = Path::new("data").join(server);
    let id = std::fs::read_to_string(base.join("current.txt"))?;
    load(&base, id.trim(), false)
}

pub fn load(base: &Path, id: &str, no_scripts: bool) -> Result<PathBuf> {
    if !valid_id(id) {
        return Err(Error::Parse("invalid snapshot ID".into()));
    }
    let root = base.join("snapshots").join(id);
    let manifest: Snapshot = serde_json::from_slice(&std::fs::read(root.join("snapshot.json"))?)?;
    if manifest.schema != 1 || (!no_scripts && !manifest.scripts) || manifest.files.is_empty() {
        return Err(Error::Parse("incomplete resource snapshot".into()));
    }
    for file in manifest.files {
        let path = Path::new(&file.path);
        if path
            .components()
            .any(|p| !matches!(p, Component::Normal(_)))
            || file.size > 64 * 1024 * 1024
        {
            return Err(Error::Parse("invalid snapshot file".into()));
        }
        let path = root.join(path);
        if std::fs::metadata(&path)?.len() != file.size
            || storage::digest(&std::fs::read(path)?) != file.sha256
        {
            return Err(Error::Parse(format!(
                "snapshot checksum mismatch: {}",
                file.path
            )));
        }
    }
    Ok(root)
}

pub fn activate(base: &Path, id: &str) -> Result<()> {
    if !valid_id(id) {
        return Err(Error::Parse("invalid snapshot ID".into()));
    }
    storage::atomic_write(&base.join("current.txt"), id.as_bytes())
}

pub fn publish(
    base: &Path,
    id: &str,
    staging: tempfile::TempDir,
    version: &super::version::VersionInfo,
    scripts: bool,
) -> Result<()> {
    if !valid_id(id) {
        return Err(Error::Parse("invalid snapshot ID".into()));
    }
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(staging.path()) {
        let entry = entry.map_err(|e| Error::Parse(e.to_string()))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry
            .path()
            .strip_prefix(staging.path())
            .map_err(|e| Error::Parse(e.to_string()))?;
        let bytes = std::fs::read(entry.path())?;
        files.push(File {
            path: path.to_string_lossy().replace('\\', "/"),
            size: bytes.len() as u64,
            sha256: storage::digest(&bytes),
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let manifest = Snapshot {
        schema: 1,
        scripts,
        version: version.clone(),
        files,
    };
    storage::atomic_write(
        &staging.path().join("snapshot.json"),
        &serde_json::to_vec_pretty(&manifest)?,
    )?;
    let parent = base.join("snapshots");
    std::fs::create_dir_all(&parent)?;
    let destination = parent.join(id);
    if destination.exists() {
        // Preserve a corrupt/incomplete previous set for diagnostics. The active
        // pointer changes only after the replacement has been fully published.
        let quarantine = tempfile::Builder::new()
            .prefix("superseded-")
            .tempdir_in(&parent)?;
        std::fs::rename(&destination, quarantine.path().join("snapshot"))?;
        let _ = quarantine.keep();
    }
    std::fs::rename(staging.path(), &destination)?;
    activate(base, id)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshot_rejects_paths_outside_its_root() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(dir.path(), "../other", true).is_err());
        assert!(activate(dir.path(), "../other").is_err());
    }
}
