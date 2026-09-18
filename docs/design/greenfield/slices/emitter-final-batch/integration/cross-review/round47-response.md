**Verdict: hoist at the visit sites, as proposed. There is no smaller correct approach, because upstream order is the creation order of one lexical-environment list and the probes show it is also not deduplicated.**

**Upstream facts that fix the design.** `hoistVariableDeclaration` (116104-116112) appends unconditionally; `var x; var x = source;` prints `var x, x;`, and `for (var {a} of source)` prints `var a, a` because `visitForInitializer` (112932-112948) both flattens the pattern (hoisting `a` through `createVariableAssignment`, producing `a = (void 0).a`) and then calls `hoistBindingElement` for the uninitialized declaration. `createSystemModuleBody` visits `externalHelpersImportDeclaration` first, then the statements in order, then materializes `endLexicalEnvironment()` after the prologue and before `__moduleName`. Imports hoist in statement order, not first: `export let {x = 1} = source; import * as ns …` gives `var _a, x, ns;`. Elided imports never reach the visitor. Root temps come from `ensureIdentifier` at pattern entry with reuse `!isDeclarationBindingElement(parent) || numElements !== 0`, so `export let {} = source; export let [] = source;` gives `var _a, _b;` with `_a = source; _b = source;`.

**Rust today.** `push_hoisted_name` (system.rs:1119) dedupes; the pre-pass allocates root temps through `binding_pattern_requires_root_temp`/`destructuring_temps` (1040-1062) and pushes all binding names (1064-1090) before any element temp; `flatten_system_object_binding` passes `reuse = true` (2131) and the array variant `!all_omitted`, so empty patterns reuse an identifier value where upstream temps it.

**Call sites to convert (one shared `hoist_name_node` that pushes without dedupe, registers generated-binding metadata, inserts `used_names`).**
1. Helper import: keep the one-time prefix at transform start (equivalent to upstream visiting it first).
2. `transform_import_declaration` (1560) when an import clause survives, and the import-equals path: hoist `getLocalNameForExternalImport` at visit.
3. `transform_hoisted_class` (2379): hoist the local/generated name before members are transformed; move the `generated_bindings` registration from the pre-pass arm (975-988) here.
4. `transform_hoisted_variable_statement` (1881): per declaration in order, initializer → flatten (root temp inside `flatten_system_object_binding`/`array` via `ensure_system_binding_identifier` with the upstream reuse rule and location; element temps at 2183-2200 with `element.original`; leaf names in the Identifier arm of `flatten_system_binding_target` 2081-2099 when the assignment is created); no initializer → recursive `hoistBindingElement` order, omitted elements skipped.
5. `transform_hoisted_using_statement` (1956): same initialized path; the retained declaration list's generated names are not hoisted.
6. `transform_for_initializer` (1836): for every declaration run the initialized path first (including uninitialized patterns, which flatten against `void 0`), then hoist the binding element when there is no initializer. Today it only flattens initialized declarations.
7. Expression temps (2729) already push at visit time; leave them.
8. Nested statements: hoists come from the execute visitors' recursion, which preserves order; the pre-pass arms for VariableStatement/VariableDeclarationList/ClassDeclaration and `collect_declaration_list_hoists` are then deleted, along with `destructuring_temps`.

**Invariants to hold.**
- No dedupe in the hoist list; `used_names` reservation stays separate (already seeded by `collect_identifier_texts`).
- Hoists only from the module environment: nested function bodies must not push into `hoisted_names` (the pre-pass currently skips function/class-expression subtrees; the visitor path must keep that property).
- Root-temp reuse rule exactly as upstream, for object and array patterns, with location = declaration at top level and `element.original` inside.
- The `default` name skip at 1084 has no upstream counterpart; find what produces a binding named `default` before carrying the skip into the new helper, otherwise it silently changes order or drops a hoist.
- Var statement placement unchanged (after prologue, before `__moduleName`).

**Controls to append beyond your four.** `var x; var x = source;` and `export var x; export var x = source;` (`var x, x;`); `for (var {a} of source) {}` (`var a, a`, body `a = (void 0).a`); `for (var j = 0, {b = 1} = source; ;) {}` (`j, _a, b`); `export let {} = source; export let [] = source;` (`_a, _b`); `{ var y = source.y; } if (source) { var {z = 2} = source; }` (`y, _a, z`); `export let {x = 1} = source; export default class {}` (`_a, x, default_1`); the import-after-destructuring case (`_a, x, ns`); `using u = source; using {v = 1} = source;` (`u, v, env_1`).
