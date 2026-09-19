# Round 160: patch review (`/tmp/emitter-r201-source.patch`)

This is from reading the patch against `d2b29439d` only. There is **one blocking defect**, a clippy failure from dead code. Everything else is correct.

## Blocking: dead code after the colon change

- After the patch, the two colon callers (`printer.rs:6848`, `6915`) were the **only** users of `emit_list_boundary_token_with_comments` (defined at 17454). It becomes an unused private method, and `clippy -D warnings` fails.
- It is also the only constructor of `TokenCommentBoundary::AdjacentListItem` (17468). Once the method is deleted, that variant is never constructed, which is a second dead-code warning. It is still *compared* at 17706 (`comment_boundary == TokenCommentBoundary::AdjacentListItem || owner_record.end != token_end_raw`).

**Fix, with no behaviour change:**
1. Delete `emit_list_boundary_token_with_comments`.
2. Delete the `AdjacentListItem` variant.
3. Reduce the condition at 17706 to `owner_record.end != token_end_raw`.

## Checked and correct

**Colon anchor and resume:** `emit_token_with_comments` (17381) takes `impl Into<TokenAnchor>`, which is exactly `OwnerEnd` plus the `BoundaryUnion` leading phase. DefaultClause still passes the keyword's `TokenEmission`, so its comment resume reaches the colon's leading phase. `default /*a*/ :` can't emit `/*a*/` twice.

**Clause trailing** (`emit_case_block` after `emit_node_id_with_context`):
- It runs after the node pipeline's `After` map record, which fixes the map order.
- It uses the CaseBlock scope, and `clause.end` never equals the CaseBlock end.
- The following `write_line(false)` is a no-op after a line comment's own `write_line`.
- Transformed clauses whose statements were replaced keep the raw clause range, so the original comment is emitted once at clause level, as TS does.

**Statement trailing in container:** this relies on the pipeline having claimed the clause range, which Root verified (15187-15222). The last statement (`end == clause.end`) is suppressed and every earlier statement is unchanged. Removing the escaped-only guard is correct.

**Collector** (19085-19106):
- CR/LF alone start collection and end trailing scans.
- U+2028/U+2029 reach the `is_whitespace_like` branch and set the pending comment's newline.
- Line comments still end at any `is_line_break`.

For each consumer at `pos > 0` (Normal leading, intervening, triple-slash resume, detached discovery), this moves native to TS. File-level `pos == 0` behaviour is unchanged.

**Same-line trailing writer delegating to the bounded collector:** equivalent to the old scanner for all CR/LF input.
- It stops at the first CR/LF (a line comment is always followed by one or by the end).
- Unterminated block comments are clamped at `rest.end` by the slice.
- `last_comment_end` now also advances past a filtered **line** comment. The old code returned there, so the result is the same.
- It writes a space if the writer isn't at line start, the comment through `write_source_comment` on the full source (maps intact), and a line for `has_trailing_new_line`. That is TS's `emitTrailingComment`.

**Prefix trimmer:** it now skips every whitespace character except CR/LF.
- Its line-comment walk still runs to CR/LF, so `// a\u2028/* b */\n` is consumed whole. That is consistent with TS, where the previous node's trailing phase owns `/* b */` across U+2028.
- When no comment follows, the trivia is returned unadvanced, as before.

**Constructor selectors:**
- `builtins.rs:12005`: `!function_body_is_missing` (same `TypeScriptVisitor` impl). A missing constructor falls through to the next member, so the **first present** constructor wins, as in TS `find`.
- `statements.rs:1011` and `legacy_decorators.rs:2841`: the same present test, with correct `TransformNode` resolution.
- The decorator-eligibility `is_some` checks correctly remain (they port `nodeCanBeDecorated`).

## Nothing else needed before the planned runs

After the dead-code fix, the patch is ready for your planned controls and regression. The controls you listed cover every changed path:
- the 16 r168 shapes;
- the 6 trimmer neighbours;
- constructor present, missing then present, and the decorator/metadata trio;
- the BOM virtual-read correction, with the embedded/double-BOM controls and the `sys.readFile` proof.

One addition fits within the existing list: include a multi-line clause whose **last** statement carries a trailing comment, with `sourceMap: true`. It is the one clause-ownership shape the prior 952 didn't pin.
