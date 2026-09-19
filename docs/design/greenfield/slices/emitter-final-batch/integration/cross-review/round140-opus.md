# Round 140 review: one real defect (dead code that fails clippy), and A, B and C are otherwise correct

I read only the production diff, the new controls and the design-doc addition, against `add70fc29`. No builds, and nothing here qualifies the candidate.

## Defect: `emit_child_boundary_comments_before_parent_end` is now dead code

- The removed wrapper was the **only** caller of `emit_child_boundary_comments_before_parent_end` (`printer.rs:~8848`). Its name now appears only in its own definition.
- It is a private method and the crate has no `allow(dead_code)`, so rustc will warn that it is never used. Workspace `clippy -D warnings` will fail.
- **Fix:** delete that method as well. Its callees still have other callers, so nothing else becomes dead:
  - `child_trailing_comments_escape_active_container`: 4612 and 5363;
  - `comment_range_end_cursor`: 16720;
  - `child_trailing_comments_escape_parent_container`: still called from the active-container check.
- This corrects my r139 statement that the shared helper "keeps its other callers". It had none apart from the wrapper.

## A. VariableStatement semicolon

The call at `printer.rs:4531` becomes `emit_trailing_comments_for_node_in_container(declaration_list, declaration_context.comments())`. That matches TS `emitVariableStatement` followed by the list's same-line trailing phase. I found no gap.

The six semicolon controls cover mixed, newline-only, line comment, ASI, same-line and nested cases.

## B. CommonJS export plan

**The helper (`factory.rs:772-792`) behaves correctly:**
- It returns the first node without the print-order flag. A node with no metadata counts as ordinary, which the textual lookup needs.
- It returns `None` for a node that isn't itself a stand-in.
- It is bounded by the metadata count.
- On the printer side, `arena.metadata(original_name)?.type_node()` gives no type when that node has no metadata, the same as the old walk.

**The lookup (`builtins.rs:7319-7326`):**
- `local_name` now feeds both `export_specifiers_by_local` and `export_specifier_locations`, so the two lookups stay consistent.
- `plan.local` remains the actual binding.

**Destructuring:** the stand-in created by `flatten_destructuring_binding_materialized` links to the binding element's name, so the lookup text is `x`. That matches TS, where flattening puts the parse `x` into the declaration and `getDeclarationName` reads it. The temp `_a` has no flag, so it keeps the old behaviour.

**The value's flags (7424-7437) are already right:**
- `clone_node` (`factory.rs:5550-5584`) creates a synthesized node and calls `set_original_node(clone, stand-in)`. That merges the stand-in's metadata into the clone: the generated binding (kept for identifiers), the print-order flag, and the stand-in's emit flags.
- The stand-in gets no emit flags from es2015. `colliding_declaration_name_substitute` sets only the text range, the semantic original and the print-order mark.
- So the clone has neither `NoComments` nor `NoSourceMap`, which matches TS's fresh substitute from `setTextRange(getGeneratedNameForNode(x), clone)`.
- CommonJS won't rewrite the clone: its chain reaches the parse `x`, so `is_non_reference_identifier_node` skips it.
- Every non-stand-in keeps the old flags.

**One missing high-value control, with a code-level cause.** The for-of head creates its stand-in through a different producer, `create_variable_declaration_plain` at `es2015.rs:~12467`. After es2015, the renamed declaration sits inside the downlevelled loop (possibly inside `_loop_1`). It reaches the same `declaration_export_plans` consumer through a nested VariableStatement. None of the 12 new shapes combines a for-of rename with an export specifier. Add one shape:
`let x = 1; export { x }; for (let x /*a*/ of [1]) { (() => x)(); }`

TS's placement of the appended `exports.x = …` here is a quirk; take the expected output from the capture, not a prediction.

## C. noCheck seeding (`orchestration.rs:159-176`)

- The new condition is `force_dts_emit || options.no_check == Some(true) || !can_include(...)`.
- Moving `options` up is harmless, because `host.compiler_options()` is a plain accessor.
- `EmitResolverMethod::CollectLinkedAliases` has no route-level admission check. `route.rs:45` only governs whether noCheck is admitted at all.
- **Checker activity:**
  - The only code on this path is `collect_linked_aliases` → `get_target_of_export_specifier` / `resolve_name` → a node-link write.
  - There is no `checkSourceFileWorker` call. `TranspileDeclaration` already runs the same path through `force_dts_emit` under `no_check_routes_run_no_source_checking`.
  - Expect that test to stay at 0, but it has to be run to confirm.
- The three unrenamed controls cover the direct, `as` and `export default` branches.

## Design-doc addition

The new paragraphs are explicitly future work:
- They keep semantic provenance, comment transfer and source-map origins as separate relations.
- They don't claim that one-to-one links are enough or that a representation has been chosen.
- They record the three r157 relationships as a migration contract.
- They state that the current fixes stay in the existing architecture and do not start the migration.

No overclaim.
