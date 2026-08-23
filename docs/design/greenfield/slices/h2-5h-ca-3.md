# H2.5h / CA-3 — the project-suite observation harness: 82 deferred rows observe, the CA-1 deferral retires

Status: design-gate packet for the fourth H2.5h corpus-adoption
packet. CA-1 (`a19bc3d7`, #468) froze the 932-candidate ES5 band
with the 82 project-suite rows carried as a typed deferral block
(`project_deferral`, owner `h2-5h-ca-3`: 41 descriptors × 2 module
variants). CA-2b (#469) and CA-2a (#470, `2ed465c1`) closed the
compiler/conformance execution families. This packet observes the
82 project rows under the same hermetic double-observation
discipline and retires the deferral: the artifact re-mints with
OBSERVED 850 → 932.

## 1. Identity, purpose, and boundary

- **Slice ID / kind:** `h2-5h-ca-3`, kind `evidence` (mjs-only:
  no crate byte changes, no h1-ladder re-mint — the CI-infra
  mjs-only discipline).
- **Purpose:** extend `crates/oracle/h2-5h-qualification.mjs` with
  a project-suite observation lane: per row, construct the hermetic
  project VFS (the SHARED whole `tests/cases/projects` tree mounted
  case-sensitively under `/.src/tests/cases/projects/...`, §5.2),
  build the in-process vendored
  `ts.createProgram` with the descriptor's roots and the module
  variant, observe writes + reported diagnostics TWICE
  (`observeTypeScript`, run-fingerprint equality), classify with
  the same `analyzeCase` feature/depth/option dispositions, and
  record the rows as OBSERVED candidates. The `project_deferral`
  block retires (0 rows / removed); `OBSERVED_CANDIDATE_CASES`
  850 → 932, `PROJECT_DEFERRED_CASES` 82 → 0.
- **Non-goals:** production project execution (the Rust
  `load_project_emit` counterpart and the census/acceptance
  comparison of project rows are CA-4's `run_h2_5h` concern —
  exactly as compiler-row production comparison lives in the
  census/acceptance, never in the mint); any change to the 850
  compiler/conformance observations (projection-excluded inputs:
  the adoption rebind must reuse all 850); any crate edit.
- **Trusted base:** the CA-2a merge `2ed465c1` (current `main`).
- **Activation state:** before — `project_deferral` carries 82
  unobserved rows; after — 932 observed rows with dispositions,
  deferral retired, every downstream checker (`.github/ci/
  qualification.mjs` deep-equal row, the 5h schema, the walk
  battery) green over the new shape.
- **Next owner:** CA-4 (acceptance wiring; consumes the 932-row
  artifact and the CA-1 §hosted clause).

## 2. Position in the ladder

CA-1 §2 ratified the ladder CA-1 → CA-2(b,a) → CA-3 → CA-4. This
packet is the CA-3 rung. After it, the corpus-adoption band's
OBSERVATION surface is complete; CA-4 wires `run_h2_5h` acceptance
over all 932 rows and executes the H2.5g → historical profile
transition.

## 3. Required-reference table

| Reference | Role | State |
| --- | --- | --- |
| `ratchets/h2-5h-qualification.v1.json` | the artifact being re-minted (850 observed + 82-row deferral) | REWRITTEN (932 observed, deferral retired) |
| `crates/oracle/h2-5h-qualification.mjs` | the mint — grows the project lane | edited |
| `ratchets/h2-candidate-dispositions.v1.json` | the 2,012-row band selection incl. the 82 project candidate rows (suite === "project") | read-only input |
| `ts-tests/tests/cases/project/*.json` (41 descriptors) + `ts-tests/tests/cases/projects/**` (mount trees) | fixture sources, identity-pinned per row | read-only |
| `crates/harness/src/upstream_suites/execution.rs` `build_project_fixture` (+ `execution/project.rs` `load_project_no_emit`) | the record's STRUCTURE layer (T0=100% gated): `current_directory = /.src/<projectRoot>`, the THREE root-selection arms, the shared whole-tree mount, per-variant `module` override; the option handling is the H0 no-emit adapter, NOT the record (§4) | read-only reference |
| `.github/ci/qualification.mjs` + `.github/ci/qualification.test.mjs` | CI checker — maps label/schema/artifact PATHS only; **needs NO edit** (§7.2) | read-only |
| `.github/ci/contracts/h2-5h-qualification.schema.json` | the load-bearing 5h schema (read at mint time via `CONTRACT_RELATIVE_PATH`): count pins 850/82 → 932/0 + the §7.2 shape churn | edited |
| `crates/oracle/h2-5h-qualification.v1` walk step (`ca2a-walk.sh` §6.5 equivalent) | the re-mint is THE walk step for this train (mjs-only: no ladder) | executed |
| CA-1 packet §5.4 (projection-exclusion) + §project_deferral contract | authority for the deferral retirement and the 850-reuse guarantee | read-only |
| `vendor/typescript-6.0.3/project-profile-classification.v1.json` | **the designated per-row plan (CA-1 ladder)**: 632 rows carrying `current_directory`, `root_selection` ({state: explicit-inputs/project-config/discovered-config, roots incl. per-root `presence`}), `module_variant` (+`baseline_folder`), `effective_profile` with per-option origins — the observation lane ASSERTS its constructed inputs against the 82 relevant rows instead of re-deriving them from Rust source | read-only plan |

## 4. Pinned upstream map

The oracle for project rows is the vendored compiler API itself
(the same in-process `ts.createProgram` + `program.emit` capture
as the 850): no `_tsc.js` span beyond those already pinned by the
5g/5h mints applies. The DESCRIPTOR semantics are pinned in
LAYERS (review finding 1 — the record is not monolithic):

- **Structure layer (semantics of record = Rust, T0-gated):**
  `build_project_fixture` (`execution.rs:2381-2432`) for
  descriptor parsing, `current_directory = /.src/<projectRoot>`,
  and the THREE root-selection arms — explicit-inputs (76 rows),
  project-config (4), discovered-config (2) — plus the SHARED
  whole-tree mount (`execution.rs:1198-1222`: every
  `tests/cases/projects/**` file under
  `/.src/tests/cases/projects/...`, case-sensitive, one mount for
  all variants). Divergence from this layer is a STOP.
- **Option layer (the observation lane's OWN projection — the H0
  adapter is NOT the record):** `load_project_no_emit`'s option
  handling (`project.rs:317-369`) is an H0 no-emit adapter: it
  forces `noEmit=true` and hard-errors on `mapRoot`/`sourceRoot`/
  `outDir`/`outFile`/`sourceMap`/`rootDir` — while the band
  CONTAINS descriptors carrying `mapRoot`/`sourceRoot`/`rootDir`
  (8 rows) and a write-observing mint cannot inherit `noEmit`.
  The observation lane therefore defines its own projection
  (§5.3a), keeping the adapter's VERIFIED floor where it is not
  no-emit-specific (Classic module resolution,
  `noErrorTruncation=false`, `skipDefaultLibCheck=false`,
  `lib.es5.d.ts` default-library pin, variant-over-config
  `module` precedence per `apply_project_runner_existing_options`
  `project.rs:317-341`) and asserting the RESULT against the
  classification artifact's `effective_profile` (with origins)
  per row.

## 5. Design

1. **Row source (the closure rule, review finding 9):** the
   dispositions artifact carries 632 project rows (316 descriptors
   × 2); the 82 CA-3 rows are the subset whose `required_slices`
   close through H2.5g ∪ {H2.5h} — the SAME closure filter the
   mint already applies to build the candidate band. The mint's
   existing project partition (`projectRows`) is the selection;
   `selection_origin` is the mint-assigned label as today.
2. **Fixture load (`loadProjectFixture`)**: read the descriptor
   json (identity triple as the deferral rows pin); parse the
   structure per the record's structure layer; the MOUNT is the
   SHARED whole `tests/cases/projects` tree (review finding 3 —
   one case-sensitive mount serving all rows, mirroring
   `execution.rs:1198-1222`), identity-recorded ONCE as a
   mount-inventory block (path/bytes/sha256/git_blob_sha1 per
   file, sorted) rather than per row; per-row `project_input`
   references the mount by its inventory fingerprint. Missing
   roots are representable (`invalidRootFile.json`'s three roots
   are missing BY DESIGN and its projectRoot directory does not
   exist — the plan artifact records `presence: "missing"`).
3. **Program construction (all THREE arms, review finding 4):**
   - explicit-inputs (76 rows): roots = `inputFiles` normalized
     against the current directory;
   - project-config (4 rows): the descriptor's `project` config
     parsed with a NEW case-SENSITIVE project parse host (the
     compiler-lane `createParseConfigHost` is case-insensitive
     and `parseConfigContext` couples to recorded compiler plans
     — neither is reused); config-parse diagnostics thread into
     the observation;
   - discovered-config (2 rows): tsconfig discovered at the
     current directory (`ProjectRootSelection::DiscoverConfig`,
     `execution.rs:2489-2491` / `project.rs:141-173`).
   Config options merge FIRST; the runner floor and the module
   variant OVERWRITE them (variant-over-config precedence,
   `project.rs:123-176` — proven by
   `emitDecoratorMetadataSystemJS`'s tsconfig `module: system`
   yielding AMD/CommonJS with `origin: "runner-default"`).
3a. **The option floor (review findings 1+5 — the observation
   lane's own projection, asserted per row against the plan
   artifact's `effective_profile` origins):**
   `moduleResolution = Classic`, `noErrorTruncation = false`,
   `skipDefaultLibCheck = false`, default library pinned to
   `lib.es5.d.ts` regardless of target (the mjs compiler host
   derives the default library from `options.target` — the
   project host must override it), `module` = the variant (amd=2
   / commonjs=1), `newLine = CarriageReturnLineFeed` (NAMED here: the plan
   artifact records no newLine, and unset would inherit the
   platform's `ts.sys.newLine`; the compiler lane pins CRLF in
   `effectiveCompilerOptions` for exactly this hermeticity — the
   project lane pins the same, and the pinned value becomes
   CA-4's Rust emit target), NO `noEmit` (the H0
   adapter's `noEmit=true` and its option REJECTIONS are
   no-emit-adapter behavior, not observation semantics: the band
   carries `mapRoot`/`sourceRoot`/`rootDir` on 8 rows and
   `declaration: false` on the two config descriptors — all
   apply as ordinary options).
4. **Observation:** `observeTypeScript` unchanged — two fresh
   program+emit captures, write records (path normalized under the
   virtual root, callback sha256/bytes, BOM, source_files),
   reported diagnostics, emit result, run-fingerprint equality
   across the two runs.
5. **Disposition (review finding 6):** `analyzeCase`'s
   feature/depth/option classification applies, with a PROJECT
   WRAPPER: (a) the every-root-reached assert relaxes to "every
   PRESENT root reached" — missing roots (per the plan artifact's
   `presence`) are recorded, not fatal (`invalidRootFile` is the
   witness); (b) the per-case `input` builder is compiler-lane
   code — project rows build `project_input` instead; the
   PER-FILE analysis rows (name, feature roots, AST depth —
   WITHOUT base64 content, which lives in the mount inventory)
   are kept under `project_input.analyzed_files` so the
   disposition engine, the `no_emit_control_cases` rollup, and
   the schema's per-suite arm agree; every summary rollup
   touching `entry.input`/`entry.files` gains the project arm.
   No production comparison at mint.
6. **Artifact shape:** project rows join `cases` with
   `suite: "project"`, `execution_route` gains a project value,
   the descriptor identity as `source`, `project_input` =
   {descriptor fields, current_directory, root_selection (arm +
   roots + presence), module_variant, mount_fingerprint} with the
   shared mount inventory recorded once at the artifact level;
   `selection_contract.observed_candidate_denominator` 932;
   `project_deferred_candidates` 0 — DERIVED today from
   `projectRows.length` (the dispositions artifact still carries
   82 project candidates), so the partition rewrite re-derives it
   from the deferral state, not the row count (review finding 8);
   the `project_deferral` block REMOVED with its asserts; the
   `candidate_definition`/`admission` PROSE updates (see 7).
7. **Reuse — the adoption rebind (review finding 2, BLOCKER):**
   `admissionContract()` hardcodes the deferral sentence and
   flows into `execution_contract`, which is part of the
   whole-artifact reuse key (`reusableStoredCases`: stored vs
   current `canonical(execution_contract)` equality). Retiring
   the deferral rewrites that text and would void all 850
   reuses. The rebind: `reusableStoredCases` compares the
   execution contract through a PROJECTION that excludes the
   admission prose (the projection covers exactly the fields
   whose semantics the CA-1 §5.4 exclusion already covers —
   selection/deferral bookkeeping), while the per-case
   observation keys stay byte-compared as today; additionally NO
   NEW GLOBAL KEY enters `inputsRecord` (the mount identity is
   artifact-level data, NOT an observation input — per-case
   identity only), preserving the `observationInputs` equality.
   The projection excludes ONLY the `admission` prose; every
   other execution-contract field stays byte-compared — a drift
   there still voids reuse by design. Precedent: the B-5 "5g
   write adoption rebind". Expected mint log:
   `reused_observations=850`, `recorded=82`.

## 6. Gap delta

Before: 82/932 candidates unobserved (typed deferral). After: 0
unobserved; the corpus-adoption observation surface is complete.
No production-band delta (mjs-only; census unaffected).

## 7. Implementation plan and file surface

1. `crates/oracle/h2-5h-qualification.mjs`: consts
   (`OBSERVED_CANDIDATE_CASES` 932, `PROJECT_DEFERRED_CASES` 0),
   `loadProjectFixture` (shared-mount inventory + descriptor
   parse), the three root arms + the case-sensitive project
   config-parse host, the §5.3a option floor, the project arm in
   the suite build + `analyzeCase` wrapper + `project_input`
   serializer, the deferral-block retirement, the partition
   asserts and BOTH `EXPECTED_TARGET_STATES`/`EXPECTED_MODULE_
   STATES` computations (preflight optionStates AND
   summary.target_states — project rows enter both or the shared
   consts contradict), the `selectionOrigins` size assert, the
   shard-ordinal denominator, the `execution_route` ternary, the
   `selection_contract` derivations, the `admissionContract`
   prose + the §5.7 reuse-key projection.
2. `.github/ci/contracts/h2-5h-qualification.schema.json` — the
   REAL CI churn (review finding 8: `.github/ci/qualification.mjs`
   and its test map paths only and need NO edit): suite enum +
   "project", the case `required` list per-suite (projects have
   no `expansion_case`/`input`/`files` — per-suite `allOf` route
   arms), top-level `required` minus `project_deferral`, the
   const-pinned tables (target_states/module_states/dispositions/
   first_deferred_slices) and count pins: observed 932,
   `typescript_runs` 1_700 → 1_864, `deterministic` 932, writes/
   diagnostics/no_emit_control totals re-pinned from the mint.
3. Walk: the 5h re-mint IS the train's cascade — review finding
   10 CONFIRMS the artifact-only downstream set: the schema JSON
   (same commit), the readiness envelope/bootstrap, the README
   row; NO h1 ladder, NO 5h-a/5g/transition/baseline re-mints.
4. Focused verification: the per-row plan-artifact asserts
   (current_directory, root arm + presence, module variant,
   effective-profile origins) for all 82 — asserting ONLY the
   profile's RECORDED fields (`target`/`module`/
   `use_define_for_class_fields`/`no_emit` with origins): the
   plan's `rejected_when_effective` entries (`allowJs` on the 4
   project-config rows, `experimentalDecorators` on the 2
   discovered-config rows) are H2-profile bookkeeping — in the
   observation lane those options APPLY, so a naive
   options==profile equality would silently break 6 rows; variant coverage 41×2;
   one hand-verified descriptor probe (baseline.json amd: the
   observation's writes cross-checked against a fresh
   vendored-CLI-equivalent run of the same VFS).

