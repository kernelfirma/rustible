//! Offline audit log verification
//!
//! This module provides standalone verification of audit log files without
//! needing the rest of the audit subsystem to be running. It reads a
//! JSON-lines file and checks hash-chain integrity.

use super::hashchain::HashChainEntry;
use std::path::Path;

/// Report returned by the audit verifier.
#[derive(Debug, Clone)]
pub struct VerificationReport {
    /// Whether the entire chain is valid.
    pub valid: bool,
    /// Number of entries that were checked.
    pub entries_checked: usize,
    /// Sequence number of the first invalid entry, if any.
    pub first_invalid: Option<u64>,
}

impl std::fmt::Display for VerificationReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.valid {
            write!(
                f,
                "VALID: all {} entries verified",
                self.entries_checked
            )
        } else {
            write!(
                f,
                "INVALID: first bad entry at sequence {} ({} entries checked)",
                self.first_invalid.unwrap_or(0),
                self.entries_checked
            )
        }
    }
}

/// Verifier for offline audit log files.
pub struct AuditVerifier;

impl AuditVerifier {
    /// Verify a JSON-lines audit log file at the given path.
    ///
    /// Returns a `VerificationReport` with the result. IO and parse errors
    /// are returned as `Err`.
    pub fn verify_file(path: &Path) -> std::io::Result<VerificationReport> {
        let content = std::fs::read_to_string(path)?;
        let mut entries: Vec<HashChainEntry> = Vec::new();

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let entry: HashChainEntry = serde_json::from_str(trimmed).map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, e)
            })?;
            entries.push(entry);
        }

        if entries.is_empty() {
            return Ok(VerificationReport {
                valid: true,
                entries_checked: 0,
                first_invalid: None,
            });
        }

        // Walk the chain entry by entry to find the first invalid one
        let entries_checked = entries.len();
        for (i, entry) in entries.iter().enumerate() {
            // Check sequence continuity
            if entry.sequence != entries[0].sequence + i as u64 {
                return Ok(VerificationReport {
                    valid: false,
                    entries_checked,
                    first_invalid: Some(entry.sequence),
                });
            }

            // Check previous_hash linkage (skip for first entry)
            if i > 0 {
                let prev = &entries[i - 1];
                if entry.previous_hash != prev.chain_hash {
                    return Ok(VerificationReport {
                        valid: false,
                        entries_checked,
                        first_invalid: Some(entry.sequence),
                    });
                }
            }

            // Recompute and verify chain_hash
            let mut chain_input = Vec::new();
            chain_input.extend_from_slice(&entry.sequence.to_le_bytes());
            chain_input.extend_from_slice(entry.event_hash.as_bytes());
            chain_input.extend_from_slice(entry.previous_hash.as_bytes());
            let expected = blake3::hash(&chain_input).to_hex().to_string();

            if entry.chain_hash != expected {
                return Ok(VerificationReport {
                    valid: false,
                    entries_checked,
                    first_invalid: Some(entry.sequence),
                });
            }
        }

        Ok(VerificationReport {
            valid: true,
            entries_checked,
            first_invalid: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::hashchain::HashChainState as Chain;
    use tempfile::NamedTempFile;

    fn write_entries(path: &Path, entries: &[HashChainEntry]) {
        use std::io::Write;
        let mut f = std::fs::File::create(path).unwrap();
        for e in entries {
            let line = serde_json::to_string(e).unwrap();
            writeln!(f, "{}", line).unwrap();
        }
    }

    #[test]
    fn test_verify_valid_file() {
        let tmp = NamedTempFile::new().unwrap();
        let mut chain = Chain::new();
        let entries: Vec<_> = (0..5).map(|i| chain.append(format!("evt-{i}").as_bytes())).collect();
        write_entries(tmp.path(), &entries);

        let report = AuditVerifier::verify_file(tmp.path()).unwrap();
        assert!(report.valid);
        assert_eq!(report.entries_checked, 5);
        assert!(report.first_invalid.is_none());
    }

    #[test]
    fn test_verify_tampered_file() {
        let tmp = NamedTempFile::new().unwrap();
        let mut chain = Chain::new();
        let mut entries: Vec<_> = (0..3).map(|i| chain.append(format!("evt-{i}").as_bytes())).collect();

        // Tamper with the second entry
        entries[1].event_hash = "tampered".to_string();
        write_entries(tmp.path(), &entries);

        let report = AuditVerifier::verify_file(tmp.path()).unwrap();
        assert!(!report.valid);
        assert_eq!(report.first_invalid, Some(1));
    }
}
