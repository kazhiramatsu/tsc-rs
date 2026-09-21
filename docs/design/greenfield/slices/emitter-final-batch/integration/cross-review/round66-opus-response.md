Six causes explain all 74 failures (648 exact / 74 failed of 722); none needs a waiver. Four are one-site fixes: CommonJS `getExports`, the ES2017 accessor names, the capture-this range, and the `?.` line break. The remaining two are broader and need the regression suites. Only a printer paren around a comma sequence hits the missing mapping, so the factory arms (#2) should change little outside these rows. The temp-name change (#4) changes a name in the output, so it needs the whole System suite. I worked from the r65 log, the fixture, the source and `_tsc.js` only, without building.

Also noted from your round-65 probes: the two object-literal decorator inputs never produce post-assigned modifiers, so I withdraw that warning. The clean static-decorator control is still worth adding.

## 1. Pattern after `using`: missing statement parenthesis (14 rows)
Rows: `using-object-pattern` and `using-nested-rest-pattern` (ES5/ES2015, plus the CJS control), and `using-local-pattern` at ES5.

**Evidence:** TS emits `(exports_1("x", x = source.x), …);`. Native output has no outer parentheses, so every later segment is one column earlier. In `using-local-pattern`, the statement's own mapping (`62:16 → 1:18`) disappears because the child mapping now lands on the same generated column.

**Cause:** upstream `createExpressionStatement` → `parenthesizeExpressionOfExpressionStatement` (20489-20511) wraps the ESNext-created `{x, y} = source` in a real ParenthesizedExpression ranged to the expression. Later passes flatten the assignment inside that node, so the parentheses stay. Rust only adds these parentheses at print time, and by then the statement no longer starts with an object literal.

**Patch** (factory.rs `apply_parenthesizer_rules`): add an ExpressionStatement arm with the full upstream rule:
- a function or arrow callee → parenthesize the callee, ranged to the callee;
- the leftmost expression is an ObjectLiteral or FunctionExpression → `ParenthesizedExpression` ranged to the expression.

This only fires when a statement is created or updated, so parsed statements are unaffected.

## 2. `void (…)` / `yield (…)` paren is not mapped (8 rows)
Rows: `scope-discarded`, all three targets; `scope-generator`, ES2015 and ESNext.

**Evidence:** the only difference is TS-only `(11,44)→1:104` and `(8,45)→1:49`, the `(` placed around a comma sequence that is ranged to the operand.

**Cause:** upstream factories wrap operands when a node is created or updated:
- `createVoidExpression`, `createTypeOfExpression`, `createDeleteExpression`, `createAwaitExpression`, `createPrefixUnaryExpression` → `parenthesizeOperandOfPrefixUnary` (20476);
- `createPostfixUnaryExpression` → `parenthesizeOperandOfPostfixUnary` (20473);
- `createYieldExpression` → `parenthesizeExpressionForDisallowedComma` (22907).

Those wrappers are ranged nodes, so they get source maps. Rust leaves these to the printer. The printer's source-ranged grammar parentheses (printer.rs:14654-14700) write `(`/`)` with no source positions.

**Patch:** in `apply_parenthesizer_rules`, add arms for those six operand kinds plus `YieldExpression.expression`, reusing the existing `parenthesize_operand_of_prefix_unary` (factory.rs:3984). This follows the three arms added in round 62.

**Alternative:** have the printer's source-ranged branch emit the operand's text-range start/end source positions. That changes behaviour printer-wide, so prefer the factory arms.

## 3. `scope-earlier-temp`: concise-body `return` is unranged (6 rows)
**Evidence:** TS-only `(8,41)→1:40` and `(8,132)→1:54`. Those are the start and end of the `return` statement ES2020 creates when it turns `(o) => o.value ?? x++` into a block.

**Cause:** upstream `convertToFunctionBlock` (20665-20671) calls `setTextRange(return, body)` and `setTextRange(block, body)`. Rust es2021.rs `merge_function_lexical_environment`, the concise arm near line 1731, sets neither range. This is the gap I noted in round 58.

**Patch:** set both text ranges to `body` there. System's own `merge_function_lexical_environment` already does this.

## 4. `for-of-postfix`: temp name `_b` instead of `_a` (2 rows)
**Evidence:** TS emits `var x, _a;` and `for (var _i = 0, _a = (exports_1("x", (_a = x++, x)), _a); …`. The System temp sits in the wrapper scope and the ES2015 for-of temp in the `execute` scope; the printer names each scope separately, so both become `_a` and the inner one shadows the outer. Native emits `_b`.

**Cause:** `used_names = collect_identifier_texts(...)` (system.rs:662) reserves every identifier in the transformed tree. That includes typed temps from earlier passes that stay in inner scopes. `next_temp_name` (975) therefore avoids `_a`.

**Patch:** build the initial reservation from:
- parsed-source identifiers;
- the generated module-name bases;
- plain synthetic identifiers that don't carry generated-binding metadata.

Earlier-pass typed temps then enter `used_names` only when System hoists them to the wrapper; `hoist_name_node` → `push_hoisted_name` already inserts them. ES2015 top-level temp `var` statements are custom prologue, so they are hoisted before any `execute` statement is visited.

**Control:** a module-level earlier-pass temp next to a System temp, which must still give `_a`/`_b`.

## 5. `optional-property-before-newline`, ESNext (2 rows)
**Evidence:** with comments, the second line is indented 4 columns less. With `removeComments`, `?.x` stays on the first line; TS breaks the line.

**Cause:** `source_gap_has_line_break` (printer.rs:12616-12656) measures from `left.end` to `right.pos`. The `?.` token's `pos` is its full start, which equals the receiver end, so the gap is always empty. Upstream `getLinesBetweenNodes` → `rangeEndIsOnSameLineAsRangeStart` measures to `skipTrivia(token.pos)`.

**Patch:** for the question-dot call only, use `skip_trivia(text, question_dot.pos)` as the end. For a plain dot, `name.pos` already sits just after the `.`, which matches upstream's end at the synthetic dot, so leave it.

## 6. CommonJS import update/re-export controls (32 rows)
**Evidence:** TS publishes through the re-export, e.g. `exports.exposed = dep_1.value = 1`, `(exports.exposed = (_a = dep_1.value++, dep_1.value), _a)`, `exports.value = dep_1.value = source[0]` and `exports.b = exports.a = source`. Native has no wrappers, no `var _a`, and leaves `[dep_1.value] = source` unflattened.

**Cause:** upstream CJS `getExports` starts with `const importDeclaration = resolver.getReferencedImportDeclaration(name); if (importDeclaration) return currentModuleInfo?.exportedBindings[getOriginalNodeId(importDeclaration)];`. Rust `export_assignment_plan` (builtins.rs ~8476-8527) only looks at value declarations. Alias symbols have none, so it returns nothing. Keep the round-59 import-first module-info change; that is what fills `exported_bindings[importDeclaration]`.

**Patch:** after the generated/local-name/synthesized gates, if `get_referenced_import_declaration` returns a declaration, return `exported_bindings[import]` with `direct_export_storage: false`, and skip the value-declaration loop. The same plan feeds CJS postfix and destructuring-flatten decisions, so all eight shapes are covered.

## 7. `await-computed-method`, ES5/ES2015 (4 rows)
**Cause:** upstream ES2017 `visitMethodDeclaration`, `visitGetAccessorDeclaration` and `visitSetAccessorDeclaration` (101069-101118) pass `node.name` through without visiting it. Rust es2017.rs:1351-1420 does `data.name = self.visit_optional_node(data.name)?`, so `[await source]` is converted to `yield` under the method's non-top-level context.

**Patch:** remove the `name` visit in those three functions.

**Control:** `class C { [(async () => 1)()]() {} }` at ES2015. Upstream leaves this arrow unlowered for the same reason, so it confirms the exact rule.

## 8. `capture-this` ES5, including the pattern and importHelpers variants (6 rows)
**Evidence:** native adds `(3,4)→0:0` and `(3,17)→2:0` on `_this = this;`, which is the SourceFile range. TS has no mapping there.

**Cause:** ES2015 `insertCaptureThisForNode` sets only a source-map range (the SourceFile) on the capture statement; Rust does the same. Upstream System `visitVariableStatement` then builds the replacement with `setTextRange(createExpressionStatement(…), node)` (112672-112674), which copies pos/end only. Rust calls `set_original_and_range` at system.rs:1826. Rust's `set_original_node` merges the original's emit metadata, as upstream `setOriginalNode` does, which brings in the SourceFile source-map range along with the NoComments and CustomPrologue flags.

**Patch:** at system.rs:1826, use `set_text_range(statement, original)` only.

**Regression suites:** after this and the other fixes, run the 1076 destructuring-comment suite and the full 722 System suite, since the printer's comment cursors can consult `original`. Also run async372, h2_2b and the qualified global tests.
