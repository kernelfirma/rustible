//! Hash chain for immutable audit log integrity
//!
//! This module implements a cryptographic hash chain using BLAKE3 to ensure
//! that audit log entries cannot be tampered with. Each entry includes a hash
//! of the previous entry, forming an immutable chain.

use serde::{Deserialize, Serialize};

/// A single entry in the hash chain.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HashChainEntry {
    /// Monotonically increasing sequence number
    pub sequence: u64,
    /// ISO 8601 timestamp of when the entry was created
    pub timestamp: String,
    /// BLAKE3 hash of the event data
    pub event_hash: String,
    /// Hash of the previous chain entry (empty string for genesis)
    pub previous_hash: String,
    /// Combined chain hash: H(sequence || event_hash || previous_hash)
    pub chain_hash: String,
}

/// Internal state for building a hash chain incrementally.
#[derive(Debug, Clone)]
pub struct HashChainState {
    /// Next sequence number to assign
    next_sequence: u64,
    /// Chain hash of the most recent entry
    last_hash: String,
}

impl HashChainState {
    /// Create a new hash chain starting from the genesis state.
    pub fn new() -> Self {
        Self {
            next_sequence: 0,
            last_hash: String::new(),
        }
    }

    /// Resume a chain from a known state (e.g. after reading an existing log).
    pub fn resume(next_sequence: u64, last_hash: String) -> Self {
        Self {
            next_sequence,
            last_hash,
        }
    }

    /// Append new event data and return the resulting chain entry.
    ///
    /// The chain hash is computed as `BLAKE3(sequence || event_hash || previous_hash)`.
    pub fn append(&mut self, event_data: &[u8]) -> HashChainEntry {
        let event_hash = blake3::hash(event_data).to_hex().to_string();
        let previous_hash = self.last_hash.clone();

        let mut chain_input = Vec::new();
        chain_input.extend_from_slice(&self.next_sequence.to_le_bytes());
        chain_input.extend_from_slice(event_hash.as_bytes());
        chain_input.extend_from_slice(previous_hash.as_bytes());
        let chain_hash = blake3::hash(&chain_input).to_hex().to_string();

        let timestamp = chrono::Utc::now().to_rfc3339();

        let entry = HashChainEntry {
            sequence: self.next_sequence,
            timestamp,
            event_hash,
            previous_hash,
            chain_hash: chain_hash.clone(),
        };

        self.next_sequence += 1;
        self.last_hash = chain_hash;

        entry
    }

    /// Verify that a slice of entries forms a valid chain.
    ///
    /// Checks that:
    /// 1. Sequence numbers are contiguous starting from `entries[0].sequence`.
    /// 2. Each entry's `previous_hash` matches the preceding entry's `chain_hash`.
    /// 3. Each entry's `chain_hash` is correctly computed.
    pub fn verify_chain(entries: &[HashChainEntry]) -> bool {
        if entries.is_empty() {
            return true;
        }

        for (i, entry) in entries.iter().enumerate() {
            // Verify sequence continuity
            if entry.sequence != entries[0].sequence + i as u64 {
                return false;
            }

            // Verify previous_hash linkage
            if i == 0 {
                // Genesis or first entry in the slice -- we cannot verify
                // backwards further, so we just check the chain_hash.
            } else {
                let prev = &entries[i - 1];
                if entry.previous_hash != prev.chain_hash {
                    return false;
                }
            }

            // Recompute chain_hash
            let mut chain_input = Vec::new();
            chain_input.extend_from_slice(&entry.sequence.to_le_bytes());
            chain_input.extend_from_slice(entry.event_hash.as_bytes());
            chain_input.extend_from_slice(entry.previous_hash.as_bytes());
            let expected = blake3::hash(&chain_input).to_hex().to_string();

            if entry.chain_hash != expected {
                return false;
            }
        }

        true
    }

    /// Get the current sequence number (next to be assigned).
    pub fn next_sequence(&self) -> u64 {
        self.next_sequence
    }

    /// Get the hash of the last entry in the chain.
    pub fn last_hash(&self) -> &str {
        &self.last_hash
    }
}

impl Default for HashChainState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_append_and_verify() {
        let mut chain = HashChainState::new();

        let e0 = chain.append(b"event-zero");
        assert_eq!(e0.sequence, 0);
        assert!(e0.previous_hash.is_empty());

        let e1 = chain.append(b"event-one");
        assert_eq!(e1.sequence, 1);
        assert_eq!(e1.previous_hash, e0.chain_hash);

        let e2 = chain.append(b"event-two");
        assert_eq!(e2.sequence, 2);
        assert_eq!(e2.previous_hash, e1.chain_hash);

        assert!(HashChainState::verify_chain(&[e0, e1, e2]));
    }

    #[test]
    fn test_tampered_entry_detected() {
        let mut chain = HashChainState::new();
        let e0 = chain.append(b"first");
        let e1 = chain.append(b"second");
        let e2 = chain.append(b"third");

        // Tamper with the middle entry
        let mut tampered = e1.clone();
        tampered.event_hash = "deadbeef".to_string();

        assert!(!HashChainState::verify_chain(&[e0, tampered, e2]));
    }

    #[test]
    fn test_empty_chain_is_valid() {
        assert!(HashChainState::verify_chain(&[]));
    }

    #[test]
    fn test_resume_continues_chain() {
        let mut chain = HashChainState::new();
        let e0 = chain.append(b"alpha");
        let e1 = chain.append(b"beta");

        // Resume from the state after e1
        let mut resumed =
            HashChainState::resume(chain.next_sequence(), chain.last_hash().to_string());
        let e2 = resumed.append(b"gamma");

        assert_eq!(e2.sequence, 2);
        assert_eq!(e2.previous_hash, e1.chain_hash);
        assert!(HashChainState::verify_chain(&[e0, e1, e2]));
    }
}
