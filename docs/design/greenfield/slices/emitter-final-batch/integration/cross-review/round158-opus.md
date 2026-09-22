# Round 158: clause map ordering, Unicode, and BOM

This is from reading the source at `d2b29439d`, the r194 partial captures, and the vendored `_tsc.js`. No edits or runs. I agree with Root: the ordinary path was never a parity anchor, and my r157 assumption was wrong.

## 1. Clause trailing comments and source maps

**The measured difference.** In `ordinary-default-empty-line`, TS line 3 is `IAAC,QAAQ,CAAC,IAAI`: the clause's map-end at the column after `default:`, then the comment's start and end. Native is `IAAC,SAAS,IAAI;IAAL`: the comment first, then a clause map-end at the start of the next line. `escaped-default-single-statement` has the same extra `IAAL`.

**The TS protocol:**
- The clause's `emitCommentsBeforeNode` claims `containerEnd = clause.end`.
- The colon's `emitTokenWithComment` runs a trailing phase only if `contextNode.end !== pos`.
- A statement whose `end === containerEnd` has its trailing comments suppressed.
- The clause runs `emitSourceMapsAfterNode` and **then** `emitCommentsAfterNode`, whose trailing phase is at `clause.end`.

**The native divergences:**
- **(a) Colon.** It uses `emit_list_boundary_token_with_comments`, i.e. `AdjacentListItem`. That runs the colon trailing phase **even when `owner.end == token_end`**, so an empty clause's comment is written inside the node worker.
- **(b) Statements.** `emit_case_clause_statements` calls plain `emit_trailing_comments_for_node(statement)` in both the single-line and multi-line branches, with no container check. So the last statement's comment (`end == clause.end`) is also written inside the worker.
- **(c) Map order.** `emit_substituted_node_with_comments` records `record_node_map_boundary(After)` only after the worker returns. Anything written in (a) or (b) therefore lands before the clause map-end.
- **(d) No clause phase.** `emit_case_block` has no clause-level trailing phase.

**Minimal changes** (no cursor, AST or map compensation):
1. **Colon (both arms):** replace `emit_list_boundary_token_with_comments` with `emit_token_with_comments_at_boundary(…, TokenCommentBoundary::OwnerEnd, TokenLeadingSpace::None, Some(PositionCommentPhase::BoundaryUnion), …)`.
   - `AdjacentListItem` and `OwnerEnd` differ only when the colon end equals `clause.end`, i.e. empty clauses. Non-empty clauses keep their colon trailing phase and `comment_resume` for `token_owned_child_prefix`.
   - For escaped clauses, the colon still scans at its arithmetic end and finds nothing, as in TS.
2. **Clause scope for statements:** in both arms, build the clause-claimed context the way the VariableStatement arm does (`expression_comment_phase_owner_for_node` → `established_container_sides` → `claim_sides`). Pass that scope into `emit_case_clause_statements`.
   - Replace both `emit_trailing_comments_for_node(statement)` calls with `emit_trailing_comments_for_node_in_container(statement, clause_scope)`.
   - This suppresses exactly the statement whose end equals `clause.end`, TS's rule. Earlier statements are unchanged.
3. **`emit_case_block`:** right after `emit_node_id_with_context(clause)`, call `emit_trailing_comments_for_node_in_container(clause, <case-block scope>)`. That scope is the one the CaseBlock arm receives with its own sides claimed; `clause.end` is never the CaseBlock end.
   - Because this runs after the clause's `After` map record, the map order becomes TS's.
   - Synthetic trailing comments already come before source trailing comments (`emit_synthetic_trailing_comments_for_node` runs inside the node pipeline after the map). That also matches TS's `emitTrailingCommentsOfNode` order.
4. **Delete the temporary escaped special case** in `emit_case_clause_statements`. Change 3 covers escaped and ordinary, empty and non-empty.

**Transformed and synthetic clauses:**
- Synthesized clauses (for example from the generators transform) have `u32::MAX` comment ranges. The clause phase emits nothing, and a colon without source-token shape emits no token comments, as TS does with `isSimilarNode === false`.
- Updated clauses keep their raw range, so the phase uses the original end, again as TS does.

