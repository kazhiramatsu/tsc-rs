## r25 review: data-only recovery implementation

**No blocking bug found.** The diff matches r22/r24 and the lifetimes hold. Items below are ordered by importance.

1. **Provenance and lifetimes are sound.** `create_missing_node` (parser.rs:1236-1262) now records `position` from `full_start_pos` before the report and attaches `missing_node` only to the index returned by `push_parse_diagnostic_with_event` (9966-10005), which is appended even on same-start dedupe. Both `try_parse`/`look_ahead` and every JSDoc checkpoint (jsdoc.rs:655-663, 976-983, 1144-1146, 1955-1985) go through `RecoveryCheckpoint`, which now truncates actions too, so speculative and JSDoc records cannot leak. The roll-back unit test drives exactly that path.

2. **Skip producers: complete for TS/TSX.** The nine recorded sites cover every error-adjacent `next_token`. The remaining candidates are not recovery: the JSX closing-tag `next_token` (5817, 5837) consumes a `>` that `parse_expected_without_advancing` already accepted; JSX children end on EOF/`</` by returning `None` without advancing (5519-5560); `parse_expected_token` creates a missing node rather than skipping. Recording `DelimitedSemicolon` is right because that branch is reachable only after `parse_expected(Comma)` failed.

3. **Reparse ranges.** `Reparsed { start: next_pos, end: full_start_pos }` is pushed inside the run before `scanner.restore` (9644-9651), so a final no-progress skip is inside the range. Retention keeps first-pass actions only when `owner_start` lies in an untouched range (9583-9589), so first-pass actions inside a run are dropped and re-recorded live. The unit expectation `end == find(" const after")` is consistent: the loop breaks when `statement_end == non_await_pos`, both being the full start that includes the leading space.

4. **Incremental guard.** `intersects` rejects reuse over `[min(skip start, statement_start), skip end]` and ignores `Reparsed` (recovery.rs:79-101), per r24. The messaged missing node's own `position` is not consulted, but that is safe: a node containing a missing child carries `THIS_NODE_OR_ANY_SUB_NODES_HAS_ERROR` via `parse_error_before_next_finished_node`, which the guard already checks (1398-1402). No correction needed.

5. **Observer parity.** Both sides use `ParseForTypeErrors`, derive JS/JSX from the path, and skip JSDoc subtrees (TS `forEachChild` never enters `jsDoc`; Rust returns on `JS_DOC`). Both count `MissingDeclaration` by kind and then recurse into its modifiers, so the widened declaration range is observed as the final node, as your note requires. `target` is numeric in the fixture and read as `i64`; `message_utf16` compares flattened text, and the Rust side asserts no chain, which holds for these rows.

**Two small issues to fix at leisure, neither blocking.**

- `record_token_skip` falls back to `token_start` for `statement_start` when no source element is active (1875-1888), while events fall back to the diagnostic `start` (9995-9999). For the top-level `ListNoProgress`/`TopLevelAwaitReparse` skips these coincide, so behaviour is identical today; document the fallback or unify it so retention rules stay one-to-one.
- `top_level_reparse_ranges_belong_to_reparsed_statements` asserts `diagnostics.len() == 2` and `events.len() == 2`. That is correct only because `const before = ;` is retained and `const after = ;` is retained by the tail range. If a later change makes `await f()` itself report, the count assertion will mask which range moved. Prefer asserting the two event starts (positions of `before` and `after`) as well.

**Confirmed unchanged.** `is_literal_only` ignores actions, so `export {}; await f();` stays literal-only with a `Reparsed` action (the test pins this). Event and origin counts are unchanged, so existing typed-refusal counts and the utf16 census are unaffected.
