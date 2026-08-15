use core::fmt;

use crate::{
    ActionKeyV1, CanonicalEncode, CanonicalError, CanonicalSink, ImplementationIdV1,
    ObjectDigestV1, SchemaIdV1,
};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum InventoryError {
    InvalidPath,
    EmptyOwnership,
    Duplicate { index: usize },
    Unsorted { index: usize },
    Collision { index: usize },
}

impl fmt::Display for InventoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPath => formatter.write_str("invalid normalized inventory path"),
            Self::EmptyOwnership => formatter.write_str("ownership record has no paths"),
            Self::Duplicate { index } => write!(formatter, "duplicate inventory entry at {index}"),
            Self::Unsorted { index } => write!(formatter, "unsorted inventory entry at {index}"),
            Self::Collision { index } => write!(formatter, "path collision at {index}"),
        }
    }
}

impl std::error::Error for InventoryError {}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NormalizedPathV1(Box<[u8]>);

impl NormalizedPathV1 {
    pub fn try_new(bytes: Vec<u8>) -> Result<Self, InventoryError> {
        if bytes.is_empty()
            || bytes.contains(&0)
            || bytes.contains(&b'\\')
            || std::str::from_utf8(&bytes).is_err()
        {
            return Err(InventoryError::InvalidPath);
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| InventoryError::InvalidPath)?;
        if text.starts_with('/') {
            return Err(InventoryError::InvalidPath);
        }
        for component in text.split('/') {
            if component.is_empty() || component == "." || component == ".." {
                return Err(InventoryError::InvalidPath);
            }
        }
        Ok(Self(bytes.into_boxed_slice()))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl CanonicalEncode for NormalizedPathV1 {
    fn encode_canonical<S: CanonicalSink>(&self, out: &mut S) -> Result<(), CanonicalError> {
        let text = std::str::from_utf8(&self.0).map_err(|_| CanonicalError::InvalidKeyOrder)?;
        crate::CanonicalValue::String(text.to_owned()).encode_canonical(out)
    }
}

impl CanonicalEncode for GlobalDispositionV1 {
    fn encode_canonical<S: CanonicalSink>(&self, out: &mut S) -> Result<(), CanonicalError> {
        let name = match self {
            Self::Present => "present",
            Self::Deleted => "deleted",
            Self::Ignored => "ignored",
            Self::Generated => "generated",
            Self::Symlink => "symlink",
            Self::Submodule => "submodule",
            Self::Unknown => "unknown",
        };
        crate::CanonicalValue::String(name.to_owned()).encode_canonical(out)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum GlobalDispositionV1 {
    Present,
    Deleted,
    Ignored,
    Generated,
    Symlink,
    Submodule,
    Unknown,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct InventoryEntryV1 {
    path: NormalizedPathV1,
    disposition: GlobalDispositionV1,
    content: Option<ObjectDigestV1>,
}

impl InventoryEntryV1 {
    pub const fn new(
        path: NormalizedPathV1,
        disposition: GlobalDispositionV1,
        content: Option<ObjectDigestV1>,
    ) -> Self {
        Self {
            path,
            disposition,
            content,
        }
    }

    pub fn path(&self) -> &NormalizedPathV1 {
        &self.path
    }

    pub const fn disposition(&self) -> GlobalDispositionV1 {
        self.disposition
    }

    pub const fn content(&self) -> Option<ObjectDigestV1> {
        self.content
    }
}

impl CanonicalEncode for InventoryEntryV1 {
    fn encode_canonical<S: CanonicalSink>(&self, out: &mut S) -> Result<(), CanonicalError> {
        out.write(b"{\"content\":")?;
        match self.content {
            Some(digest) => write_hex(out, digest.as_bytes())?,
            None => out.write(b"null")?,
        }
        out.write(b",\"disposition\":")?;
        self.disposition.encode_canonical(out)?;
        out.write(b",\"path\":")?;
        self.path.encode_canonical(out)?;
        out.write(b"}")
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CollisionKindV1 {
    Exact,
    CaseFolded,
    UnicodeEquivalent,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PathCollisionV1 {
    first: NormalizedPathV1,
    second: NormalizedPathV1,
    kind: CollisionKindV1,
}

impl PathCollisionV1 {
    pub fn try_new(
        first: NormalizedPathV1,
        second: NormalizedPathV1,
        kind: CollisionKindV1,
    ) -> Result<Self, InventoryError> {
        if first > second || (first == second && kind != CollisionKindV1::Exact) {
            return Err(InventoryError::Collision { index: 1 });
        }
        Ok(Self {
            first,
            second,
            kind,
        })
    }

    pub fn first(&self) -> &NormalizedPathV1 {
        &self.first
    }

    pub fn second(&self) -> &NormalizedPathV1 {
        &self.second
    }

    pub const fn kind(&self) -> CollisionKindV1 {
        self.kind
    }
}

impl CanonicalEncode for CollisionKindV1 {
    fn encode_canonical<S: CanonicalSink>(&self, out: &mut S) -> Result<(), CanonicalError> {
        let name = match self {
            Self::Exact => "exact",
            Self::CaseFolded => "case_folded",
            Self::UnicodeEquivalent => "unicode_equivalent",
        };
        crate::CanonicalValue::String(name.to_owned()).encode_canonical(out)
    }
}

impl CanonicalEncode for PathCollisionV1 {
    fn encode_canonical<S: CanonicalSink>(&self, out: &mut S) -> Result<(), CanonicalError> {
        out.write(b"{\"first\":")?;
        self.first.encode_canonical(out)?;
        out.write(b",\"kind\":")?;
        self.kind.encode_canonical(out)?;
        out.write(b",\"second\":")?;
        self.second.encode_canonical(out)?;
        out.write(b"}")
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NegativeLookupV1 {
    requested: NormalizedPathV1,
    algorithm: SchemaIdV1,
    roots: Box<[NormalizedPathV1]>,
    listing_digest: ObjectDigestV1,
}

impl NegativeLookupV1 {
    pub fn try_new(
        requested: NormalizedPathV1,
        algorithm: SchemaIdV1,
        roots: Vec<NormalizedPathV1>,
        listing_digest: ObjectDigestV1,
    ) -> Result<Self, InventoryError> {
        if roots.windows(2).any(|pair| pair[0] >= pair[1]) {
            let index = roots
                .windows(2)
                .position(|pair| pair[0] >= pair[1])
                .map_or(0, |index| index + 1);
            return Err(InventoryError::Unsorted { index });
        }
        Ok(Self {
            requested,
            algorithm,
            roots: roots.into_boxed_slice(),
            listing_digest,
        })
    }

    pub fn requested(&self) -> &NormalizedPathV1 {
        &self.requested
    }

    pub const fn algorithm(&self) -> SchemaIdV1 {
        self.algorithm
    }

    pub fn roots(&self) -> &[NormalizedPathV1] {
        &self.roots
    }

    pub const fn listing_digest(&self) -> ObjectDigestV1 {
        self.listing_digest
    }
}

impl CanonicalEncode for NegativeLookupV1 {
    fn encode_canonical<S: CanonicalSink>(&self, out: &mut S) -> Result<(), CanonicalError> {
        out.write(b"{\"algorithm\":")?;
        write_hex(out, self.algorithm.as_bytes())?;
        out.write(b",\"listing_digest\":")?;
        write_hex(out, self.listing_digest.as_bytes())?;
        out.write(b",\"requested\":")?;
        self.requested.encode_canonical(out)?;
        out.write(b",\"roots\":[")?;
        encode_paths(&self.roots, out)?;
        out.write(b"]}")
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct GeneratedOwnershipV1 {
    output: NormalizedPathV1,
    generator: ActionKeyV1,
    implementation: ImplementationIdV1,
}

impl GeneratedOwnershipV1 {
    pub const fn new(
        output: NormalizedPathV1,
        generator: ActionKeyV1,
        implementation: ImplementationIdV1,
    ) -> Self {
        Self {
            output,
            generator,
            implementation,
        }
    }

    pub fn output(&self) -> &NormalizedPathV1 {
        &self.output
    }

    pub const fn generator(&self) -> ActionKeyV1 {
        self.generator
    }

    pub const fn implementation(&self) -> ImplementationIdV1 {
        self.implementation
    }
}

impl CanonicalEncode for GeneratedOwnershipV1 {
    fn encode_canonical<S: CanonicalSink>(&self, out: &mut S) -> Result<(), CanonicalError> {
        out.write(b"{\"generator\":")?;
        write_hex(out, self.generator.as_bytes())?;
        out.write(b",\"implementation\":")?;
        write_hex(out, self.implementation.as_bytes())?;
        out.write(b",\"output\":")?;
        self.output.encode_canonical(out)?;
        out.write(b"}")
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BuildSystemOwnershipV1 {
    producer: ImplementationIdV1,
    inputs: Box<[NormalizedPathV1]>,
    outputs: Box<[NormalizedPathV1]>,
    opaque: bool,
}

impl BuildSystemOwnershipV1 {
    pub fn try_new(
        producer: ImplementationIdV1,
        inputs: Vec<NormalizedPathV1>,
        outputs: Vec<NormalizedPathV1>,
        opaque: bool,
    ) -> Result<Self, InventoryError> {
        if inputs.is_empty() || outputs.is_empty() {
            return Err(InventoryError::EmptyOwnership);
        }
        if !strict_paths(&inputs) || !strict_paths(&outputs) {
            return Err(InventoryError::Unsorted { index: 1 });
        }
        Ok(Self {
            producer,
            inputs: inputs.into_boxed_slice(),
            outputs: outputs.into_boxed_slice(),
            opaque,
        })
    }

    pub const fn producer(&self) -> ImplementationIdV1 {
        self.producer
    }

    pub fn inputs(&self) -> &[NormalizedPathV1] {
        &self.inputs
    }

    pub fn outputs(&self) -> &[NormalizedPathV1] {
        &self.outputs
    }

    pub const fn opaque(&self) -> bool {
        self.opaque
    }
}

impl CanonicalEncode for BuildSystemOwnershipV1 {
    fn encode_canonical<S: CanonicalSink>(&self, out: &mut S) -> Result<(), CanonicalError> {
        out.write(b"{\"inputs\":[")?;
        encode_paths(&self.inputs, out)?;
        out.write(b"],\"opaque\":")?;
        crate::CanonicalValue::Bool(self.opaque).encode_canonical(out)?;
        out.write(b",\"outputs\":[")?;
        encode_paths(&self.outputs, out)?;
        out.write(b"],\"producer\":")?;
        write_hex(out, self.producer.as_bytes())?;
        out.write(b"}")
    }
}

fn strict_paths(paths: &[NormalizedPathV1]) -> bool {
    paths.windows(2).all(|pair| pair[0] < pair[1])
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum UnknownInputPolicyV1 {
    FailClosed,
    ImpactAll,
}

impl CanonicalEncode for UnknownInputPolicyV1 {
    fn encode_canonical<S: CanonicalSink>(&self, out: &mut S) -> Result<(), CanonicalError> {
        let name = match self {
            Self::FailClosed => "fail_closed",
            Self::ImpactAll => "impact_all",
        };
        crate::CanonicalValue::String(name.to_owned()).encode_canonical(out)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WorkspaceInventorySpecV1 {
    entries: Box<[InventoryEntryV1]>,
    negatives: Box<[NegativeLookupV1]>,
    generated: Box<[GeneratedOwnershipV1]>,
    build_systems: Box<[BuildSystemOwnershipV1]>,
    policy: UnknownInputPolicyV1,
}

impl WorkspaceInventorySpecV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        entries: Vec<InventoryEntryV1>,
        negatives: Vec<NegativeLookupV1>,
        generated: Vec<GeneratedOwnershipV1>,
        build_systems: Vec<BuildSystemOwnershipV1>,
        policy: UnknownInputPolicyV1,
    ) -> Result<Self, InventoryError> {
        if !strict_entries(&entries)
            || !strict_by_path(&generated)
            || !strict_by_output(&build_systems)
        {
            return Err(InventoryError::Unsorted { index: 1 });
        }
        if negatives
            .windows(2)
            .any(|pair| pair[0].requested >= pair[1].requested)
        {
            return Err(InventoryError::Unsorted { index: 1 });
        }
        Ok(Self {
            entries: entries.into_boxed_slice(),
            negatives: negatives.into_boxed_slice(),
            generated: generated.into_boxed_slice(),
            build_systems: build_systems.into_boxed_slice(),
            policy,
        })
    }

    pub fn entries(&self) -> &[InventoryEntryV1] {
        &self.entries
    }

    pub fn negatives(&self) -> &[NegativeLookupV1] {
        &self.negatives
    }

    pub fn generated(&self) -> &[GeneratedOwnershipV1] {
        &self.generated
    }

    pub fn build_systems(&self) -> &[BuildSystemOwnershipV1] {
        &self.build_systems
    }

    pub const fn unknown_policy(&self) -> UnknownInputPolicyV1 {
        self.policy
    }
}

impl CanonicalEncode for WorkspaceInventorySpecV1 {
    fn encode_canonical<S: CanonicalSink>(&self, out: &mut S) -> Result<(), CanonicalError> {
        out.write(b"{\"build_systems\":[")?;
        encode_values(&self.build_systems, out)?;
        out.write(b"],\"entries\":[")?;
        encode_values(&self.entries, out)?;
        out.write(b"],\"generated\":[")?;
        encode_values(&self.generated, out)?;
        out.write(b"],\"negatives\":[")?;
        encode_values(&self.negatives, out)?;
        out.write(b"],\"unknown_policy\":")?;
        self.policy.encode_canonical(out)?;
        out.write(b"}")
    }
}

fn strict_entries(entries: &[InventoryEntryV1]) -> bool {
    entries.windows(2).all(|pair| pair[0].path < pair[1].path)
}

fn strict_by_path(entries: &[GeneratedOwnershipV1]) -> bool {
    entries
        .windows(2)
        .all(|pair| pair[0].output < pair[1].output)
}

fn strict_by_output(entries: &[BuildSystemOwnershipV1]) -> bool {
    entries
        .windows(2)
        .all(|pair| pair[0].outputs < pair[1].outputs)
}

fn encode_paths<S: CanonicalSink>(
    paths: &[NormalizedPathV1],
    out: &mut S,
) -> Result<(), CanonicalError> {
    for (index, path) in paths.iter().enumerate() {
        if index != 0 {
            out.write(b",")?;
        }
        path.encode_canonical(out)?;
    }
    Ok(())
}

fn encode_values<T: CanonicalEncode, S: CanonicalSink>(
    values: &[T],
    out: &mut S,
) -> Result<(), CanonicalError> {
    for (index, value) in values.iter().enumerate() {
        if index != 0 {
            out.write(b",")?;
        }
        value.encode_canonical(out)?;
    }
    Ok(())
}

fn write_hex<S: CanonicalSink>(out: &mut S, bytes: &[u8]) -> Result<(), CanonicalError> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.write(b"\"")?;
    for byte in bytes {
        out.write(&[HEX[(byte >> 4) as usize], HEX[(byte & 0xf) as usize]])?;
    }
    out.write(b"\"")
}
