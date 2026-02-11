//! xCAT migration support.
//!
//! Provides parsers and mappers for importing xCAT object definitions
//! (produced by `lsdef -t node -l`, `lsdef -t group -l`, etc.) into
//! Rustible inventory structures.

pub mod objects;
