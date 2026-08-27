# H2.6b ca-2 — acceptance wiring + transition landing

Status: design-gate pass; rides the h2/6b-ca closure train (one full
gate at the train's final head). Closing this rung closes H2.6b.

## 1. Identity, purpose, and boundary

`h2-6b-ca-2`, kind `runtime`, rung 4 (final) of the h2-6b.md §8
ladder: `run_h2_6b` joins the fixed hosted acceptance boundary, the
local CI gains the `h2-6b-oracle` freshness phase, and the H2.6b
transition lands in the pin lattice in ONE commit — acceptance wiring
and the first production sweep run FIRST, the transition set lands
LAST in the same train (the 2026-08-27 roadmap-review ordering).

## 2. `run_h2_6b` — the run_h2_6a clone surface

Clone base `crates/xtask/src/h2_2c_acceptance.rs::run_h2_6a` plus its
private support surface (validation, execution inputs, per-case
execution, manifest loader, and the shared `h2_slice_ratchet_join`
with a 6b label). The 6b deltas, and NOTHING else:

1. **Inputs**: `ratchets/h2-6b-qualification.v1.json` (validated
   against phase `H2.6b-inline-and-roots`, 6 cases); manifest path
   `ratchets/h2-6b-known-divergences.v1.json`; write-env
   `TSRS_H2_6B_WRITE_DIVERGENCES`.
2. **Option floor (THE amended requirement, h2-6b.md §8.4):**
   `EmitOptionFloor` gains a `MapFamily` variant that passes ALL FIVE
   map-family options through the projection —
   `sourceMap`/`inlineSourceMap`/`inlineSources`/`sourceRoot`/
   `mapRoot` per row settings; ONLY the 6b prepare uses it. The 6a
   `SourceMap` floor and the Established floor stay byte-identical
   (the 6a-ca-2 §4-A `apply_compiler_setting` precedent: a silently
   dropped option manufactures fake divergence — the first-sweep
   review must check for uniform facet patterns before adopting ANY
   manifest).
3. **Per-case comparison** (facet model unchanged): two deterministic
   production emits vs the frozen bytes — writes (paths, bytes, BOM,
   ORDER; inline units have NO `.js.map` write), callback `data`
   (`sourceMapUrlPos` for both lanes), `emitResult.sourceMaps` incl.
   inline data-URI payload agreement, `emittedFiles`, diagnostics
   (the conflict corners carry TS5051/5053/5069 sets), `emit_refused`
   as a facet.
4. **Deferred COUNT-ONLY rule** unchanged; totals assert against the
   ca-1 artifact's admitted/deferred split.
5. **Divergence manifest**: facet-exact, shrink-only, named-owner —
   created ONLY if the first sweep proves divergence. Expectation from
   the m-2 emit gate (26 parity + conflict corners first-run green):
   plausibly EMPTY; absence-required loader semantics when empty.

## 3. Wiring (same-commit set)

1. `fn acceptance`: append `run_h2_6b` after `run_h2_6a` — the fixed
   unsplit hosted boundary grows by exactly one call.
2. A `h2-6b-acceptance` subcommand mirroring `h2-6a-acceptance`.
3. Local CI: a `h2-6b-oracle` resume phase
   (`InputScope::NodeRuntimeOracle`) cloned from
   `ci_h2_6a_oracle_gates`: `node --check` + `--check` on the 6b
   qualification generator (steady-state cost = the receipt hit).
4. Policy pin refresh in the same commit (whatever `ledger check` +
   the acceptance-plan tests name as stale when `fn acceptance`
   changes — the CA-4 precedent).

## 4. The transition landing (ONE commit, LAST in the train)

`crates/oracle/h2-5g-profile.mjs` transition block +
`.github/ci/contracts/h2-5g-profile.schema.json` consts +
`crates/oracle/h2-5h-a-foundation.mjs` parent-profile pins:

- `active_runtime_slices` += `"H2.6b"`;
  `inactive_runtime_slice_count` 13 → 12; completed count += 1.
- New `h2_6b_*` adoption fields (candidate/admitted/exact/
  known_divergences/deferred — values from the first green
  `run_h2_6b` sweep), mirroring the `h2_6a_*` block.
- `next_slice`/`next_slice_scope`/`next_runtime_activation_slice`: per
  the schedule review at landing — expected `H2.6c`; the ratified
  gt6 + darwin-RSS infrastructure train lands BETWEEN this close and
  H2.6c, outside this lattice (new-ci-evidence-dag.md sequencing
  amendment).
- Schema consts updated to every changed value; runtime-admission
  counts re-measured by the generator, never hand-edited.
- `crates/oracle/h2-transition.mjs`: chain-walk hash re-mint only.

## 5. Close markers

Handoff close markers here and in the slices index README (H2.6b:
closed, evidence = the ca-1 artifact + the first green `run_h2_6b`
totals + the train's final-head gate record). No STAGE change (mid-H2
slice).

## 6. Prohibitions

No band re-observation inside `run_h2_6b`; no manifest without a
proven diverging first sweep; no hand-derived schema consts; the
hosted boundary stays fixed and unsplit; the `MapFamily` floor is
6b-lane-only — every other entry point keeps its existing floor
byte-for-byte.

## 7. Acceptance

`run_h2_6b` green locally and on hosted via `cargo xtask acceptance`;
`h2-6b-oracle` phase green; adjacent bands re-verified at the head
(`run_h2_5h` 932 = 796+92+44, `run_h2_6a` 177 = 130+45+2); transition
landing walk-converged; the train's final-head full gate + hosted
`gates` close H2.6b.
