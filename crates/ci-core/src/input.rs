use crate::{ApplicationNamespaceV1, ImplementationIdV1, InputDigestV1, SchemaIdV1};

/// The identity of one semantic input payload.
///
/// This is deliberately an inert record in FCI-1a. It does not serialize,
/// hash, inspect the filesystem, or imply that the payload has been verified.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CanonicalInputRefV1 {
    pub namespace: ApplicationNamespaceV1,
    pub schema: SchemaIdV1,
    pub implementation: ImplementationIdV1,
    pub payload: InputDigestV1,
}
