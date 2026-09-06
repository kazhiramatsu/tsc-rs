# h2-7b-w5 — experiment register

Packet: [h2-7b-w5.md](h2-7b-w5.md), revision 3. Trusted base:
`main @3f90839e`. Train / PR: `h2/7b-w5`, #509.

## Result and validation

The final candidate removes all 18 H2.7b divergence rows. Every removal has a
passing control comparing the complete frozen TypeScript observation, including
diagnostics, writes and their metadata, emit result, source maps and exit code.
The two initially open diagnostic cases also match their complete observations.
Full-corpus regression validation runs once in unchanged hosted
`cargo xtask acceptance`; its final-head result is recorded in the PR body.
The duplicate local H2.7b sweep was deliberately terminated before completion,
and is not a passing result.

The following results describe the first 16-case candidate (`6c21f859`). Final
checker and compiler/conformance checks are refreshed after adding D2/D5; their
completed results are recorded below and in the final PR body.

| first candidate local check | result | execution time |
|---|---|---|
| compiler focused + CLI / emit-session / filesystem / H1 I/O contracts | 85 passed, 10 existing Node-oracle audits ignored | 35.83 s |
| checker library | 1,705 passed | 1.14 s |
| emitter library | 460 passed | 0.28 s |
| types library | 30 passed | 0.00 s |
| source-comment topology contracts | 18 passed | 0.00 s |
| diagnostic conformance | 5,908 fixtures / 7,691 cases; 49,024 / 49,024 matched; T0/T1/T2/T3=100%; FP=0, FN=0 | not separately timed |
| formatting / whitespace | passed | — |

The added alias-resolution unit control initially included an unrelated unused
local suggestion. Its input was corrected and oracle-checked; the final full
checker run above is green. No implementation diagnostic was suppressed.

## Workflow findings

- The edit loop uses direct Cargo focused tests. `cargo xtask test <role>`
  delegates to the same Cargo test operation (`workspace_maintenance.rs:67`)
  after rebuilding the dev-profile launcher and its dependencies.
- First reproduction: 2 expected failures, 1.57 s execution, preceded by
  2m37s dev and 1m49s test builds. The final compiler contract build took
  2m30s. The final diagnostic launcher build took 7m41s including an earlier
  artifact-lock wait. Build time dominates these local test bodies.
- Preparation commits `8ac859fe` / `24b6205e` from the stopped concurrent
  session are retained. Codex owns the implementation in this checkout.
- The preparation head's successful hosted check does not validate these
  implementation bytes. The PR must pass hosted acceptance after this commit.
- No historical source/profile certificate walk or full developer
  `cargo xtask ci --baseline 3f90839e` was run for this user-authorized pilot.
  Skipped checks are not represented as passing; hosted case coverage and
  upstream expected results are unchanged.

## D2/D5 closure evidence

`checkingObjectWithThisInNamePositionNoCrash`: the upstream minimal-source trace
shows that correct diagnostic display comes from syntactic return-expression
inference while the signature return cache itself still becomes `any`.
The missing diagnostic-renderer expression-inference route is now implemented.
The route keeps the existing display context and mapper. Named-property and
contextually typed controls keep the signature fallback; computed-name rejection
can infer the return expression independently of the circular signature cache.

`recursiveConditionalTypes`: upstream's minimal recursive-box getter first
resolves its symbol type during `inferFromProperties`, then subsequent accesses
reuse it. Rust's first demand was identical, but its completed overload
transaction discarded that accessor type. The deferred check then recomputed it
and reported a new circularity. Accessor types and their completed diagnostics
now survive selected/rejected trials using the existing once-result journal;
rollback and abort still restore the original state.

Both full frozen-case controls passed together (2 passed, 7.02 s). The second
case also passed individually (1 passed, 5.71 s) before the display change.
No diagnostic is suppressed. The preserved-circularity controls verify that
real TS7023 and overload errors remain observable.

Refreshed D2/D5 validation: checker library 1,708 passed (1.35 s; build 4m54s);
compiler focused/CLI/I/O contracts 87 passed, 10 existing ignored (52.70 s;
build 6m17s). Full diagnostic conformance again matched all 49,024 diagnostics
across 5,908 fixtures / 7,691 cases, T0/T1/T2/T3=100%, FP=0, FN=0
(dev build 8m26s). Those diagnostic results preceded the comment-only System
repair below; final hosted acceptance rechecks them at the landing head.

The investigation ran in an isolated worktree while the first candidate's
hosted check ran. The two ready fixes join PR #509, with hosted acceptance
required again at the final implementation head; the earlier result alone
does not qualify the additional fixes.

## Hosted regression and repair

The first candidate's hosted run `34026212895` failed after 32m20s in H2.5g
at `systemModule7.ts#default`, index 4119. Full diagnostic conformance and
H1 through H2.5f had passed. The failure was a missing leading comment on a
namespace IIFE relocated into System's `execute` body, not a changed upstream
expectation. This demonstrates why complete hosted coverage remains required.

