use core::fmt;

macro_rules! fixed_identifier {
    ($name:ident) => {
        #[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name([u8; 16]);

        impl $name {
            pub const fn from_bytes(bytes: [u8; 16]) -> Self {
                Self(bytes)
            }

            pub const fn as_bytes(&self) -> &[u8; 16] {
                &self.0
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter
                    .debug_tuple(stringify!($name))
                    .field(&self.0)
                    .finish()
            }
        }
    };
}

fixed_identifier!(ProtocolDomainV1);
fixed_identifier!(ApplicationNamespaceV1);
fixed_identifier!(SchemaIdV1);
fixed_identifier!(ImplementationIdV1);

/// The complete v1 registry of domain-separated wire objects. The values are
/// protocol identifiers, not application names, and each variant maps to one
/// unique 16-byte tag.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ProtocolDomainTagV1 {
    ApplicationNamespace,
    CanonicalInput,
    ActionKey,
    BuildArtifact,
    Root,
    Graph,
    NodeSpec,
    SourceSnapshot,
    AdapterDescriptor,
    AdapterRegistry,
    Object,
    Outcome,
    Interior,
    CandidateManifest,
    ConflictRegistry,
    AuthorityReceipt,
    PublicationEvent,
    Generation,
    Head,
    EvidenceSnapshot,
    PublicationSnapshot,
    TrustTransition,
    PolicyProof,
    Lease,
    GcPlan,
}

impl ProtocolDomainTagV1 {
    pub const fn as_bytes(self) -> &'static [u8; 16] {
        match self {
            Self::ApplicationNamespace => b"fci.v1.namespace",
            Self::CanonicalInput => b"fci.v1.canonical",
            Self::ActionKey => b"fci.v1.actionkey",
            Self::BuildArtifact => b"fci.v1.buildarti",
            Self::Root => b"fci.v1.root-----",
            Self::Graph => b"fci.v1.graph----",
            Self::NodeSpec => b"fci.v1.nodespec-",
            Self::SourceSnapshot => b"fci.v1.snapshot-",
            Self::AdapterDescriptor => b"fci.v1.adaptdesc",
            Self::AdapterRegistry => b"fci.v1.adapreg--",
            Self::Object => b"fci.v1.object---",
            Self::Outcome => b"fci.v1.outcome--",
            Self::Interior => b"fci.v1.interior-",
            Self::CandidateManifest => b"fci.v1.candidate",
            Self::ConflictRegistry => b"fci.v1.conflict-",
            Self::AuthorityReceipt => b"fci.v1.receipt--",
            Self::PublicationEvent => b"fci.v1.pubevent-",
            Self::Generation => b"fci.v1.generatio",
            Self::Head => b"fci.v1.head-----",
            Self::EvidenceSnapshot => b"fci.v1.evidence-",
            Self::PublicationSnapshot => b"fci.v1.pubsnap--",
            Self::TrustTransition => b"fci.v1.trust----",
            Self::PolicyProof => b"fci.v1.policy---",
            Self::Lease => b"fci.v1.lease----",
            Self::GcPlan => b"fci.v1.gcplan---",
        }
    }

    pub const fn domain(self) -> ProtocolDomainV1 {
        ProtocolDomainV1::from_bytes(*self.as_bytes())
    }

    pub const fn all() -> &'static [Self; 25] {
        &[
            Self::ApplicationNamespace,
            Self::CanonicalInput,
            Self::ActionKey,
            Self::BuildArtifact,
            Self::Root,
            Self::Graph,
            Self::NodeSpec,
            Self::SourceSnapshot,
            Self::AdapterDescriptor,
            Self::AdapterRegistry,
            Self::Object,
            Self::Outcome,
            Self::Interior,
            Self::CandidateManifest,
            Self::ConflictRegistry,
            Self::AuthorityReceipt,
            Self::PublicationEvent,
            Self::Generation,
            Self::Head,
            Self::EvidenceSnapshot,
            Self::PublicationSnapshot,
            Self::TrustTransition,
            Self::PolicyProof,
            Self::Lease,
            Self::GcPlan,
        ]
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum NamespaceLineageV1 {
    Original,
    Fork { parent: ApplicationNamespaceV1 },
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum NamespaceError {
    EmptyNamespace,
    EmptyParent,
    SelfFork,
    RenameChangesIdentity,
}

pub fn validate_namespace_lineage(
    namespace: ApplicationNamespaceV1,
    lineage: NamespaceLineageV1,
) -> Result<(), NamespaceError> {
    if namespace.as_bytes().iter().all(|byte| *byte == 0) {
        return Err(NamespaceError::EmptyNamespace);
    }
    if let NamespaceLineageV1::Fork { parent } = lineage {
        if parent.as_bytes().iter().all(|byte| *byte == 0) {
            return Err(NamespaceError::EmptyParent);
        }
        if parent == namespace {
            return Err(NamespaceError::SelfFork);
        }
    }
    Ok(())
}

pub fn validate_rename(
    prior: ApplicationNamespaceV1,
    current: ApplicationNamespaceV1,
) -> Result<(), NamespaceError> {
    if prior == current {
        Ok(())
    } else {
        Err(NamespaceError::RenameChangesIdentity)
    }
}
