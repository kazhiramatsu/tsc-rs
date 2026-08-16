use sha2::{Digest, Sha256};

use crate::{
    ActionKeyV1, AdapterRegistryDigestV1, ApplicationNamespaceDigestV1, ApplicationNamespaceV1,
    AuthorityReceiptDigestV1, BuildArtifactIdV1, ConflictRegistryDigestV1, GraphDigestV1,
    InputDigestV1, ObjectDigestV1, OutcomeDigestV1, ProtocolDomainTagV1, PublicationEventDigestV1,
};

const DOMAIN_LENGTH_BYTES: usize = 4;
const PAYLOAD_LENGTH_BYTES: usize = 8;

fn framed_digest(domain: ProtocolDomainTagV1, parts: &[&[u8]]) -> [u8; 32] {
    let payload_length = parts.iter().map(|part| part.len() as u64).sum::<u64>();
    let mut hasher = Sha256::new();
    hasher.update((domain.as_bytes().len() as u32).to_be_bytes());
    hasher.update(domain.as_bytes());
    hasher.update(payload_length.to_be_bytes());
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().into()
}

fn framed_digest_with_part_lengths(domain: ProtocolDomainTagV1, parts: &[&[u8]]) -> [u8; 32] {
    let payload_length = parts
        .iter()
        .map(|part| 8u64 + part.len() as u64)
        .sum::<u64>();
    let mut hasher = Sha256::new();
    hasher.update((domain.as_bytes().len() as u32).to_be_bytes());
    hasher.update(domain.as_bytes());
    hasher.update(payload_length.to_be_bytes());
    for part in parts {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part);
    }
    hasher.finalize().into()
}

pub fn hash_application_namespace(
    namespace: ApplicationNamespaceV1,
) -> ApplicationNamespaceDigestV1 {
    ApplicationNamespaceDigestV1::from_bytes(framed_digest(
        ProtocolDomainTagV1::ApplicationNamespace,
        &[namespace.as_bytes()],
    ))
}

pub fn hash_input(bytes: &[u8]) -> InputDigestV1 {
    InputDigestV1::from_bytes(framed_digest(ProtocolDomainTagV1::CanonicalInput, &[bytes]))
}

pub fn hash_object(bytes: &[u8]) -> ObjectDigestV1 {
    ObjectDigestV1::from_bytes(framed_digest(ProtocolDomainTagV1::Object, &[bytes]))
}

pub fn hash_action_key(namespace: ApplicationNamespaceV1, canonical_input: &[u8]) -> ActionKeyV1 {
    let namespace_digest = hash_application_namespace(namespace);
    ActionKeyV1::from_bytes(framed_digest_with_part_lengths(
        ProtocolDomainTagV1::ActionKey,
        &[namespace_digest.as_bytes(), canonical_input],
    ))
}

pub fn hash_graph(bytes: &[u8]) -> GraphDigestV1 {
    GraphDigestV1::from_bytes(framed_digest(ProtocolDomainTagV1::Graph, &[bytes]))
}

pub fn hash_build_artifact(bytes: &[u8]) -> BuildArtifactIdV1 {
    BuildArtifactIdV1::from_bytes(framed_digest(ProtocolDomainTagV1::BuildArtifact, &[bytes]))
}

pub fn hash_outcome(bytes: &[u8]) -> OutcomeDigestV1 {
    OutcomeDigestV1::from_bytes(framed_digest(ProtocolDomainTagV1::Outcome, &[bytes]))
}

pub fn hash_adapter_registry(bytes: &[u8]) -> AdapterRegistryDigestV1 {
    AdapterRegistryDigestV1::from_bytes(framed_digest(
        ProtocolDomainTagV1::AdapterRegistry,
        &[bytes],
    ))
}

pub fn hash_conflict_registry(bytes: &[u8]) -> ConflictRegistryDigestV1 {
    ConflictRegistryDigestV1::from_bytes(framed_digest(
        ProtocolDomainTagV1::ConflictRegistry,
        &[bytes],
    ))
}

pub fn hash_authority_receipt(bytes: &[u8]) -> AuthorityReceiptDigestV1 {
    AuthorityReceiptDigestV1::from_bytes(framed_digest(
        ProtocolDomainTagV1::AuthorityReceipt,
        &[bytes],
    ))
}

pub fn hash_publication_event(bytes: &[u8]) -> PublicationEventDigestV1 {
    PublicationEventDigestV1::from_bytes(framed_digest(
        ProtocolDomainTagV1::PublicationEvent,
        &[bytes],
    ))
}

const _: usize = DOMAIN_LENGTH_BYTES + PAYLOAD_LENGTH_BYTES;
