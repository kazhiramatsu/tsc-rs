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
cargo test --offline          # substrate + shadow-adapter tests
cargo run --offline --bin shadow   # one report-only sample
```

The shadow adapter reads the repository READ-ONLY (the H2 oracle and
ratchet inventory, the typed pin grammar, the walk driver, and the
latest real chain-walk certificate) and writes one deterministic run
under `new-ci/target/new-ci-shadow/runs/<run-id>/report.{json,md}`.
It is report-only: it does not couple to a gate, invoke oracle write
mode, observe the 5g qualification rung, or schedule future samples.
Provenance of the mission and boundaries: `SPEC.md`.

## Caveats before any trusted use

Reconciliation note (2026-08-28): `STATUS.md` records the M1-M3
milestones complete — M3 added the transaction manifest, leases, and
status machinery — so the second bullet below is PARTIALLY stale as a
progress statement. The list is retained deliberately: per the
evidence-DAG packet's sequencing amendment (item 4), this caveat list
IS the trusted-promotion checklist, and no item is struck until the
promotion review verifies it.

- The SHA-256 implementation is local (offline constraint). Replace
  with the `sha2` crate before this code participates in any trust
  decision.
- Receipt store: local mint + GC-root stub only at M0; M3 landed the
  transaction manifest, lease, and status layers (see `STATUS.md`) —
  the blob CAS and remote paths of the design document remain
  unimplemented.
- The 9,027-case observation node is modeled, not wired to the oracle
  (the Phase 0 report-only shadow adapter is the planned consumer).
- Legacy FCI crates are recorded and superseded, not ported — see
  `LEGACY-FCI-DISPOSITION.md`.
