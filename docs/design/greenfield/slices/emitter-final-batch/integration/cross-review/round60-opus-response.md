The flattener hooks and the System destructuring port match upstream, and the new TS observations show no helper carry between files, so no carry design is needed. Of the three r55 fixes, the CustomPrologue one has an ordering bug. The residual map failures have three separate causes, and the pattern-after-using one is a real semantic bug, not just a map difference. All of this is from reading code, fixtures and the r55 log; nothing was built or run.

## 1. Flattener hooks and System destructuring port

**The hooks are neutral for ES2015.**
- The default `allocate_flatten_temp` is the old body unchanged.
- `complete_flattened_assignment` runs after the range and original are set, only on the path without a callback, so its identity default changes nothing.
- `request_flatten_helper` defaults to `request_emit_helper`.

**The predicate matches `hasExportedReferenceInDestructuringTarget` (113118-113141).**
- A non-compound `=` recurses into its left side.
- An array spread, shorthand name and property-assignment initializer each recurse.
- An Identifier counts only when its export container is the SourceFile.
- An object `SpreadAssignment` and any other node are false.
- Generated identifiers get no parse-tree node, so they are false.
- The entry test uses the left side before visiting and passes `!value_is_discarded`, as `visitDestructuringAssignment` (113106-113116) does.

**Publication is correct.** `complete_flattened_assignment` calls `exports_for_identifier` on the pattern target before it is visited. That covers the generated, LOCAL_NAME, synthesized and import-first gates, and returns nothing for non-identifier targets. The wrap goes around the assignment that already carries the range and original, which is the eager equivalent of the print-time `substituteBinaryExpression`. The flattener only visits its inputs, never the assignments it creates, so nothing is published twice.

**Temps.** At module level, the temp name comes from `next_temp_name`, is pushed to the hoisted names, allocated with `allocate_planned`, and registered in the map, so the wrapper `var` carries the same identity. Nested temps use the same function environment as postfix.

**Late helpers: no carry.** In all 16 two-file rows (`late-{read,rest}-{binding,assignment}/second-true`, with importHelpers on and off, at ES2015 and ESNext), `after.js` contains no helper.
- ESNext's late `__rest` and `__read` are referenced but never defined.
- ES2015's `__rest` is defined because ES2018 requests it before System runs.
- Keep the no-op. No carry design is needed.

## 2. The three applied r55 fixes

1. **CustomPrologue order: needs one more fix.** Visiting the prefix before the tslib import is right (`copyPrologue`, 24827-24869, then 112204). The results going into the outer wrapper is also right. But upstream adds the hoisted `var` last, through `insertStatementsAfterStandardPrologue(statements, endLexicalEnvironment())` (112206 → 12919-12945). That puts it straight after the directives, **before** the custom-prologue output. Rust pushes it after (system.rs:794).
   - This shows whenever the custom prologue produces a statement. Example: ES2015's `var _this = this;` capture (CustomPrologue) becomes `_this = this;`. Upstream prints `"use strict"; var _this, f; _this = this; var __moduleName…`.
   - **Fix:** record `standard_end = outer.len()` right after the use-strict step, and insert the hoisted var statement at that index instead of pushing it.
   - **Control:** `export const f = () => this;` at System ES5, with importHelpers off and on.
2. **Namespace import publication: correct.** `getDeclarationName` → `getName` (24788-24797) clones the name with NoSourceMap and NoComments. `substituteExpressionIdentifier` (113265-113298) only substitutes ImportClause and ImportSpecifier, which yield a ranged `m_1.x` or `m_1.default`. A namespace stays as the unmapped name.
3. **Using statement update: correct in intent** (112639-112661: `updateVariableStatement(node, <modifiers without export>, updateVariableDeclarationList(list, decls))`).
   - One detail differs. `updateVariableDeclarationList` → `createVariableDeclarationList(decls)` → `createNodeArray(<plain array>)` gives an **unranged** declarations array. Rust's `update_node_array` keeps the original array range.
   - Prefer an unranged `create_node_array`.
   - **Control:** `export using /*c*/ r = source, /*d*/ s = source;` at ESNext.

## 3. Residual map failures

In the r55 log, the left side is native and the right side is TS; the fixtures confirm this.

