//! Pure, repository-independent Functional-CI value seams.
//!
//! FCI-1a intentionally contains inert identifiers and input references only.
//! Canonical encoding, hashing, graph evaluation, effects, and authority are
//! introduced by later dependency-ordered packets.

#![forbid(unsafe_code)]

mod digest;
mod ids;
mod input;

pub use digest::{InputDigestV1, ObjectDigestV1};
pub use ids::{ApplicationNamespaceV1, ImplementationIdV1, ProtocolDomainV1, SchemaIdV1};
pub use input::CanonicalInputRefV1;
