Confirmed: `emit_case_block` runs leading comments and the node for each clause, but **no clause-end trailing phase**. So the colon's arithmetic trailing phase is the only thing standing in for TS's clause-node trailing comments.

# Round 156: escaped `default` loses the comment after its colon

This is from reading the source at `3e28cb213` plus the decoded r186 record. No edits or builds.

## TypeScript's exact phase

The source is `def\u0061ult: // Error, fourth 'default' clause.`, a clause with no statements. Let S be the keyword start.

1. **`emitDefaultClause`** (`_tsc.js:119482`) calls `emitTokenWithComment(DefaultKeyword, node.pos, …)`, which returns the **arithmetic** `S + "default".length = S+7`. That position is inside the escape `\u0061`.
2. **`emitCaseOrDefaultClauseRest`** (119486): there are no statements, so it takes the list arm, `emitTokenWithComment(ColonToken, S+7)`.
   - `skipTrivia(S+7)` stops immediately (the character is `6`), so there are no leading comments.
   - It writes `:` and gets `pos = S+8`. Because `contextNode.end (S+13) ≠ S+8`, it runs `emitTrailingCommentsOfPosition(S+8)`, which scans from `1` inside the escape and finds **nothing**.
3. **The clause node's own trailing phase** (`emitCommentsAfterNode`) then runs at `clause.end = S+13`, right after the real `:`. That emits ` // Error, fourth …`.

For an ordinary `default: // c`, the colon's arithmetic end equals `clause.end`. The colon's trailing phase is skipped (`end === pos`), and the clause phase emits the comment at the same position.

## Native's exact phase

- **The DefaultClause arm** (`printer.rs:6876` at `3e28`) emits the keyword with `emit_token_with_comments` from `original_node_start_cursor`. Its returned cursor is also the arithmetic `S+7`, which matches TS.
- **The colon** uses `emit_list_boundary_token_with_comments` (17442), with `TokenCommentBoundary::AdjacentListItem`.
  - With AdjacentListItem, the colon's trailing phase runs **even when `owner.end == token_end`**. It runs at `token_end = S+8`, and `collect_source_comment_ranges(…, S+8, true)` finds nothing, just as in TS.
- **`emit_case_block`** emits each clause's leading comments and then the node, but has **no clause-end trailing phase**.
  - Native deliberately delegates the clause-end boundary to either the colon's AdjacentListItem trailing phase (empty clause) or the last statement's own trailing phase.
  - That delegation is exact only while the colon's arithmetic end equals `clause.end`. An escaped keyword makes the spelling shorter than its source width, the two positions diverge, and nobody visits `clause.end`.

This isn't a parser or source-range problem, and the arithmetic cursor itself is correct. TS keeps it for leading comments and for the single-line-arm colon map (`colon_map_default`). The missing piece is the clause-node trailing phase.

## Minimal fix: restore TS's clause trailing phase only where the delegation breaks

In the DefaultClause arm, and in CaseClause for symmetry, after `emit_case_clause_statements`:
- **If** the clause has no statements,
- **and** the node has source-token shape (`node_has_source_token_shape`),
- **and** the colon's returned source cursor position ≠ the clause's raw `end`,
- **then** run the clause-owned trailing phase at the clause end: `emit_trailing_comments_for_node(transformation, node, writer)`. That is the ordinary same-line trailing phase at the node's comment-range end, honouring `NO_TRAILING_COMMENTS` and comments-disabled.

**Why this scope is safe:**
- When the positions are equal (every unescaped clause), the colon phase already covered `clause.end`. The guard keeps every current output byte-identical and can't produce a duplicate.
- The arithmetic keyword and colon cursors, the leading phases and the colon map aren't changed.
- **Clauses with statements need no change.** With statements, TS's colon trailing scan at the arithmetic `S+8` finds nothing. A same-line `// c` after an escaped `default:` is then the first statement's *first-line* trivia, which TS's leading scan skips, so TS drops it. Native's colon phase at `S+8` also finds nothing, and the first statement's leading phase uses the same first-line rule, so it should drop it as well. Pin that with control 4 below rather than assuming it.

## Controls (JS with `sourceMap: true`; removeComments false and true)

1. The original `switchStatementsWithMultipleDefaults.ts`: the complete command must be exact.
2. `switch (x) { def\u0061ult: // c\n }` (escaped, empty clause, line comment).
3. `switch (x) { def\u0061ult: /* c */ }` (block comment, same line).
4. `switch (x) { def\u0061ult: // c\n    x++; }` (escaped, with statements; expect the TS drop).
5. `switch (x) { def\u0061ult: x++; // c\n }` (single-line statement arm).
6. `switch (x) { c\u0061se 1: // c\n }` (escaped `case`; its colon cursor comes from the expression end, so the guard should not fire).
7. `switch (x) { def\u0061ult /*a*/ : // c\n }` (comment between keyword and colon; TS's arithmetic leading scan at `S+7` finds nothing).
8. Ordinary `default: // c` and `case 1: // c`, empty and non-empty. These must be byte-identical to today, maps included.
