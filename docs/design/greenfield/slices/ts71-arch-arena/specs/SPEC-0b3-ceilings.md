# SPEC-0b.3 — the four ceiling experiments (throwaway builds, never merged)

Track: [ts71-arch-arena](../README.md), phase 0b ([SPEC-0b](SPEC-0b.md) §0b.3). Each experiment is one lane in its own clone,
one cargo feature on a branch from main that is **never merged**; its only product is a number the integrator measures. Outputs
may differ from tsgo (say so if they do). Lane rules as [SPEC-arena](SPEC-arena.md) (no commits; cumulative patches under
`target/lane-<name>/patches/`; `STATUS.md`; offline; `nice -n 20 cargo … -j 2`; one cargo command at a time; time box one working
day each). Builds allowed per lane: `cargo check --workspace` and **one** `cargo build -p tsc-rs-cli` in **dev** profile with the
feature on, plus the focused tests of the crate you touch. **No release build and no measurement by the lane**: the integrator
builds release and measures in a quiet window (`serial.py`, 1 thread, instructions; `phasecmp.py` for same-named function ratios).

The feature name is `ceiling-<name>`; with the feature off the binary must be byte-identical to main (record the dev CLI hash with
the feature off before and after your patch). Keep the diff small and local; do not refactor around it.

## (i) `ceiling-bump` — allocation ceiling (lane `bump`)

In `crates/cli` (the binary crate), behind the feature: a `#[global_allocator]` that bump-allocates from 64 MiB `mmap` chunks,
`dealloc` is a no-op, `realloc` allocates and copies, alignment honored, thread-safe with an atomic bump (one chunk at a time
behind a mutex when a chunk is exhausted). Memory grows without bound; that is the point. Print the total bytes handed out on
exit to stderr only when `TSRS_CEILING_REPORT` is set. Also keep mimalloc as the feature-off allocator exactly as today. The
integrator runs next.js, zod, TypeScript compiler and hono (VS Code only if the total fits in memory). Answers: how many
instructions are allocator + free work.

## (ii) `ceiling-dense-links` — links indirection ceiling (lane `links`)

In `crates/checker/src/links.rs`, behind the feature: the three hot tables (`NodeLinks` 16 B by `NodeId`, persistent
`SymbolLinks` 12 B by persistent `SymbolId`, transient `SymbolLinks` 20 B by transient index, `TypeLinks` 24 B by `TypeId`) read and
written through plain dense `Vec<Record>` indexed by the id's index (grow on write with `resize`; zero records = vacant, as today's
`IdSlot` encoding), replacing `PagedTable` on those paths only. Capacity hints from the program's node/symbol counts. Safe
indexing (`[]`); no `unsafe`. Memory grows (a dense node-links vector over all node positions is ≈ 275 MiB per checker on VS
Code); that is accepted for the ceiling. The journal/revert paths must keep working (run `cargo test -p tsc-rs-checker --lib
speculate` and `links`). Answers: the indirection tax of `PagedTable` (page → slot → chunk → record) on the hottest reads.

## (iii) `ceiling-fe4` — front-end ceiling (lane `fe4`)

The loader parses each file at private identities and later relocates and publishes it (`relocate_with_leases`,
`apply_identity_relocation`, `publish`). Behind the feature: lease the file's identity ranges **before** parsing from an estimate
(nodes ≤ text bytes / 2 + 64, arrays and items proportionally; use `NodeArena::symbol_bound`-style bounds where they exist) and
parse directly at the final positions, so no relocation and no copy happen on the common path; when an estimate is exceeded,
fall back to today's path for that file (count the fallbacks and print them under `TSRS_CEILING_REPORT`). Keep program order,
diagnostics order and all outputs identical (the node-order audit found no cross-file id-order dependency, but verify with
`diffout.py` on the 8 projects — the integrator runs it). Answers: what the publication/adopt bucket really costs when removed,
on today's representation. Together with the phase ledger this bounds the front-end gain available without the AST rewrite.

## (iv) `ceiling-direct-data` — dispatch ceiling (lane `dispatch`)

`Node::data()` decodes the payload through a `match` on `kind` into an owned `NodeData`, and the caller matches again. Behind the
feature, at the 50 hottest `NodeData::` match sites of `crates/checker/src` (ranked by the line profile under
`target/codex-line-profile-20261107/reviewed/` — the integrator supplies the ranked list in `HOT-SITES.txt` in the clone root),
replace `match node.data() { NodeData::X(d) => …, _ => … }` with the existing kind-checked single decoders (`node.as_x()`), or, when
the site reads one or two fields, with direct slot reads through the generated per-kind decoders. No semantic change; `cargo test
-p tsc-rs-checker` focused on the touched modules. Answers: whether the double dispatch is worth generating hot accessors for.

## Deliverables (each lane)

The patch, `STATUS.md` with the feature-off hash equality, the dev build with the feature on, the focused tests, and for (iii) the
fallback counter. Nothing else.
