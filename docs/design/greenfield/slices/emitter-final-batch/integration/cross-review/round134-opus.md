Changing `equal_cursor` in the `VariableDeclaration` initializer arm alone, from `original_node_end_cursor` to the existing `node_end_cursor` for the selected declared type, erased type or name, is the correct ownership fix. No narrower adapter is needed. I found no native transform whose `VariableDeclaration` would lose comments that TS keeps. This is from source reading and TS probes (`/tmp/r80/hh.mjs`, `jj.mjs`); I made no edits or builds.

## 1. The TS rule has two gates

1. **Position.** `emitInitializer(node.initializer, node.type?.end ?? node.name.emitNode?.typeNode?.end ?? node.name.end, node, …)` takes **raw** `end` values and doesn't follow the original chain. For a cloned name, `cloneIdentifier` sets the original link but leaves `pos`/`end` at -1. `emitLeadingCommentsOfPosition(-1)` returns immediately, so nothing is emitted. That is why your three CloneName oracles print `let x = 1;`.
2. **Context node.** `emitTokenWithComment(=, pos, …, contextNode = declaration)` only emits comments when `getParseTreeNode(declaration)` has the same kind. Native applies the same gate through `node_has_source_token_shape(owner)` in `emit_token_with_comments_at_boundary`, so a synthesized declaration never gets `=` comments in either implementation, whatever the cursor.

So the cursor only matters for declarations that are parse nodes or updates of parse declarations. There, TS reads the name's or type's own raw range, which is exactly `node_end_cursor` (`printer.rs:17315`). `original_node_end_cursor` (`:17276`) is right for other callers; leave it alone.

## 2. Native transforms that could break under this change: none found

I checked every native site that gives a `VariableDeclaration` a cloned or regenerated name:

| Site | What it does | Result under `node_end_cursor` |
|---|---|---|
| es2015 colliding-name rename (`es2015.rs:5894-5953`) | `get_generated_name_for_node(original)`, then `set_text_range(generated, name)` | Raw range kept. TS keeps the parse name and substitutes `x_1` at print, printing `var x_1 \n    /*c*/ = 2;`; native's `node_end_cursor` gives the same position. |
| Destructuring flattening (`flatten_destructuring.rs:1567`) | clone plus `with_original_and_range` | Range kept. The flattened declarations are also synthesized, originating from binding elements rather than declarations, so gate 2 applies in both. |
| `using` lowering (`es_next.rs:~1385`) | clone plus `set_text_range(clone, name)` for source names | Range kept. Clones for generated bindings have no range in either. |
| `clone_node_with_source_spelling` (`es2015.rs:5258`: local/declaration names for class, function, enum, namespace) | deliberately leaves the raw range synthetic, as the comment there says | Doesn't matter: those clones name **synthesized** declarations (`var C = (function(){…}())`, `var E;`, `var N;`), so gate 2 suppresses `=` comments. TS at ES5 prints `var C = /** @class */ …` with no `/*b*/`. |
| Assignment clones (`es2015.rs:6227/6709`, `es2017.rs:2014`, generators, class fields, System) | clone names for assignments | Not `VariableDeclaration`s, so this arm doesn't apply. |
| erased type or declared type | uses the parse type node | Raw range equals the original, so no change. |

**Is the test transformer faithful?** Yes. `cloneNode(identifier)` giving an original link with a synthetic range, plus factory updates on the ancestors, keeps the updated declaration similar to its parse node. That is exactly the TS situation in which gate 1 alone decides.

## 3. Minimal proposal

- In the initializer arm, compute
  `equal_cursor = self.node_end_cursor(transformation, declared_type.or(erased_type).unwrap_or(name))?`
- Keep everything else as it is:
  - the guarded name/type trailing helper;
  - the unconditional space;
  - `SourceLeading` with a plain cursor;
  - the shared token collector and source maps.
- A synthetic cursor already takes the no-comment path in `emit_token_with_comments_at_boundary`.

## 4. Controls

- the three CloneName printer oracles (newline, inline, mixed);
- the ES5 colliding rename: `let x = 1; { let x\n/*c*/ = 2; x; }` and the `/*a*/\n/*c*/` variant, where TS prints `var x_1 \n    /*c*/ = 2;` and `var x_1 /*a*/ \n    /*c*/ = 2;`;
- the ES5 converted-loop row: `var y \n    /*c*/ = function () {…}`;
- a `using x /*c*/\n/*d*/ = f()` row at ES2022 and ESNext;
- the ES5 class/enum/namespace rows above, which confirm gate 2 and print no comments;
- the existing ES5 for-of name-comment rows, and the for-await container rows.

The strict negative syntax tests running first to record baselines is the right order.
