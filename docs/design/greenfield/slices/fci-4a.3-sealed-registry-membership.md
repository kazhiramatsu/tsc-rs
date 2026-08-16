# FCI-4a.3: sealed registry, membership, and testkit

Status: **ready (FCI-4a.3 v1; non-authoritative shadow)**.

This packet closes the generic in-memory seam: typed adapter codecs can be
registered, a consuming builder seals an exact descriptor set, and only the
sealed registry can invoke its private monomorphized decode/re-encode
function. It also completes pending adapter/composite membership through
private typestate construction and adds the development-only generic testkit.
No outcome manifest, projection, CAS/cache, authority, or live runner entry is
created.

## Base and allowed paths

- Trusted base: the closed FCI-4a.2 commit recorded in
  `ratchets/fci-readiness/fci-4a.3.v1.json`.
- Allowed source paths: `Cargo.toml`, `Cargo.lock`, `ci-core` graph/model/
  registry/membership/lib sources, their tests, the `ci-testkit` package and
  tests, this packet, the slice index, and its readiness envelope.
- Forbidden: outcome/projection/CAS/cache/live evaluation, H2 or workspace
  adapters, compiler/oracle/xtask, workflows, and any production dependency on
  `ci-testkit`.

## Frozen API

```rust
pub trait AdapterCodec: Send + Sync + 'static {
    type RawObservation: CanonicalEncode;
    fn descriptor() -> AdapterDescriptorV1;
    fn decode(bytes: &[u8]) -> Result<Self::RawObservation, AdapterDecodeError>;
}

pub struct AdapterRegistration { /* descriptor + private typed function */ }
impl AdapterRegistration {
    pub fn of<C: AdapterCodec>() -> Self;
}

pub struct AdapterRegistryBuilder { /* consuming protected registrations */ }
impl AdapterRegistryBuilder {
    pub const fn new() -> Self;
    pub fn register(&mut self, registration: AdapterRegistration)
        -> Result<(), RegistryError>;
    pub fn seal(
        self,
        expected: &AdapterDescriptorSetV1,
    ) -> Result<VerifiedAdapterRegistry, RegistryError>;
}

pub struct VerifiedAdapterRegistry { /* private exact descriptor/function set */ }
impl VerifiedAdapterRegistry {
    pub fn decode_reencode(
        &self,
        descriptor: AdapterDescriptorV1,
        bytes: &[u8],
    ) -> Result<Vec<u8>, RegistryError>;
}

pub fn complete_adapter_input<I, V>(
    pending: &PendingMembership<I, V>,
    values: Vec<(I, V)>,
) -> Result<CompleteAdapterInput<I, V>, MembershipError>
where
    I: Clone + Ord;

pub fn complete_composite_input<I, V>(
    pending: &PendingMembership<I, V>,
    values: Vec<(I, V)>,
) -> Result<CompleteCompositeInput<I, V>, MembershipError>
where
    I: Clone + Ord;
```

`AdapterRegistryBuilder::register` rejects duplicate descriptors. `seal`
consumes the builder, sorts registrations deterministically, requires exact
descriptor-set membership, and computes a purpose-specific registry digest.
The only runtime operation is the sealed registry's private monomorphized
`decode -> canonical re-encode -> byte equality` function; malformed or
noncanonical bytes remain `AdapterDecodeError`, never a miss or semantic
verdict. There is no late registration, unseal, candidate callback, id/kind
branch, or `Any` downcast.

`ActionModel` and the pure `LeafVerdict`, `DerivedVerdict`, and `AdapterVerdict`
families are typed over adapter-owned values. Rejected verdicts are completed
values with stable codes; invariant errors remain separate. Proposal records
from FCI-4a.2 are consumed by the model contract but no prepared execution is
constructed here.

`CompleteMembership` still has no public constructor. The completion helpers
require exact sorted id/value pairs and reject missing, unexpected, duplicate,
or unsorted members before constructing the private sealed marker. Adapter and
composite inputs therefore cannot be fabricated from a raw `Vec` or a partial
set.

`ci-testkit` is a dev-only package containing two structurally different fake
record shapes. It depends on the framework crates for conformance fixtures;
normal runtime packages must not depend on it.

## Proof

```text
cargo test -p tsc-rs-ci-core --test registry_membership
cargo test -p tsc-rs-ci-testkit
cargo tree -p tsc-rs-ci-testkit --edges normal,build,dev
node .github/ci/slice-readiness.mjs --check fci-4a.3
```

Fixtures cover duplicate/missing/unexpected registration, exact seal and
registry digest, canonical decode/re-encode and rejection, pending-to-complete
adapter/composite membership, and two testkit shapes using one generic core.
