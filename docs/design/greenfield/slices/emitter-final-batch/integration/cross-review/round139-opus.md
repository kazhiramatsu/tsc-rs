# r157 review: A and B are correct as far as they go, B needs a second part, and C has a one-line cause

- **A:** Root's proposal is correct as stated.
- **B:** the lookup-key fix is necessary but not sufficient. Without a second change the output becomes `exports.x = x_1;` with no `/*a*/` and no mapping.
- **C:** the cause is independent of the rename. The native declaration orchestrator never checks `noCheck` when deciding whether to seed alias visibility. The fix is one condition.

All of this is from reading the source at `add70fc29`, not from builds.

## A. VariableStatement drops TS's plain semicolon behaviour: Root's proposal is correct

**Confirmed from source:**
- In TS, `emitVariableStatement` (`_tsc.js:118606-118614`) is `emit(declarationList); writeTrailingSemicolon()`. The only comments between the list and `;` come from the list's own trailing phase, which is same-line only.
- `emit_child_boundary_comments_before_terminator` has exactly one caller, `printer.rs:4531`.
- That call goes through `emit_comments_at_cursor` with `PositionCommentPhase::BoundaryUnion` (`printer.rs:17846`), which also collects comments on following lines. That is where the extra `\n/*b*/` comes from.
- `emit_trailing_comments_for_node_in_container` (`16651`) matches TS:
  - it applies the container-end check (`retains_end`);
  - it reads the comment range, so the for-of list's `EndOnly` range works;
  - it honours `NO_TRAILING_COMMENTS` and `NotEmittedStatement`;
  - it collects same-line comments only.
- **Fallback no longer applies.** The replacement drops the old helper's parent/child raw-range fallback (`child_trailing_comments_escape_parent_container`, 8791). TS has no such fallback; it compares only `containerEnd` and `declarationListContainerEnd`.
- **Scope used.** `declaration_context.comments()` is the statement's claimed scope (`claim_sides`, 4495-4506). The for-of statement's `StartOnly` range claims no end, so the list's `/*a*/` goes before `;`.
- **The shared helper stays.** `emit_child_boundary_comments_before_parent_end` still has its other callers.

