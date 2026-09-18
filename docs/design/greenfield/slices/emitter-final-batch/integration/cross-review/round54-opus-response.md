I traced both issues to one line each on the System side. The binder, checker and ESNext lowering all match upstream. `clone_binding_target` and NoHoisting are not the cause. I made no edits and ran no Cargo, so nothing below has been compiled or run; the r54 records and the code are the only evidence.

## Issue 1: `export using` / `export await using` loses `exports_1("r", …)` at ES5 and ES2015

**How upstream publishes it:**
- The ESNext transform drops the `export` modifier. `transformUsingDeclarations` calls `updateVariableStatement(statement, undefined, varList)` (_tsc.js:103510-103518). So `hoistVariableStatement` sees `isExported = false` (103648), and no `export let r;` or export specifier is created. r53 said otherwise; that was wrong. The target `r` is `cloneNode(node.name)` with LocalName, ExportName and InternalName cleared (103667-103670).
- Upstream `moduleInfo` therefore has no entry for `r`. `collectExportedVariableInfo` only records LocalName names anyway (92932-92934).
- The callback is added when the tree is printed. `substituteBinaryExpression` (113299-113309) calls `getExports(r)` (113318-113330). `getReferencedExportContainer` (87870-87899) resolves the parse-tree name to the exported local symbol and returns the SourceFile. Then `append(exportedNames, getDeclarationName(valueDeclaration))` publishes `r`.
- At ESNext, System's `using` branch (112639-112661) goes through `createVariableAssignment`, which applies `preventSubstitution` (112723-112726). That is why TS emits `using r_1 = r = source;` with no callback.

**Rust, layer by layer:**
- **Binder/checker: match upstream.** `bind_block_scoped_declaration` (binder/src/bind.rs:700-706) calls `declare_module_member` for a SourceFile container, and `is_block_or_catch_scoped` covers `using` (BLOCK_SCOPED = 7). `emit_get_referenced_export_container_for_symbol` (checker/src/modules.rs) follows the same ExportValue → parent → SourceFile path.
- **ESNext lowering: matches upstream.** es_next.rs:912-915 sets `modifiers: None`. So `CommonJsModuleInfo` correctly has neither `exported_bindings[decl]` nor `exports_by_local["r"]` for `r`.
- **Clone path: already proven to work.** The using-alias rows publish `s` from inside the same try block. So the System visitor reaches the cloned `r`, passes the pos/SYNTHESIZED gate (system.rs:2732-2738), and `get_referenced_value_declaration` resolves it.
- **The defect is in `SystemVisitor::exports_for_identifier`, system.rs:2763-2773.** When `exported_from_source` is true, Rust returns `exports_by_local[text]`, which is a module-info fact. Upstream returns `getDeclarationName(valueDeclaration)`, which is a resolver fact. Direct `export let` never reaches this branch, because Rust's `exported_bindings` also holds the declaration's own name (builtins.rs:3929) and returns early at 2747-2750. That is why the export-let controls could not tell the two apart.

**Smallest fix (system.rs:2740-2773):**
1. Collect the result as today: the `exported_bindings` hit, or the `exports_by_local` fallback when `exported_from_source`.
2. If `exported_from_source` and a `value_declaration` exists, build the declaration's name node:
   - `TransformNode::new(node.source(), value_declaration.node())`, guarded by `value_declaration.source() == resolver_node.source()` (the same idiom as system.rs:2681 and es2015.rs:548);
   - read its name node the way `get_name_of_declaration` does (es2015.rs:796);
   - convert it with `super::ModuleExportName::from_node`.
3. Put that name at the front only if no entry with the same `as_js()` is already present.

**Why this stays narrow:**
- It uses the resolver's declaration name node, not the identifier text.
- For every direct export the name is already in `exported_bindings`, so nothing changes there.
- It only fires when lowering removed the declaration's own export modifier.
- It also covers `export using r; export { r as s }`. Upstream gives `[r, s]`; Rust's early return currently gives `[s]`.

**Left alone on purpose:** for `export { y as z }; export let y` Rust nests the calls in the opposite order to upstream (`[z, y]` vs `[y, z]`). That is a separate latent difference and this change does not touch it.

## Issue 2: ES5 `export await using` hoists `result_1` out of `finally`

This is not NoHoisting being lost; upstream never sets NoHoisting on this list. What upstream does:
- `result_1` is created as a `Const` list (103791).
- The ES2015 transform turns it into a `var` list and calls `setOriginalNode(declarationList, node)` (106429-106431).
- System's `visitBlock` makes the `finally` block the enclosing container (113019-113026).
- `shouldHoistVariableDeclarationList` (112695-112697) tests **`getOriginalNode(node).flags & BlockScoped`**. That reaches the Const list, so upstream does not hoist.

Rust ES2015 already sets the original (es2015.rs:5629). The mistake is in `should_hoist_declaration_list`, system.rs:919, which reads `self.context.arena().node(list)?.flags`, i.e. the current var list with flags 0.

**Fix:** read the flags from `self.context.arena().get_original_node(list)`, which ports `getOriginalNode`. Do not use `get_emit_original_node`. The NoHoisting half stays on the current node, as upstream does. `transform_for_initializer` (system.rs:1689) uses the same predicate, so it is covered too.

**Blast radius — needs the widest controls.** This changes every ES5 System `let`/`const` declared inside a block, loop, catch, case block or try body. Upstream keeps these as `var` inside the construct. Rust currently hoists them, and none of the existing suites shows which way those rows go.

## Controls for the full native comparison

**Publication fix** (System, each at ES5, ES2015 and ESNext; ESNext must stay byte-identical, with no publication):
1. `export using r = source;`
2. `export await using r = source;`
3. `export using r = source; export { r as s };` — derived from upstream: `exports_1("s", exports_1("r", r = …))`
4. `using r = source; export { r as s };` — existing alias row
5. `export using a = s1, b = s2;`
6. `export let r = 1; { using r = source; }` — the inner `r` must **not** publish
7. CommonJS `export using r = source;` at ES5 and ES2015 — isolates the resolver: the fix doesn't touch it, and it needs the same container answer
8. The existing `export let y` top/block/try rows, unchanged

**Hoisting fix** (System ES5; ES2015 should be unchanged):
- a. `export await using r = source;`
- b. `{ using r = source; }` and `{ await using r = source; }`
- c. `{ let a = 1; }` and `{ const {a} = o; }`
- d. `for (let i = 0; i < 2; i++) {}`, `for (const k of xs) {}`, `for (let k in o) {}`
- e. `try { const t = 1; } catch { let c = 2; }`
- f. `switch (x) { case 1: let c = 1; }`
- g. Top-level `let a = 1; export { a };` — the SourceFile container still hoists
- h. `{ for (let i = 0; i < 2; i++) { f(() => i); } }` — checks whether any Rust-only original link on ES2015's synthesized `_loop_1` or `state` var lists now wrongly stops hoisting

I did not touch the recovery-next worktree.
