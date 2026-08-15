# FCI-4b: immutable inventory and ownership schema

Status: **ready (FCI-4b v1; non-authoritative shadow)**.

This packet adds the pure value model used to describe one complete inventory
of a sealed source snapshot. It records normalized paths, global dispositions,
absence/negative lookups, generated ownership, build-system ownership, path
collisions, and the explicit unknown-input policy. The values are immutable
after construction, contain no filesystem handles, and do not read the ambient
worktree. A later repository adapter may obtain these values from its own
`SourceSnapshotProvider`; it must not add a repository or VCS branch to
`ci-core`.

## Base and allowed paths

- Trusted base: the closed FCI-4a.3 commit recorded in
  `ratchets/fci-readiness/fci-4b.v1.json`.
- Allowed source paths: `crates/ci-core/src/lib.rs`,
  `crates/ci-core/src/inventory.rs`, its inventory tests, this packet, the
  slice index, and its readiness envelope.
- Forbidden: filesystem/snapshot providers, runner effects, graph impact or
  transition authority, outcomes, CAS/cache, H2/workspace adapters,
  compiler/oracle/xtask, workflows, and production testkit dependencies.

## Frozen API

```rust
pub struct NormalizedPathV1(/* validated UTF-8 workspace-relative bytes */);

pub enum GlobalDispositionV1 {
    Present, Deleted, Ignored, Generated, Symlink, Submodule, Unknown,
}

pub struct InventoryEntryV1 {
    /* path, disposition, optional content digest */
}

pub struct NegativeLookupV1 {
    /* requested path, lookup schema, ordered roots, listing digest */
}

pub struct GeneratedOwnershipV1 {
    /* output path, generator action key, implementation id */
}

pub struct BuildSystemOwnershipV1 {
    /* producer, ordered inputs/outputs, opaque flag */
}

pub struct PathCollisionV1 {
    /* ordered pair and Exact/CaseFolded/UnicodeEquivalent kind */
}

pub enum UnknownInputPolicyV1 { FailClosed, ImpactAll }

pub struct WorkspaceInventorySpecV1 {
    /* strict entries, negatives, generated ownership, build ownership,
       and unknown policy */
}
```

`NormalizedPathV1` rejects empty, absolute, non-UTF-8, NUL-containing,
backslash-containing, empty-component, `.` and `..` paths. Every ordered
collection uses strict `Ord` ordering, so duplicates are rejected before a
spec can be constructed. Path collisions retain both spellings and an
explicit collision kind; a collision record cannot reverse its ordered pair.
Content is an optional object digest because deleted, ignored, generated,
symlink, submodule, and unknown entries need not have regular-file bytes.

Negative lookups bind the requested path to the exact lookup schema, ordered
search roots, and the directory-listing digest that proved absence. Generated
ownership binds an output to one action key and semantic implementation.
Build-system ownership retains sorted direct input/output paths and an
`opaque` bit; an opaque producer is evidence that later impact planning must
remain conservative rather than treating an unexplained output as a cache
hit. `UnknownInputPolicyV1::FailClosed` stops planning when no validated owner
exists; `ImpactAll` permits only a separately validated conservative-all-raw
owner in the later impact packet. This schema itself never chooses that owner.

All values implement the framework's bounded canonical encoding once their
packet-owned fields are rendered. Canonical encoding is a pure operation over
the value and cannot inspect a path, clock, environment, VCS, or process.
The source snapshot guard and immutable mount remain FCI-3c effect-bound
values; this packet only describes the resulting inventory evidence.

## Proof

```text
cargo xtask test ci-core
cargo test -p tsc-rs-ci-core --test inventory
node .github/ci/slice-readiness.mjs --check fci-4b
```

Fixtures cover exact global dispositions and ownership, path traversal and
collision rejection, strict ordering, explicit unknown/opaque policy, and a
source-literal audit proving that the generic inventory core has no
repository/tool branch. The separate FCI-5c.1 packet consumes this seam for
the H2 membership shadow; this packet itself does not claim H2 membership or
authority.