**Boundary controls** (in addition to Root's ordinary-semicolon neighbours):
- ASI: `let x = 1 /*a*/\n/*b*/\nf();`. The list end equals the statement end, so the statement claims it and `/*a*/` must come after `;`.
- Two same-line comments: `let x = 1 /*a*/ /*b*/;`.
- A synthesized statement around a parsed list (for example a CommonJS non-exported `let` → `var` in a nested block). This is the case where the dropped fallback used to answer.

## B. CommonJS nested rename is missing `exports.x = x_1 /*a*/;`

**Part 1 (Root's finding) is confirmed.**
- TS `appendExportsOfDeclaration` (`_tsc.js:111743-111760`) looks up `getDeclarationName(decl)`, which is a clone of the still-unsubstituted `x`.
- Native `declaration_export_plans` (`builtins.rs:7307-7345`) keys by `identifier_text_owned(leaf.name)`, which is `x_1`.

The repair has to change **both** lookups: `export_specifiers_by_local` (7322) and `export_specifier_locations` (7328, which uses the same `local_name`). Use the pre-substitution name's text only when `leaf.name` carries `generated_binding_print_order`, and keep `local: leaf.name`. To find that name, walk the same chain as `variable_initializer_pre_substitution_type`: the first original that doesn't carry the flag.
- **Best form:** factor that walk into one crate-level helper used by both the printer and `builtins.rs`, not two copies.
- **Other readers checked, none affected:**
  - 3429: hoisted/direct publications, which run only at top level;
  - 4213: the map builder;
  - 6084: import bindings only.

**Part 2 is missing: the value's flags.**
- In TS, `getName` (`_tsc.js:24788-24797`) gives the clone `NoComments | NoSourceMap`.
- At print time, es2015 `substituteIdentifier` replaces that clone with `setTextRange(getGeneratedNameForNode(x), clone)`, a fresh node with **no emit flags**. That is where TS's `/*a*/` and its source-map segment come from.
- Native `create_declaration_export_statements` (`builtins.rs:7415-7421`) clones `plan.local` and adds `NO_SOURCE_MAP | NO_COMMENTS`. The native stand-in never goes through a print-time substitution, so those flags survive.

The minimal fix is in that else-branch. When `plan.local` carries the print-order flag, keep the clone and `set_text_range(value, plan.local)` but **do not add** those two flags; every other case is unchanged.
- **Safe from CommonJS rewriting:** the clone's original chain reaches the parse `x`, so `is_non_reference_identifier_node` skips it. TS also skips it, because it is a generated identifier.
- **Statement flags unchanged:** the statement keeps `NO_COMMENTS`, matching TS `createExportStatement` without `allowComments`.
- **Source map:** the exact tuple must include the map row for `x_1` in the appended statement.

**Controls:**
- `export { x as y }` alias form;
- an uncaptured nested `let x` (no rename, so no append);
- destructuring `{ let {a: x} = o; (() => x)(); }`, which goes through the `binding_name_leaves` recursion;
- System over the same four configs, which must stay exact.

## C. noCheck `.d.ts` loses `declare let x: number;`: my independent diagnosis

**Confirmed mechanism:**
1. TS `emitDeclarationFileOrBundle` (`_tsc.js:116650-116652`) seeds alias visibility on this condition:
   `(emitOnly && !getEmitDeclarations(options)) || options.noCheck || emitResolverSkipsTypeChecking(emitOnly, forceDtsEmit) || !canIncludeBindAndCheckDiagnostics(sourceFile, options)`
   It then calls `collectLinkedAliases(sourceFile)` (116716-116735). For `export { x }`, `resolver.collectLinkedAliases(name, true)` runs `getTargetOfExportSpecifier`, which sets `isVisible = true` on the top-level `let x` declaration (55675-55727).
2. The native port of that call site is `emitter/src/declarations/orchestration.rs:159-175`, and its guard is only `force_dts_emit || !resolver.can_include_bind_and_check_diagnostics(source)`. **The `options.noCheck` term is missing.**
   - The native `can_include_bind_and_check_diagnostics` (`checker/src/emit.rs:~300-315`) ports `_tsc.js:18898-18905`, which has no noCheck term either. For `.ts` sources it returns true.
   - So a noCheck program never seeds.
   - The `ProgramNoCheck` route's own documentation (`emitter/src/route.rs:21-23`) names `116650 collectLinkedAliases` as part of that route; the orchestrator never implemented it.
3. **Why checked mode is correct:** the checker's own pass seeds visibility from `checkExportSpecifier` (`checker/src/modules.rs:10008`). noCheck skips that pass, so `emit_is_declaration_visible` (`declaration_emit.rs:555`) falls through to `emit_determine…`. A non-exported declaration in an external module returns false, so the statement is dropped while `export {x};` stays. That matches the capture, and the missing source-map segment follows from it.
4. **The checker is not at fault:** `collect_linked_aliases` / `build_visible_node_list` (`declaration_emit.rs:1522-1600`) match TS.

**Smallest correct repair:**
```rust
if force_dts_emit
    || options.no_check == Some(true)
    || !resolver.can_include_bind_and_check_diagnostics(source)?
```
- Read `options` from `host.compiler_options()`, which is currently fetched just after this loop; move it up. `execute.rs:936` already reads `options.no_check == Some(true)` the same way.
- Leave the `emitOnly && !getEmitDeclarations` term out. The orchestrator has no emitOnly input, and no failing row needs it.
- This does not turn on checking. It uses only the resolver path that `TranspileDeclaration` (forced noCheck, reached through `force_dts_emit`) already runs.
- **`no_check_routes_run_no_source_checking` should stay 0.** It counts `checkSourceFileWorker` executions (`transpile_routes_contract.rs:714-745`), and the seeding path resolves aliases without running it; `TranspileDeclaration` already takes this path under the same test. I have not confirmed that by running it.

**Boundary controls:**
1. Rename-free: `let x = 1; export { x };`, noCheck declaration, CommonJS and System. This should fail at current bytes, which proves the cause is independent of the rename, and pass after the fix.
2. `let x = 1; export { x as y };` with noCheck.
3. `const x = 1; export default x;`, which exercises the `ExportAssignment` branch.
4. The same three with checks on must stay byte-identical.
5. Run `transpile_routes_contract` from the contracts binary. It contains `program-no-check` declaration cases whose recorded results may move. If a KNOWN-open row becomes exact, report it rather than retiring it; that decision isn't part of this repair.
