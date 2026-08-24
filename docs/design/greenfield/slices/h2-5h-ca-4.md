# H2.5h / CA-4 — acceptance wiring: `run_h2_5h`, the divergence ratchet, hosted append, and the transition flips

Status: design-gate packet for the FINAL corpus-adoption rung.
CA-1 (#468) froze the 932-candidate band; CA-2b (#469) + CA-2a
(#470) swept the compiler/conformance execution families (185 → 79
census rows, every residual typed r1–r5); CA-3 (#471,
`dd0052ef`) observed the 82 project rows (932/932 observed,
admitted=888 / deferred=44). This packet wires the band into
acceptance and closes the slice.

## 1. Identity, purpose, and boundary

- **Slice ID / kind:** `h2-5h-ca-4`, kind `runtime` (crate
  changes: `run_h2_5h` + `load_project_emit` → the full h1-ladder
  cascade + a runtime-input closure bump for each new file).
- **Purpose:**
  1. `crates/harness` **`load_project_emit`** — the emit
     counterpart of `load_project_no_emit`, built on the CA-3
     §5.3a option floor (NO forced `noEmit`, no option
     rejections; Classic resolution, `lib.es5.d.ts` pin, CRLF,
     variant-over-config `module`, descriptor options applied);
  2. `crates/xtask` **`run_h2_5h`** — the ordered acceptance
     pipeline over all 932 artifact cases: admitted
     compiler/conformance rows execute through the 5g prepared
     routes (recorded-compiler-plan ×231 / qualified-vfs ×619),
     admitted project rows through `load_project_emit`, deferred
     rows are COUNT-ONLY (the exact `run_h2_5g` parity — the 5g
     pipeline never executes deferred dispositions); the CA-2b
     blocked-row contract (the exact emit-result-diagnostics
     compare under `noEmitOnError`) carries over from the census;
  3. the **divergence ratchet**: a frozen manifest
     (`ratchets/h2-5h-known-divergences.v1.json`) listing every
     admitted row whose production bytes are KNOWN to diverge,
     each entry joined to its typed residual owner (r1–r5 from
     CA-2a §8-A.6, or a project-lane owner discovered by the
     first `load_project_emit` sweep). Gate: diverging+listed =
     pass, diverging+unlisted = FAIL (new regression),
     exact+listed = FAIL (stale entry — the manifest only
     shrinks), exact+unlisted = pass. Baked totals assert
     932/888/44 plus the manifest size;
  4. **activity admission**: H2.5h joins the admitted runtime
     slices in the shared executor (the census env-skip and the
     pre-5h accounting retire); the 5h rows SKIP the whole
     per-slice expectation block (`:1221-1341` — the exhaustive
     `match accepted_slice` at `:1196` forces the new arm at
     compile time); CRITICALLY (review finding 5), every
     NON-5h slice's expectation compare gains an explicit
     `activity.runtime_slice(H2RuntimeSlice::H2_5h) == 0` row —
     the guard-list extension alone would silently delete the
     5g band's H2.5h==0 proof (an identity-transform regression
     can be byte-invisible);
  5. the local-gate **`h2-5h-oracle` phase** (mirroring
     `h2-5g-oracle`'s check-receipt fast path) and the
     **`fn acceptance` append** (the established 5a..5g chain
     pattern) with the same-commit
     `qualification-policy.v2.json` `rust_source_sha256` pin
     refresh (precedent `5a0275bc`);
  6. the **transition surface** (review finding 7 — the ORIGINAL
     flip design targeted frozen artifacts and is replaced): the
     `h2-transition` ladder/ROOT_SPECS rows are FROZEN H2.0a
     history (the schema has no "landed" state; no prior landing
     ever flipped a row — H2.1a still reads "next") and receive
     only the standard chain-walk `INPUT_HASHES` re-mint; the
     LIVE landing surface is the `transition` block of
     `ratchets/h2-5g-profile.v1.json`
     (`h2-5g-profile.mjs:608-632`: `next_slice`,
     `next_runtime_activation_slice`,
     `target_es2015_transform_owner`, the historical-profile
     fields) — this packet specifies its exact field changes for
     the H2.5h landing (the H2.5g→historical / H2.5h-live
     transition the handoff names), with the 5g-profile schema
     updated in the same commit (both in allowedPaths); plus the
     `h2-5h-a` handoff item-4 CA-4 LANDED marker + the
     slice-close statement; the es2018 ObjectRestSpread re-base
     decision recorded (see §12).
- **Non-goals:** fixing any r1–r5 residual (each keeps its named
  owner; the manifest is their ledger); the hosted
  engine/verifier/profile registry (untouched — the N+1/shadow
  protocol applies to that registry, not to the slice append);
  any approved-runner performance re-mint (`h2-baseline` is
  insensitive to this wiring: the H0/H1 baseline workloads light
  zero H2.5h activity in the current tree, battery-verified).
- **Trusted base:** the CA-3 merge `dd0052ef` (current `main`).
- **Activation state:** before — the 932-row artifact is
  evidence-only; after — hosted `cargo xtask acceptance` executes
  the band with the divergence ratchet on every candidate, the
  local gate carries the `h2-5h-oracle` freshness phase (the
  local gate never runs the band itself — §5.4), and the
  corpus-adoption slice is CLOSED.
- **Next owner:** the r1–r5 residual packets (each shrinks the
  manifest); the linear-`containerPos` design gate (r5).

## 2. Position in the ladder

The last rung of the CA-1 ladder. After this packet the H2.5h
corpus-adoption slice is complete: observation (CA-1/CA-3),
execution families (CA-2b/CA-2a), and acceptance (CA-4).

## 3. Required-reference table

| Reference | Role | State |
| --- | --- | --- |
| `ratchets/h2-5h-qualification.v1.json` (932/888/44) | the acceptance source of truth | read-only |
| `crates/xtask/src/h2_2c_acceptance.rs` `run_h2_5g` (:2230) + `execute_slice_observed_with_inputs` + `H2_5gExecutionInputs` | the clone base: pipeline, per-case execution, activity guard | edited (run_h2_5h + H2.5h admission) |
| `crates/harness/src/upstream_suites/execution/project.rs` (`load_project_no_emit`, `MountedProjectHost`) + `execution.rs` (`load_qualified_compiler_emit`) | the structure layer + the artifact-driven route pattern for `load_project_emit` | edited (the emit loader) |
| the CA-3 packet §5.3a (the option floor) + the artifact's `project_input`/`project_mount` | the project execution contract | read-only |
| CA-2a §8-A.6 (r1–r5 row lists) + the census v10 log | the manifest's initial compiler/conformance entries (79) | read-only evidence |
| `crates/compiler/tests/integration/h2_5h_ca2b_seam_contract.rs` + `h2_5h_ca2a_promote_contract.rs` | remain the focused replays; `run_h2_5h` is the band-wide machine gate they anticipated | read-only |
| `.github/ci/qualification-policy.v2.json` `hosted_acceptance.rust_source_sha256` | pins refreshed in the same reviewed commit as the `fn acceptance` append | edited |
| `crates/oracle/h2-transition.mjs` | FROZEN history — receives only the chain-walk `INPUT_HASHES` re-mint (finding 7: its ladder/ROOT_SPECS rows never flip on landings) | re-minted (hashes only) |
| `crates/oracle/h2-5g-profile.mjs` `transition` block (:608-632) + `.github/ci/contracts/h2-5g-profile.schema.json` + `ratchets/h2-5g-profile.v1.json` | THE live landing surface: the H2.5h transition field changes | edited + re-minted |
| `crates/emitter/src/activity.rs` (H2.5h slice) | already models the slice; the ADMISSION set in the executor changes, not this file | read-only |
| `docs/design/greenfield/slices/h2-5h-a.md` | handoff close + the hosted clause + the ObjectRestSpread decision | edited |

## 4. Pinned map

No new `_tsc.js` spans: the wiring consumes frozen observations.
The binding contracts are OURS: the artifact schema (932/888/44),
the CA-3 option floor, the 5g executor's exact-compare +
blocked-row semantics (both already reviewed), and the divergence
manifest schema introduced here:

```json
{ "schema": 1,
  "cases": [ { "case_id": "...",
               "owner": "h2-5h-ca-2a-r1..r5 | h2-5h-project-r<N>",
               "writes_diverging": N,
               "diagnostics_diverging": false,
               "emit_result_diverging": false } ],
  "manifest_fingerprint_sha256": "..." }
```

At least one facet must be non-trivial per entry. The pass rule
is FACET-EXACT (review finding 4): a listed row passes only when
its OBSERVED divergence facets equal the LISTED facets — a
write-diff listing does not absorb a later diagnostics
regression on the same row, and the observed `writes_diverging`
count is verified against the listed `N`.

Owners must be one of the NAMED residuals; a manifest entry
without a named owner is invalid. The manifest is a ratchet:
entries are removed when their rows go exact (the shrink is the
review surface, the `escapes --write-manifest` idiom), never
added except by a reviewed regression triage.

## 5. Design

1. **`load_project_emit`** (harness): constructs the mounted
   project host exactly as `load_project_no_emit` (shared tree,
   case-sensitive, `current_directory`), then applies the CA-3
   §5.3a floor instead of the H0 adapter's option layer: config
   parse (for the config arms) → descriptor options → variant
   `module`; `moduleResolution` Classic default;
   `noErrorTruncation=false`; `skipDefaultLibCheck=false`;
   default library `lib.es5.d.ts`; `newLine` CRLF; NO `noEmit`.
   Returns a `PreparedProgram` for `ProgramSession` like
   `load_compiler_emit`. Root selection: explicit rows pass EVERY
   `requested` root normalized against `current_directory` —
   missing roots included: the program loader owns the
   missing/unsupported-root diagnostic chain
   (`crates/program/src/loader.rs:515`), and the two ADMITTED
   `invalidRootFile` variants have all three roots missing with
   frozen 6053/6054/6231 (+2×5107) diagnostics and zero writes
   (review finding 3 — a present-only filter can never reproduce
   them); the two config arms (project-config ×4,
   discovered-config ×2) use the parsed config file list — all
   three arms asserted against the artifact record.
2. **`execute_h2_5h_case`**: suite dispatch. The clone base's
   shared comparison path raises OPAQUE errors and aborts at the
   first mismatched write (`assert_exact_writes`
   `h2_2c_acceptance.rs:349-415`; diagnostics `:726-774`;
   emit-result `:873-889`), so the 5h lane gets its OWN
   comparison function (review finding 1) — the shared `assert_*`
   helpers stay untouched for the other nine slices:
   - determinism and prepare/emit failures stay HARD errors
     (never divergence);
   - writes compare counts ALL diverging writes (no first-abort)
     into the typed record;
   - reported diagnostics and the emit result compare as typed
     outcomes; the clone base's `:877-880` requirement that BOTH
     emit-result diagnostic sets be EMPTY is replaced by the
     exact emit-result-diagnostics compare (the CA-2b blocked-row
     contract): the band has 3 admitted rows with non-empty
     emit-result diagnostics and 4 with `emit_skipped=true`;
   - `ordered_map` is generic in `R`
     (`bounded_pipeline.rs:66-73`): the per-case result is
     `Result<H2_5hCaseOutcome, String>` where the outcome carries
     the divergence facets.
   Compiler/conformance rows execute through the 5g-style
   prepared routes with the H2.5h-admitted activity list and NO
   typed-activity expectation compare (the artifact records
   none); project rows load via `load_project_emit` and run
   `emit_with_reported_diagnostics_for_harness` twice
   (repetitions=2). The 5h execution-inputs struct builds its own
   project-plan map (`H2_5gExecutionInputs::load` filters
   projects out and bakes `compiler_cases.len() == 7_276`,
   `:221`). Deferred rows: COUNT-ONLY, the exact `run_h2_5g`
   parity (`:1463-1470`, `:2241-2247` — the 5g pipeline never
   executes deferred dispositions; §1.2's earlier
   typed-failure phrasing is corrected to this).
3. **The ratchet gate**: after the pipeline, join the divergence
   records against the manifest per §4's four-outcome rule; the
   totals print `candidates=932 exact=<E> known_diverging=<K>
   deferred=44` and assert `E + K = 888` with `K =` the manifest
   size.
4. **Local ci phase `h2-5h-oracle`**: the qualification `--check`
   with the receipt fast path (mirror the `h2-5g-oracle` phase
   registration — registration-only in `main.rs`; the local gate
   NEVER runs `cargo xtask acceptance` or `run_h2_5g`, finding 8,
   so `run_h2_5h` executes on HOSTED via `fn acceptance` and in
   the manual commands only). `run_h2_5h` reuses
   `h2_5g_worker_count()`/`TSRS_H2_5G_WORKERS` (the workflows
   directory is a forbidden prefix — a fresh env var would
   silently run one worker hosted); the hosted budget (+932 rows
   ≈ +2-4 min inside the 45-minute `gates` timeout) is checked at
   the first hosted run. No 5h owner-controls registration in
   this packet (the 5a..5g pattern's owner controls defer to the
   residual packets — recorded here as the deviation).
5. **`fn acceptance` append**: `h2_2c_acceptance::run_h2_5h`
   after `run_h2_5g` (`main.rs:4380`); the policy pin refresh in
   the same commit — the touched pinned files are exactly
   `main.rs` + `h2_2c_acceptance.rs` (+ `bounded_pipeline.rs`
   only if edited). The divergence manifest gets NO
   `.github/ci/qualification.mjs` registry row (the contract
   table is a fixed list, not closed-world over `ratchets/`;
   the manifest validates solely inside `run_h2_5h` — finding
   6's recorded decision).
6. **Transitions**: the `h2-5g-profile.mjs` transition block's
   H2.5h landing fields (exact changes specified at
   implementation against the live block, schema updated in the
   same commit; a surprise beyond field-level edits STOPS);
   `h2-transition.mjs` receives only the chain-walk hash re-mint;
   handoff close markers.

## 6. Gap delta

Before: the band's production state is invisible to gates (the
census was a scratch instrument). After: every merge candidate
executes 932 rows with FP=0-style strictness (exact or
named-known-diverging), and the residual burn-down is a
first-class ratchet.

## 7. Implementation plan

1. `load_project_emit` + its focused harness test (one explicit
   row + one config row, frozen-observation replay).
2. `execute_h2_5h_case` + `run_h2_5h` + the manifest
   reader/joiner; unit tests for the four ratchet outcomes.
3. The first full `run_h2_5h` sweep mints the manifest's project
   entries (compiler/conformance entries seeded from census v10);
   the manifest is reviewed row-by-row against §8-A.6 owners.
4. ci phase + `fn acceptance` append + policy pins.
5. Transition re-mints; handoff close; §8-A notes.
6. The walk (full crate cascade + closure bumps for each new
   file) + gate + hosted; merge via PR.

## 8. Evidence and amendments

Packet §8-A records: the measured manifest size and per-owner
counts; any project-lane divergence family discovered by the
first sweep (named `h2-5h-project-r<N>` with evidence); every
implementation-time deviation per the stop rule.

### §8-A. Implementation-time notes (2026-08-24)

1. **Measured band state (the first full sweep):** 932 =
   exact 795 + known_diverging 93 + deferred 44. The 79
   compiler/conformance manifest entries join census v10
   EXACTLY (0 discrepancy — `run_h2_5h` reproduces the census
   instrument). Owners: r1 22, r2 4, r3 4, r4 27, r5 22,
   h2-5h-project-r1 8, h2-5h-project-r2 6.
2. **Project-lane families discovered (per §8's anticipation):**
   - **h2-5h-project-r1 (8 rows, `emit_refused`)**: the
     production emitter's fails-closed option preflight
     (`execute.rs` — the l0-source-options owner surface,
     FORBIDDEN to this packet) refuses `mapRoot`/`sourceRoot`/
     `rootDir`; the manifest gains the `emit_refused` facet for
     exactly this typed-refusal shape (a §4 schema amendment).
   - **h2-5h-project-r2 (6 rows, diagnostics)**:
     emitDecoratorMetadataSystemJS ×2 + nodeModulesImportHigher
     ×2 + nodeModulesMaxDepthIncreased ×2 — project-lane
     diagnostics parity (decorator-metadata and
     node_modules-depth resolution reporting).
3. **The H0 validation scope is not observation semantics:**
   `load_project_emit` does NOT run `validate_config_plan` —
   its watchOptions/typeAcquisition/compileOnSave refusals are
   no-emit-adapter behavior; the CA-3 oracle parsed those
   configs through the vendored parser, which tolerates them
   (the discovered-config row `emitDecoratorMetadataSystemJS`
   carries `compileOnSave`). The §4 layer split covers this.
4. The typed-refusal detection matches the emitter's
   `unsupported emit compiler option` message at the session
   boundary (the typed error surface lives in forbidden crates;
   the single well-known message is the envelope-compatible
   join, recorded here for the r1 packet to replace).
5. The transition block landed: active slices +H2.5h (inactive
   15 → 14), next = H2.6a (exact-map-json scope),
   `target_es2015/generators_transform_owner =
   complete-with-h2-5h-divergence-ratchet`, and the five
   h2_5h_* count fields (932/888/795/93/44) — mjs + schema
   const in the same commit.

6. **Walk discoveries (the closure + pin surfaces):** (a) the
   new loader test enters the 5g-profile runtime-input closure
   in NEW_RUNTIME_INPUTS (254 → 255) while the test REGISTRY
   (`crates/harness/tests/contracts.rs`) belongs in the
   inventory's EXCLUDED gate-infrastructure list — the two lists
   are distinct and the first walk mis-filed both; (b)
   `h2-5h-a-foundation.mjs` pinned the PRE-landing 5g-profile
   transition values — its parent-profile expectations now
   assert the landed block (H2.6a / +H2.5h / the ratchet owner
   strings); (c) `l0-source-options` carries a line-anchored
   `main.rs` pin that moved with the append (2-line re-mint);
   (d) review finding 6's "no `.github/ci/qualification.mjs`
   edit" was INCOMPLETE: the CA-1 hosted clause's actual
   mechanism is `HOSTED_ACCEPTANCE_QUALIFIED_CALLS` + the
   derived canonical `fn acceptance` body pin in that file —
   `run_h2_5h` appends there, with the test's synthetic
   entrypoint fixture updated in kind. All four surfaces are
   envelope-amended in this train.

## 9. Acceptance

1. `run_h2_5h` green locally with the printed totals matching the
   baked asserts; 2. the manifest validates (named owners only,
   fingerprint fresh); 3. `cargo xtask acceptance` (hosted
   entrypoint) green locally; 4. full local gate + hosted green at
   the final head; 5. the transition artifacts re-minted and their
   checkers green; 6. the handoff close markers landed; merge
   commit via PR.

## 10. Traceability

CA-1 §ladder (this packet closes it); CA-2a §8-A.6 (the residual
owners the manifest joins); CA-3 §5.3a (the option floor
`load_project_emit` implements); the h2-5h-a hosted clause (the
append + pin protocol); the symbol-diff-known.txt allowlist
precedent (the ratchet's shape); the escapes manifest idiom (the
shrink-only review surface).

## 11. Prohibitions

No un-owned manifest entries; no ratchet loosening (a new
divergence is a gate failure, never a manifest append without
reviewed triage); no engine/verifier/profile registry changes; no
approved-runner performance claims; no edits to the frozen
observations.

## 12. Unresolved items

- The project-lane divergence count (measured by the first sweep;
  §8-A records it; families named there).
- The es2018 ObjectRestSpread re-base decision: RECORD in the
  handoff whether the es2018 helper base stays as-landed (B-5)
  or re-bases on the 5h evidence — the decision text lands in
  §8-A + h2-5h-a.md (no code either way in this packet).
- (superseded by the amended §1.6/§5.6: the live surface is the
  `h2-5g-profile` transition block with its own field-level stop
  rule; the frozen `h2-profile-transition` artifact is untouched
  beyond the standard chain-walk hash re-mint.)

## 13. Citation status

`run_h2_5g`/`fn acceptance`/the policy pins/`load_project_no_emit`
read in-tree at the trusted base; the artifact totals (932/888/44)
read from the live CA-3 artifact; the census v10 79-row list
preserved in the session evidence; the baseline-insensitivity
claim battery-verified at the CA-3 head.
