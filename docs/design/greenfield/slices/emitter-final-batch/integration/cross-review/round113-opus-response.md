The three r107 controls fail for two separate missing phases in native, which the r107 bundle-metadata analysis did not cover. JS output misses the type-end trailing comment that TS emits on a variable name reused as `exports.y`. d.ts output misses the type annotation's own trailing comment on initialized declarations. Both need small, local fixes. This comes from Node probes of the vendored TS (`/tmp/r80/w.mjs`, `x.mjs`) plus reading the printer source; I made no edits or builds.

## Failure 1: JS `exports.y /** type */ /** name */ = 1` (AMD bundle, CommonJS per-file)

**Upstream provenance** (source `export let y /** name */: number /** type */ = 1;`):
- The TS transform's `visitVariableDeclaration` (`_tsc.js:95104`) calls `setTypeNode(updated.name, node.type)`. This is the **only** place upstream sets a type node.
- The module transform reuses that **original** name identifier `y` [10,12] as the property name. TS's tree after all transforms, with the identifier still carrying `typeNode` [25,32]:
  ```
  PropertyAccessExpression[10,12]
    Identifier(exports)[-1,-1]
    Identifier(y)[10,12]  typeNode[25,32]
  ```

**What TS emits, in order** (`emitCommentsAfterNode`):
1. The access claims container end 12.
2. The name's trailing phase at 12 is suppressed, because 12 equals the container end.
3. The name's **type-node** phase runs at 32. That is not a container end, so it prints `/** type */`.
4. The access's own trailing phase at 12, with the outer container restored, prints `/** name */`.

**Native.** The `PropertyAccessExpression` arm writes the name's leading comments explicitly, then calls `emit_identifier_name_with_context`, which has no trailing or type-node phase. Step 4 already happens, which is why `/** name */` appears. Step 3 is missing.

**Smallest fix.** In the `PropertyAccessExpression` arm, right after `emit_identifier_name_with_context(name)`, add a type-node phase:
- **Condition:**
  - `!expression_context.nested_comments_suppressed()`;
  - the name has `type_node` metadata;
  - the name's **original parse parent is a `VariableDeclaration`**.
- **Call:** `emit_deferred_expression_trailing_comments(Some(&DeferredExpressionSourceComments::nested(expression_context.comments(), LeadingAndTrailing)), type_owner)`. `type_owner` is the name's owner (flags and kind) with the type node's raw range, the same shape as the r104/e80 no-initializer branch.
- **Why the parent gate is needed.** Native also sets `type_node` on **parameter** names (`builtins.rs:~11651`); upstream doesn't. TS confirms that parameter properties don't print the type comment: `this.p /** n */ = p;`. Without the gate, a reused parameter name would print an extra `/** t */`.
- **Existing phases are unaffected:**
  - `VariableDeclaration` and `Parameter` names don't go through this arm.
  - The access's own trailing phase stays after it, which gives TS's order: type, then name.

## Failure 2: d.ts `: number /** type */` (all three rows)

**Provenance.**
- The declaration transform keeps the source declaration's range. For initialized `y`, the `VariableDeclaration` comment range is [10,48] (it includes `= 1`) and the list is [6,48].
- The type node `number` [25,32] is emitted with its own comment phase.
  - Initialized `y`: its end 32 differs from the container end 48, so TS prints `: number /** type */`.
  - Uninitialized `x`: the declaration ends at 32, so the type's phase is suppressed and the list/terminator path prints the comment.
- The native **bundle** before-colon `/** name */ /** type */` already matches. It comes from the no-initializer branch's name type-node phase, which also shows that native's d.ts declaration keeps the range [10,48].

**Native.** `emit_type_annotation` writes `:`, the type's leading comments, and the type node, but never a trailing phase for the type. For `x`, the list/terminator path covers it. For `y`, nothing prints the comment at 32.

**Smallest fix.** In the `VariableDeclaration` arm's `declaration_syntax` branch only, right after `emit_type_annotation(...)`, call `emit_trailing_comments_for_node_in_container(type_node, initializer_context.comments(), writer)`.
- Use `initializer_context` because it holds the **declaration's** claim, not `expression_context` (the list's claim). That makes `x` suppressed and `y` emitted, exactly as in TS.
- Keep `nested_comments_suppressed` as a guard.
- Leave the shared `emit_type_annotation` unchanged. Its other callers (parameters, properties, returns) have different container ownership and aren't in the failing set.

## Neighbours (TS output observed at ES5, declaration on)

| Case | JS | d.ts |
|---|---|---|
| System `export let y /** name */: number /** type */ = 1;` | `exports_1("y", y /** name */ /** type */ = 1);` (bare identifier path: name, then type) | `export declare let y /** name */: number /** type */;` |
| CommonJS `export let a /** n */: number /** t */ = 1, b /** m */: string /** u */;` | `exports.a /** t */ /** n */ = 1;` | `export declare let a /** n */: number /** t */, b /** m */: string /** u */;` |
| CommonJS `export const c /** n */: 1 /** t */ = 1;` | `exports.c /** t */ /** n */ = 1;` | `export declare const c /** n */: 1 /** t */;` |
| Parameter property `constructor(public p /** n */: number /** t */) {}` (gate guard) | `function C(p /** n */ /** t */) {` then `this.p /** n */ = p;` | `p /** n */: number; /** t */` and `constructor(p /** n */: number /** t */);` |
| Class property `x /** n */: number /** t */ = 1;` (no type node upstream) | `this.x /** n */ = 1;` | `x /** n */: number /** t */;` (class-member path, outside this fix; control only) |

The System row goes through the bare-identifier emit path rather than the access arm. If it fails natively, it is the same missing type-node phase on the ordinary identifier path, with the same `VariableDeclaration` gate. Record it as its own cause rather than widening this fix blindly.

The two non-block do fixes (trailing phase before `write_line_or_space`, and source-leading after the child) are consistent with the r112 analysis and independent of these three rows.
