//! Immutable local evidence foundations.
//!
//! This module stores recording identities and versioned transcript
//! interpretations. It exposes no commands or executable capability; later
//! citation and generation layers build on these declarative records.

pub mod citation;
pub mod hashing;
pub mod models;
pub mod repository;
pub mod resolver;

pub use citation::*;
pub use hashing::{canonical_transcript_payload, hash_transcript_version};
pub use models::*;
pub use repository::*;
pub use resolver::*;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod resolver_tests;
