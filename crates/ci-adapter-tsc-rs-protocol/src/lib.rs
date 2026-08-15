use core::fmt;

use tsc_ci_core::{
    ActionKeyV1, CanonicalEncode, CanonicalError, CanonicalSink, GraphDigestV1, ImplementationIdV1,
    InputDigestV1, InvocationIdV1, ObjectDigestV1, SchemaIdV1,
};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ProtocolError {
    EmptyIdentity,
    InvalidLimit,
    InvalidRepetition,
    InvalidRange,
    Unsorted { index: usize },
    Gap { index: usize },
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "protocol error: {self:?}")
    }
}

impl std::error::Error for ProtocolError {}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ActionInvocationV1 {
    action: ActionKeyV1,
    schema: SchemaIdV1,
    implementation: ImplementationIdV1,
    input: InputDigestV1,
    invocation: InvocationIdV1,
    source_snapshot: ObjectDigestV1,
    repetition: u8,
    attempt: u8,
    max_output_bytes: u64,
}

impl ActionInvocationV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        action: ActionKeyV1,
        schema: SchemaIdV1,
        implementation: ImplementationIdV1,
        input: InputDigestV1,
        invocation: InvocationIdV1,
        source_snapshot: ObjectDigestV1,
        repetition: u8,
        attempt: u8,
        max_output_bytes: u64,
    ) -> Result<Self, ProtocolError> {
        if is_zero(action.as_bytes())
            || is_zero(schema.as_bytes())
            || is_zero(implementation.as_bytes())
            || is_zero(input.as_bytes())
            || is_zero(invocation.as_bytes())
            || is_zero(source_snapshot.as_bytes())
        {
            return Err(ProtocolError::EmptyIdentity);
        }
        if repetition > 1 {
            return Err(ProtocolError::InvalidRepetition);
        }
        if max_output_bytes == 0 {
            return Err(ProtocolError::InvalidLimit);
        }
        Ok(Self {
            action,
            schema,
            implementation,
            input,
            invocation,
            source_snapshot,
            repetition,
            attempt,
            max_output_bytes,
        })
    }

    pub const fn action(&self) -> ActionKeyV1 {
        self.action
    }

    pub const fn schema(&self) -> SchemaIdV1 {
        self.schema
    }

    pub const fn implementation(&self) -> ImplementationIdV1 {
        self.implementation
    }

    pub const fn input(&self) -> InputDigestV1 {
        self.input
    }

    pub const fn invocation(&self) -> InvocationIdV1 {
        self.invocation
    }

    pub const fn source_snapshot(&self) -> ObjectDigestV1 {
        self.source_snapshot
    }

    pub const fn repetition(&self) -> u8 {
        self.repetition
    }

    pub const fn attempt(&self) -> u8 {
        self.attempt
    }

    pub const fn max_output_bytes(&self) -> u64 {
        self.max_output_bytes
    }
}

