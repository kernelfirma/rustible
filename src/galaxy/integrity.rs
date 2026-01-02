//! Integrity verification for Galaxy artifacts
//!
//! Provides checksum computation and verification for downloaded collections and roles.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

use super::error::GalaxyResult;

/// Checksum algorithm
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChecksumAlgorithm {
    /// SHA-256 (default)
    Sha256,
    /// SHA-512
    Sha512,
    /// MD5 (legacy, not recommended)
    Md5,
}

impl Default for ChecksumAlgorithm {
    fn default() -> Self {
        Self::Sha256
    }
}

/// File integrity information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileIntegrity {
    /// File path
    pub path: String,
    /// Checksum algorithm
    pub algorithm: ChecksumAlgorithm,
    /// Checksum value
    pub checksum: String,
    /// File size in bytes
    pub size: u64,
}

/// Report from integrity verification
#[derive(Debug, Clone)]
pub struct IntegrityReport {
    /// Artifact name
    pub artifact: String,
    /// Whether verification passed
    pub passed: bool,
    /// Error message if verification failed
    pub error: Option<String>,
    /// Files checked
    pub files_checked: usize,
    /// Files failed
    pub files_failed: usize,
}

/// Integrity verifier for Galaxy artifacts
pub struct IntegrityVerifier;

impl IntegrityVerifier {
    /// Compute checksum of data
    pub fn compute_checksum(data: &[u8], algorithm: ChecksumAlgorithm) -> String {
        match algorithm {
            ChecksumAlgorithm::Sha256 => {
                let mut hasher = Sha256::new();
                hasher.update(data);
                format!("{:x}", hasher.finalize())
            }
            ChecksumAlgorithm::Sha512 => {
                use sha2::Sha512;
                let mut hasher = Sha512::new();
                hasher.update(data);
                format!("{:x}", hasher.finalize())
            }
            ChecksumAlgorithm::Md5 => {
                let digest = md5::compute(data);
                format!("{:x}", digest)
            }
        }
    }

    /// Compute checksum of a file
    pub async fn compute_file_checksum(
        path: &Path,
        algorithm: ChecksumAlgorithm,
    ) -> GalaxyResult<String> {
        let data = tokio::fs::read(path).await?;
        Ok(Self::compute_checksum(&data, algorithm))
    }

    /// Verify checksum matches expected value
    pub fn verify_checksum(data: &[u8], expected: &str, algorithm: ChecksumAlgorithm) -> bool {
        let actual = Self::compute_checksum(data, algorithm);
        actual.eq_ignore_ascii_case(expected)
    }

    /// Verify file checksum
    pub async fn verify_file(
        path: &Path,
        expected: &str,
        algorithm: ChecksumAlgorithm,
    ) -> GalaxyResult<bool> {
        let actual = Self::compute_file_checksum(path, algorithm).await?;
        Ok(actual.eq_ignore_ascii_case(expected))
    }

    /// Verify a collection artifact against its manifest
    pub async fn verify_collection(collection_path: &Path) -> GalaxyResult<IntegrityReport> {
        let manifest_path = collection_path.join("FILES.json");

        if !manifest_path.exists() {
            return Ok(IntegrityReport {
                artifact: collection_path.display().to_string(),
                passed: true,
                error: Some("No FILES.json manifest found".to_string()),
                files_checked: 0,
                files_failed: 0,
            });
        }

        let manifest_content = tokio::fs::read_to_string(&manifest_path).await?;
        let manifest: FilesManifest = serde_json::from_str(&manifest_content)?;

        let mut files_checked = 0;
        let mut files_failed = 0;

        for file_info in &manifest.files {
            let file_path = collection_path.join(&file_info.name);
            if !file_path.exists() {
                files_failed += 1;
                continue;
            }

            files_checked += 1;

            if let Some(ref expected_checksum) = file_info.chksum_sha256 {
                let actual =
                    Self::compute_file_checksum(&file_path, ChecksumAlgorithm::Sha256).await?;
                if !actual.eq_ignore_ascii_case(expected_checksum) {
                    files_failed += 1;
                }
            }
        }

        Ok(IntegrityReport {
            artifact: collection_path.display().to_string(),
            passed: files_failed == 0,
            error: if files_failed > 0 {
                Some(format!("{} files failed verification", files_failed))
            } else {
                None
            },
            files_checked,
            files_failed,
        })
    }
}

/// FILES.json manifest structure
#[derive(Debug, Deserialize)]
struct FilesManifest {
    files: Vec<FileEntry>,
    #[serde(default)]
    format: u32,
}

#[derive(Debug, Deserialize)]
struct FileEntry {
    name: String,
    #[serde(default)]
    ftype: String,
    #[serde(default)]
    chksum_type: Option<String>,
    #[serde(default)]
    chksum_sha256: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_sha256() {
        let data = b"hello world";
        let checksum = IntegrityVerifier::compute_checksum(data, ChecksumAlgorithm::Sha256);
        assert_eq!(
            checksum,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn test_verify_checksum() {
        let data = b"hello world";
        let expected = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
        assert!(IntegrityVerifier::verify_checksum(
            data,
            expected,
            ChecksumAlgorithm::Sha256
        ));
    }

    #[test]
    fn test_checksum_case_insensitive() {
        let data = b"hello world";
        let expected = "B94D27B9934D3E08A52E52D7DA7DABFAC484EFE37A5380EE9088F7ACE2EFCDE9";
        assert!(IntegrityVerifier::verify_checksum(
            data,
            expected,
            ChecksumAlgorithm::Sha256
        ));
    }
}