**Controls** (with maps, removeComments false and true, at minimum ES2015 and ES5/System):
1. Empty ordinary `default: // c` and `case 1: // c` (the failing ones).
2. Empty escaped `default`/`case`, and the intervening-comment shape.
3. Single-line `default: x++; // c` and `case 1: x++; // c`.
4. **A multi-line body whose last statement has a trailing comment:** `case 1:\n  a();\n  b(); // c\n`. This is the same latent defect. Most original corpus cases don't enable source maps, so they couldn't reveal it.
5. A comment after a **non-last** statement (must stay on that statement).
6. An empty clause immediately followed by another clause, and the last clause before `}`.
7. The original `switchStatementsWithMultipleDefaults.ts`.

## 2. Unicode: TS never lets U+2028/U+2029 start collecting or end a trailing scan

**TS source:** `iterateCommentRanges` (`_tsc.js:8491-8585`). Only CR/LF (8510-8524) set `collecting = true`, and only they `break scan` in trailing mode. U+2028/U+2029 fall to the default branch (8571-8577): whitespace-like, marking `pendingHasTrailingNewLine` for a pending comment, and **continuing** without enabling collection or ending a trailing scan. Line comments still end at any `isLineBreak`.

**Why each measured case follows from that:**
- **Function/namespace body** `{\u2028/* a */\u2028\u2028return 1; }`: the `statements.pos > 0` scan never starts collecting, so TS has no detached or leading comment and drops `/* a */`. The source-file header works because `pos === 0` starts collecting.
- **`f(/* a */\u2028/* b */\u2028x)`:** the list-start trailing-of-position scan collects **both** comments, each with `hasTrailingNewLine`, so each is followed by `writeLine`. `x`'s leading scan collects nothing.

**Native owners:**
- **(i) The collector.** `collect_source_comment_ranges`'s line-break branch treats every `is_line_break` like CR/LF: it sets `collecting` and breaks trailing scans.
- **(ii) A second scanner with the same divergence.** `emit_same_line_trailing_comments` (`printer.rs:19167`) stops at any `is_line_break`, used by node trailing phases.

**Fix: change both together.** In both functions, only `\r`, `\n` and CRLF get today's line-break handling. U+2028 and U+2029 become whitespace that sets the pending comment's `has_trailing_new_line`.
- Fixing only the collector would *drop* `/* b */` in `x; /* a */\u2028/* b */\u2028y;`. It would no longer be a leading comment of `y`, and the unchanged trailing scanner would still stop at the first U+2028.
- The writers already honour `has_trailing_new_line` (`emit_source_intervening_comments_of_position` and `emit_source_trailing_comments_of_position_with_filter` write a line).
- Round 154's detached discovery stays correct: grouping uses `contains_two_line_breaks` (TS's line map, where U+2028 *is* a line), and file headers at `pos == 0` still collect.

**Controls:**
1. The three measured shapes.
2. `x; /* a */\u2028/* b */\u2028y;` (statement trailing across U+2028).
3. `x; // a\u2028y;` (line comment ended by U+2028).
4. The U+2029 variants.
5. Existing CR/LF/CRLF and file-header Unicode controls (must stay exact).

## 3. BOM column: an input-protocol mismatch, not a printer bug

- **TS observer:** its host (`observe-emitter-r168-corpus-controls.mjs:120-127`) returns the fixture string verbatim, including `\ufeff`. So TS positions the comment at column 1.
- **Native:** it reads sources through `decode_host_text` (`program/src/text.rs:80`, a port of `sys.readFile`, called from `loader.rs:2265`), which **strips** a UTF-8 BOM. Positions are 0-based without it.
  - Later lines agree (`var` at column 16 on both sides). Only line-0 columns differ, which is exactly the observed diff.
  - `source_comment_utf16_location` is correct for the text it's given. `calculate_source_indent` affects comment text layout only, never map columns, so Root's suspect isn't the cause.
- **Real `tsc` behaves like native** here: `sys.readFile` strips the BOM. The original-corpus harness also strips it (`harness/src/lib.rs:746`).
- **Recommendation:** don't touch the printer or shift coordinates. The control compares two different input decodings, so it can't serve as qualification evidence as minted. The correct fix is in the observer protocol: strip a leading U+FEFF in the in-memory `read`, matching `sys.readFile`, and re-mint that one shape. That is a fixture change for you and Root to decide on. Until it is fixed, record it as an input-protocol mismatch, not an emitter failure.

## Scope

- The clause change affects only CaseClause/DefaultClause trailing ownership.
- The Unicode change affects only how the two scanners treat U+2028/U+2029.
- The listed controls are the minimal neighbours. The multi-line last-statement comment is the one new shape the current 952 don't cover.
