The declaration-prep candidate (6bd1c5817 plus the uncommitted `printer.rs` diff) looks correct, and I don't see anything unsafe in it. The identity gate is the least-impact version I'd recommend. Two things remain open until the native comparison: the System bare-identifier path, and one namespace neighbour I'd add. This is from reading the diff, the fixture inputs and native builtins, plus TS probes (`/tmp/r80/y.mjs`); I made no edits or builds.

## The diff

**d.ts `VariableDeclaration` phase** (after `emit_type_annotation`, inside `declaration_syntax`)
- The guard is `!initializer_context.nested_comments_suppressed()`.
- It uses `initializer_context.comments()`, which is the declaration's own claim.
  - **Initialized (`y`):** the declaration ends at 48 (it includes `= 1`) and the type at 32. The end isn't retained, so it prints `: number /** type */`, as in TS.
  - **Uninitialized (`x`, and `b` in the multi-declaration case):** the declaration end equals the type end, so it is retained here. The list/terminator path prints the comment once, so there is no duplicate.
- A synthesized inferred type has range -1, so the helper does nothing for it.
- The shared `emit_type_annotation` is untouched, so the d.ts class-property and parameter-property controls keep their existing paths.

**`PropertyAccess` name phase**
- **Order of checks:** the metadata check comes first, then `get_original_node(name) == name`, then the parse `parent` kind is `VariableDeclaration`. Errors from `node()`/`transpose()` propagate rather than being swallowed.
- **Owner:** it takes the name's flags and kind, not the type's. That matches `emitCommentsAfterNode`, which calls `emitTrailingCommentsOfNode(node, emitFlags, typeNode.pos, typeNode.end, …)` with the name's flags.
- **Scope:** it runs inside the access arm's scope, where the access has claimed [10,12]. Output order is type phase, then the access's own trailing phase, giving `exports.y /** type */ /** name */` as in TS.
- **The gate is correct against native producers:**
  - CommonJS/AMD: `create_export_access_from_name` keeps the parse name. TS `transformInitializedVariable` likewise uses `node.name` directly, so the gate passes.
  - Native-only parameter `type_node`: excluded by the parent check.
  - Clones (for example generators' `x = 1`) whose type node was inherited through native `merge_from`: excluded by the identity check.
  - ES5 extended escape `\u{61}`: native builds a fresh identifier with no metadata, so it prints only `/** n */`, matching your TS observation. At ES2015 the parse name is reused, so it prints type then name, also matching TS.
  - Namespace exports: `namespace_member_name` builds the access from text with a fresh identifier (`builtins.rs:15240`), so it never matches the gate.

Thanks for falsifying my r114 `\u{1d400}` corner. The BMP `\u{61}` controls are the right replacement.

## The eight controls

| Control | Expected native path |
|---|---|
| system-binding | Bare identifier inside `exports_1("y", y … = 1)`. The new code **doesn't cover this path**. TS prints `y /** name */ /** type */`: the identifier's own trailing phase, then the type phase. If native fails, add the same gated phase after the node's own trailing phase on the ordinary identifier pipeline, keeping that order. |
| multiple-declarations | JS `exports.a /** t */ /** n */ = 1;` via the access gate. d.ts: `a` prints through the new phase, `b` through the terminator. |
| literal-annotation | Same shape as the multi-declaration case, for `c`. |
| parameter-property | Excluded by the parent gate. JS `this.p /** n */ = p;`. d.ts path untouched. |
| class-property | Not a variable name. JS `this.x /** n */ = 1;`. d.ts class-member path untouched; it may already match or be a separate cause. |
| escaped-variable-es5 / es2015 | As described above. |
| hoisted-generator | The no-initializer branch prints `var x /** n */ /** t */`. The `x = 1` clone gets no type phase: the access gate doesn't apply, and the generic path has none. |

## One neighbour to add (read-only TS observation)

`export namespace N { export let y /** n */: number /** t */ = 1; }` at ES5 or ES2015 CommonJS.
- TS prints `N.y = 1;` with **no** comments, because `getNamespaceMemberNameWithSourceMapsAndWithoutComments` sets NoComments.
- Native `namespace_member_name` calls `set_text_range(access, original_name)` and sets no comment flags. It might therefore print `N.y /** n */ = 1`.
- If it does, that is an existing mismatch unrelated to this change, not a regression. The type gate can't fire there.

No change is needed to the gate. I didn't look at the public constructor types from the lint work; that's out of scope here.
