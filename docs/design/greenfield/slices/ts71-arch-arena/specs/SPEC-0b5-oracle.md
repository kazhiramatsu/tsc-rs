# SPEC-0b.5 — the fingerprint oracle behind a trait, baseline reproduced

Track: [ts71-arch-arena](../README.md), phase 0b ([SPEC-0b](SPEC-0b.md) §0b.5). Lane rules as [SPEC-arena](SPEC-arena.md)
(no commits; patches under `target/lane-oracle/patches/`; `STATUS.md`; offline; `nice -n 20 cargo … -j 2`; one cargo command at a
time; time box one working day). Builds allowed: `cargo build --release -p tsc-rs-binder --example syntax_fingerprint -j 2` (one
release build of one example is allowed and required), `cargo test -p tsc-rs-binder`.

## Goal

`crates/binder/examples/syntax_fingerprint.rs` enumerates symbols and flows by dense index (`SymbolId::new(base + i)`,
`FlowId::new(index)`, lines ≈ 516–534) and reads binder facts through today's `BindData` maps. The arena rewrite (phase B) replaces
both. Put every such access behind a small trait implemented on today's code, and prove the recorded baseline is reproduced
byte for byte, so the oracle can be re-implemented on the new model without changing its output.

## Deliverables

1. In the example (or a small module under `crates/binder/src/` if the example needs it public): `trait FingerprintSource` with
   methods for (a) symbols in creation order, (b) flow nodes in creation order with their antecedent lists, (c) the per-node
   binder facts the hash covers today (symbol, local symbol, locals table, flow, end/return flow, the six flow kinds `node_flows`
   reads, next container, binder-written flags), (d) the parse-side facts (kind, range, payload fields through
   `observable_fields`, JSDoc attachment). Today's `Binder`/`BindData` implement it; the hashing code calls only the trait.
2. No change in output: `scripts/syntax_fingerprint.py --compare target/claude-tools/handover/perf-tsrs/syntax-baseline/<latest>.tsv`
   over the conformance corpus must report zero differences. The baseline directory and the exact file you compared against go in
   `STATUS.md` with the command and its last lines. (The bench corpora under `~/dev/bench-corpora` may be included if present; say
   which corpora ran.)
3. A note in the example's module doc listing the dense-index assumptions that were removed and the trait method that replaced each.
4. `STATUS.md` records the example binary's SHA-256 before and after the change (the baseline was produced by the old binary; a
   future lane needs the "before" hash to prove continuity).

## Out of scope

Changing what is hashed; JSON files (excluded today by extension — keep); the arena implementation of the trait (phase B).

## Acceptance (integrator)

`--compare` zero differences on the conformance corpus; the trait is the only path from the hash to binder/parser data.
