//! Immutable local evidence foundations.
//!
//! This module stores recording identities and versioned transcript
//! interpretations. It exposes no commands or executable capability; later
//! citation and generation layers build on these declarative records.

pub mod citation;
pub mod enrollment;
pub mod hashing;
pub mod models;
pub mod provenance;
pub mod repository;
pub mod resolver;

pub use citation::*;
pub use enrollment::*;
pub use hashing::{canonical_transcript_payload, hash_transcript_version};
pub use models::*;
pub use provenance::*;
pub use repository::*;
pub use resolver::*;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod resolver_tests;

#[cfg(test)]
mod provenance_tests;

#[cfg(test)]
mod enrollment_tests;
