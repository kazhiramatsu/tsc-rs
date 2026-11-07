# SPEC-0b — the de-risk week: B1 anchor, ceiling experiments, PGO, oracle trait (phase 0b)

Track: [ts71-arch-arena](../README.md). Added 2026-11-07 after the adversarial reviews (Opus B2/B3/M13/M14, dispositions in
`target/claude-tools/handover/arch-arena/REVIEW-DISPOSITIONS.md`). Nothing in this phase changes the compiler's behavior on
main; it produces the numbers the phase gates are set from and tests the design's premise before any freeze. Four lanes,
**at most two building at a time**, the integrator owns every measurement window.

## 0b.1 Phase ledger on main (lane, 2 person-days)

Port the M1 instrument (`target/claude-tools/handover/perf-tsrs/m1-lane-record/patches/`, `REPORT-m1.md` for the method:
`proc_pid_rusage(RUSAGE_INFO_V4)` snapshots at coarse phase boundaries, `ri_instructions`, monotonic, overhead qualified) onto
#913 behind cargo feature `phase-ledger` (default off; the normal binary is unchanged byte for byte — verify with a hash of the
CLI built without the feature before and after). Boundaries, architecture-neutral so they survive the rewrite: `config`,
`load` (= program creation start → binder start; today this contains parse, request collection, resolution, publication/adopt),
`bind`, `checker-init`, `semantic` (= checker-init end → diagnostics finalization start), `finalization`. Output: one JSON
line per phase with retired instructions, wall, and footprint, plus the process total; a script `scripts/phase_ledger.py` that
runs a binary N rounds with warm-up and prints the median table and the reconciliation (sum of phases vs. total, remainder
shown). Lane rules as SPEC-arena. No subject change in the same patch.

## 0b.2 B1 anchor (integrator, one quiet window)

With 0b.1 merged: tsc-rs #913+ledger and tsrs (`tsrs-sym-nopgo`, `--singleThreaded`, its own `--extendedDiagnostics` phase
rows) on the 8 projects, 1 thread, warm-up 2, 9 rounds, medians, plus `serial.py` instruction totals and peak footprint of the
ledger-free binary (the ledger binary's totals are for the split only). Record B1 in the design document §8 as the base of
every gate: `front end = load + bind`, `semantic`, `other`, with tsrs beside each. Also the 4-thread and default wall/peak
(`frontier.sh`) for the wall gates.

## 0b.3 Ceiling experiments (lanes, throwaway builds, never merged; ≈ 4 person-days)

Each is a branch from #913 whose only purpose is a number; outputs may differ from tsgo (say so); each is measured by the
integrator with `serial.py` (instructions, 1 thread, VS Code / next / zod) and `phasecmp.py` (same-named checker function
ratios vs tsrs) in a quiet window.

- (i) **Allocation ceiling**: global allocator = bump, `dealloc` is a no-op (memory grows; measure next/zod/TypeScript compiler, VS
  Code only if it fits in 16 GiB). Answers: how many instructions are allocator + free work.
- (ii) **Links ceiling**: the five hottest links reads (`node_links.resolved_type`, `symbol_links.type_of_symbol`,
  `symbol_links.declared_type`, `type_links.resolved_members`, `node_links.resolved_symbol`) through dense, unchecked
  arrays indexed by id (no PagedTable). Answers: the indirection tax of the links layer.
- (iii) **Front-end ceiling**: parse directly into final identities with no publication/adopt (FE4's mechanism), binder
  per-node outputs written inline into a widened node record (temporary), module requests recorded during parse; run only
  parse + bind + fingerprint on VS Code (no checker). Answers: the reachable front-end instruction count without the checker
  migration.
- (iv) **Dispatch ceiling**: the top 50 `NodeData::` match sites in the checker (by profile sample count from
  `codex-line-profile-20261107/reviewed/`) rewritten to kind-checked direct field reads; measure. Answers: whether the
  `data()` double dispatch is a cost worth generating accessors for.

Decision thresholds (design §8, review B3): if (iii) does not show the front end at ≤ B1_front − 6 G, the front-end phase is
re-scoped (resolution/loader included) or re-aimed; if (i)+(ii) together do not bring the same-named checker ratio from ≈ 1.22×
to ≤ 1.10×, the checker phase's semantic gate is set from what they do show, and the owner decides whether the phase is worth
its cost. These are decision rules written before the numbers exist.

## 0b.4 PGO release pipeline (lane, 2 person-days)

`.github/workflows/pgo.yml` (DRAFT) and `scripts/pgo_build.py` into a release workflow: training corpus = hono + two
conformance batches + zod (pinned), `-Cprofile-generate` → merge → `-Cprofile-use` release CLI, artifact published on a
tag. Report in the packet: VS Code 1-thread instructions normal vs PGO for #913 (slice 8 measured −20.8% on the 11-02 base).
From here every comparison with tsrs is reported twice: normal-vs-normal and PGO-vs-PGO. PGO is not counted as an
architectural gain of the rewrite.

## 0b.5 Fingerprint oracle behind a trait (lane, 1 person-day)

`crates/binder/examples/syntax_fingerprint.rs` enumerates symbols and flows by dense index (`SymbolId::new(base + i)`,
`FlowId::new(i)`, lines 518–532). Put the enumeration behind a small trait (`symbols in creation order`, `flows in creation
order`, `binder fields of a node`), implement it on today's code, and prove the recorded baseline
(`target/claude-tools/handover/perf-tsrs/syntax-baseline/`) is reproduced byte for byte (`scripts/syntax_fingerprint.py
--compare`). The front-end phase then implements the trait on the arena model without changing the oracle's output.

## Exit of phase 0b

B1 recorded with tsrs beside it; the four ceilings recorded with their decision outcomes; PGO workflow green once; the oracle
trait merged with the baseline reproduced. The owner decides, from the ceilings, whether to open the checker phase (and the
freeze) or to re-aim.