### A. ESNext optional-dot / optional-element (8 rows)
- **Evidence (optional-dot):** TS maps `?.` at source 62 (token start) and 64 (token end). Native maps 61 (after `/*a*/`) and emits no end segment.
- **Upstream:** `emitPropertyAccessExpression` (118223-118250) uses `emit(node.questionDotToken)`, a full node emission. Its start map is `skipTrivia(token.pos)` and its end map is `token.end`.
  - A plain `.` goes through `emitTokenWithComment`, which writes the token without a map of its own.
  - `emitElementAccessExpression` and `emitCallExpression` also use `emit(node.questionDotToken)`.
- **Rust:** printer.rs:7471-7530 sends `?.` down the `emit_token_with_comments` path with the comment-resume anchor as its position.
- **Fix:** in the PropertyAccess, ElementAccess and Call optional arms, keep the current comment handling. Add source positions taken from the question-dot token node itself: start at the token start (skipping trivia), end at `token.end`, respecting the token's emit flags.
- **Optional-element also has a text difference.** Native puts `'x'` one column later (31 against 30) and every later segment shifts. Source maps are compared before JS bytes (h2_7b_w4a_controls.rs:414), so the JS almost certainly differs too. Confirm with a focused run.
  - Upstream emits `[` through `emitTokenWithComment(OpenBracketToken, node.expression.end)`. Its position is `skipTrivia(expression.end)`, which here is the `?.` start. It emits no map, and its trailing-comment scan starts one character later, inside `?.`.
  - Port that literally rather than anchoring `[` after `?.`.
- **Controls:** `a /*a*/ ?. /*b*/ b`, `a ?.[ /*b*/ 'x' /*c*/ ]`, `f /*a*/ ?. ()`, and the same without comments.

### B. namespace-export (6 rows)
- **Evidence:** native adds `0:0 → 1:0` and `13:3 → 1:47`, which are the start and end of the `System.register(…)` statement, mapped to the namespace.
- **Cause:** system.rs:867-873 gives the wrapper the original and range of `first_runtime_declaration_original` (added in H2.2b, 6a7a95ec2). Upstream (112124-112141) creates the wrapper with no range and no original; only the statement array is ranged, to `node.statements`.
- Rows with `export {}` first pass because there is no donor. Here `declare const` is removed, so the namespace becomes the first runtime statement.
- None of the 36 TS-observed System namespace/enum module rows maps the wrapper.
- **Prediction:** the new `namespace-member-update` rows in system-binding-publication will fail the same way.
- **Fix:** remove the donor for System.
- **Regression check:** re-run the H2.2b System namespace/enum rows. If one needs it, find its actual upstream mechanism rather than restoring the donor.
- **Controls:** a namespace at 0:0, a namespace after an ambient declaration, an enum first, and a namespace with a leading `/** doc */`.

### C. pattern-after-using, ES5 and ES2015 (4 rows): semantic bug
- **What native emits:** the maps decode to `({ x, y } = source);`, not flattened and not published. TS emits `(exports_1("x", x = source.x), exports_1("y", y = source.y));`.
- **Upstream:** `hoistInitializedVariable` (103664-103676) clones only identifiers. Binding patterns go through `factory.converters.convertToAssignmentPattern` (20748-20790), which produces an ObjectLiteral/ArrayLiteral with shorthand or property elements ranged to the originals.
- **Rust:** es_next.rs:1089 uses `VariableInitializerClone`, which clones even an `ObjectBindingPattern`. The result prints the same, but its left side is not a literal:
  - ES2015 (at ES5) never treats it as a destructuring assignment, so non-exported patterns stay in ES2015 syntax in ES5 output.
  - The new System predicate also requires an Object/ArrayLiteral on the left, so the System port does not fix this.
- **Fix:** expose the flattener's existing `convert_to_assignment_pattern` family (flatten_destructuring.rs:1255-1520). Those functions only need the context and source, so a mechanical change of their signature to `(context, source)` changes no behavior. In `hoist_variable_statement`, call it for binding-pattern names and keep `clone_binding_target` for identifiers.
- **Controls** (all after a top-level `using`):
  - `export let {x, y} = source;`, `export let [p, q] = source;` and `export let {a: {b}, ...r} = source;` at ES5, ES2015 and ESNext;
  - non-exported `let {m} = source; export {};` at ES5: must be flattened by ES2015 and not published;
  - CommonJS ES5 `using r = source; export let {x} = source;`, the same conversion without System.
