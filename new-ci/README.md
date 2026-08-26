# new-ci — evidence-DAG substrate prototype + shadow reporter

Standalone Cargo project, deliberately NOT a member of the repository
workspace: it adds zero crates/*.rs bytes, so it never re-stales the
oracle ladder and is invisible to the gate. Promotion state: ratified
2026-08-26 as (a) design packet in
`docs/design/greenfield/new-ci-evidence-dag.md` (the normative
document — read that first), (b) this directory as an out-of-workspace
tool with the shadow adapter as a standing reporter, (c) the pin-span
extraction feeding gate-tax 5's typed pin-index.

## Usage

```
cd new-ci
cargo test --offline          # substrate unit tests (9)
cargo run --offline --bin shadow   # regenerates shadow-report.md
```

The shadow adapter reads the repository READ-ONLY (pin grammars over
`crates/oracle/h2-*.mjs`, `git show` for incident replays) and writes
`new-ci/shadow-report.md` (generated — not committed): the ladder's
dependency graph with core/envelope projection digests per script, and
the classification of a named incident commit. Provenance of the
mission and boundaries: `SPEC.md`.

## Caveats before any trusted use

- The SHA-256 implementation is local (offline constraint). Replace
  with the `sha2` crate before this code participates in any trust
  decision.
- Receipt store: local mint + GC-root stub only; the transaction
  manifest, leases, and blob CAS of the design document are not yet
  implemented.
- The 9,027-case observation node is modeled, not wired to the oracle.
