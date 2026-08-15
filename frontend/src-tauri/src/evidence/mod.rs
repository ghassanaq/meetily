//! Immutable local evidence foundations.
//!
//! This module stores recording identities and versioned transcript
//! interpretations. It exposes no commands or executable capability; later
//! citation and generation layers build on these declarative records.

pub mod hashing;
pub mod models;
pub mod repository;

pub use hashing::{canonical_transcript_payload, hash_transcript_version};
pub use models::*;
pub use repository::*;

#[cfg(test)]
mod tests;