impl CanonicalEncode for ActionInvocationV1 {
    fn encode_canonical<S: CanonicalSink>(&self, out: &mut S) -> Result<(), CanonicalError> {
        out.write(b"{\"action\":")?;
        write_hex(out, self.action.as_bytes())?;
        out.write(b",\"attempt\":")?;
        write_u64(out, self.attempt as u64)?;
        out.write(b",\"implementation\":")?;
        write_hex(out, self.implementation.as_bytes())?;
        out.write(b",\"input\":")?;
        write_hex(out, self.input.as_bytes())?;
        out.write(b",\"invocation\":")?;
        write_hex(out, self.invocation.as_bytes())?;
        out.write(b",\"max_output_bytes\":")?;
        write_u64(out, self.max_output_bytes)?;
        out.write(b",\"repetition\":")?;
        write_u64(out, self.repetition as u64)?;
        out.write(b",\"schema\":")?;
        write_hex(out, self.schema.as_bytes())?;
        out.write(b",\"source_snapshot\":")?;
        write_hex(out, self.source_snapshot.as_bytes())?;
        out.write(b"}")
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ObservationEnvelopeV1 {
    action: ActionKeyV1,
    schema: SchemaIdV1,
    implementation: ImplementationIdV1,
    repetition: u8,
    bytes: Box<[u8]>,
}

impl ObservationEnvelopeV1 {
    pub fn try_new(
        action: ActionKeyV1,
        schema: SchemaIdV1,
        implementation: ImplementationIdV1,
        repetition: u8,
        bytes: Vec<u8>,
        max_bytes: u64,
    ) -> Result<Self, ProtocolError> {
        if is_zero(action.as_bytes())
            || is_zero(schema.as_bytes())
            || is_zero(implementation.as_bytes())
        {
            return Err(ProtocolError::EmptyIdentity);
        }
        if repetition > 1 {
            return Err(ProtocolError::InvalidRepetition);
        }
        if max_bytes == 0 || bytes.len() as u64 > max_bytes {
            return Err(ProtocolError::InvalidLimit);
        }
        Ok(Self {
            action,
            schema,
            implementation,
            repetition,
            bytes: bytes.into_boxed_slice(),
        })
    }

    pub const fn action(&self) -> ActionKeyV1 {
        self.action
    }

    pub const fn schema(&self) -> SchemaIdV1 {
        self.schema
    }

    pub const fn implementation(&self) -> ImplementationIdV1 {
        self.implementation
    }

    pub const fn repetition(&self) -> u8 {
        self.repetition
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl CanonicalEncode for ObservationEnvelopeV1 {
    fn encode_canonical<S: CanonicalSink>(&self, out: &mut S) -> Result<(), CanonicalError> {
        out.write(b"{\"action\":")?;
        write_hex(out, self.action.as_bytes())?;
        out.write(b",\"implementation\":")?;
        write_hex(out, self.implementation.as_bytes())?;
        out.write(b",\"observation\":")?;
        write_hex(out, self.bytes.as_ref())?;
        out.write(b",\"repetition\":")?;
        write_u64(out, self.repetition as u64)?;
        out.write(b",\"schema\":")?;
        write_hex(out, self.schema.as_bytes())?;
        out.write(b"}")
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RootReceiptV1 {
    graph: GraphDigestV1,
    profile: ObjectDigestV1,
    root: ObjectDigestV1,
    outcome: ObjectDigestV1,
    membership: ObjectDigestV1,
}

impl RootReceiptV1 {
    pub const fn new(
        graph: GraphDigestV1,
        profile: ObjectDigestV1,
        root: ObjectDigestV1,
        outcome: ObjectDigestV1,
        membership: ObjectDigestV1,
    ) -> Self {
        Self {
            graph,
            profile,
            root,
            outcome,
            membership,
        }
    }

    pub const fn graph(&self) -> GraphDigestV1 {
        self.graph
    }

    pub const fn profile(&self) -> ObjectDigestV1 {
        self.profile
    }

    pub const fn root(&self) -> ObjectDigestV1 {
        self.root
    }

    pub const fn outcome(&self) -> ObjectDigestV1 {
        self.outcome
    }

    pub const fn membership(&self) -> ObjectDigestV1 {
        self.membership
    }
}

impl CanonicalEncode for RootReceiptV1 {
    fn encode_canonical<S: CanonicalSink>(&self, out: &mut S) -> Result<(), CanonicalError> {
        out.write(b"{\"graph\":")?;
        write_hex(out, self.graph.as_bytes())?;
        out.write(b",\"membership\":")?;
        write_hex(out, self.membership.as_bytes())?;
        out.write(b",\"outcome\":")?;
        write_hex(out, self.outcome.as_bytes())?;
        out.write(b",\"profile\":")?;
        write_hex(out, self.profile.as_bytes())?;
        out.write(b",\"root\":")?;
        write_hex(out, self.root.as_bytes())?;
        out.write(b"}")
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CaseIdV1(String);

impl CaseIdV1 {
    pub fn try_new(value: String) -> Result<Self, ProtocolError> {
        if value.is_empty() || value.contains('\0') {
            return Err(ProtocolError::EmptyIdentity);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl CanonicalEncode for CaseIdV1 {
    fn encode_canonical<S: CanonicalSink>(&self, out: &mut S) -> Result<(), CanonicalError> {
        tsc_ci_core::CanonicalValue::String(self.0.clone()).encode_canonical(out)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ShardRangeV1 {
    start: u32,
    end: u32,
}

impl ShardRangeV1 {
    pub const fn try_new(start: u32, end: u32, denominator: u32) -> Result<Self, ProtocolError> {
        if start >= end || end > denominator {
            return Err(ProtocolError::InvalidRange);
        }
        Ok(Self { start, end })
    }

    pub const fn start(&self) -> u32 {
        self.start
    }

    pub const fn end(&self) -> u32 {
        self.end
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ShardSpecV1 {
    id: CaseIdV1,
    range: ShardRangeV1,
    case_ids_digest: ObjectDigestV1,
}

impl ShardSpecV1 {
    pub fn try_new(
        id: CaseIdV1,
        range: ShardRangeV1,
        case_ids_digest: ObjectDigestV1,
    ) -> Result<Self, ProtocolError> {
        if is_zero(case_ids_digest.as_bytes()) {
            return Err(ProtocolError::EmptyIdentity);
        }
        Ok(Self {
            id,
            range,
            case_ids_digest,
        })
    }

    pub fn id(&self) -> &CaseIdV1 {
        &self.id
    }

    pub const fn range(&self) -> ShardRangeV1 {
        self.range
    }

    pub const fn case_ids_digest(&self) -> ObjectDigestV1 {
        self.case_ids_digest
    }
}

impl CanonicalEncode for ShardSpecV1 {
    fn encode_canonical<S: CanonicalSink>(&self, out: &mut S) -> Result<(), CanonicalError> {
        out.write(b"{\"case_ids_digest\":")?;
        write_hex(out, self.case_ids_digest.as_bytes())?;
        out.write(b",\"end\":")?;
        write_u64(out, self.range.end as u64)?;
        out.write(b",\"id\":")?;
        self.id.encode_canonical(out)?;
        out.write(b",\"start\":")?;
        write_u64(out, self.range.start as u64)?;
        out.write(b"}")
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FixedPlanV1 {
    profile: CaseIdV1,
    suite: CaseIdV1,
    schema: SchemaIdV1,
    denominator: u32,
    shards: Box<[ShardSpecV1]>,
    membership_digest: ObjectDigestV1,
    qualification_digest: ObjectDigestV1,
    policy_ids: Box<[CaseIdV1]>,
}

impl FixedPlanV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        profile: CaseIdV1,
        suite: CaseIdV1,
        schema: SchemaIdV1,
        denominator: u32,
        shards: Vec<ShardSpecV1>,
        membership_digest: ObjectDigestV1,
        qualification_digest: ObjectDigestV1,
        policy_ids: Vec<CaseIdV1>,
    ) -> Result<Self, ProtocolError> {
        if denominator == 0
            || is_zero(schema.as_bytes())
            || is_zero(membership_digest.as_bytes())
            || is_zero(qualification_digest.as_bytes())
        {
            return Err(ProtocolError::EmptyIdentity);
        }
        if shards.windows(2).any(|pair| pair[0].id >= pair[1].id) {
            let index = shards
                .windows(2)
                .position(|pair| pair[0].id >= pair[1].id)
                .map_or(0, |index| index + 1);
            return Err(ProtocolError::Unsorted { index });
        }
        if policy_ids.windows(2).any(|pair| pair[0] >= pair[1]) {
            let index = policy_ids
                .windows(2)
                .position(|pair| pair[0] >= pair[1])
                .map_or(0, |index| index + 1);
            return Err(ProtocolError::Unsorted { index });
        }
        let mut expected = 0;
        for (index, shard) in shards.iter().enumerate() {
            if shard.range.start != expected {
                return Err(ProtocolError::Gap { index });
            }
            expected = shard.range.end;
        }
        if expected != denominator {
            return Err(ProtocolError::Gap {
                index: shards.len(),
            });
        }
        Ok(Self {
            profile,
            suite,
            schema,
            denominator,
            shards: shards.into_boxed_slice(),
            membership_digest,
            qualification_digest,
            policy_ids: policy_ids.into_boxed_slice(),
        })
    }

    pub fn profile(&self) -> &CaseIdV1 {
        &self.profile
    }

    pub fn suite(&self) -> &CaseIdV1 {
        &self.suite
    }

    pub const fn schema(&self) -> SchemaIdV1 {
        self.schema
    }

    pub const fn denominator(&self) -> u32 {
        self.denominator
    }

    pub fn shards(&self) -> &[ShardSpecV1] {
        &self.shards
    }

    pub const fn membership_digest(&self) -> ObjectDigestV1 {
        self.membership_digest
    }

    pub const fn qualification_digest(&self) -> ObjectDigestV1 {
        self.qualification_digest
    }

    pub fn policy_ids(&self) -> &[CaseIdV1] {
        &self.policy_ids
    }
}

impl CanonicalEncode for FixedPlanV1 {
    fn encode_canonical<S: CanonicalSink>(&self, out: &mut S) -> Result<(), CanonicalError> {
        out.write(b"{\"denominator\":")?;
        write_u64(out, self.denominator as u64)?;
        out.write(b",\"membership_digest\":")?;
        write_hex(out, self.membership_digest.as_bytes())?;
        out.write(b",\"policy_ids\":[")?;
        encode_values(&self.policy_ids, out)?;
        out.write(b"],\"profile\":")?;
        self.profile.encode_canonical(out)?;
        out.write(b",\"qualification_digest\":")?;
        write_hex(out, self.qualification_digest.as_bytes())?;
        out.write(b",\"schema\":")?;
        write_hex(out, self.schema.as_bytes())?;
        out.write(b",\"shards\":[")?;
        encode_values(&self.shards, out)?;
        out.write(b"],\"suite\":")?;
        self.suite.encode_canonical(out)?;
        out.write(b"}")
    }
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

fn write_u64<S: CanonicalSink>(out: &mut S, value: u64) -> Result<(), CanonicalError> {
    let text = value.to_string();
    out.write(text.as_bytes())
}

fn is_zero(bytes: &[u8]) -> bool {
    bytes.iter().all(|byte| *byte == 0)
}
