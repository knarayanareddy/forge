use crate::model_registry::ModelProfile;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct DownloadPlan {
    pub hf_repo: String,
    pub hf_file: Option<String>,
    pub dest: PathBuf,
    pub expected_sha256: Option<String>,
}

impl DownloadPlan {
    pub fn from_profile(repo_root: &Path, profile: &ModelProfile) -> Result<Self, HfHubError> {
        Ok(Self {
            hf_repo: profile.hf_repo.clone().ok_or_else(|| HfHubError::Download(format!("{} has no hf_repo", profile.label)))?,
            hf_file: profile.hf_file.clone(),
            dest: repo_root.join(profile.model_path.as_ref().ok_or_else(|| HfHubError::Download(format!("{} has no model_path", profile.label)))?),
            expected_sha256: None,
        })
    }
}

#[derive(Error, Debug)]
pub enum HfHubError {
    #[error("HTTP error: {0}")] Http(#[from] reqwest::Error),
    #[error("IO error: {0}")] Io(#[from] std::io::Error),
    #[error("Download error: {0}")] Download(String),
    #[error("Checksum mismatch: expected {expected}, got {actual}")] ChecksumMismatch { expected: String, actual: String },
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

pub async fn download_file(plan: &DownloadPlan) -> Result<PathBuf, HfHubError> {
    let file = plan.hf_file.as_deref().ok_or_else(|| HfHubError::Download("hf_file required".into()))?;
    let url = format!("https://huggingface.co/{}/resolve/main/{}", plan.hf_repo.trim_matches('/'), file.trim_start_matches('/'));
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(600)).build()?;
    let resp = client.get(&url).send().await?;
    if !resp.status().is_success() { return Err(HfHubError::Download(format!("HTTP {} for {url}", resp.status()))); }
    let bytes = resp.bytes().await?;
    if let Some(expected) = &plan.expected_sha256 {
        let actual = sha256_hex(&bytes);
        if actual != *expected { return Err(HfHubError::ChecksumMismatch { expected: expected.clone(), actual }); }
    }
    if let Some(parent) = plan.dest.parent() { std::fs::create_dir_all(parent)?; }
    std::fs::write(&plan.dest, &bytes)?;
    Ok(plan.dest.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sha256_known_empty() {
        assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }
}
