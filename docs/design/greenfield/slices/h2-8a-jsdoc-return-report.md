# H2.8a G5c JSDoc return annotation — work report

Companion to [h2-8a-jsdoc-return-design.md](h2-8a-jsdoc-return-design.md)
(design and readiness) for the request in [h2-8a-jsdoc-return.md](h2-8a-jsdoc-return.md).
This report records what was measured, what was changed per cause, every
heavy command with its real exit code, and what is handed to the
integrator. Hosted acceptance, the PR and any global profile update stay
with Codex; nothing here claims them.

## 1. Start point

- Worktree `/Users/hiramatsu/dev/tsc-rs-jsdoc-return`, branch
  `work/h2-8a-jsdoc-return`, start HEAD `46b9743b5` = runtime base
  `7d6bc9848e97c26b0438da5ec73bd08c9a175c9a` plus two documentation commits
  (`a29047869`, `46b9743b5`). The worktree was never reset.
- `node scripts/check-h2-8a-jsdoc-return-selection.mjs` at the start source:
  exit 0 ("15 file pins, 12 upstream owners, one unchanged original case,
  strict before failure twice"). After the production edits the same script
  reports the expected stale pins for the edited files; it is the
  preparation-time identity check, not a readiness gate.
- The frozen before receipt (`h2-8a-jsdoc-return-before.v1.json`) matches
  the start source, so the baseline was not re-run blindly; the instrumented
  trace run below reproduced it byte for byte and is the fresh before.
- Heavy native execution policy: dedicated target `target/jsdoc-return`,
  `CARGO_BUILD_JOBS=2`, `taskpolicy -b nice -n 15`, `--test-threads=1`, one
  command at a time; every command/exit/log hash is in the run directory's
  `receipt.json` (`run-step.py` copied into each run directory).

## 2. Before (start source)

Run directory: `target/jsdoc-return-runs/before-20260914-155056`. The production tree carried only the temporary trace
instrumentation (`instrumentation.patch` in the run directory); the Rust
tree hashes at the start were the selection inventory's.

| Step | Command (abridged) | Exit | Result |
| --- | --- | --- | --- |
| `g5c-original-before-trace` | `cargo test -p tsc-rs-compiler --test h2_8a_declaration_comment_ranges original_shared_g5c_complete_command -- --exact --nocapture --test-threads=1` | 101 | two identical complete captures, identical to the frozen receipt's, four `/writes/1` difference paths; trace lines in the design §4 |
| `controls-before` (55-case fixture) | `cargo test -p tsc-rs-compiler --test h2_8a_jsdoc_return -- --nocapture --test-threads=1` | 101 | 21 exact positives, 34 failures (§2.1) |
| `checker-lookup-before` | `cargo test -p tsc-rs-checker --lib -- syntactic_type_node_builder::tests --test-threads=1` | 101 | 12 existing tests pass; the new node-identity control fails on 39 hosts × 2 repetitions, every mismatch `Null` where upstream owns the annotation node |
| `controls-before-58` (final fixture) | same as `controls-before` on the 58-case fixture | 101 | 22 exact, 36 failures: the 55 first-round actuals are byte-identical to `controls-before`; `r3-last-block-owns-tags-declaration` is exact through the semantic fallback (the syntactic route still selected the first block, see the after node-identity control); `r6-asserted-returns-semantic-fallback` prints `c(x: any): null` for `any`; `r6-asserted-await-returns-semantic-fallback` prints `Promise<number>` for `Promise<any>` (both C2) |

### 2.1 Classification of the 55 first-round controls

Exact before (positives to preserve, 21): `r1-body-star-without-annotation`,
`r1-plain-body-without-annotation`, `r2-function-declaration`,
`r2-parenthesized-function`, `r2-object-literal-method`, `r2-class-method`,
`r3-inner-tag-before-outer`, `r3-untyped-return-tag`,
`r3-callable-type-tag-only`, `r5-async-promise`,
`r5-async-generator-without-annotation`, `r5-single-return-without-annotation`,
`r5-multiple-returns-without-annotation`, `r6-ts-direct-type-ignores-jsdoc`,
`r6-accessor-pair`, `r6-jsdoc-construct-signature`,
`r6-annotation-body-mismatch`, `r7-noemitonerror-blocked`,
`r7-noemit-with-declaration`, `r8-g4a-function-parameter-and-return`,
`r8-g4b-class-method-owned-comment`.

Failures (34), by cause:

- **D1 (syntactic return worker reads only the direct attachment), 31 rows**:
  the `.d.ts` prints `any` where the outer annotation must be reused —
  `r1-original-d-string`, `r1-original-e-template-intersection`,
  `r1-returns-alias`, `r2-exports-property`, `r2-variable-initializer`,
  `r2-arrow-expression-body`, `r2-prototype-method`, `r2-ordinary-property`,
  `r2-property-assignment-function`, `r3-last-block-owns-tags`,
  `r3-sibling-jsdoc-not-inherited` (`s1(): number` from the shortcut),
  `r4-*` (5 rows, `any` instead of `T | U`, `T`, `Point`,
  `import("./other").Thing`, `T`/`T[]`), `r5-literal-annotation`,
  `r5-union-annotation`, `r5-callable-type-literal`, `r5-jsdoc-function-type`,
  `r5-generator` (`Generator<number, any, any>` from the semantic fallback
  instead of the reused `Generator<number>`), `r6-unresolved-annotation`,
  `r7-allowjs-without-checkjs`, `r7-emit-declaration-only`,
  `r7-declaration-map` (declaration text and the dependent map positions),
  `r7-remove-comments`, `r8-lone-surrogate-escape-literal`,
  `r8-astral-pair-literal`, `r8-replacement-character-literal`,
  `r8-detached-prefix-with-return`, `r8-parameter-tag-binding-with-return`;
  plus `r3-return-tag-over-callable-type-tag`, whose only difference is the
  TS2322 message rendering `'() => any'` for `'() => string'` — the
  diagnostic display serializes the signature through the same node-builder
  route.
- **D2 (first-return-tag selection)**: `r3-untyped-then-typed-return-tags`
  — the start code selected the second, typed tag, then the reuse check
  refused it and the semantic fallback printed `null` (see C2); upstream
  prints `any` through the shortcut.
- **C2 (return aggregation drops JSDoc type assertions, semantic)**:
  `r3-type-assertion-parenthesized-owner` — Rust reports TS2352 with
  "Type 'null' is not comparable to type 'string'" and exit 2 where upstream
  reports nothing (exit 0). Proven against the pinned compiler
  (`return /** @type {*} */(null)` is `any` upstream, a bare `return null`
  is `null` in both) and located in
  `check_and_aggregate_return_expression_types` (design §3.1).

The three controls added after this classification
(`r3-last-block-owns-tags-declaration`,
`r6-asserted-returns-semantic-fallback`,
`r6-asserted-await-returns-semantic-fallback`) have their before record in
`controls-before-58`.

## 3. Changes per cause

Final production source hashes (SHA-256, recorded in `target/jsdoc-return-runs/after-20260914-160710/source-hashes.txt`):

- `crates/binder/src/node_util.rs` `6b31b8aef0ebe96496146197f6e5639d03ae80df65e82620f5cf3b616a9e1e65` (start `7bb0a612b1307952f6ef4995394b6f618a27447f02c053e29ff681a421db0f04`)
- `crates/checker/src/syntactic_type_node_builder.rs` `5c4f1dd122c0dc3f3b02478d66b563418af7dfe070ceac0c1a758a8809301637` (start `655accdcf99fc6a636f56dc2c379ca4cff754978f4b5873dfd217379a7fe731e`)
- `crates/checker/src/functions.rs` `6f5ff3876614787f8896b94b87029d29f9e59acc0143a1616622ed97ba748cd5` (start `7e38449bae70690d119ec3345f41149560efc6087e36e94224cb2b1b9462d93d`)

### 3.1 D1/D2 — owned JSDoc return annotations in the syntactic builder

Commit `ee39b67cd` (`fix(checker): reuse owned JSDoc return annotations in the
syntactic builder`). `crates/binder/src/node_util.rs` gains the AST-only
ports `get_jsdoc_return_tag` (getJSDocReturnTag, 11708–11710) and
`get_jsdoc_return_type` (getJSDocReturnType, 11728–11744) over the existing
`visit_owned_jsdoc_tags` traversal, plus a private
`jsdoc_type_expression_type` projection; `SyntacticBuildSession::get_jsdoc_return_type`
delegates to the port on the host's own parsed source and wraps the result in
the host's `TransformSourceId`; `direct_jsdoc_tags` is deleted. No other
function changes. Witnesses: the original G5c, R1–R5, R7, R8 and the
diagnostic-display rows of R3 (design §6).

### 3.2 C2 — JSDoc type assertions in return aggregation

Commit `f85c1cb23` (`fix(checker): keep JSDoc type assertions in return
aggregation`). In `CheckerState::check_and_aggregate_return_expression_types`
the two `node_util::skip_parentheses_pub` calls (return expression, `await`
operand) become `self.skip_parentheses_excluding_jsdoc_type_assertions`,
the existing port of `skipParentheses(node, /*excludeJSDocTypeAssertions*/ true)`.
Witnesses: `r3-type-assertion-parenthesized-owner`,
`r6-asserted-returns-semantic-fallback`,
`r6-asserted-await-returns-semantic-fallback`.

### 3.3 D3 — diagnostic signature display

Commit `1e1a66135` (`fix(checker): read owned JSDoc return annotations in
diagnostic signature display`). In `CheckerState::serialize_return_type_for_signature_slice`
(`check.rs`, the text-slice port of `createReturnFromSignature` used by
error display) the direct `.type` match becomes
`self.effective_return_type_node(declaration)` and the single-return
shortcut runs only when no annotation exists (design §3.2, §5.4). Witness:
`r3-return-tag-over-callable-type-tag` (TS2322 names `'() => string'`).

### 3.4 Frozen controls and their consumers

Commit `e4db6eb85` (`test(compiler): freeze JSDoc return annotation controls`).
`scripts/observe-h2-8a-jsdoc-return.mjs`,
`crates/compiler/tests/fixtures/h2-8a-jsdoc-return.json` (SHA-256
`687d28b13459dff3b472515c430f66165c4bdaaec6f07bbbb893ca2997d54ba3`, 58
cases, 116 complete executions, 70 traced functions),
`crates/compiler/tests/h2_8a_jsdoc_return.rs` and the checker unit control
`jsdoc_return_lookup_matches_upstream_node_identity`.

## 4. After

### 4.1 Intermediate battery at the D1/D2/C2 head (`target/jsdoc-return-runs/after-20260914-160710`)

Run on the tree with the binder/syntactic-builder delegation and the C2
skip only (D3 not yet applied; source hashes in that directory's
`source-hashes.txt`). Stopped after step 5 once D3 was identified, because
D3 edits `check.rs` and the final head needs its own complete run.

| Step | Exit | Elapsed |
| --- | --- | --- |
| `jsdoc-return-tests` | 101 | 210.83 s |
| `comment-ranges-all-six` | 0 | 156.92 s |
| `checker-syntactic-builder` | 101 | 172.52 s |
| `binder-crate` | 0 | 12.05 s |
| `checker-node-builder-jsdoc-functions` | 101 | 0.42 s |

- `jsdoc-return-tests`: `original_g5c_complete_command` **passes** (the
  original tuple exact twice through the shared comparator); 57/58 controls
  exact twice; the single divergence is `r3-return-tag-over-callable-type-tag`
  whose TS2322 message still names `'() => any'` (D3, design §3.2).
- `comment-ranges-all-six`: all six tests pass, including the previously
  strict-failing `original_shared_g5c_complete_command`.
- `checker-syntactic-builder`: 12/12 existing tests pass; the new
  node-identity control failed on three test-harness defects, none in
  production: TypeScript names the `ImportType` kind `LastTypeNode` through
  `SyntaxKind[value]` (observer now emits canonical names), the arena's
  positions are UTF-8 byte offsets while the trace holds UTF-16 offsets (the
  control now converts), and the arena also holds parser-abandoned
  speculative `async function` nodes (the control now walks the tree in
  forEachChild order). The fixture was re-minted (kind names only; the same
  58/116/70 counts) to SHA-256
  `687d28b13459dff3b472515c430f66165c4bdaaec6f07bbbb893ca2997d54ba3`.
- `binder-crate`: pass. `checker-node-builder-jsdoc-functions`: 237 pass, the
  only failure is the same harness-defective control.

### 4.2 Final battery (`target/jsdoc-return-runs/after2-final`)

Run on the final source (D1/D2 + C2 + D3 plus the frozen controls; source
hashes in that directory's `source-hashes.txt`, identical to the committed
files of §3). Every step is one demoted, sequential native command.

| Step | Exit | Elapsed |
| --- | --- | --- |
| `jsdoc-return-tests` | 0 | 151.81 s |
| `comment-ranges-all-six` | 0 | 119.07 s |
| `checker-syntactic-builder` | 0 | 136.72 s |
| `binder-crate` | 0 | 0.62 s |
| `checker-node-builder-jsdoc-functions` | 0 | 0.35 s |
| `compiler-adjacent-suites` | 0 | 624.43 s |
| `fmt-check` | 0 | 3.25 s |
| `clippy-changed-crates` | 101 | 17.76 s |
| `checker-library-full` | 0 | 3.18 s |
| `clippy-program-alone-inherited` | 101 | 11.25 s |

- `jsdoc-return-tests` (exit 0): `jsdoc_return_controls_match_complete_commands_twice`
  — all 58 controls exact on both repetitions (116 captures in
  `captures-controls`, every capture `exact: true`); `original_g5c_complete_command`
  — the unchanged original tuple exact twice through the shared
  original-corpus comparator.
- `comment-ranges-all-six` (exit 0): all six `h2_8a_declaration_comment_ranges`
  tests pass, including `original_shared_g5c_complete_command` (strict,
  never skipped; exit 101 before) and the 41 focused/parameter-tag/detached
  controls with G4a/G4b.
- `checker-syntactic-builder` (exit 0): 13 passed — the 12 existing tests and
  `jsdoc_return_lookup_matches_upstream_node_identity` (70 hosts, upstream
  node identity for `get_jsdoc_return_type` and `effective_return_type_node`,
  twice).
- `binder-crate` (exit 0): 71 + 2 passed. `checker-node-builder-jsdoc-functions`
  (exit 0): 238 passed (`node_builder*`, `jsdoc`, `functions::tests`).
- `compiler-adjacent-suites` (exit 0): `h2_8a_declaration_specifiers` 11,
  `h2_8a_utf16_identity_recovery_controls` 2, `h2_8a_utf16_review_fix_controls`
  1 (25 controls ×2) — all passed.
- `fmt-check` (exit 0). `checker-library-full` (exit 0): 1738 passed, 0 failed.
- `clippy-changed-crates` (exit 101): `cargo clippy -p tsc-rs-binder -p
  tsc-rs-checker -p tsc-rs-compiler --all-targets -- -D warnings` stops in
  `tsc-rs-program` (a path dependency clippy also lints) with 145 errors —
  `result_large_err` ×92, `useless_conversion` ×48, `redundant_closure` ×3,
  `comparison_to_empty` ×1, `iter_skip_next` ×1 — all in
  `crates/program/src/{loader,prepared,module_resolution,config,path,js_path}.rs`,
  files this slice never touched. `clippy-program-alone-inherited` (exit
  101): the same command on `-p tsc-rs-program` alone reports the identical
  145 errors, so the step is inherited lint debt of the current toolchain,
  not a regression. `clippy-changed-crates-warn` (below): the same three
  crates without `-D warnings`, filtered to the changed files.
- Emitter declaration-reprint contracts were not re-run: the emitter crate's
  tests use test resolvers and never execute the checker, and no shared
  type-node *printing* changed — only the checker's choice of the return
  annotation node for JavaScript signatures. The compiler-level suites above
  exercise the real printer on the changed output.

- `clippy-changed-crates-warn` (exit 0, 114 s): `cargo clippy -p tsc-rs-binder
  -p tsc-rs-checker -p tsc-rs-compiler --all-targets` without `-D warnings`
  reports 341 pre-existing warnings across the workspace path dependencies
  and the checker (`structural.rs`, `modules.rs`, `engine.rs`, …); none is in
  `node_util.rs`, `functions.rs`, `syntactic_type_node_builder.rs`, the two
  test files or the fixture, and the four in `check.rs` — `check.rs:5288` (useless conversion to the same type: `tsc_types::JsString`); `check.rs:11763` (this expression creates a reference which is immediately dereferenced by the compiler); `check.rs:12882` (useless conversion to the same type: `tsc_types::JsString`); `check.rs:13216` (useless conversion to the same type: `tsc_types::JsString`) — lie
  outside the D3 function (which starts at line 10066). The changed code
  itself is clippy-clean.

## 5. Run directories, commands and exits

All heavy commands ran from the worktree root with `CARGO_BUILD_JOBS=2`,
`CARGO_TARGET_DIR=<worktree>/target/jsdoc-return`, `taskpolicy -b nice -n 15`
and `--test-threads=1`, one at a time; `receipt.json` in each directory
records the exact argv, environment overrides, exit code, elapsed seconds,
HEAD and log hashes (written by `run-step.py`, also copied there).

| Directory | Purpose |
| --- | --- |
| `target/jsdoc-return-runs/warm-build` | `cargo test --no-run` of the compiler test binary (exit 0) |
| `target/jsdoc-return-runs/before-20260914-155056` | before: G5c trace, 55-control run, checker control, 58-control run (§2) |
| `target/jsdoc-return-runs/after-20260914-160710` | intermediate battery at the D1/D2/C2 head (§4.1) |
| `target/jsdoc-return-runs/after2-final` | final battery at the final source (§4.2) |

Light commands (exit 0 unless stated): `node scripts/observe-h2-8a-jsdoc-return.mjs --write`
then `--check` (twice per mint: the initial 55-case mint, the 58-case
re-mint, the canonical-kind-name re-mint), `node scripts/check-h2-8a-jsdoc-return-readiness.mjs`
(fails closed before the production symbols exist; exit 0 at the final
source), `cargo fmt --all -- --check` (exit 1 once on two new test files,
formatted, then exit 0), `node scripts/check-h2-8a-jsdoc-return-selection.mjs`
(exit 0 at the start source; after the edits it reports the expected stale
preparation pins for `crates/binder/src/node_util.rs`,
`crates/checker/src/syntactic_type_node_builder.rs` and
`crates/checker/src/functions.rs`; every other pin, including the original
case, the artifacts and the upstream owners, is unchanged).

## 6. Other owners and handoff

Recorded, not changed here (each with the evidence that isolates it):

- **`skip_parentheses_pub` call sites in the checker (16 remaining)**:
  `contextual.rs:1341`, `functions.rs:1447`, `expr.rs:2243/3315/3923/4271`,
  `literals.rs:752`, `operators.rs:1354/1828/1830/1864/4162/4174/4195/4332`.
  C2 fixed only the two sites of `check_and_aggregate_return_expression_types`
  whose upstream counterpart provably passes `excludeJSDocTypeAssertions`
  (78959–79008). The others need their own upstream comparison; no G5c
  control reaches them.
- **JSDoc construct signatures in diagnostic display**: the slice renderer
  never modeled the `isJSDocConstructSignature` arm of
  `createReturnFromSignature`; `r6-jsdoc-construct-signature` renders
  semantically and matches upstream, so nothing is reached.
- **Semantic `get_jsdoc_return_type` fall-through** (`jsdoc.rs`): when the
  first return tag carries a type expression without a type it falls to the
  `@type` route, upstream returns `undefined`; the parser never produces that
  shape, so it is unreachable and left as is.
- **Parser attachment of inline JSDoc inside a one-line object literal or
  class body**: upstream attaches nothing there (probed 2026-09-14); the
  controls keep every block on its own line so this parser question is not
  exercised.
- **Global totals**: the original 769-row matrix and the H2.8a close are not
  re-measured here; only the single G5c tuple and its controls are claimed.

Handoff to the integrator (Codex): the branch `work/h2-8a-jsdoc-return` with
the per-cause commits of §3 (`ee39b67cd`, `f85c1cb23`, `1e1a66135`, then the controls
`e4db6eb85` and the documentation commit that follows it), the frozen controls, this report and the design;
the run directories of §5 (not committed, under `target/`); the hosted
acceptance, the PR and any global profile or manifest update remain the
integrator's. The next integration step is the ordinary one: merge `main`
into the branch only at the integration stage, re-run the two focused runners
(`h2_8a_jsdoc_return`, `h2_8a_declaration_comment_ranges`) on the merged
head, then open the PR.
