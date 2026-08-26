# H2.6a ca-2 — acceptance wiring + transition landing

Status: design-gate pass, implementation deferred to the train (this
rung's Rust/xtask and transition edits collide with the train's walk;
they land on `h2/6a-m2` after m-3 closes, with this packet as the
authority). Authored on `h2/6a-ca-prep` under the 2026-08-25
thick-train directive.

## 1. Identity, purpose, and boundary

`h2-6a-ca-2`, kind `runtime`, rung 5 (final) of the h2-6a.md §8
ladder: `run_h2_6a` joins the fixed hosted acceptance boundary, the
local CI gains the `h2-6a-oracle` freshness phase, and the H2.6a
transition lands in the pin lattice in ONE commit. Closing this rung
closes H2.6a.

## 2. `run_h2_6a` — the run_h2_5h clone surface (pinned at authoring)

Clone base `crates/xtask/src/h2_2c_acceptance.rs::run_h2_5h`
(:2674-2718 at authoring) plus its private support surface
(`validate_h2_5h_qualification` :2285, `H2_5hExecutionInputs` :2315,
`execute_h2_5h_case` :2398, `load_h2_5h_divergence_manifest` :2549,
`h2_5h_ratchet_join` :2622). The 6a deltas, and NOTHING else:

1. **Inputs**: `ratchets/h2-6a-qualification.v1.json` (validated
   against phase `H2.6a-source-map`, 177 cases); manifest path
   `ratchets/h2-6a-known-divergences.v1.json`; write-env
   `TSRS_H2_6A_WRITE_DIVERGENCES`.
2. **Worker env**: the same `h2_5g_worker_count()` pool (the shared
   worker-ceiling policy), `min`-ed against 177.
3. **Execution routes**: `recorded-compiler-plan` and `qualified-vfs`
   only — the qualification artifact carries no `project-mount` row
   (ca-1 census guard); the route match arm for `project-mount` is a
   typed error, not a silent skip.
4. **Per-case comparison** (the `execute_h2_5h_case` facet model,
   unchanged): two deterministic runs of the production
   `ProgramSession` emit against a memory sink; facets
   `writes_diverging` (paths, bytes, BOM, ORDER — the frozen writes
   now include `.js.map` bytes and the callback `data` argument),
   `diagnostics_diverging`, `emit_result_diverging` (now including
   `sourceMaps` entries and `emittedFiles`), `emit_refused` (an
   `unsupported emit compiler option` refusal recorded as a facet, not
   a crash). The compact-observation projection extends to the ca-1
   artifact's `data_*` and `source_maps` fields — the ONLY
   compare-side code delta.
5. **Deferred COUNT-ONLY rule** (unchanged): the 2 `deferred-to-slices`
   rows (`unicodeEscapesInNames02.ts` ×2, first owner H2.9) count as
   deferred without execution; totals assert
   `exact + known_diverging == 175 && deferred == 2`.
6. **Divergence manifest**: facet-exact, shrink-only, every entry
   named-owner — created ONLY if the first full sweep proves diverging
   rows. Expectation from the m-3 evidence: the 36-case witness floor
   is byte-exact through the production path, so the manifest is
   plausibly EMPTY; if so, no manifest file is created and the
   manifest-load arm requires absence (the 5h loader semantics).
   MetaProperty rows CANNOT appear (ca-1 measured zero band inputs
   with `new.target`/`import.meta`), and no `module: System` row
   exists, so the m-3 system-splice refusal cannot surface here
   either.

## 3. Wiring (same-commit set)

1. `fn acceptance` (main.rs :4360-4383 at authoring): append
   `h2_2c_acceptance::run_h2_6a(&workspace)` after `run_h2_5h` — the
   fixed unsplit hosted boundary grows by exactly one call.
2. A `h2-6a-acceptance` subcommand mirroring `h2-5h-acceptance`
   (:4490) for local single-band runs.
3. Local CI: a `h2-6a-oracle` resume phase
   (`InputScope::NodeRuntimeOracle`) cloned from `ci_h2_5h_oracle_gates`
   (:8737): `node --check crates/oracle/h2-6a-qualification.mjs` +
   `node crates/oracle/h2-6a-qualification.mjs --check` (steady-state
   cost = the gate-tax-3 receipt hit).
