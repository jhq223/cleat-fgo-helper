//! Bounded downloads and versioned, verified cache files.
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{io::Write, path::Path};

pub fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| Error::Io(e.error))?;
    Ok(())
}

#[derive(Serialize, Deserialize)]
struct Receipt {
    source: String,
    size: usize,
    sha256: String,
}

pub fn read_cache(path: &Path, source: &str, limit: usize) -> Option<Vec<u8>> {
    let receipt: Receipt =
        serde_json::from_slice(&std::fs::read(path.with_extension("receipt.json")).ok()?).ok()?;
    if receipt.source != source
        || receipt.size > limit
        || std::fs::metadata(path).ok()?.len() != receipt.size as u64
    {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    (digest(&bytes) == receipt.sha256).then_some(bytes)
}

pub fn write_cache(path: &Path, source: &str, bytes: &[u8]) -> Result<()> {
    atomic_write(path, bytes)?;
    let receipt = Receipt {
        source: source.into(),
        size: bytes.len(),
        sha256: digest(bytes),
    };
    atomic_write(
        &path.with_extension("receipt.json"),
        &serde_json::to_vec(&receipt)?,
    )
}

pub async fn get(client: &reqwest::Client, url: &str, limit: usize) -> Result<Vec<u8>> {
    let mut response = client.get(url).send().await?.error_for_status()?;
    if response.content_length().is_some_and(|n| n > limit as u64) {
        return Err(Error::Parse("download exceeds size limit".into()));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if chunk.len() > limit.saturating_sub(bytes.len()) {
            return Err(Error::Parse("download exceeds size limit".into()));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cache_rejects_changed_versions_and_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("resource.bin");
        write_cache(&path, "v1", b"complete").unwrap();
        assert_eq!(read_cache(&path, "v1", 100).unwrap(), b"complete");
        assert!(read_cache(&path, "v2", 100).is_none());
        assert!(read_cache(&path, "v1", 2).is_none());
        std::fs::write(&path, b"tampered").unwrap();
        assert!(read_cache(&path, "v1", 100).is_none());
    }
}
