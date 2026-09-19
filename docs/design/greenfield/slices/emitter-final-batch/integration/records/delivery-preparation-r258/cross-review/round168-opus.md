# Round 168: review of the D draft generator (`/tmp/emitter-delivery-draft-r256.py`)

I read the script only; I didn't execute it. There are **no runtime blockers**. Two guards are **material** to sentences the draft asserts, so add them before D is generated. The rest are narrow wording and link fixes.

## Material guards: the draft asserts these facts without checking them

1. **Walk convergence.** The text says the profiles "were checked through the canonical converged walk and final gate", but nothing in the script proves a walk ran.
   - A final CI exit code of 0 doesn't imply it: the CI's dry walk only compares the crate-tree hash with `target/chain-walk/converged-crates.sha256`, and **passes when that record is absent** (`chain-walk.sh:320-335`: "absent (a green walk will mint it)").
   - **Fix:** require a walk receipt (run id, green tail, and `converged-crates.sha256` equal to V's crate tree, recorded before CI) and assert it. Otherwise reduce the sentence to "re-validated by the final gate at V".
2. **Hosted checks.**
   - As written, `SKIPPED`/`NEUTRAL` are accepted for every check, so a rollup whose checks were all skipped would pass, yet the draft says "passed … the required hosted checks".
   - **Fix:** require the named required check (`gates`) to be `COMPLETED` with conclusion `SUCCESS` on head V. Also assert `hosted['number'] == 561`, since "PR #561" is hard-coded in the text.

## Narrow guards to add

3. **Resumed gate.** If the demoted gate is resumed (the perf-ceiling rerun at normal priority), the final log may cover only the measurement phase. Assert the receipt's phase/journal record shows **every** phase green at V, or record the resumed run explicitly, before writing "passed the complete, unsplit local command".
4. **Validation tree.** Assert `audit['validation_tree'] == git rev-parse V^{tree}`. The text prints that value but the script never checks it.
5. **Link targets.** Check that every relative link target exists at V (`git cat-file -e V:path`). The draft checks `test_references` and the profiles, but not:
   - `records/architecture-comments-before-r201.json`;
   - `README.md`;
   - `../../../emitter-architecture.md`;
   - `owner_identity_evidence`;
   - `typescript_source`.
6. **Landing guard stays outside the script.** Make sure the D commit's `git diff --name-only M..D` is exactly the record file, README `STATUS` is unchanged, and `git diff --check` plus link checks pass (the r227 `record_only_guard`).

## Wording to narrow

- **The per-row test references.** "Scoped source/test references, exercised through the corresponding final-gate targets…" implies every test in each file ran, and `compiler/tests/contracts.rs` includes 10 intentionally ignored H0 CLI oracle audits. Suggested replacement:

  > Source and test files containing this row's scoped controls. A file reference is not a claim that every test in the file ran; ignored tests remain ignored. Execution is attributed to the final-gate targets and the separately recorded focused proofs.

- **Target counts.** Where counts appear, use the final-CI per-target counts. Don't add up overlapping command sets.
- **The transpile sentence:** make the two outcomes explicit rather than implying parity:
  > …an invalid Unicode identifier whose composite recovery is attributed to H2.9, and `target: 100` (`ScriptTarget.JSON`), where Rust keeps a typed refusal and TypeScript reports an internal Debug Failure.

## Correct as written; keep

- `HEAD == M`; a clean worktree; `--is-ancestor V M`; `V^{tree} == M^{tree}`.
- The CI receipt checks: head V, clean, exit 0, exact argv, and log SHA-256 and byte count.
- The profile SHA-256 values read at V; "no stale H2 input"; the H2.5g count of 921.
- 22 H2 ladder profiles + transition + H1 = 24. H1 is labelled historical lineage, and the text says hashes bind identity, not execution.
- Owner declaration/body hashes recomputed at V over UTF-16 offsets.
- The adjacent checker correction stated as "no new architecture owner".
- Exactly 18 rows at `active-qualified`, no "pending" text, and the output file opened with `'x'` (never overwrites).
- The out-of-scope list: H2.9 in general, build/watch/builder, public re-emit, TS7, and the 110 parser-census load failures.

**The walk-prep note** (Node cache under the canonical `WORKSPACE/target`, a cold first 5g observation allowed, the `new-ci/target` symlink) is fine. `new-ci/` is outside the `crates/` runtime closure and outside the v36 inventory `READ` set. Just keep `git status --porcelain` clean apart from ignored paths.
