//! Pure, repository-independent Functional-CI value seams.
//!
//! FCI-1a intentionally contains inert identifiers and input references only.
//! Canonical encoding, hashing, graph evaluation, effects, and authority are
//! introduced by later dependency-ordered packets.

#![forbid(unsafe_code)]

mod adapter;
mod canonical;
mod digest;
mod graph;
mod graph_schema;
mod hash;
mod identity;
mod ids;
mod input;

pub use adapter::{
    AdapterDescriptorError, AdapterDescriptorSetV1, AdapterDescriptorV1, AdapterIdV1,
};
pub use canonical::{
    decode_canonical, decode_object_with_keys, BoundedBytesSink, CanonicalDecoder, CanonicalEncode,
    CanonicalError, CanonicalSink, CanonicalValue, DecodeError, StrictJsonDecoder,
};
pub use digest::{
    ActionKeyV1, AdapterRegistryDigestV1, ApplicationNamespaceDigestV1, AuthorityReceiptDigestV1,
    BuildArtifactIdV1, ConflictRegistryDigestV1, GraphDigestV1, InputDigestV1, ObjectDigestV1,
    OutcomeDigestV1, PublicationEventDigestV1,
};
pub use graph::{
    ActionRecord, AdapterInstanceRefV1, CompleteMembership, CompositeProfileV1, InstanceIdV1,
    NodeClass, NodeRecord, PendingMembership, RootRecord,
};
pub use graph_schema::{ActionGraph, GraphSchemaError};
pub use hash::{
    hash_action_key, hash_adapter_registry, hash_application_namespace, hash_authority_receipt,
    hash_build_artifact, hash_conflict_registry, hash_graph, hash_input, hash_object, hash_outcome,
    hash_publication_event,
};
pub use identity::{
    BuildComponentSetV1, BuildComponentV1, DisclosureEntryV1, DisclosureError, DisclosureHistoryV1,
    EvidenceAudienceV1, ExecutionPlatformV1, FilesystemAccessV1, IdentityError, InvocationIdV1,
    InvocationIdentityV1, NetworkAccessV1, PlatformTokenV1, ProcessObservationStatusV1,
    ProcessObservationV1, PublicEnvironmentEntryV1, ReuseScopeV1, SandboxCapabilitiesV1,
    SecretFreeEnvironmentV1, ToolIdV1, ToolRefV1, ToolRoleV1, ToolchainSetV1,
};
pub use ids::{
    validate_namespace_lineage, validate_rename, ApplicationNamespaceV1, ImplementationIdV1,
    NamespaceError, NamespaceLineageV1, ProtocolDomainTagV1, ProtocolDomainV1, SchemaIdV1,
};
pub use input::CanonicalInputRefV1;
