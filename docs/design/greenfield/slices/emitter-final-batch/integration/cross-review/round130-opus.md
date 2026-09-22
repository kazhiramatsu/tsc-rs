Your theory holds, and TS's own comment code confirms it. The TS fix is the one you propose: when the erased type has no source extent, use the source-leading `=` lane. One part of the proposal as written is likely wrong, though: to match TS byte-for-byte when a comment follows a line break, the lane must write the separating space before the token's comment phase, as TS's `emitInitializer` does. This is from reading source plus TS probes (`/tmp/r80/ff.mjs`); I made no edits or builds.

## What TS does

For `emitVariableDeclaration` with an erased type:
1. `emit(name)` runs `emitCommentsAfterNode(name)`:
   - first the name's own trailing phase at `name.end`, which prints `/*n*/`;
   - then a second phase at `getTypeNode(name)`'s raw `pos`/`end`.
2. That second phase is skipped unless `(pos > 0 || end > 0) && pos !== end`. The missing type is `TypeReference[13,13]`, so the phase is skipped and no same-line comment after `:` is printed.
3. `emitInitializer` then calls `writeSpace()` followed by `emitTokenWithComment(=, typeNode.end)`. That only emits **leading** comments at 13, and those are collected only after a line break.

So `/*c*/`, `//c` and `/*t*/` are dropped.

**How this maps to native:**
- The `BoundaryUnion` at `equal_cursor = erased_type.end` stands in for two TS phases at once: the type-node trailing phase and the `=` leading phase.
- For an empty type the first phase doesn't exist in TS, so the correct native lane is `SourceLeading` at the same cursor.
- `emit_parsed_variable_type_trailing_comments` and `emit_deferred_expression_trailing_comments` already skip empty ranges. You're right not to add another guard there.
- The owner and phase are right: this is the `VariableDeclaration` initializer `=` in the non-`declaration_syntax` branch.

**No identity gate is needed.** The condition is TS's own skip rule applied to the erased type's raw range: `!(pos != end && (pos > 0 || end > 0))`.
- It also covers a synthesized type (-1, -1), which TS skips the same way. Native gives a synthetic cursor there, so no comments come out either way.
- A valid parsed type always has a non-empty extent, so ordinary typed variables keep `BoundaryUnion`: `export let x: number /*c*/ = 1;` becomes `export let x /*c*/ = 1;`.
- Combine it with the existing condition: `container_owns_equal_boundary || erased_type_has_no_extent`.

## The detail most likely to fail

**TS output measured:**
- `export let x:\n/*c*/ = 1;` becomes `"export let x \n/*c*/ = 1;\n"`.
- `export let x: /*a*/\n/*b*/ = 1;` becomes `"export let x \n/*b*/ = 1;\n"`.

**Why.** The **space before the line break** comes from `emitInitializer`'s `writeSpace()` running **before** the token's comment phase.

**What native would do.** Native `emit_source_leading_token_with_context(…, TokenLeadingSpace::Required, …)` emits comments first and only then ensures the space (`emit_token_with_comments_at_boundary`). I expect `x\n/*c*/ = 1`, missing the space. This is inferred from the code order, not observed.

**Suggested fix.** In this lane, write the space before the token and keep `Required`, which `ensure_token_leading_space` won't duplicate. `emit_for_binding_keyword_with_source_leading_comments` already follows this pattern.

**Same pattern elsewhere.** The r104 container-owned `=` lane has the same order. Leave it unchanged unless a control shows a difference, but add one newline control for that lane too.

## Controls (TS output measured)

| Source | ESNext / ESM | CommonJS (ES5 or ESNext) |
|---|---|---|
| `export let x: /*c*/ = 1;` | `export let x = 1;` | `exports.x = 1;` |
| `export let x:\n/*c*/ = 1;` | `export let x \n/*c*/ = 1;` | `exports.x = 1;` |
| `export let x: /*a*/\n/*b*/ = 1;` | `export let x \n/*b*/ = 1;` | `exports.x = 1;` |
| `export let x: //c\n = 1;` | `export let x = 1;` | `exports.x = 1;` |
| `export let x: = /*v*/ 1;` | `export let x = /*v*/ 1;` | `exports.x = 1;` (comment dropped) |
| `export let é /*n*/: /*t*/ = 1;` | `export let é /*n*/ = 1;` | `exports.é /*n*/ = 1;` |
| `export let x: number /*c*/ = 1;` (valid-type control) | `export let x /*c*/ = 1;` | `exports.x /*c*/ = 1;` |
| `let x: /*c*/ = 1, y: /*d*/ = 2; export {};` | `let x = 1, y = 2;` | `var x = 1, y = 2;` at ES5 |

- In the CommonJS rows TS drops the comments, including `/*v*/`, on the module-rewrite path. That path is a different consumer, so these rows are worth including.
- The CommonJS `é` row also exercises the r113 property-access type phase on an empty type. It must stay silent, printing `/*n*/` but not `/*t*/`.

## Regression scope

The change only takes effect when an erased type has no source extent. That only happens for the newly admitted missing-type shape, or for synthesized types, which produce no comments in either lane. Recommended runs:

1. **This candidate's controls:** the 92 r129 commands with the 3 failures, plus the new rows above, at ES2022/ESNext and ES5, CommonJS/System/ESNext, JS with maps, observed twice.
2. **Neighbours in the same `VariableDeclaration` arm:**
   - r113 and r117 (System, CommonJS, namespace, generator, parameter-property type comments);
   - r104 for-await comment controls (the container-owned lane);
   - r107 declaration comment controls.
3. **Existing suites:** the full emitter test suite and the 291-case h2-8c transpile set.

For sources that parse without recovery the arm's behaviour is unchanged. The 1930/4088 byte-identity reruns are therefore confirmation, not a required gate for this change.
