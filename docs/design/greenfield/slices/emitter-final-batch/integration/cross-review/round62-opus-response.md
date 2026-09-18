I found one concrete error: formatting changes from a different rustfmt style that the pinned toolchain will reject. The three review areas themselves have no functional errors. I also found one small divergence from upstream in the printer's line-break check, and one missing control. This is a code read only, with no builds.

## Concrete error: formatting churn the pinned rustfmt will reject
The pinned toolchain is rustfmt 1.93 on edition 2021. The committed code already follows that style: lowercase names first in import lists, e.g. `{compute_line_starts, PositionIndex}` and `{parse_source_file, ParseOptions, SourceFile}`. The diff reorders these to the 2024 style (uppercase first) and reflows one expression the 2024 way:
- printer.rs:6-27, the three `use` blocks;
- tests/unit/bundle_printer/tests.rs, the import and the `referenced_collision_queries:` reflow;
- tests/unit/comment_scope_predicate/tests.rs, the import.

`cargo fmt --check` under the pin will flag all of these, and none of them relates to the fix. **Fix:** revert those hunks, or run the pinned `cargo fmt`. The imports added to system.rs and flatten_destructuring.rs already use the pinned style.

## 1. Flattener converter refactor: no drift
- `convert_to_*` and `make_{array,object}_assignment_pattern` now take the context and source. Each body is the same statement sequence as before: `with_original_and_range` on the spread, assignment, property-assignment, shorthand and literal nodes; the same `propagate_child_flags` sums; and the same `CONTAINS_ES_2015`/`2018`/`REST_OR_SPREAD`/`OBJECT_REST_OR_SPREAD` bits.
- These match upstream 20716-20787: `createSpreadElement`/`createSpreadAssignment`/`createShorthandPropertyAssignment` flags, with `setOriginalNode(setTextRange(…, element), element)`.
- `create_binary_in_context` computes the old `child_flags` fold, then applies the same EqualsToken pattern-flag arm. The generic wrappers, `create_flatten_{object,array}_pattern` and `array_nodes`, just forward. The default host hooks are untouched.
- **ESNext use** (es_next.rs:1085-1103): this matches `hoistInitializedVariable` (103664-103676). Patterns are converted and identifiers are cloned. The converted pattern reuses the parsed name, `propertyName` and initializer nodes, as upstream does. There is no sharing hazard: `hoist_binding_identifier` goes through `materialize_hoisted_binding_name`, which clones, so the hoisted `var`/export declarations never contain those nodes. The initializers come from the lowered statement (`lower_statement`), so they have already been visited, as upstream's `visitor(statement)` does before hoisting.

## 2. Printer: optional `?.`
This matches `emitPropertyAccessExpression` / `emitElementAccessExpression` (118223-118280):
- **Property access:** the line-break check before the dot now measures from the receiver to the `?.` token (upstream `getLinesBetweenNodes(node, expression, token)`). The check after it measures from the token end to the name. There is no dot-dot for `?.`. The token is emitted as a node with its own comment phase and start/end maps, and its trailing claim is carried forward to the name.
- **Element access:** `?.` is emitted as a node. `[` stays anchored at the raw receiver end, which matches `emitTokenWithComment(OpenBracketToken, node.expression.end)`. Two TS behaviours in the new fixture depend on this:
  - `/*b*/` is dropped in `source ?.[ /*b*/ 'x']`, because the `[` trailing scan starts one past `skipTrivia(expression.end)`, inside `?.`.
  - The comment is emitted twice in `source\n /*a*/ ?.[…]`: once as the leading comment of the `?.` node, and again by `[`'s `emitLeadingCommentsOfPosition(expression.end)`.

  The Rust structure reproduces both, because the returned `?.` emission is not passed to `[`. The native run will confirm.
- **Minor exactness divergence:** `preserve_source_lines` still requires the **name** to have a source range. Upstream's before-dot check tests the parent, the receiver and the **token**. Only the after-dot check involves the name. For a `?.` access whose name is synthetic but whose token is parsed, Rust drops the line break before the dot and upstream keeps it. **Fix:** base the before-dot check on `node_has_source_text_range(question_dot)` when the token exists, and keep the name in the after-dot check. No current transform rewrites the name of a `?.` access at ESNext.
- **Missing control:** a substituted receiver. Every optional control uses the local `source`. Add `import { value } from "./dep"; export const v = value /*a*/ ?. x; export const w = value\n /*a*/ ?.[ 'x' ];` at ESNext System. Upstream substitutes at print time and ranges the substitute to the original identifier; Rust substitutes early in the visitor. Comment ownership and mapping around `?.` then depend on that range.

## 3. Round-60 System changes
- **Hoisted `var` placement** (system.rs:752, 796): `standard_prologue_end` is recorded after the directives and any created `"use strict"`, and before the custom-prologue output. This matches `insertStatementsAfterStandardPrologue(statements, endLexicalEnvironment())` (112206 → 12919-12945).
  - The tslib hoist still follows the custom-prologue hoists (112204).
  - Functions from the custom prologue go into `hoisted_declarations`, which matches `hoistedStatements`.
  - `export const f = () => this;` and its variant with a destructuring binding are in the fixture.
- **Wrapper donor removal** (system.rs:867-869): this now matches 112124-112141. The wrapper has no range, no original and no flags; only the statement array is ranged to `node.statements`; and the SourceFile keeps NoTrailingComments. The old NO_LEADING/NO_TRAILING flags only mattered while the wrapper had a range, so removing them together is correct.
- **Using rewrite:** this matches 112639-112661. The declarations array is a new unranged array (upstream `createNodeArray(<plain array>)`), while the original list and statement are updated in place with their range and `using` flags kept, and export modifiers are removed. It only runs for SourceFile-hoisted `using`, as upstream's `shouldHoistVariableDeclarationList` requires. Multi-declaration and commented forms are in the fixture.

I found no missed edge cases in these three items beyond the substituted-receiver control above.