4. Policy pin refresh in the same commit (the acceptance-plan/policy
   pins that hash the acceptance entrypoint surface — the CA-4
   precedent: whatever `cargo xtask ledger check` + the acceptance-plan
   tests name as stale when `fn acceptance` changes).

## 4. The transition landing (ONE commit, the CA-4 pattern)

`crates/oracle/h2-5g-profile.mjs` transition block (:626-662 at
authoring) + `.github/ci/contracts/h2-5g-profile.schema.json` consts +
`crates/oracle/h2-5h-a-foundation.mjs` parent-profile pins, all in one
commit:

- `active_runtime_slices` += `"H2.6a"`;
  `inactive_runtime_slice_count` 14 → 13;
  `completed_runtime_slices` 22 → 23.
- `completed_slice` stays a 5g identity field (the artifact is the 5g
  profile); the H2.6a adoption lands as new
  `h2_6a_candidate_cases: 177`, `h2_6a_admitted_cases: 175`,
  `h2_6a_exact_cases`, `h2_6a_known_divergences`,
  `h2_6a_source_deferred_cases: 2` fields (values from the first
  green `run_h2_6a` sweep), mirroring the `h2_5h_*` block.
- `next_slice`/`next_slice_scope`/`next_runtime_activation_slice`: per
  the schedule review at landing time (post-h1-completion-slices.md
  row after H2.6a — expected `H2.6b` unless the review at landing
  says otherwise; gate-tax 4 + the subset train gate land BETWEEN
  ca-2 and the next slice per the 2026-08-25 directive, outside this
  lattice).
- Schema consts updated to every changed value; the generator's
  runtime-admission counts (`runtime_admissions`) re-measured by the
  generator itself, never hand-edited.
- `crates/oracle/h2-transition.mjs`: chain-walk hash re-mint ONLY (no
  disposition content change — the 15,642-row census is untouched by
  H2.6a's landing; its H2.6a rows stay listed with their own
  dispositions).

## 4-A. Implementation-time amendments (2026-08-25)

1. **The settings floor was the missing delta.** The shared
   recorded-plan/qualified-VFS projection
   (`crates/harness/src/upstream_suites/execution.rs`
   `apply_compiler_setting`) silently DROPS `sourcemap` — correct for
   the frozen 5g/5h bands (mapless on both sides: the 15 sourceMap-
   carrying 5h rows are exact precisely because both lanes dropped it)
   but fatal for H2.6a (the ca-1 oracle observed WITH maps). The first
   sweep proved it as 175 uniform write/emit-result facets — a
   compare-infrastructure artifact, not divergence; that manifest was
   discarded, not adopted. Fix: `EmitOptionFloor
   { Established, SourceMap }` threaded through the projection chain
   with floor-suffixed public variants; existing entry points keep the
   Established floor byte-for-byte, and ONLY the 6a lane's prepare
   passes `SourceMap`. Every other map-family option stays dropped on
   both floors until its owning slice admits it.
2. **The four-outcome join is shared with a slice label**
   (`h2_slice_ratchet_join`); the 5h wrapper delegates with its own
   label, so 6a failures name H2.6a instead of H2.5h. Otherwise the
   join is byte-identical to the CA-4 form.

## 5. Close markers

The handoff close markers land here and in the slices index README row
(H2.6a: closed, evidence = the ca-1 artifact + the first green
`run_h2_6a` totals + the train's final-head gate record), and the
`STAGE`-adjacent records follow the standing rule: `ratchet.toml` /
`STAGE` only if the schedule marks H2.6a as a milestone boundary
(expected: no STAGE change; H2.6a is a mid-H2 slice).

## 6. Prohibitions

No band re-observation inside run_h2_6a (the ca-1 artifact is the sole
oracle authority); no manifest creation without a proven diverging
first sweep; no schema consts hand-derived (every count from the
generator's own output); the hosted boundary stays fixed and unsplit
(`cargo xtask acceptance` gains the call, no selectors).

## 7. Acceptance

`run_h2_6a` green locally (175 exact + 0-or-manifest known_diverging +
2 deferred) and on hosted via `cargo xtask acceptance`; `h2-6a-oracle`
phase green; transition landing walk-converged; the train's final-head
full gate + hosted `gates` close H2.6a.
