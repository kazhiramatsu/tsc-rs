I found one blocker and two low-risk amendments. The blocker is a new temp-name collision from the narrowed System name reservation. The other items match upstream. This is a code read of `git diff crates/emitter` over 18c0e70a4 only; nothing was built.

## Blocker: the narrowed reservation can reuse a temp name in the wrapper
`collect_system_reserved_names` (system.rs:4516-4541) no longer reserves typed temps from earlier passes up front. They are only added to `used_names` when `hoist_name_node` hoists them into the wrapper.

That is safe for temp `var` statements that sit in the custom prologue, which System now visits first. It is not safe for typed temps declared inside an ordinary top-level variable statement, which System hoists only when it reaches that statement. If a System temp was allocated in an earlier statement, it can take the same spelling.

- **Example:** ES5 `export let x = 0; const v = x++; var [a] = f();`. ES2015 flattens this to `var _a = f(), a = _a[0]` with a typed local `_a`, and System hoists it after the postfix temp has already taken the string `_a`.
- **Result:** a duplicate `var x, _a, …, _a` and a clobbered value. The print finalizer can't prevent it, because it reserves parsed names only.
- **Upstream:** both are temps in the wrapper scope, named lazily in print order, so `_a` and `_b`.
- **Fix:** at construction, also reserve the typed binding names that will end up in the wrapper. These are the leaf names of declaration lists that System hoists at SourceFile-container level: not NoHoisting, and either the container is the SourceFile or the original list is not block-scoped. That is exactly `should_hoist_declaration_list`'s domain. Keep excluding typed temps that stay in `execute` (NoHoisting for-of initializers) or in nested functions. That keeps the `for-of-postfix` fix and removes the collision.
- **Controls:** ES5/ES2015 System `export let x = 0; const v = x++; var [a] = f();` and `…; let {b} = f();`. Include the statement-order swap too.

## Low-risk amendment: unary operand checks should use kind predicates, not precedence
Upstream decides by node kind:
- `parenthesizeOperandOfPrefixUnary` (20476) tests `isUnaryExpressionKind`, which covers the unary kinds plus TypeAssertion plus every left-hand-side kind.
- `parenthesizeOperandOfPostfixUnary` (20473) tests `isLeftHandSideExpressionKind`.

Rust approximates both with `expression_precedence` (≥ UNARY and ≥ LEFT_HAND_SIDE). The two disagree for these kinds:

| Operand | Upstream | Rust precedence | Effect |
|---|---|---|---|
| ArrowFunction | prefix and postfix: parenthesized | PRIMARY → not parenthesized | `void (() => …)` loses the ranged factory paren; the printer adds an unmapped one |
| NonNullExpression | left-hand side → no paren | UNARY → postfix gets a paren | `x!++` only if a NonNull survives into a transform |
| ExpressionWithTypeArguments, ImportKeyword, MissingDeclaration | left-hand side → no paren | INVALID → paren | not produced in practice |

**Fix:** use the kind predicates the printer already has (`is_unary_expression_kind` and `is_left_hand_side_expression_kind`, used by the printer's grammar-parenthesis logic). Apply them after `skip_partially_emitted_expressions`, in both new arms and in the existing `parenthesize_operand_of_prefix_unary` (factory.rs:3984). That helper claims to be a port of the same upstream function.

## The other items check out
1. **Statement arm:** matches 20489-20511.
   - The IIFE branch wraps the callee in a paren ranged to the callee, updates the call, then rebuilds the PartiallyEmitted ancestors, like `restoreOuterExpressions`.
   - `leftmost_expression(emitted, false)` with ObjectLiteral/FunctionExpression is right, and the new paren flags come from `propagateChildFlags` as upstream does.
   - Minor: the two `unwrap()` calls in the ancestor walk should return `TransformError` instead.
   - **Adjacent effect:** any transform that builds `ExpressionStatement(Call(FunctionExpression…))` without an explicit paren now gets a factory paren. That matches upstream (the TS namespace/enum IIFEs are built that way), but where the function expression has a text range, new source-map segments appear. The h2_2b namespace/enum rows are mandatory.
2. **Yield, delete, typeof, void, await, prefix and postfix operands:** these are the right owners and match 22723-22907. Yield uses the disallowed-comma rule.
3. **Variable replacement statement** (system.rs:1823-1831): `set_text_range` only, matching 112672-112674. For parse-tree originals the range is the same as before. What is lost is the merged emit metadata: the SourceFile source-map range, and flags such as NoComments/CustomPrologue from synthesized originals. The printer's comment paths that looked through `original` now see only the range, and the 1076 destructuring-comment suite is the guard for that.
4. **CommonJS import-first early return:** returning `exported_bindings[import]` (possibly empty) matches `getExports`. It runs after the local-name, generated and synthesized gates, and `direct_export_storage: false` is right for aliases.
5. **ES2017:** not visiting the name of a method or accessor matches 101069-101118. The es2021 conversion of a concise arrow body now ranges both the `return` statement and the block to the body, with `multiLine` false, matching 20665-20671.
6. **Optional-chain line break:** measuring to the trivia-skipped start only for `QuestionDotToken` is right. The plain-dot path still ends at `name.pos`, which is equivalent to upstream's synthetic dot.

## Regression suites needed
The factory change touches every transform, so run the full battery at the final bytes:
- the System suite (764 cases, with the prior 722 byte-identical), plus the two new wrapper-collision controls;
- h2_2b namespace/enum, async372, and the 1076 export-destructuring-comments suite;
- the qualified global tests;
- ES2015 class/arrow/generator, ES2017 (await in names, async IIFE), ES2019/ES2021 nullish in concise arrows, and decorators, all through the full emitter witness sets;
- transpile routes and CommonJS export controls.
