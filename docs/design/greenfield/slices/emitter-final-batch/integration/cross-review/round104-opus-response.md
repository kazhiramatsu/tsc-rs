Here are the two `/*binding*/` roots and the scratch review; each needs a narrow change. Sources: `printer.rs`, `comment_cursor.rs` and `factory.rs` in Recovery-next at 663da, the vendored `_tsc.js`, and Node probes `/tmp/r80/q.mjs` and `r.mjs`. I made no edits and ran no builds.

## 1. ES2015/17 `x /*binding*/ =`: don't switch the `=` call to SourceLeading unconditionally

**The TS rule, from the vendored source:**
- `emitTokenWithComment(=, name.end)` only emits **leading** comments at `name.end` (`emitLeadingCommentsOfPosition`).
- `iterateCommentRanges` with `pos > 0` only collects comments that come after a newline, so same-line comments are never emitted there.
- The name's same-line comment is emitted by the **name's own trailing phase** (`emitCommentsAfterNode`, at `commentRange.end`, and again at `typeNode.end` if a type was erased).
- That trailing phase is guarded by `forEachTrailingCommentToEmit`: it runs only when `end !== containerEnd && end !== declarationListContainerEnd`.

**How native splits the work:**
- `emit_node_with_hint(name, …)` passes no deferred source comments, so the name gets no trailing phase.
- The name's leading comments are emitted explicitly just before it.
- The `=` call, with `BoundaryUnion` and a plain cursor that has no resume point, is therefore the **only** place the name's trailing comments get emitted. With no resume point, `emit_comments_at_cursor_with_phase` emits them without any container check.
- So the root cause is that TS's container-end guard is missing on native's stand-in for the name's trailing phase. The comment-phase choice is only how it shows up.

**Why an unconditional `SourceLeading` switch regresses.** It would drop every same-line name comment before `=`, which TS keeps:
- `const x /*c*/ = 1;` stays as written (ES5 prints `var x /*c*/ = 1;`).
- `const x: number /*c*/ = 1;` becomes `const x /*c*/ = 1;`.
- The existing control `active_transform_contract.rs:4034` (`erased_parameter_types_keep_both_comment_boundaries`) expects ES5 `var rest /* name side */ = [];`.

**Narrow fix, same single call site.** Choose the phase with the guard TS already has, which native exposes as `CommentEmissionScope::retains_end` (`comment_cursor.rs:209`):
- `initializer_context.comments().retains_end(utf16(equal_cursor))` → `SourceLeading`
- otherwise → `BoundaryUnion` (unchanged)

Here `equal_cursor` is already the name end, or the erased type's end.

What this gives:
- **for-await:** the updated declaration and list both keep the range [95,102]. That end is claimed, so the call becomes `SourceLeading`; `/*binding*/` is printed only after `;`.
- **`const x /*c*/ = 1` and `var rest /* name side */ = []`:** the claimed end is after the initializer (or the parameter), not the name, so they stay `BoundaryUnion`.
- **Erased types:** the unguarded `emit_trailing_comments_at_node_position(name)` stays as it is. The guard only affects the `=` call.

## 2. ES5 missing hoisted `x /*binding*/`: the name's trailing phase is missing when a declaration has no initializer

- The probe shows TS's hoisted list is a synthesized list [-1,-1] containing the **original** `x` [100,102]. Nothing has claimed 102, so the name's trailing phase prints `/*binding*/`.
- Native's `VariableDeclaration` arm emits the name's trailing comments only through the `=` call. With no initializer, as for every hoisted declaration built at `generators.rs:4317`, nothing emits them.
- The list loop's `emit_trailing_comments_for_node(declaration)` uses the **declaration's** range. That range is synthesized (-1) for hoisted declarations, so it emits nothing.
- The hoist sites and the NoComments flags are not the cause.

**Narrow fix:** in the same arm, when there is no initializer, emit the name's trailing comments at the name end under the same guard:
- skip when `initializer_context.comments().retains_end(name_end)`;
- also respect the name's `NO_TRAILING_COMMENTS` flag and disabled comments.

It covers both kinds of declaration without duplicating:
- **Parsed `var x /*c*/;` or `var x /*c*/, y;`:** the declaration claims its end, which equals the name end, so the guard suppresses it. The existing declaration-level trailing emission still prints the comment once.
- **Hoisted declarations (synthesized, ranged name):** the guard doesn't apply, so the comment prints, as in TS. The same applies to generator hoists that carry a copied comment range (`generators.rs:1041`).

This does change output for every hoisted name that has a source range (generators, ES2015 loops, other `var` hoists). That is TS-faithful, but rerun the complete emitter suites. Add one targeted control for `let x /*a*/: number;`: TS prints `/*a*/` through the name-end phase, and native may already print it through the list-item path. If it does, the two paths need reconciling.

## 3. Object-rest scratch file (`/tmp/emitter-es2018-rest-r104.rs`)

The diff adds `prepare_for_await_object_rest`, splits the binding statement into a raw builder plus a visiting wrapper, and clears the type and exclamation token.

**Matches TS (`_tsc.js:102196-102240`):**
- Parentheses are skipped. Nothing is prepared unless the unwrapped initializer is a list or pattern containing object rest.
- The temp is local, not hoisted.
- The `let` declaration and its list are ranged to the original initializer, including parentheses.
- The body binding is built raw, is not visited, and is ranged to the unwrapped initializer. It uses the first declaration only, with type and `!` cleared.
- The body block is:
  - multi-line, with no original;
  - ranged to the original statement;
  - its statement array is ranged to the original statements array, or to the single statement.
- `update_node(original, ForOf{list, block})` keeps the range and the original chain.
- Each part is visited once:
  - the expression, through the existing plan;
  - the head `let _e = _c`, through the visiting wrapper;
  - the prepared block, through `data.statement`, which flattens the raw rest binding.

**Required change: the multi-line marking now silently does nothing.**
- `visit_for_await_statement` now calls `mark_enclosing_block_multi_line(range_owner)` with the **updated** node when there are no labels.
- That function walks `record.parent` (`es2018.rs:1590`).
- `update_node` → `clone_node` sets `parent = None` (`factory.rs:5552`), so the walk stops immediately.
- Keep the pre-preparation source for-of for that call, and use the updated node only for `set_original_and_range`. Labeled loops are unaffected because their range owner is the label.

**Verify with the 24 red cases, not by assumption: temp spelling.**
- The temp is allocated **before** the await plan's temps.
- TS names temps in print order: `_e` in the block case, `_g` for `x.y`, `_h` and `_m` nested.
- If `allocate_local_temp()` fixes a provisional ordinal at allocation, you'll see an early letter such as `_a`.

**Block scope (`visitIterationBody`) is not needed.**
- In TS, only class fields call `addBlockScopedVariable` (`:97630`, `:97784`). es2018 never does, so its `startBlockScope`/`endBlockScope` around the body always collects nothing.
- The probe (`/tmp/r80/r.mjs`) confirms where the computed-key temps go:
  - **binding:** into the same `const` list, `const _f = _e, _g = k(), a = _f[_g], r = __rest(…)`;
  - **assignment:** hoisted function-level `var` temps (`_d, _e` at ES2017) used in `(_d = _g, _e = k(), …)`;
  - **labeled and nested:** the same, with no block-scoped declarations.
- Only a class expression inside the loop body would add block-scoped variables, and that happens in the class-fields pass, not here. Skip `start_block_scope`/`end_block_scope`.
