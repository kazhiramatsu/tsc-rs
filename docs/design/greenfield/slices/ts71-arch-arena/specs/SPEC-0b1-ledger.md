# SPEC-0b.1 — the phase ledger on main behind a cargo feature

Track: [ts71-arch-arena](../README.md), phase 0b ([SPEC-0b](SPEC-0b.md) §0b.1). Lane rules as [SPEC-arena](SPEC-arena.md)
(no commits; cumulative patches under `target/lane-ledger/patches/`; `STATUS.md`; offline; `nice -n 20 cargo … -j 2`;
`--test-threads=1`; one cargo command at a time; time box two working days). Builds allowed: `cargo check --workspace` (dev),
`cargo build -p tsc-rs-cli` in **dev** profile with and without the feature, `cargo test -p tsc-rs-types` and the crate(s) you touch.
No release build (the integrator builds release in a quiet window), no benchmarks, no measurement claims.

## Goal

Port the M1 observation instrument to main `aefd66ce6` as cargo feature `phase-ledger` (default off) so the integrator can measure
B1 (tsc-rs vs tsrs, per phase, retired instructions) with a binary whose feature-off form is byte-identical to today's CLI.

## Inputs

- `target/claude-tools/handover/perf-tsrs/m1-lane-record/` — `SPEC-m1.md`, `REPORT-m1.md` (method: `proc_pid_rusage(RUSAGE_INFO_V4)`
  snapshots at coarse phase boundaries, `ri_instructions`, monotonicity, probe calibration, exclusive-child subtraction, and the
  rule that attribution is valid only when loading and checking are serialized), `patches/` (the observation patch on B0
  `c3485397a`; it will not apply cleanly — port by hand).
- `crates/types/src/trace.rs` (`TSRS_PHASE_TRACE`: the existing opt-in wall/memory phase trace and its seam in the CLI).
- `crates/cli`, `crates/compiler`, `crates/checker/src/lib.rs` (`bind_reserved_in_program_order`, the checker pool entry).

## Deliverables

1. Feature `phase-ledger` in `tsc-rs-types` (and the crates that forward it). With the feature off: no code change in any hot path
   (verify: `cargo build -p tsc-rs-cli` dev binary hash equal before/after your patch with the feature off — record both hashes).
2. With the feature on and `TSRS_PHASE_LEDGER=<path>` set, the CLI writes one JSON line per boundary with: phase name, cumulative
   `ri_instructions`, `ri_user_time`, `ri_system_time`, wall ns, `ri_lifetime_max_phys_footprint`, and a final `total` line. Phase
   boundaries (architecture-neutral, see SPEC-0b): `config` (start → options ready), `load` (program creation start → binder start,
   including parse, request collection, resolution, publication/adopt today), `bind`, `checker-init`, `semantic` (checker-init end →
   diagnostics finalization start), `finalization` (→ just before exit). Nested spans are not emitted; the remainder is `total −
   sum(phases)`. The probe must not change the order of work.
3. `scripts/phase_ledger.py`: runs a binary N rounds (`--warmup`, `--rounds`, default 2/9) on a case from `serial.py`'s ROOTS with
   `TSRS_WORKERS=1 TSRS_CHECKERS=1`, parses the JSON lines, prints per-phase medians (instructions in G, wall, footprint) and the
   reconciliation against `/usr/bin/time -l`'s instruction total of a feature-off run of the same binary (the integrator supplies
   that number; the script accepts it as `--native-total`). Also a `--tsrs` mode that parses tsrs's `--extendedDiagnostics` phase
   rows for the same table shape (tsrs prints parse/bind/check times; instructions per phase are not available from tsrs — print
   wall only and say so in the header).
4. Tests: a unit test for the JSON parsing and reconciliation arithmetic; a smoke test that the feature-on CLI on a tiny project
   emits all six phases monotonically.

## Out of scope

Any change to what the compiler does; the 4-thread case (the method is invalid there; the script refuses `TSRS_CHECKERS>1`
unless `--wall-only`); PGO; the instrument in the same patch as any subject change.

## Acceptance (integrator)

Feature-off binary hash identical; feature-on smoke test; `phase_ledger.py` on hono in a quiet window reconciles within 0.3% of
the native total (the M1 gate was 0.2% on coarse); then the B1 window.
