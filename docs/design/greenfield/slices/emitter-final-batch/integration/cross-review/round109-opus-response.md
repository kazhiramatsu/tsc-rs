None of the three failures comes from the r99–r106 changes. All three causes are already ancestors of `c1e699ab5` and were only exposed now, because this is the first full integration-contract run. The test expectations are correct: pinned TS prints `this?.x.y` and a single space before each comment. Each failure has a narrow printer fix, and I would not touch the shared spacing helper. I made no edits and ran no builds; the only probes were Node against the vendored TS (`/tmp/r80/u.mjs`).

## Pinned TS output

`createPrinter({removeComments:false}).printFile(...)` on the vendored 6.0.3, for the exact test inputs:
- `import /* 取込KW😀 */ { foo /* 取込別称🍀 */ as bar } /* 取込FROM🌊 */ from …` — one space before each comment.
- `/*a*/ do /*b*/ { } /*c*/ while …` — one space.
- `this?.x.y`, `this?.[key].y`, `this?.x()`, `typeof this?.x`, `this?.x.y();` — no parentheses.

`transpileModule` gives the same do/while output. Don't change any expectations.

## Failure 1: `this(?.)x.y`

**Cause.** `emit_ordinary_token_after` (`printer.rs:~14191`, introduced as `emit_access_question_dot` in 77fed35b4) sends the `?.` token node through the normal node pipeline with the **parent access's** `expression_context`.
- In `this?.x.y`, the inner access is the left side of the outer one, so its context carries `LeftSideOfAccess { optional_chain: true }`.
- The grammar dispatch (`~14655`) then runs `left_side_of_access_requires_parentheses` on the `QuestionDotToken`. A token isn't a left-hand-side kind, so it gets a source-ranged paren.
- In TS, `emit(node.questionDotToken)` is a plain emit with no parenthesizer rule.
- The same bug fires for `PrefixUnaryOperand` (`typeof a?.b.c`), `NewCallee`, and `ExpressionStatementCallee`. It also affects the yield `*` token, which shares this helper.

**Fix, one helper.** In `emit_ordinary_token_after`, use `expression_context.for_child(ExpressionSyntaxContext::NORMAL)` instead of `expression_context` for both the nested-suppressed `emit_node_with_hint` call and the main call.
- This keeps the comment scope and the nested-comment suppression, and drops the parent's grammar and no-ASI flag. Neither applies to the token.
- Leave the deferred comment anchors (`preceding`, `parent`) as they are.

**Impact.** The only effect is removing the bogus paren, along with the paren map boundaries from b21bef2b7 that it carried. That output was wrong JS, so none of the 1,930 passing complete commands can depend on it. Optional chains only reach this path at ES2020 and later targets.

## Failures 2 and 3: two spaces before trailing comments

**Cause.** Four call sites write the separator space **before** calling a `BoundaryUnion` token that also stands in for the previous child's trailing-comment phase:

| Site | Line | Code |
|---|---|---|
| import clause → `from` | `3869` | `write_space` + `emit_space_prefixed_token_with_comments(from, end(clause))` |
| export `{…}` → `from` | `4202` | same pattern |
| specifier → `as` | `13262` | same pattern |
| DoStatement block → `while` | `~7082` | `write_space`, then `emit_while_clause` (`9541`), which emits `while` the same way anchored at `end(statement)` |

- Before be917acbf and 287b7a62f, `!has_trailing_whitespace()` in the trailing-comment helpers hid this wrong order.
- Those commits made the helpers match upstream (`emitTrailingComment` always prefixes a space when the line has content) for the r39 case of an empty recovery operand. So the helpers are now right and these callers are wrong.
- TS order: `emit(child)` runs the child's trailing phase (`" /*c*/"`), then `writeSpace()`, then `emitTokenWithComment`, which only emits comments that come after a newline.

**Fix.** Don't revert the helpers, and don't just delete the space. Deleting it would print `{ }/*c*/ while` when the comment comes after a newline, because TS writes the space before those comments. Use the pattern `else`/`finally` already use (`6522`, `7157`):
1. `let anchor = self.emit_trailing_comments_for_node_as_token_anchor(transformation, child, writer)?;`
   - `child` is the clause, the export clause, the specifier's `property`, or the do statement's body.
   - This emits the same-line trailing comments and returns an anchor with a resume point.
2. `writer.write_space(" ")`, as today.
3. Pass `anchor` to the existing token call instead of the plain `original_node_end_cursor`. For do/while, `emit_while_clause` needs to accept a `TokenAnchor`, or you add a thin variant. With a resume point, `BoundaryUnion` skips the comments already written (`emit_comments_at_cursor_with_phase` only emits trailing comments when there is no resume) and still emits the after-newline comments after the space.

Scope: only the do-while **block** branch. The non-block `write_line_or_space` branch is a separate case; note it below.

**Impact.**
- Output changes only where a same-line comment sits between the child and the token.
- The comment position doesn't change, so its source map is the same. The only map effect is the generated column shifting one left after the removed space.
- The shared helpers are untouched, so the r39 async/recovery complete commands keep their upstream double space.

## Neighbour cases to add (observe TS first; all small)

- **Spacing, three per site:** a same-line trailing comment, a comment after a newline, and both at once.
  - `do {}\n/*l*/ while (x);`
  - `do {} /*t*/\n/*l*/ while (x);`
  - `import { a } /*t*/\n/*l*/ from "m";`
  - `import d /*t*/ from "m";`
  - `export { a /*t*/ as b } from "m";`
  - `import type { a } /*t*/ from "m";`
- **Non-block do (record only):** `do x(); /*t*/\nwhile (y);` — same pattern but not fixed by this change.
- **Unchanged-path controls:**
  - `import * as ns /*t*/ from "m"`
  - `export * /*t*/ from "m"`
  - one r39 empty-recovery-operand case
- **Optional chain:**
  - `typeof a?.b.c`
  - `void a?.b`
  - `a?.b.c();` as a statement
  - `new (a?.b)()`
  - `a?.b!.c`
  - `a /*1*/ ?. /*2*/ b.c` with comments
  - an ES2020 `sourceMap:true` case covering the `?.` token map
- **Yield:** `(yield* g()).x` and `yield /*a*/ * /*b*/ x` — the same helper, so the existing yield controls stay valid.

Noted that the r108 ledger comments are approved and that Census 56011 is running; I didn't touch the shared target.
