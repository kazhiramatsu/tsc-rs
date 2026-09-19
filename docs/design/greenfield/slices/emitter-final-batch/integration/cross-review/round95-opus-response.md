No proof bypass in `r95-successor-candidate.diff`; it enforces the B invariants. Three operational points and one pin weakness remain. I agree the nested case `((x\n as number)\n as string);` should become a positive; the native capture refutes my r93 "different boundaries" label. For the heritage printer, reuse `emit_comma_element_end_comments` at exactly the two upstream positions. Read-only: the diff, `/tmp/emitter-nested-paren-r90/different-boundaries.txt`, the current printer and recovery source, and `_tsc.js`.

## 1. Successor proof: no bypass found

The diff enforces:
- the same snapshot identity, digest code and input-ID set;
- successor core equal to the snapshot for every input;
- all six boolean profiles on both current and successor;
- no change outside `statement_gaps`/`context_recovery`, and no true→false loss;
- recovery facts equal for every input, from an identical probe (`probe_files_sha256` now also covers the wrapper and the selector);
- source changes limited to `recovery.rs` and its unit test file, with lock, toolchain, types, diagnostics and every other syntax file equal to the current source hashes;
- the committed, clean successor tree;
- the successor syntax pin carried into native and the comparator, with the 67 syntax tree still checked in the input workspace (`<head>:crates/syntax`).

Bytes are unchanged when there's no successor (pinned by the regression-hash test).

**Recovery facts are deterministic:** `ParseRecovery` holds only `Vec`s of derived-`Debug` types, with no hashing or floats. A future edit to those type definitions would change the `Debug` text and fail closed, which is the safe direction.

**Operational points:**
1. **Re-run the current replay.** Selector equality needs `current.probe_files_sha256 == successor.probe_files_sha256` and `current.recovery_facts_sha256`. An existing current replay built with the old probe has neither, so re-run the current (candidate-kind) replay with this probe. The mandatory `current == snapshot` core+profiles check still binds it to census 67. Projection and merge-base replays need no rerun unless you want uniform probe hashes.
2. **Successor tree must be committed:** Recovery-next is currently dirty (`M crates/syntax/tests/unit/parser/recovery.rs`). Successor mode correctly refuses until the test flip is committed.
3. **Object reachability:** `predicate_diff = git -C <successor> diff <replay HEAD> HEAD -- recovery.rs` needs the replay tree's HEAD object in the successor's object store. Worktrees of the same repository share it; a separate clone would fail explicitly.

**Pin weakness (low):**
- **Native pins only syntax:** the native validator pins only `HEAD:crates/syntax`, while the successor proof also covers types, diagnostics and `Cargo.lock`. Optionally also compare the code workspace's types and diagnostics trees and `Cargo.lock` against `selection.successor.source_files_sha256`. Today they're equal by construction.
- **Diff hash not re-verified:** `predicate_diff_sha256` is recorded but not checked again. The `recovery.rs` file hash inside `source_files_sha256` is the binding pin, so this is acceptable.

**API:** no problems. `selection.get("successor").unwrap_or(selection)` followed by `string(...)` makes a null or empty successor fail, as the unit test covers. `git(&workspace, …)?` inside `json!` is fine, and the selector's `changes` is only read under the same `successor is not None` condition that defines it.

## 2. The nested case: agree, make it a positive

The capture confirms one direct chain:
- outer paren [77,81], inner paren [79,81];
- missing-close events at start 83 / full start 81, one reported and one suppressed; the chain count is 2;
- the speculative event (83/85) is excluded by its `full_start`;
- the two skipped `)` are independent old-rule gaps:
  - 92, preceded by the `number` statement [85,92] ending at 92;
  - 104, preceded by the `string` statement [97,104] ending at 104.

Nothing bridges and nothing is weakened. My r93 "different boundaries" label was wrong.

- **Tests:** flip the test to positive, add the 12 whole-command TS controls, and keep the assertion, comma, mixed-closer, no-owner and forged-`full_start` negatives.
- **A genuine different-boundary negative,** if you want one: a closer chain whose paren ends differ, e.g. `(a, (x //c\n as number))`. The existing comma negative already covers that.

## 3. Heritage comma-list comments (the r89 printer finding)

**Upstream** (`emitNodeListItems`, :120068-120141): for `node.types` (format 528 = `CommaDelimited | SpaceBetweenSiblings`):
- **Before each delimiter:** when `previousSibling.end != parent.end` and the previous sibling has no `NoTrailingComments`, `emitLeadingCommentsOfPosition(previousSibling.end)`.
- **After the list:** the same test and call, unless comments are disabled.

The heritage-clause list itself (`HeritageClauses`) has no delimiters, so neither rule applies to it.

**Minimal reuse:** `emit_comma_element_end_comments` (printer.rs:11941) already implements exactly this:
- the guards: comments disabled or nested comments suppressed, `NoTrailingComments`, `end == parent.end`, `container_pos`;
- the `SourceLeading` phase through `emit_comments_at_cursor_with_phase`, which passes `only_print_js_doc_style`;
- so `.d.ts` output stays JSDoc-only.

In `emit_heritage_list`, only when the separator is `", "` (types lists):
- call it with `(parent, previous)` before writing each separator;
- call it once after the loop for the last element.

**No duplication:** a non-last element's own trailing phase has already printed same-line comments at its end, and the `SourceLeading` phase only collects comments after a newline.

**Don't use `emit_list_item_position_comments`:** as you say, its false arm (intervening comments) doesn't apply `only_print_js_doc_style`, so it would leak non-JSDoc comments into declarations.

**Wait for native results:** upstream's per-child intervening `emitTrailingCommentsOfPosition(child.pos)` (for example `implements A, /*c*/ B`) is a separate question. Decide it only from the 132-case native results. If the ordinary child's leading phase already prints that comment once, no change is needed.

**Scope:** the listed fixtures only; native qualification is still pending.
