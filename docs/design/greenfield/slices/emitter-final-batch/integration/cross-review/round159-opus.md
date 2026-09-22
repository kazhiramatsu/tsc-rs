The CaseBlock arm passes its received `expression_context` straight into `emit_case_block`. So whether the clause gets its own claim depends on the generic node pipeline, not on the arms.

# Round 159: constructor follow-ups, scanner plan, clause scope

This is from reading the source at `d2b29439d` plus `_tsc.js`. No edits or builds.

## A. Constructor follow-ups: fix exactly three selectors, all ports of `getFirstConstructorWithBody`

| Native site | TS counterpart | Current test | Action |
|---|---|---|---|
| `builtins.rs:12005` `prepend_parameter_property_members` | `transformClassMembers` (`_tsc.js:94567`): `getFirstConstructorWithBody(node)` | `data.body.is_some()` | **Fix.** Keep the loop's `continue` past a missing constructor (TS `find` semantics), then `break` on the first present one. |
| `declarations/statements.rs:1008` `first_constructor_with_body` | declaration `transformTopLevelDeclaration` (115551) | `is_some` | **Fix.** |
| `legacy_decorators.rs:2839` `first_constructor`. Its only caller is `constructor_handoff` (2859). | `getAllDecoratorsOfClass` (93137): `getFirstConstructorWithBody` | `is_some` | **Fix.** With the original selector returning `None`, the handoff returns `Ok(None)` and the `MissingTransformHandoff` error can't occur. The current-member `.is_some()` at 2863 needs no change: it is only reached when a present original exists, and the TypeScriptVisitor keeps that constructor. |

**Leave these three alone:**
- `legacy_decorators.rs:2928` (`class_element_has_body`);
- `legacy_decorators.rs:2962` (parameter-decorator owner);
- `builtins.rs:15929-15931` (`class_member_or_child_is_decorated`).

They port `nodeCanBeDecorated` and `childIsDecorated`, which test `node.body !== void 0` / `parent.body !== void 0` (`_tsc.js:14665`, `14668`). `is_some()` is already the exact TS test.

**The predicate** for all three fixes: present means the body exists and `!(pos == end && pos != u32::MAX && kind != EndOfFileToken)`. That is the same projection as the ES2015 fix.

**Open point.** TS `classOrConstructorParameterIsDecorated` (14692) also calls `getFirstConstructorWithBody`, and `shouldAddParamTypesMetadata` (94712) does too. I found no native port by name, so it may be folded into the class-facts code.

**Controls** (`experimentalDecorators` and `emitDecoratorMetadata`; ES2015 and ES5; JS and d.ts):
1. `constructor(public x: number) => 1;`: expect an empty class, and a d.ts with the signature but no `x` property.
2. `constructor(@dec x: number) => 1;`: expect no `__param`/`__decorate`.
3. `@dec class C { constructor(x: number) => 1; }`: the class decorator stays, but with no `design:paramtypes`.
4. Two constructors, missing then present (`constructor(public a) => 1; constructor(public b: number) {}`): TS takes the **second**, for both parameter properties and d.ts.
5. A present constructor with `public x` (unchanged).

If control 2 or 3 still emits `__param` or `design:paramtypes`, look for the class-facts site next.

## B. Scanner implementation plan

**Collector change:** correct as proposed. Only CR/LF (and CRLF) set `collecting` and end a trailing scan. U+2028/U+2029 are whitespace that set the pending comment's `has_trailing_new_line`.

**Replacing `emit_same_line_trailing_comments` with the collector over `&rest.source[..rest.end]`** is correct. Details checked:
- **Limit.** The slice bounds the scan at `rest.end` as before. `SourceTrivia` ends fall on trivia boundaries, so no comment is cut.
- **Position 0.** The collector's `position == 0` branch (collect from the start, skip a shebang) can't matter: trailing phases never start at 0 on a non-empty node.
- **Line-break flag.** In trailing mode, CR/LF break *before* the pending flag is set, so a block comment before CRLF gets no `write_line`. That is exactly TS's `if (trailing) break scan` ordering.
- **Writer.** This matches `emitTrailingComment`: a space if not at line start, the comment with `emitPos` on both sides, then `writeLine` if `hasTrailingNewLine`.
  - A line comment followed by CR/LF now gets `write_line` from the scanner. Callers that already call `write_line(false)` stay correct, because that is a no-op at line start.
  - Returning `last_comment_end` including filtered comments keeps the old resume contract.

**Required companion: `strip_same_line_comment_prefix`** (`printer.rs:19276`). It skips only `' '`/`'\t'` and stops at U+2028. Its callers are:
- `emit_source_file_statement_list_trailing_comments` (18129);
- `emit_empty_block_comments` (18231);
- `emit_comments_before_close_brace` (18348).

After the trailing fix, `function f() { x; /* a */\u2028/* b */\u2028}` would have `x`'s trailing phase emit both comments. The close-brace path would then trim only `/* a */`, stop at U+2028, and hand `\u2028/* b */\u2028` to the ASCII `emit_leading_comments`, **emitting `/* b */` twice**. TS emits it once, because its leading scan at `}` never starts collecting before CR/LF.

**Minimal fix:** in `strip_same_line_comment_prefix`, treat every non-CR/LF `is_whitespace_like` character (U+2028/U+2029 included) as same-line whitespace. The walk then stops at the first CR/LF, as TS's first-line rule does.

**Leave the ordinary DelimitedListStart `emit_leading_comments`** as you said. For the call case, the list-start intervening phase collects both comments, and the leading worker then resumes after them.

**Controls:** the three measured shapes, plus one per trimmer caller with maps:
1. Before `}` in a function: `function f() { x; /* a */\u2028/* b */\u2028}`
2. An empty block: `{ /* a */\u2028/* b */\u2028}`
3. File end: `x; /* a */\u2028/* b */\u2028` at EOF.
4. `x; // a\u2028y;`
5. The U+2029 variants.

## C. Clause scope: don't claim twice

- The CaseBlock arm passes its received `expression_context` unchanged into `emit_case_block`. That context should already carry the CaseBlock's claim from the generic node pipeline, as the r153 arrow fix relied on for its parent claim.
- Clauses are emitted through `emit_node_id_with_context`, which goes through the same pipeline. So the **Case/Default arms' own `expression_context.comments()` should already hold `containerEnd = clause.end`**.

**Recommended shape:**
1. **Verify with one witness:** in the Default arm, confirm `expression_context.comments()` retains the clause's end, and in `emit_case_block`, that its context retains the CaseBlock's end.
2. **If confirmed:**
   - in both statement branches, call `emit_trailing_comments_for_node_in_container(statement, expression_context.comments())`, using the arm's context;
   - in `emit_case_block`, after each `emit_node_id_with_context(clause)`, call `emit_trailing_comments_for_node_in_container(clause, expression_context.comments())`, using `emit_case_block`'s own context, i.e. the CaseBlock scope;
   - drop the `active_expression_comment_scope(None, …)` construction in `emit_case_block`.
3. **Only if the witness shows the pipeline does not claim for clauses**, claim inside the Case/Default arms, next to the node, not in the caller.

**Colon, both arms:** keep the `BoundaryUnion` leading phase and use `OwnerEnd` for the trailing phase. Delete the temporary escaped special case.
