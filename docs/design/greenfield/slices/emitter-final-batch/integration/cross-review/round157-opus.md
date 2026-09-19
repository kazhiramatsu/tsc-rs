The `record_node_map_boundary(After)` for the clause runs after the worker returns (`printer.rs:15402` ff.). So the new comment is written before the clause's map-end record. That is the same writer sequence the existing ordinary path already produces through the colon's AdjacentListItem phase.

# Round 157: review of the follow-up diff (`3e28cb213..worktree`)

I read the source only; no builds. There are no blocking defects. Two statements in the change notes need correcting, and each needs one control.

## 1. Unicode detached discovery (`detached_leading_trivia_end`)

**Correct:**
- It uses `collect_source_comment_ranges(…, false)` for the TS comment set, and groups with `contains_two_line_breaks` (CRLF counts once; U+2028/U+2029 count).
- It requires two breaks between the last grouped comment and `code_start`.
- `emitted_end` is the end of the `is_whitespace_like` run after the last comment. For ASCII gaps this is the same position the old cursor reached. `resume_next` still comes from the collector's last comment.

**Correction: it is not byte-identical for all ASCII inputs.** The old scanner counted every comment from `start`. The collector applies TS's first-line rule at `pos > 0`: comments on the owner's opening line are *not* leading comments. That differs for non-file (node-array) owners:
- `{ /* a */\n\n/* b */\n\n x; }`
  - **Old:** the group was `a` alone, ending at the first blank line. `resume_next` then found no collectable comment, so there was **no** detached prefix, and `b` went through the ordinary leading phase.
  - **New:** the collector yields `[b]`, `b` is followed by a blank line, so `b` is detached.
  - **TS:** `getLeadingCommentRanges(text, node.pos)` also skips `a`, so the new behaviour is TS's. It can still change output.
- **Control:** `function f() { /* a */\n\n/* b */\n\n  x; }` and the same inside a namespace body, with maps. Beyond that, the full original JS replay you already planned is required, not only the new controls.

## 2. ES2015 `get_first_constructor_with_body`: correct

The present test is `pos == u32::MAX || pos != end || kind == EndOfFileToken`, exactly TS's `!nodeIsMissing`. A synthesized body counts as present. A class-fields rebuilt Block that copied the zero-width range counts as missing, which is the TS quirk in r155's fields control. The single caller is unchanged.

## 3. Empty-clause trailing completion: guard correct; placement matches the existing ordinary path

**Placement relative to maps and comments:**
- In TS, the clause's `emitCommentsAfterNode` runs **after** its source-map phase (`emitSourceMapsAfterNode`).
- In native, `emit_substituted_node_with_comments` records `record_node_map_boundary(After)` after the worker returns. So the new comment is written *before* the clause's map-end record.
- That is exactly where the **ordinary** empty clause's comment is written today: the colon's AdjacentListItem trailing phase runs inside the worker. Ordinary `default: // c` with maps is exact in the existing controls, and the escaped case now produces the identical writer sequence.
- So parity holds exactly when the ordinary path's parity holds. Confirm that ordinary `default: // c` and `case 1: // c` **with `sourceMap: true`** are among the r193 shapes and exact. If they ever diverge, fix both at the clause-list level in `emit_case_block` after the node's map-end, not in this helper alone.

**No duplication, and the guard can't fire in the other shapes:**

| Shape | Colon's returned end vs `clause.end` | Guard |
|---|---|---|
| ordinary `default:` | equal | doesn't fire |
| `default /*a*/ : // c` | the colon re-skips trivia from the keyword end, so its end is the real colon end, equal to `clause.end` | doesn't fire |
| CaseClause (colon from the expression end, then skip) | equal | doesn't fire |
| synthesized or transformed clauses without source shape | `node_has_source_token_shape` is false, or the cursor is synthetic (`source_position` is `None`) | doesn't fire |
| clauses with statements | not empty | doesn't fire |

**Malformed missing colon (`default // c` + `}`):** the guard *would* fire, at `clause.end` = keyword end, and that matches TS's clause trailing phase. Reaching it needs a report-only `';'`/`':' expected` Parser event with no missing node, and none of the current owner rules (flush ExpressionStatement, terminator, class-body gap) covers a clause. So it is unreachable under current admission. Check that no input in the r185 selection is admitted with such an event rather than assuming it.

## 4. Deleting `write_comment_with_normalized_newlines`

Its only use was the triple-slash writer, which r153's `write_source_comment` replaced. The deletion is correct.

## Must-have controls within your planned 944

1. The non-file first-line case from §1 (JS with maps; removeComments false).
2. Ordinary `default: // c` and `case 1: // c` with `sourceMap: true` (§3 parity anchor).
3. The original `switchStatementsWithMultipleDefaults.ts` complete command.
4. The r155 fields and two-constructor ES5 shapes (already minted).
