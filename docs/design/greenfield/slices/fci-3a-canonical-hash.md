# FCI-3a: canonical bytes and hashes

Status: **ready (FCI-3a v1; non-authoritative shadow)**.

This packet makes Rust canonical bytes and domain-separated purpose-specific
digests available to the generic `ci-core` crate. It does not add adapter
registration, execution values, outcomes, CAS/cache, publication, or H2
types. Typed schema owners supply their allowed field set to the generic
unknown-field helper; `ci-core` contains no repository vocabulary.

## Base and allowed paths

- Trusted base: the closed FCI-2b commit recorded in
  `ratchets/fci-readiness/fci-3a.v1.json`.
- Allowed source paths: `Cargo.toml`, `Cargo.lock`, the `ci-core` manifest and
  canonical/hash/id/digest/lib sources, their unit tests, this packet, the
  slice index, and its readiness envelope.
- Forbidden: `ci-runner`, compiler/emitter/oracle/xtask paths, workflows, H2
  profiles, adapter registration, execution, outcomes, authority, CAS/cache,
  and provider code.

## Frozen canonical API

```rust
pub trait CanonicalSink {
    fn write(&mut self, bytes: &[u8]) -> Result<(), CanonicalError>;
    fn remaining(&self) -> u64;
}

pub trait CanonicalEncode {
    fn encode_canonical<S: CanonicalSink>(
        &self,
        out: &mut S,
    ) -> Result<(), CanonicalError>;
}

pub trait CanonicalDecoder {
    type Output;
    fn push(&mut self, chunk: &[u8]) -> Result<(), DecodeError>;
    fn finish(self) -> Result<Self::Output, DecodeError>;
}

pub enum CanonicalValue {
    Null,
    Bool(bool),
    Signed(i64),
    Unsigned(u64),
    String(String),
    Array(Vec<Self>),
    Object(Vec<(String, Self)>),
}

pub struct BoundedBytesSink { /* bounded test/control-plane sink */ }
pub struct StrictJsonDecoder { /* bounded push decoder */ }

pub fn decode_canonical(
    bytes: &[u8],
    max_bytes: u64,
    max_depth: usize,
) -> Result<CanonicalValue, DecodeError>;

pub fn decode_object_with_keys(
    bytes: &[u8],
    max_bytes: u64,
    max_depth: usize,
    allowed_keys: &[&str],
) -> Result<CanonicalValue, DecodeError>;
```

The encoder emits UTF-8 JSON with sorted decoded-key UTF-8 bytes, preserved
array order, shortest signed/unsigned decimal integers, no floats,
no whitespace, no final newline, and exactly the v1 control escapes. Slash,
non-ASCII, U+2028, and U+2029 remain raw UTF-8. The bounded sink rejects a
write before crossing its ceiling. Strict decoding rejects whitespace,
alternate escapes, uppercase hex, leading zeroes, decimal/exponent numbers,
duplicate or unsorted keys, invalid/unpaired surrogates, trailing bytes,
depth overflow, and any input whose canonical re-encoding differs. Valid
surrogate pairs are combined before that exact re-encoding check. The typed
field helper rejects keys outside its caller-supplied set without embedding an
adapter name in `ci-core`.

## Frozen domains and digest framing

`ProtocolDomainTagV1::all()` contains the complete 25-tag v1 registry:
application namespace, canonical input, action key, build artifact, root,
graph, node spec, source snapshot, adapter descriptor, adapter registry,
object, outcome, interior, candidate manifest, conflict registry, authority
receipt, publication event, generation, head, evidence snapshot, publication
snapshot, trust transition, policy proof, lease, and GC plan. Tags are fixed
16-byte protocol identifiers and are unique.

Every frame is:

```text
u32_be(domain_tag_byte_length)
domain_tag_bytes
u64_be(canonical_payload_byte_length)
canonical_payload_bytes
```

Action keys additionally encode each input part as `u64_be(length) || bytes`
inside the framed payload, binding the application-namespace digest and
canonical input separately. Typed digest newtypes include
`ApplicationNamespaceDigestV1`, `InputDigestV1`, `ObjectDigestV1`,
`ActionKeyV1`, `GraphDigestV1`, `BuildArtifactIdV1`, `OutcomeDigestV1`,
`AdapterRegistryDigestV1`, `ConflictRegistryDigestV1`,
`AuthorityReceiptDigestV1`, and `PublicationEventDigestV1`. No generic digest
cast or unregistered domain function is exported.

`NamespaceLineageV1` and its validators require a non-empty stable namespace,
reject self/empty forks, and require a rename to preserve the stable identity.
Changing a display name therefore cannot alias or abandon a namespace.

## Proof

```text
cargo test -p tsc-rs-ci-core --test contracts canonical
cargo test -p tsc-rs-ci-core --test contracts hashes
cargo test -p tsc-rs-ci-core --test contracts identifiers
node .github/ci/slice-readiness.mjs --check fci-3a
```

Fixtures cover every permitted escape, raw slash/non-ASCII/U+2028, key order,
integer width, bounds, unknown/duplicate fields, surrogate handling, exact
frame/hash bytes, domain uniqueness, cross-namespace non-aliasing, and fork or
rename rejection. Later packets add typed schema fixtures and adapter parity;
this packet reserves no payload migration or event schema.