## 8. Evidence, ratchet, and documentation amendments

1. This packet doc; 2. envelope `h2-5h-ca-3` (ready; predecessors
   = [h2-5h-ca-2a receipt]) + bootstrap allowedPacketIds + README
   row; 3. the re-minted artifact + updated checkers; 4. the
   `h2-5h-a` handoff item-4 CA-3 LANDED marker. Implementation-time
   discoveries append §8-A per the stop rule.

## 9. Acceptance

1. Mint green with `reused_observations=850 recorded=82`; both
   fingerprint runs equal per row; 2. `--check` green twice
   (receipt reuse on the second); 3. `.github/ci/qualification.mjs
   check` + `slice-readiness --check` green; 4. the walk battery
   green (no h1 re-mint — asserted by `git diff --name-only` being
   mjs/json/md-only); 5. full local gate + hosted at the final
   head; merge commit via PR.

## 10. Traceability

CA-1 §5.4/§deferral (the retirement contract), CA-2b §9.4 (the
census discipline CA-4 will reuse over the 932), the
`build_project_fixture` rules (the semantics of record), the
tsrs2 mjs-only walk lesson (no ladder for CI-infra-only trains).

## 11. Prohibitions

No crate edits; no change to any of the 850 stored observations
(byte-adopted); no production execution at mint; no schema
loosening (counts pin exactly 932/0/41×2).

## 12. Unresolved items

- (CLOSED at review) The root-arm split is 76 explicit / 4
  project-config / 2 discovered-config (the plan artifact's
  `root_selection.state`); `parseConfigContext` is NOT reusable
  (recorded-plan coupling + case-insensitivity) — §5.3 defines
  the project parse host.
- (CLOSED at review) Descriptor option properties in the band:
  `mapRoot`/`sourceRoot`/`rootDir` (8 rows) and
  `declaration: false` (2 config descriptors) — §5.3a applies
  them as ordinary options.
- The writes/diagnostics/typescript_runs schema re-pins are read
  off the first green mint (mechanical; a surprise beyond
  re-pinning STOPS). `newLine` is NOT such an item — it is the
  §5.3a-named CRLF input option.

## 13. Citation status

`build_project_fixture` + `load_project_no_emit` read in-tree at
the trusted base; the deferral block shape read from the live
artifact (41×2 verified); the CA-1 §5.4 projection-exclusion
re-verified by the CA-2a walks (reused 850/850 twice).
