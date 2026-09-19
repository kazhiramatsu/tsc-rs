I found no correctness bug in the canonical graph, the field guard, the selector or the helper extraction. There are three capture/guard items to close before the 80-minute census run. For native reconstruction I recommend option B. This is a code read only; nothing was built.

## Graph digest and field guard
**Canonical graph** (`recovery_parse_snapshot.rs`): correct.
- It walks breadth-first from the root. Each node or array gets its index the first time it is reached, and later references reuse that index. So cycles terminate, and a node shared by two parents hashes as shared, not duplicated.
- Allocation IDs never enter the hash; the test with node/array ID bases 0 and 100/200 covers this.
- Fields are visited in the generated observable order, then the extra fields in a fixed order. The generator file is unchanged since the merge base, so the order is the same in all three builds.
- If `parent` points at a node that isn't otherwise reachable (a speculative leftover), the walk pulls it in. That can only add differences between builds, never hide one, so it's fail-safe.
- `Node` and `NodeArray` are destructured without `..`. SourceFile facts cover every field except `snapshot`, which is represented by the text hash, `arena`/`root`, which are the graph itself, and `parse_recovery`, which is deliberately excluded.

**API check:** every item the digest uses is public in the census tree:
- `tsc_syntax::nodes::{Node, NodeArray}`, `for_each_observable_field` and `ObservableField`;
- `CommentDirective(Kind)` and `FileReference`;
- `SourceFile::text()`, plus `arena.nodes()`, `node_mut` and `node_array_mut` for the tests.

The borrows in `digest` (the two callbacks that capture `graph`, pattern bindings through `&Node`, `&i32 & i32`) are fine. I don't see a compile error.

**Field guard (`validate_schema`):** sound for its purpose.
- `variant_body` matches on `NodeData::X(` including the parenthesis, so no variant name is mistaken for a prefix of another.
- `rust_fields` skips `//` lines.
- The SourceFile field list must match exactly.
- The `include_str!` paths resolve to the candidate's syntax sources even when the probe is built against an older tree. The replay script's assertion that those files are byte-identical in the baseline tree (replay-recovery-parse.py:43) closes that gap.

**Helper extraction:** `source_request_parse_options` is exactly the original block, with the caller now reading `javascript_file` from it. The removed locals (`is_declaration_file`, `language_variant`, `force_external_module`, `module_detection`) have no remaining uses in the worker. Behaviour is unchanged.

## Items to fix before the full census
1. **The projection-baseline guard is looser than claimed** (replay-recovery-parse.py:44-52). It checks that the reverted tree's `parser.rs` is the whole 81d5aa52e file, with every other syntax file equal to the candidate. It never checks that the candidate's `parser.rs` differs from 81d5aa52e only inside the await helper. Any other `parser.rs` change after 81d5 would also be folded into the "projection" delta.
   - That is sound, because the union only grows, but it no longer measures just the helper.
   - Either assert that `git diff 81d5aa52e <candidate> -- crates/syntax/src/parser.rs` touches only the helper's line spans, or relabel this baseline "pre-interface parser" and don't claim it is helper-only.
2. **`command_input` lacks the plan identity that option B needs.**
   - Recorded compiler plans export the fixture path and blob, `effective_settings`, write order and roots, but not `plan.variant` (`configuration_index`, `key`, `description`, `upstream_name`, `overrides`). Add it, together with the loader's limits.
   - Projects export the descriptor blob, but not `fixture.source.relative_path`. Add it.
3. **Load failures:** the selector correctly lists them without claiming coverage. Keep that list in the final qualification record so a later qualification can't absorb them silently.

## Selector
The selector checks that every replay covers the same input-ID set against the same snapshot and digest code. It requires the current replay to equal the census's own digests, core and profiles, for every input. It then puts a row into the union if:
- any of its inputs, acceptance or module role, differs in core or profiles under the projection or merge-base baseline; or
- it appears in any of the five profiles' newly-admitted or newly-refused sets, checked against the same universe, loader, manifest and load failures.

That is the round-70 induction rule applied correctly. Requiring `command_input` on every selected row, with the documents it references, is right.

## Native reconstruction: option B
**B:** rebuild the plan through the existing, tested harness builders from an explicit identity, then assert that its serialized form equals the captured `command_input`.
- **Recorded compiler rows:** fixture path plus blob SHA, verified against the git-pinned vendor tree, and the variant key, fed through the same plan builder. Serialize the result with the census's `plan_input` function and require byte-equality with the capture, including the `prepared` summary. That summary covers the prepared roots, the source paths and hashes, and the compiler and program option debug hashes.
- **Qualified and candidate rows:** the capture already embeds the exact artifact input, so pass it straight to `load_qualified_compiler_emit_with_symlinks` and assert the `prepared` summary.
- **Project rows:** descriptor path plus blob, module variant and scenario, fed through the project plan builder; assert the mount-file hashes and the `prepared` summary. That makes project rows reconstructible if any are selected.

This keeps loader, option-floor, case-sensitivity and symlink semantics in one place, never looks inputs up by case ID, and fails on any drift.
- **A** would re-implement plan construction from JSON, creating a second planner to trust.
- **C** would need `CompilerOptions`, `ProgramOptions` and config diagnostics to round-trip through serialization, which is fragile.

**TS observer:** read `command_input` directly.
- Build the VFS from the units in write order (keeping undefined content distinct from empty) and the symlinks in FileSet order.
- Take the options from the settings with the same established floor. For a config root, parse the config unit with `parseJsonConfigFileContent`.
- Then assert that TS's loaded non-library source list and hashes equal `command_input.prepared.source_files`. A mismatch there is a reconstruction error, reported rather than skipped.

Items 1 and 2 are small capture changes. Making them before the census run means the run doesn't have to be repeated for the observer or native consumer.
