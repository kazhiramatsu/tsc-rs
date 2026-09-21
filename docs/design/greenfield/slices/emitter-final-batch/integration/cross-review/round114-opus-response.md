The 6bd1c5817 diff looks correct against 993f3fed0. Both extra scope changes match upstream, and I see no duplicate or container-projection problem for normal sources. For the r113 gate, use the name's own identity rather than its original's parent: native's metadata merge copies `type_node` onto clones, which TS never does. This is from reading source and the vendored `_tsc.js`; I made no edits or builds.

## 6bd1c5817 versus 993f3fed0

**Four post-child tokens now use the source-leading policy.**
- Import `from`, export `from` and specifier `as` call `emit_source_leading_token_with_context(…, Required, expression_context)`. This matches r112.
- `emit_while_clause` now takes `(anchor, phase)`:
  - `WhileStatement` keeps `BoundaryUnion` at `node.pos`. That is behaviour-neutral: the owner's `pos` equals the anchor, so the comment phase is skipped anyway.
  - Both do branches first run the body's trailing phase, then write the space (block) or line break (non-block), then use `SourceLeading`. That is TS's `emitDoStatement` order.
  - A comment after a newline, as in `do x();\n/*l*/ while`, still prints via the token's leading phase, because the helper emits same-line comments only.

**Extra 1: ImportClause named-bindings trailing phase.** This matches TS. `emitImportClause` ends with `emit(node.namedBindings)`, whose own trailing phase is guarded by the clause's claimed end.
- **Normal source.** The bindings' end equals the clause end. Inside the ImportClause arm, `expression_context.comments()` holds the clause's claim, so `retains_end` is true and the comment is suppressed. The `from` anchor helper then prints it once. This covers `import * as ns /*t*/ from`, `import { a } /*t*/ from` and `import d, { a } /*t*/ from`.
- **Clause comment-range override.** The claim is the override end, so the bindings phase emits `/*c*/`. That is exactly TS's `} /*c*/ /*ib*/ from` row.
- **Synthesized clause with no range.** The claim comes from the outer declaration (after `;`), so the bindings phase prints the comment and the clause helper prints nothing. That is still a single emission, matching TS.
- The only duplicate risk would be the bindings node emitting its own trailing phase elsewhere. Before this change a normal source printed the comment exactly once through the `from` path, so it doesn't.

**Extra 2: named import/export elements use the in-container list-end helper.** This also matches TS, where each list element's trailing phase is guarded by the container that `NamedImports`/`NamedExports` claimed.
- **Normal source.** The last element ends before `}`, so it is not retained and still prints, as before.
- **The named-exports override row.** The container end equals the last element's end, so `/*eb*/` is suppressed inside the list and emitted outside, matching TS `{ a /*q*/ as b } /*eb*/`.
- **No last-child problem.** The previous helper effectively had an empty container, and it only differs from this one when an element's end coincides with a claimed end. Metadata overrides cause that, not ordinary parse ranges.
- Watch one case: a synthesized `NamedImports` with no range inside a clause that has an override ending at the last element. The inherited claim then suppresses the element's comment, which I believe is also TS's behaviour. You already have that as the r112 import-clause override row.

## r113 gate for the type-node phase

**Why "original's parse parent is `VariableDeclaration`" is not enough on its own.**
- Upstream sets a type node in exactly one place: `setTypeNode(updated.name, node.type)` (`_tsc.js:95104`), on the name node itself.
- TS `mergeEmitNode` (used by `setOriginalNode`/`cloneNode`) does **not** copy `typeNode`.
- Native `EmitMetadata::merge_from` (`metadata.rs:926-927`) **does** copy it.
- So every native clone whose original chain reaches a `VariableDeclaration` name carries a type node that TS's clone lacks. Example: generators' `x = 1` from `transformInitializedVariable`, where TS prints `x = 1;` with no type comment.
- A gate based only on the original's parse parent would print extra `/** t */` in those cases.

**Suggested gate:**
1. `metadata(name).type_node().is_some()`;
2. `transformation.arena().get_original_node(name) == name`, meaning this is the parse node itself, not a clone or update;
3. `transformation.arena().node(name)?.parent` has kind `VariableDeclaration`.

**Why this works:**
- For CommonJS/AMD, TS `transformInitializedVariable` (`_tsc.js:~111`) uses `node.name` directly as the property name. Native `create_export_access_from_name` (`builtins.rs:10853-10861`) likewise keeps the source name as the property child. So condition 2 holds there.
- Condition 3 excludes native-only parameter type nodes (`builtins.rs:~11651`).
- Condition 2 excludes clones and updates that inherited the metadata through native `merge_from`.
- These are existing arena accessors (`get_original_node`, `node().parent`); no new helper is needed.

**One corner case to add to your five neighbours.** At ES5, when a name uses an extended Unicode escape, native builds a new identifier instead of reusing the name (`builtins.rs:10871-10875`). TS still reuses the name and prints the type comment, so this gate won't restore it natively. Add `export let \u{1d400} /** n */: number /** t */ = 1;` at ES5 CommonJS if you want that corner covered. Otherwise record it as known.

**Record as a dependency (not a fix now).** `merge_from` copying `type_node` is a native deviation from `mergeEmitNode`. It is harmless as long as consumers gate on identity as above. Changing `merge_from` would affect the r107 bundle metadata path, so leave it.
