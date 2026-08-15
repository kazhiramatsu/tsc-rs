//! Pure, repository-independent Functional-CI value seams.
//!
//! FCI-1a intentionally contains inert identifiers and input references only.
//! Canonical encoding, hashing, graph evaluation, effects, and authority are
//! introduced by later dependency-ordered packets.

#![forbid(unsafe_code)]

mod adapter;
mod digest;
mod graph;
mod ids;
mod input;

pub use adapter::{
    AdapterDescriptorError, AdapterDescriptorSetV1, AdapterDescriptorV1, AdapterIdV1,
};
pub use digest::{InputDigestV1, ObjectDigestV1};
pub use graph::{
    ActionRecord, AdapterInstanceRefV1, CompleteMembership, CompositeProfileV1, InstanceIdV1,
    NodeClass, NodeRecord, PendingMembership, RootRecord,
};
pub use ids::{ApplicationNamespaceV1, ImplementationIdV1, ProtocolDomainV1, SchemaIdV1};
pub use input::CanonicalInputRefV1;