The block's inherited-container guard now respects the existing relocated-list
metadata: relocated statements inherit only their explicit detached-prefix
resume and retake ordinary leading comments at their new location. Direct and
detached-header namespace controls retain the upstream placement. The failing
case and all 77 admitted System-format H2.5g rows passed exact frozen-observation
replays twice each before the final push (dev rebuild 4m05s). Final product
checks and the hosted run are recorded in the PR body.

The next hosted run, `34029626985` at `0b4146cb`, passed full diagnostic
conformance and every H1 through H2.5g check, including all 8,511 admitted
H2.5g observations. It then stopped because `dynamicImportWithNestedThis_es5`
had become exact and remained in the H2.5h divergence manifest. This is a
failed overall run; the later hosted bands had not executed.

An independent full H2.5h observation found eight now-exact listed cases,
unchanged facets for all other old entries, and one new write mismatch:
`optionalChainingInArrow.ts#target=es5`. The eight removals exactly match
the separately created commit `4c615ddd` (58 → 50), which was reviewed and
retained. The raw observation's new mismatch was never adopted as an exception.

A focused compiler contract reproduced the missing comment before a concise
arrow's synthesized ES5 return. Upstream calls `emitBlockFunctionBody` directly
outside the node comment pipeline (`_tsc.js:119021-119031`); its detached-list
phase does not claim the body's own range. Rust's shared funnel now preserves
the inherited comment scope on that direct function-body route. The rebuilt
block no longer steals the return statement's expression-leading comment.
The 19 focused controls passed (24.57 s), including the ES5 regression, System
relocation, all 18 W5 rows and previous-wave controls. Formatting and whitespace
checks passed. Normal H2.5h acceptance then passed: 932 candidates, 838 exact,
50 unchanged known divergences, 44 deferred, repetitions=2. Later-band and
final product/hosted results are recorded in the PR body.

The full H2.7b runner requires an absent manifest to represent zero known
divergences (`load_h2_vector_divergence_manifest`), matching its own manifest
writer. The initial empty JSON representation was rejected before case
execution and is removed. This changes the zero-entry representation only;
every newly diverging case still fails the ordinary manifest join.

## Integrator amendments after the pilot's first two hosted runs (2026-09-06 evening)

- 19:38 hosted run 1 (6c21f859) RED: H2.5g `compiler/systemModule7.ts#default` — the synthesized System `execute` block's first statement lost its leading comment (the W5-P3 container-prefix carry); repaired by the pilot session in f71a17d0 (+ a control in `emit_session_contract.rs`).
- 20:53 hosted run 2 (0b4146cb) RED: H2.5h — eight rows the w5 ports made byte-exact were still in the 5h manifest ("is exact now (shrink the manifest)"); the integrator re-minted and audited the 5h manifest against the base (removed 8, owners preserved: `dynamicImportWithNestedThis_es5`, `asyncArrowFunction11_es5`, `classAbstractAccessor`, `accessorsOverrideProperty7`, `computedPropertyNames1_ES5`, `objectSpread`, `objectSpreadNegative`, `thisTypeInAccessors`; 4c615ddd) — and the local 5h run then exposed a NEW 5h divergence hosted had not reached: `conformance/expressions/optionalChaining/optionalChainingInArrow.ts#target%3Des5` — the ES5 arrow-body `return` (ranged to the expression body by `transform_function_body`) lost `// single-line comment`. Root cause: upstream's `emitSignatureAndBody` calls `emitBlockFunctionBody` directly, so a function body block never claims `containerPos`; the Rust funnel pipelines the block, and the w5 first-statement guard (`parent_comment_container_owned_prefix_for_owner`, tsc's `pos === containerPos`) suppressed the comment. Landed fix (`da53b93d`): the printer uses `is_function_body_block` to retain `expression_context.comments()` for direct function-body emission, instead of claiming the block range through `active_expression_comment_scope`. Control: `es5_arrow_return_keeps_its_expression_leading_comment` in `crates/compiler/tests/integration/emit_session_contract.rs` (exact ES5 output from the frozen 5h observation).
- The pilot left `ratchets/h2-7b-known-divergences.v1.json` EMPTY; `cargo xtask h2-7b-acceptance` refuses ("must be absent when it is empty") — the file is removed; the 7b sweep with it absent: 1,557 exact / 0 diverging / 36 deferred (suite conformance 468/0, project 196/0) — **the H2.7b divergence manifest is closed (185 → 0 over w1-w5)**.
- Integrator-reported supplementary results (worktree `tsc-rs-w5-x`; distinct from the final-candidate runs recorded in the PR body): 5h 838 / 50 / 44 (no new divergence), 6c 321 / 318 / 4, 6a 171 / 4 / 2, 6b 6 / 0 / 0, 5g full 9,027 / 8,511, 7b 1,557 / 0 / 36; the w3b / w4b / w5b lane controls + `emit_session_contract` 98 green; xtask 270 / 0; clippy / fmt clean. Workflow finding: shared-printer changes need adjacent-band coverage, and a full observation should be reviewed before shrinking stale divergence entries because a normal hosted run can stop at the first stale entry. The reported local costs (5h + 6x ≈ 8 min, full 5g ≈ 25 min) inform that choice; they do not replace the user-authorized pilot schedule in the W5 packet. Final results are recorded in the PR body so a results-only commit does not restart hosted acceptance.
