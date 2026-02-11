//! Artifact and Image Signing & Verification
//!
//! Provides cryptographic signing and verification of artifacts (playbooks,
//! roles, collections, container images) to ensure supply-chain integrity.
//!
//! Uses blake3 keyed hashing for HMAC-based signatures and blake3 for
//! artifact content hashing.

pub mod keys;
pub mod signer;
pub mod trust;
pub mod verifier;

pub use keys::{KeyId, KeyStore, SigningAlgorithm, SigningKeyPair};
pub use signer::{ArtifactSigner, SignatureBundle};
pub use trust::TrustPolicy;
pub use verifier::{ArtifactVerifier, TrustLevel, VerificationResult};
