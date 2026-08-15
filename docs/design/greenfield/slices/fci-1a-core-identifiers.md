# FCI-1a: pure core identifiers and dependency boundary

Status: **ready (FCI-1a v1; non-authoritative shadow)**.

This packet is the first production Functional-CI packet. It is intentionally
small and pre-closure safe: it creates only inert, generic values in
`ci-core`. It does not change an H2.5g command or produce semantic evidence.

## Base and prerequisites

- Trusted base: `f7f0b1c59e1951f9295048034c1b9fed7c19b33f` (the packet-control
  bootstrap commit).
- Predecessor: `packet-control-bootstrap` proof and its readiness receipt.
- No FCI-1b or later symbols may be referenced.

## Allowed paths

- `Cargo.toml` workspace member and dependency wiring only;
- `crates/ci-core/Cargo.toml`;
- `crates/ci-core/src/lib.rs`;
- `crates/ci-core/src/ids.rs`;
- `crates/ci-core/src/digest.rs`;
- `crates/ci-core/src/input.rs`;
- `crates/ci-core/tests/identifiers.rs`;
- `ratchets/fci-readiness/fci-1a.v1.json`; and
- this packet and the slice index status line.

Any emitter, parser, checker, compiler, `xtask`, workflow, H2 profile,
qualification artifact, cache, runner, graph, adapter, outcome, or testkit
file is forbidden.

## Frozen package boundary

`tsc-rs-ci-core` is `publish = false`, has no workspace dependency, and may
depend only on the standard library in this packet. The crate must not mention
`tsc-rs`, `H2`, `Cargo`, `TypeScript`, `oracle`, `xtask`, a case id, or a
repository path in production code.

The public symbols and signatures are fixed:

```rust
pub struct ProtocolDomainV1([u8; 16]);
pub struct ApplicationNamespaceV1([u8; 16]);
pub struct SchemaIdV1([u8; 16]);
pub struct ImplementationIdV1([u8; 16]);

pub struct ObjectDigestV1([u8; 32]);
pub struct InputDigestV1([u8; 32]);

pub struct CanonicalInputRefV1 {
    pub namespace: ApplicationNamespaceV1,
    pub schema: SchemaIdV1,
    pub implementation: ImplementationIdV1,
    pub payload: InputDigestV1,
}
```

Each identifier is `Clone + Copy + Eq + Ord + Hash + Debug` with private
fields. Each has:

```rust
pub const fn from_bytes(bytes: [u8; N]) -> Self;
pub const fn as_bytes(&self) -> &[u8; N];
```

The digest types use `N = 32`; the four identity types use `N = 16`.
`CanonicalInputRefV1` is `Clone + Eq + Ord + Hash + Debug` and has no hashing,
serialization, filesystem, or validation behavior in this packet. Canonical
encoding and strict decoding belong exclusively to FCI-3a. `ObjectDigestV1`
is reserved for later immutable objects; no object store API is introduced.

## Implementation order

1. Add the package and workspace wiring with no production dependency edge.
2. Add the four identity newtypes and two purpose-specific digest newtypes.
3. Add `CanonicalInputRefV1` as an inert value whose field order is fixed by
   the declaration above.
4. Add unit tests for byte round trips, equality/ordering, and cross-type API
   separation. Add a source audit test that rejects repository/compiler nouns
   and effect imports in `ci-core/src`.
5. Record the exact package/source/test digests in the readiness envelope.

## Proof and non-goals

The packet proof is:

```text
cargo test -p tsc-rs-ci-core
cargo tree -p tsc-rs-ci-core --edges normal,build,dev
node .github/ci/slice-readiness.mjs --check fci-1a
```

Expected results are a green identifier suite, no non-standard dependency
edge, no forbidden production literal, and a readiness receipt bound to the
packet body. This packet does not claim a functional cache, resume behavior,
H2.5g shadow result, or performance improvement. Those arrive only through
the later dependency-ordered packets.
