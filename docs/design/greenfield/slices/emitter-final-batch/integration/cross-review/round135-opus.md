These two rows are covered for diagnostics by the conformance gate and for the TS side of the command by a TS reference observation, but no native whole-command observation exists for them. The smallest honest fix is a focused native command comparison of just these two IDs against the existing frozen TS observation, reusing the existing observer. No new loader or protocol is needed. This is from bounded read-only queries of the artifacts and sources; I made no edits or builds, and didn't read the 337 MB snapshot.

## Existing evidence for the two `noemit=true` rows

**Diagnostic conformance: covered.** `ratchets/conformance-matches.v1.json.zst`, view `2xxx`, key `conformance/moduleResolution/bundler/bundlerImportTsExtensions.ts`, has entries for both `allowImportingTsExtensions=false,noEmit=true` and `allowImportingTsExtensions=true,noEmit=true`. Each records 2 `matched`, 2 `multiplicity_complete`, and 2 each at `t1`/`t2`/`t3`, the same as the `noEmit=false` siblings. This is parse-plus-check diagnostic evidence, enforced by the conformance gate (FP=0 with the accepted-set ratchet; baseline `3b1f5fe87fd3…`). It is not a command or emit observation.

**TS command reference: covered.** In `ratchets/h2-8a-observations.v1.json` (`cases[366]` and its `true` sibling), both rows have:
- `disposition: "typescript-reference-only"` and `repetitions: 2`;
- `writes: []`, `exit_code: 2`;
- `emit_result: {emit_skipped: false, diagnostics: [], emitted_files: null, source_maps: null}`;
- 8 and 3 reported diagnostics respectively.

**Native command: never observed.**
- `ratchets/h2-8a-candidates.v1.json` assigns them to `["H2.8a","H2.9"]`, with the reason "effective noEmit=true retained outside emitter admission".
- `h2-candidate-dispositions.v1.json` has `route:noEmit=true`, `execution_state: "not-run"`.
- The PLAN-BASE native comparator (`crates/compiler/tests/integration/h2_7d_original_corpus_shared.rs:~930-990`) runs native commands only for rows owned by H2.7d alone (or H2.7d with H2.7e). Others are checked with `assert_reference`, which checks the reference only.
- The census tried `load_qualified_compiler_emit_with_symlinks`, which correctly refuses `noEmit=true` (`program/src/loader.rs:989`).

**Other routes don't apply:**
- They aren't recorded compiler plans; conformance IDs come only from qualification or candidate artifacts. So `load_compiler_no_emit` and `upstream_no_emit_harness_contract.rs` never select them.
- EF7 (`emitter-final-universe.json`) does handle `noEmit` rows ("noEmit rows record diagnostics, status and exit with an empty write set"), but contains no `noemit%3Dtrue` IDs.
- EF6 (`emitter_final_batch.rs:~217`) covers only the `noemit%3Dfalse` sibling.

## Parse coverage versus command or emit qualification

- **Parse:** your r147 sibling derivation is the right argument. The `noEmit=false` siblings have the same 39 roles and 30 exact (file name, text, `ParseOptions`) tuples, and the prepared and source-request parse options never read `noEmit`. Together with the conformance diagnostics above, parse behaviour is witnessed.
- **Emit qualification:** not applicable. With `noEmit=true` the emitter isn't invoked, and TS writes nothing.
- **What's missing:** only native equality of the whole command: diagnostics, bucket order, exit code 2, the `emit_result` shape and an empty write set.

## Smallest supplemental check

Add a focused entry, alongside the EF4/EF5/EF6 entries in `emitter_final_batch.rs` or as a small test next to it, that:
1. Selects exactly these two IDs from `ratchets/h2-8a-candidate-inputs.v1.json`.
2. Builds each input with the existing `memory_host(case, &input_artifact, &libraries)` and runs the existing whole-program `observe(case, &host, &libraries)` from `h2_7d_original_corpus_shared.rs`. That is the same `emitFilesAndReportErrorsAndGetExitStatus` observer EF7 already uses for `noEmit` rows.
3. Compares with the unchanged `assert_complete(&actual, &observations[id]["typescript_observation"])`, twice each, from fresh Programs.
4. Asserts input identity (`row["input_sha256"] == reference["input_sha256"]`), exactly as the shared test does.

**What must not change:**
- The census snapshot and its 110 recorded failures.
- The selector and identity checks.
- The public harness API; the entry reuses test-crate functions only.
- No fake emit error is introduced.

**What to record per row:**
- the existing conformance-match evidence;
- the TS reference;
- the new native command result: exact twice, or a mismatch.

Don't record them as loader success, and don't record any emit qualification.
