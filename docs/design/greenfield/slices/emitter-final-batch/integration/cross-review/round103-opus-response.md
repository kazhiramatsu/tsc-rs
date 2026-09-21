Short answers: (1) your source-map repair is right, with four conditions; (2) I pinned the TS comment rules, fully explain the `{ /*body*/` difference, and narrowed the two `/*binding*/` differences to trace points; (3) nothing new; (4) the boxing diff is correct, with no API or cause issue. All of this is source reading and Node probes against the vendored TS; I made no edits and ran no builds.

## 1. Missing maps on grammar parentheses (the 9 callee/access/spread cases)

Your diagnosis holds. For the source-ranged kind of virtual paren, TS builds a real ParenthesizedExpression with `setTextRange(paren, expr)`. That paren goes through the normal pipeline: comments, then source maps, then emit.
- The opening map uses `getSourceMapRange(paren)`. The paren has no emit node, so this is its own raw range, the source is the current file, and the position is `emitSourcePos(skipSourceTrivia(pos))`. It is recorded **after** the paren's leading comments and **before** `(`.
- `(` and `)` are written by `emitTokenWithComment`, which records no token maps.
- The closing map at `end` is recorded **after** `)` and **before** the trailing comments.

The native source-ranged branch (`printer.rs:~14685`) writes `(`/`)` around the child's maps and records neither boundary.

Conditions for a faithful fix:
- **Range.** Use the raw `pos`/`end` of the node that goes inside the parens, not its `source_map_range` metadata. `setTextRange` copies only pos/end, and the new paren has no metadata of its own. In TS, the printer only parenthesizes **substituted** nodes (Substitution phase: `currentParenthesizerRule(lastSubstitution)`), so for those the raw range comes from the substitution result, as you wrote. Apply trivia skipping to the start side only.
- **Placement.** Record the Before map after the outer leading-comment phase and before `(`. Record the After map after `)` and before the outer trailing comments. Leave the inner comment phases where they are.
- **Flags.**
  - The child's `NoSourceMap`, `NoLeadingSourceMap`, `NoTrailingSourceMap` or `NoNestedSourceMaps` must **not** suppress the paren's maps; in TS the paren is the parent.
  - An **ancestor's** `NoNestedSourceMaps` must suppress them. In TS that sets `sourceMapsDisabled` before the paren's source-map phase is chosen. Reuse whatever state native ordinary nodes consult for this, for example inside a hoisted declaration.
- **Unranged branch unchanged.** New-callee with a call at the left edge, computed property name, assignment right side and export default all use `createParenthesizedExpression` without a range in TS. They have pos = -1 and record no maps.

Scope: the source-ranged branch is shared by several contexts. TS gives every one of them a ranged paren:
- left side of access, and the new/statement-callee access branch;
- prefix and postfix unary operands;
- disallowed comma;
- expression statement;
- arrow concise body.

So the shared fix is correct for all of them, but it changes maps beyond the nine rows. Rerun all the neighbour and boundary sets afterwards.

On printer versus factory: fixing the printer is the smallest faithful option. Moving the access and callee rules into `apply_parenthesizer_rules` would touch every update of an access or call. The only difference is that TS fixes the paren's range at the moment the factory runs, while native reads the child's final range. They differ only if a later transform re-ranges that child. In the observed chains the PEE keeps its range, so I don't expect a divergence.

## 2. For-await comments: the TS rules

Probe (`/tmp/r80/p.mjs`, same input as the failing cases):

**ES2015.**
- The binding statement's list has range [95,102] and is marked synthesized, because it was updated from the original list.
- It contains the **original** parsed `x` [100,102]. The list's end equals `x.end`.
- Because the list's range is valid, TS's comment phase sets `containerEnd` and `declarationListContainerEnd` to 102. That applies to synthesized nodes too.
- `x`'s trailing comments at 102 are therefore suppressed. `/*binding*/` is printed only by the statement (range [95,102]) after `;`.
- The `=` token goes through `emitTokenWithComment(=, name.end, …, decl)`. Its leading scan at `pos > 0` only collects comments after a newline, so nothing is printed there.
- Native prints `x /*binding*/ =`, which is one of two failures:
  - **(a) Name trailing comment not deduped.** The declaration-list claim at `printer.rs:4515` isn't producing end 102 for this updated list. `comment_range_for_node` itself looks correct, since it falls back to the raw range.
  - **(b) `=` leading comment collected wrongly.** `emit_space_prefixed_token_with_comments` collects same-line comments where TS's newline rule collects none.
  - The ES2017 target fails the same way, so the ES2015 transform isn't involved.
  - To tell them apart: when `x` is emitted, check whether the active scope's `declaration_list_container_end` is 102. If it is, the cause is (b).

**ES5.**
- The generator's hoisted `var …, x, …` uses the **original** `x` [100,102] inside a synthesized [-1,-1] list, so nothing claims 102 and TS prints `x /*binding*/`. This matches `hoistVariableDeclaration(variable.name)`.
- The body's `x = _c` uses a synthesized clone with no comment range.
- Native's hoist sites (`generators.rs:880-893`, and the clone plus comment-range path at 1041) and the environment builder at `:4317` match TS. Neither is the cause.
- Check first whether the hoisted `TransformNode` is still the parse node [100,102] with no comment-suppressing flags. If it isn't, find where ES2015 or es2018 replaces it. If it is, the printer is suppressing it, so look for a container claim leaking into that scope.
- Don't fix this by setting or clearing NoComments: TS prints this comment twice, once in the hoisted list and once after the body statement.

**`{ /*body*/` (ES2015/17).**
- The new loop body block has range [136,164] but **no original**, so `getParseTreeNode(block)` is undefined.
- In `emitTokenWithComment(OpenBrace, …, block)`, `isSimilarNode` is therefore false, so no comments are printed around `{`.
- `/*body*/` is also not a leading comment of `fn(x);`: it's at `pos > 0` on the same line.
- The TS rule is that token comments need a parse-tree node of the same kind, found through `original`. Having a text range is not enough.
- Native's brace-token path must apply that identity test and must not treat `set_text_range(body, visited_body)` as provenance.

## 3. Object-rest pre-pass

No new concerns. The single computed-key rest case (`const {[k()]: a, ...r}`) is enough to exercise where `visitIterationBody` puts hoisted temps.

## 4. Lint-prep boxing diff (staged): no API or cause issues

- **Checker.** Construction uses `diagnostic.map(Box::new)` at the single construction site, and the single consumer unboxes with `.map(|d| *d)`.
- **`PreparationError`.** It uses `Box::new` at `from_resolution_js`. `resolution()` uses `as_deref()`. `Error::source` uses `as_deref()` before the `dyn Error` cast, so downcasting still works.
- **`ProgramLoadError`.**
  - Only the two private constructors box.
  - `Error::source` uses `Some(source.as_ref())` for Host and Resolution. The only `AsRef` implemented by `Box<T>` is `AsRef<T>`, so this returns `&HostError`/`&ResolutionError`.
  - `Decode` and `Preparation` are unchanged.
- **New test.** It downcasts Host, Resolution, the nested Host cause of a Resolution, and a Preparation that carries a resolution. Every item it references exists. `ResolutionError::source` returns the Host cause, so the nested `is::<HostError>()` assertion holds.
- **The nine test patterns.** The `*source` comparisons keep their assertions. The nested pattern is now two `let … else` bindings with the same panic message. `matches!(*source, … ref feature …)` borrows rather than moves.
