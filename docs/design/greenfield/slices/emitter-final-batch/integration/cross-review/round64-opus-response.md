The b652 fixture is not enough: this needs a real before/after parse pass, which can run in a throwaway build without touching production code. The recorded-plan fallback in the observer should also be replaced by exporting the exact input the census loaded. This is from reading the fixture, git history and the scripts only; nothing was built.

## Why b652 can't be the baseline
1. **Its recorded inputs no longer match.** Three artifacts listed in `inputs` have different hashes now: `h2-6c-qualification.v1.json`, `h2-7de-candidates.v1.json` and `h2-8a-candidates.v1.json`. A check that fails when the hashes differ would stop immediately.
2. **The loaders changed after b652.** `git diff b652..HEAD -- crates/harness` shows `upstream_suites/execution.rs` (+/-456 lines), a new `execution/js_paths.rs` (442 lines) and `execution/project.rs` (240 lines). The same case ID can now load different units, and earlier load failures may now load. The baseline records the plan manifest only by path and counts, with no hash, so this drift can't be detected from the fixture.
3. **The census tool isn't in the b652 tree.** `crates/xtask/src/utf16_literal_recovery_census.rs` was only added later, in d364a056a. So `head: b652…` doesn't pin the code that produced the report.
4. **It measures from the wrong starting parser, so rows can escape.** The change you want to capture is between the parser just before the await correction and the current one, not between b652 and now. Two kinds of row escape a comparison against the old `still_refused` list:
   - rows that were clean at b652, became refused through intermediate parser changes, and are clean again after the correction;
   - rows whose b652 verdict is unknown because they are new or were load failures then.

## Recommended design: two passes, fail-closed
**1. Per-row digest.** Add a census output with one entry per row, including rows with no recovery, keyed by (case_id, universe, loader). Each entry records:
- for each unit: path, UTF-16 length and SHA-256 of the text, and parse diagnostics as (code, start, length);
- the per-statement `(kind, pos, end, AwaitContext)` list;
- a hash of the tree shape: pre-order (kind, pos, end, flags without the context bits);
- the recovery event and action counts;
- the five profile verdicts and the final supported/refused verdict.

Also write the load failures with their error text, and an input manifest: every artifact hash, the SHA-256 of the plan manifest, and `git rev-parse HEAD:vendor/typescript-6.0.3`.

**2. Two builds from one checkout of the candidate head.**
- **B** is the tree as it is.
- **A** is a throwaway worktree where only `crates/syntax/src/parser.rs` is replaced by the pre-correction version. Use the last production-qualified parser (the merge base with main), so that both the interface fix and the await projection fall inside the delta.
- Keep the current `recovery.rs` and `lib.rs`, so both passes use the same predicate and loader code and only the parser differs.
- Before running, show that `git diff <base> HEAD -- crates/harness crates/xtask` changes nothing that affects parsing or loading. If the old `parser.rs` doesn't compile against the current syntax crate, stop and report it; don't silently swap in more of the old crate.

**3. Comparison.** Abort unless A and B have identical row-key sets, load-failure sets, per-unit paths and text hashes, and input manifests.

Then report U, the set of rows where anything parse-related differs:
- **diagnostics changed** (this covers refused→clean and accepted→refused);
- **final verdict changed** under the same predicate code;
- **AwaitContext or tree-shape changed with identical diagnostics.** Report these separately, not as admissions. But also add to U any row whose tree-shape hash changed. That is cheap and doesn't depend on the claim that the ASTs are equal. Rows where only AwaitContext changed and the tree shape is equal can rely on the 285 syntax controls plus the clean complete-command controls.

The five profile reports stay unchanged. The rows to qualify are the union of U and the five profiles' `newly_admitted` sets. Assert that every row in that union has two identical TS complete-command runs and a native comparison, with no skips.

You can keep the comparison against b652's `still_refused` as a secondary sanity report, but not as the source of the admission set.

## Exporting the loaded input (census side, rows in U only)
Add a selection mode that reloads only the selected keys and writes each row's input straight from what the loader built. Use the schema of the qualified artifacts' `input` field, which the observer's `decode` already reads. The fields map to `CompilerExecutionPlan` / `CompilerFixtureInput` (execution.rs:1489-1590) as follows:

| Export field | Source |
|---|---|
| `case_id`, `universe`, `loader`, `floor` | the row key, plus the loader and `EmitOptionFloor` actually used |
| `current_directory`, `use_case_sensitive_file_names` | the plan's own values. **Do not hard-code `true`.** |
| `roots` | `root_selection.program_root_units` (paths) |
| `files: [{path, utf8_base64, utf8_bytes, utf8_sha256}]` | units in `vfs_write_order`; `content: None` becomes `{path, absent: true}` |
| `virtual_config` and `root_selection: {kind: "explicit" \| "config", config_path}` | `config_unit`, `root_selection` |
| `settings: [{name, value}]` | `effective_settings`, in its existing order |
| `vfs_symlinks: [{link_path, target_path}]` | `global_symlinks` followed by the per-unit `document_symlinks`, normalized, in FileSet order |

## Observer changes (`observe-emitter-context-recovery-corpus.mjs`)
**Use the export, not the artifact lookup.** Read inputs from the export instead of `locate()`. Today the recorded-plan fallback takes the first qualified artifact with the same case ID. That can be a different loader, a different option floor, or a different case-sensitivity from the row the census actually parsed.

**Assert identity.** Check each file's SHA-256 against the digest's unit hash.

**Fail closed on unsupported rows; no skips.**
- A `config` root selection has to go through `ts.parseJsonConfigFileContent`, the way the h2-5g oracle does. The current observer adds the config as a plain file but takes options only from `settings`, so tsconfig options would be dropped.
- Project loaders and shared mounts are not handled. None of b652's 571 refused rows use a project loader, but assert that none of the selected rows do.

**Replace the parse-diagnostic assertion.** `assert.ok(runs[0].parse_diagnostic_units.length)` fails by design on newly clean rows. Instead, assert that TS's per-unit parse diagnostic codes equal pass B's codes, which are empty for clean rows.
