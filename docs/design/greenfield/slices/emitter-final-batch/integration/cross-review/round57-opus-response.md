The postfix repair matches upstream for value shape, ranges, the operand clone and callback grouping. It leaves two gaps: parentheses in three statement positions, and where the temp is declared. The import branch ports `getReferencedDeclaration` correctly, but on its own it only fixes the export-import-equals cases. Named, namespace and default re-exports still get no wrapper, because module info collects export specifiers the old way. Details below; this was a code read only, with no Cargo and no edits.

On round 56: you're right, the ElementAccess arm already existed and my wording implied otherwise. I also accept your ExpressionWithTypeArguments probe results.

## Postfix (49aee1bce, system.rs:2588-2606)

**Shape matches upstream** (113152-113165):
- `_a = x++` is ranged to `node`.
- `(save, cloneNode(operand))` is ranged to `node`.
- Each export call wraps that comma group in order, and `create_export_call_with_name` keeps the comment-range handling.
- The final `, _a` is ranged to `node`.
- The clone is of the operand before visiting, as upstream does. Its position is −1: `cloneNode` doesn't copy `pos`/`end`, and Rust's clone is also unranged.

**Parenthesization.** Removing the explicit `create_parenthesized` means the parent now decides. Current coverage:

| Parent | Upstream | Rust |
|---|---|---|
| Call/new args, array elements | 20479-20482 | factory `parenthesize_comma_delimited_expression_children` ✓ |
| var/param/property/binding initializer, property assignment, shorthand | `asInitializer`, 24138/24160 | factory `parenthesize_initializer_for_disallowed_comma` ✓ |
| Binary operand, conditional, arrow concise body, export default, computed name | factory rules | factory ✓ |
| Prefix-unary / typeof / void / delete / await operand, access left side, `yield`, spread | parenthesizer / emit | printer grammar contexts ✓ |
| `return`, template span, element-access argument | no parentheses | no parentheses ✓ |
| **`case x++:`** | `createCaseClause` 24088 → parentheses | factory has no arm; printer NORMAL (printer.rs:6590) ✗ |
| **`switch (x++)`** | `createSwitchStatement` 23221 → `switch ((…))` | NORMAL (printer.rs:6531) ✗ |
| **`for (a of x++)`** | `createForOfStatement` 23161 | NORMAL (printer.rs:6315 region) ✗ |

The old explicit parentheses happened to produce the right output in these three rows; the new code drops them. It also fixed the old wrong `return (…)`.

**Fix:** add `CaseClause.expression`, `SwitchStatement.expression` and `ForOfStatement.expression` arms to `parenthesize_initializer_for_disallowed_comma` (factory.rs:6157). That function is already the shared factory boundary for this rule, and its parentheses take the expression's range, as upstream's do.

**Temp scope (found while reviewing, not part of this commit).** `push_hoisted_name` always adds the temp to the module-level `var` list (system.rs:952; there is no lexical-environment stack).
- Upstream `createTempVariable(hoistVariableDeclaration)` declares it in the current lexical environment.
- That is the module only for top-level statements and top-level `FunctionDeclaration` bodies: `visitFunctionDeclaration` visits the body with a plain `visitNode`, which does not open a new environment.
- Function expressions, arrows, methods, accessors, constructors and nested functions go through `visitEachChild` → `visitFunctionBody`, which opens a new environment.
  - `export let x = 0; const g = () => x++;` upstream becomes `() => { var _a; return exports_1("x", (_a = x++, x)), _a; }`, and each function starts again at `_a`.
  - Rust emits a concise arrow with a module-level `_a`, and `_b` for the second function.

**Fix:** port the `visitFunctionBody` boundary in System's child visit using the shared `context.start_lexical_environment` / `hoist_variable_declaration` / `end_lexical_environment` (transform.rs:737-801). Merge the declarations the way class_fields.rs:3453 does, and convert a concise arrow body to a block when a declaration exists. Top-level `FunctionDeclaration` bodies keep module hoisting.

## Import identity (uncommitted, system.rs:2759-2810)

**The `getExports` side is right.** It matches 113336-113348: the import declaration is selected first, and the alternative-value loop only runs when there is none. The four new name arms only take effect when the container is this file's SourceFile, which in practice means `export import a = …`. The same-source guard on the container name is kept.

**Export-import-equals, internal and external: fixed.**
- An exported alias is declared straight into `container.symbol.exports`, so Reference mode returns the SourceFile container, and the new `ImportEqualsDeclaration` arm supplies `a`.
- For the internal form, the TS transform's `export var a = N` gives a matching `exported_bindings` entry, provided Rust keeps the variable's original set to the import-equals node. Your fixture will show whether it does; either way the dedup keeps a single `a`, first.

**Named / namespace / default re-exports: still not fixed.** For `import {a} from "m"; export {a}; a = 1;` upstream gives `exports_1("a", m_1.a = 1)`.
- Upstream fills `exportedBindings[importSpecifier]` because `addExportedNamesForExportDeclaration` resolves import-first (92889: `getReferencedImportDeclaration(name) || getReferencedValueDeclaration(name)`).
- Rust module info (builtins.rs:4069-4074) calls only `get_referenced_value_declaration`. For an alias symbol that returns `None` (modules.rs:606-617: aliases have no `value_declaration`).
- So `exported_bindings.get(import_declaration)` misses, and there is no container name (the alias is local). The new branch returns `[]`.
- The same gap drops the alias half of `export import a = require("m"); export { a as b }; a = 1;`. Upstream gives `[a, b]`; Rust gives only `[a]`.

**Fix:** in builtins.rs:4069, resolve with `resolver.get_referenced_import_declaration(resolver_node)?` first, falling back to `get_referenced_value_declaration`. This is shared `CommonJsModuleInfo`, so it also changes CommonJS: CJS `getExports` returns `exportedBindings[importDeclaration]` for imports, so `exports.a = m_1.a = 1` should appear too. Add the matching CJS controls to the 126-case fixture.

**Consequence for postfix once imports publish:** the current-value clone (`, x` in both postfix branches) is never visited. Upstream substitutes it at print time, so for an imported, re-exported operand it prints `m_1.a`. Rust would print a bare `a`. Pass the clone through `substitute_import_identifier` in both branches. That helper resolves through the clone's original and ranges the access to the clone, which is unranged, as upstream's is.

## Decisive controls to add

For each: System at ES5, ES2015 and ESNext. Import rows also at CJS ES5/ES2015.
1. `export let x = 0; switch (x++) { case x++: }` and `for (const a of x++ as any) {}`: parentheses in the three new factory arms.
2. `export let x = 0; const g = () => x++; const h = function () { return x++; }; function k() { return x++; }`: temp placement (`k` stays module-level) and per-function `_a` naming.
3. `import {a} from "m"; export {a}; a = 1; const v = a++;`, plus `import * as ns` / `import d` versions and `export { a as b }`: import-first module info plus substitution of the current-value clone.
4. `export import a = require("m"); export { a as b }; a = 1;`: expected `[a, b]` order.
