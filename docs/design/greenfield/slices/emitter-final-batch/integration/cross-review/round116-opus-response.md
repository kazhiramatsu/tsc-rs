If the System control fails, the fix is one gated call in the printer's ordinary comment pipeline, reusing the r113 identity gate. The namespace risk I raised in r115 was based on the wrong call chain, and no mismatch is expected there. This is from source reading at aea09cbaf plus TS probes (`/tmp/r80/z.mjs`); I made no edits or builds.

## 1. How TS prints the System identifier

- `transformSystemModule.transformInitializedVariable` (`_tsc.js:~112698-112708`) calls `createAssignment(node.name, …)`.
- `createVariableAssignment` (`:~112728-112731`) hoists `factory.cloneNode(name)`, but the assignment itself uses the parsed name node: `createExportExpression(name, preventSubstitution(setTextRange(createAssignment(name, value), location)))`.
- So the identifier in `exports_1("y", y = 1)` is the declaration name, which carries `typeNode`.
- Its comments come from `emitCommentsAfterNode`:
  1. its own trailing phase at 12 prints `/** n */` (the enclosing statement claims 0–49, so 12 isn't retained);
  2. then the type-node phase at 32 prints `/** t */`.
- The order is **name then type**. That is the reverse of the property-access case, where the access's own trailing phase comes last.

## 2. Native call chain

- **The identifier is the parsed name.** `system.rs:1994-2012` (`flatten_system_binding_target`, Identifier arm) calls `create_assignment(target, value)` with `target` = the declaration's name child, the same node id as in the parse tree. Hoisting uses `hoist_name_node` (`system.rs:920`), which pushes **text** or a generated binding, never a clone. So `var y;` gets no metadata and prints plain, as in TS.
- **It already goes through the ordinary comment phase.** `BinaryExpression` (`printer.rs:~7880-7897`) emits `left` through `emit_expression_child_with_source_comments` (`:14716`). That wraps it with `DeferredExpressionSourceComments::nested(…, LeadingAndTrailing)` and enters `emit_node_with_hint_and_source_comments_worker`.
- **That worker currently prints only the node's own trailing phase.** There is no type-node phase, so native should print `y /** n */ = 1` and the control should fail as predicted.

## 3. Least-impact insertion point

**Where.** In `emit_node_with_hint_and_source_comments_worker`, in the two **ordinary** branches only:
- the "deferred but not no-ASI-parenthesized" branch (`printer.rs:~15102`);
- the final `else` branch (`~15145`).

Insert immediately after the existing `emit_deferred_expression_trailing_comments(…, owner, …)` call in each.

Leave out the grammar-paren and no-ASI-paren branches: an identifier never takes those, and in TS the paren is a separate node with its own phase.

**Conditions (all required):**
1. `deferred_source_comments` is `Some` and `owns_trailing()`. When `deferred` is `None`, including nested-suppressed contexts, `emit_node_with_hint` passes no deferred comments and nothing is added.
2. `metadata(substituted).type_node()` is present. Check this first so the common path does no extra traversal.
3. `get_original_node(substituted) == substituted`. This excludes clones that inherited `type_node` through native `merge_from`, for example `generators.rs:1084` `clone_node(name)` for `x = 1`, where TS prints no comments.
4. The parse `parent` of `substituted` is `VariableDeclaration`. This excludes the Rust-only type metadata on parameter names.

Use `substituted`, not `node`. TS's comment phase runs after substitution, and System wraps the target in `preventSubstitution`, so they are the same node here.

**Call.**
- `emit_deferred_expression_trailing_comments(transformation, deferred_source_comments.as_ref(), type_owner, writer)`
- `type_owner = ExpressionCommentPhaseOwner { range: <raw type pos/end>, ..owner }`, so it keeps the **name's** flags and kind. `NO_TRAILING_COMMENTS` then suppresses both phases, as TS's shared `skipTrailingComments` does.
- The deferred container is the one the owner's own call used, which is TS's `savedContainer*`.

**Return value, maps and callbacks.**
- **Return value:** discard the type call's result and keep `trailing` as the owner's `VisitedHere` anchor at the name end. The binary operator anchor (`access_target_trailing_anchor_at(left_comments, name_end)`) and its resume stay exactly as today.
- **Source maps:** the node's After map is recorded inside `emit_substituted_node_with_comments`, before trailing comments, which matches TS (source-map phase nested inside the comment phase).
- **Callbacks:** the new call runs inside the `emitted` closure, before `transformation.after_emit_node(hint, node)` (`~15176`), so notification ordering is unchanged.

**Why no duplicates.**
- The `VariableDeclaration` arm emits its name via `emit_node_with_hint` with no deferred comments, so condition 1 fails there. Its explicit branch stays the only emitter.
- `PropertyAccess` names go through `emit_identifier_name_with_context`, which also passes no deferred comments. The r113 explicit site stays the only emitter there.

Optionally, extract one helper `emit_variable_name_type_trailing(name, deferred, owner)` holding conditions 2–4, used by both the new site and the `PropertyAccess` site.

## 4. Namespace correction

You're right. Exported namespace variables go through `transform_namespace_exported_variables` (`builtins.rs:12872`) and then `create_namespace_export_target` (`:13333`).
- That builds a fresh `N.member` from text and gives it only a source-map range (`set_source_map_range_from(target, name)`, `:~12935`). It has no text range and no comment range.
- It therefore prints no comments, the same as TS `N.y = 1;` (which uses `getNamespaceMemberNameWithSourceMapsAndWithoutComments`).
- The fresh identifier has no `type_node` and no parse parent, so the gate can't fire either.
- I'm withdrawing my r115 expected-mismatch claim. The namespace control is still worth keeping as a guard.

## 5. Controls (TS ES5 System output observed)

| Source | TS output |
|---|---|
| `export let y /** n */: number /** t */ = 1;` | `exports_1("y", y /** n */ /** t */ = 1);` |
| `export let u /** n */: number /** t */;` | only `var u;` (no assignment) |
| `export {}; let z /** n */: number /** t */ = 1;` | `z /** n */ /** t */ = 1;` (non-exported: still the parsed name) |
| `export let a /** n */: number /** t */ = 1, b /** m */: string /** u */ = "";` | `exports_1("a", a /** n */ /** t */ = 1), exports_1("b", b /** m */ /** u */ = "");` |
| `declare const o: any; export let { p /** n */ }: any /** t */ = o;` | `exports_1("p", p /** n */ = o.p);` (binding element: no type phase; the parent gate excludes it) |
| `export function f() { let w /** n */: number /** t */ = 1; return w; }` | `function f() { var w /** n */ /** t */ = 1; return w; }` (`VariableDeclaration` arm, unchanged path) |

Plus, from earlier rounds:
- **ES5 generator hoisting:** TS prints `x = 1;` with no comments. This covers the clone exclusion (condition 3).
- **CommonJS `exports.y` row:** covers the `PropertyAccess` site still printing type then name, with no second emission from the generic path.
