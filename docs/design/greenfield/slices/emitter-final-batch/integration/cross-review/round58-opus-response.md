The existing pieces cover almost all of this: the shared lexical-environment API, typed bindings, and the print-time name finalizer. Codex's direction is mostly right, with three corrections:

- **Non-exported top-level functions also get their own scope.** Only exported top-level function declarations keep module-level temps.
- **Don't use typed temps at module level, and don't change `hoisted_names` into an enum.** Module temps stay as strings.
- **Nested-function temps must bypass System's spelling-keyed name tables completely.**

This is a code read only, with no Cargo and no edits.

## Two corrections to earlier rounds
- **Round 57 was imprecise.** Upstream System `visitFunctionDeclaration` (_tsc.js:112576-112600) only keeps module scope for exported declarations, including `export default`: it visits parameters with plain `visitNodes` and the body with `visitNode`. A non-exported top-level declaration goes through `visitEachChild` → `visitParameterList`/`visitFunctionBody`, which opens its own scope. The fixture's `postfix-return` is `export function f`, so module-level `_a` is correct there.
  - `transform_hoisted_function` (system.rs:972) currently treats both kinds the same, through `update_generic`.
- **Round 55's for-of wording was wrong.** The fixture's local `var _i = 0, source_1` confirms it: the lowered head is NoHoisting.

## What already exists
- **The shared lexical-environment API is complete** (transform.rs:737-817). It has start, suspend, resume and end, plus the IN_PARAMETERS and VARIABLES_HOISTED_IN_PARAMETERS flags that `hoist_variable_declaration` sets. System just doesn't use it.
- **The print-time finalizer runs after System** (transform.rs:1077, `finalize_generated_names_for_print`). A binding created with `TargetBinding::allocate` gets the FinalizerTraversal naming policy (target_bindings.rs:191-205), so its final name is chosen by walking the final tree scope by scope, the way the printer names temps lazily.
  - So the provisional `_a` is safe, and no `GeneratedBindingScopes` object is needed inside System. It also interacts correctly with temps from earlier passes in the same function; ES2020 `??` temps are the example.
- **es2021 has the closest pattern, but it is private:**
  - `visit_function_scope` (es2021.rs:1277) runs a closure and always ends the environment afterwards.
  - `merge_function_lexical_environment` / `merge_statement_array` (1702/1768) port `mergeLexicalEnvironment`.
  - `lower_parameter_default` (1573) ports `addDefaultValueAssignmentsIfNeeded`.
  - There is no shared visitor to reuse. I recommend porting these into System locally and leaving es2021 untouched.

## Design

**1. Keep the module path exactly as it is.** While no function scope is open, postfix keeps using `next_temp_name`/`push_hoisted_name`/`create_identifier`. That preserves every module ordinal behind the 901/1503 controls.
- Typed module temps would be wrong. The finalizer only reserves parsed names, so a typed module temp could also be spelled `_a` next to a string `_a` in the same wrapper scope.

**2. Temps inside a function scope** (add a `function_scope_depth: usize` field; a temp is nested when it is greater than 0):
- Create `binding = TargetBinding::allocate(context, "_a".into())`.
- Create identifiers with the factory's `create_node(Identifier)` plus `binding.write_generated_metadata`, like es2021.rs:1995. **Do not** use System's `create_identifier`: it attaches metadata by looking up the text in `generated_bindings` (system.rs:3358), so an inherited `_a` would lend its identity.
- Never touch `generated_bindings`, `used_names` or `temp_ordinal`.
- Declare the temp with `context.hoist_variable_declaration(decl)`.
- A small `enum TempRef { Module(String), Function(TargetBinding) }` local to the postfix code (system.rs:2588) is enough.
- `ensure_system_binding_identifier` (2104) is only reached from top-level declaration flattening. Add a debug assertion that it runs at depth 0.

**3. Where function scopes open.** In `visit_expression`, add arms before the generic fallback for FunctionExpression, ArrowFunction, MethodDeclaration, Get/SetAccessor, Constructor, ClassStaticBlockDeclaration, and FunctionDeclaration. A FunctionDeclaration reaching that point is always nested, because top-level ones and those in top-level blocks go through `transform_hoisted_function`.
- In `transform_hoisted_function`, check for the `export` modifier before `remove_export_modifiers`. Exported declarations keep the current path. Non-exported ones use the same scope helper.

**4. Scope helper, following upstream's `visitEachChild` order:**
1. Take `parameters`/`body` out of the node data. Visit the remaining children (modifiers, decorators, name including computed names, type parameters) with `try_visit_transform_children`, in the **enclosing** scope. Types are already erased, so nothing is lost by visiting the type early.
2. `start_lexical_environment`, then increment the depth.
3. Only if parameters exist: set IN_PARAMETERS, visit them, then clear the flag. If VARIABLES_HOISTED_IN_PARAMETERS is set and the target is ES2015 or later, lower **every** parameter as `addDefaultValueAssignmentsIfNeeded` does (91168-91180): identifiers with an initializer become `if (a === void 0) { a = … }`, binding patterns become an alias plus an initialization statement, and rest parameters are skipped.
   - This needs the target on the visitor.
   - The binding-pattern alias can be allocated when needed with `TargetBinding::allocate`, because final naming follows traversal order; es2021 plans ahead only for its own finalize.
4. `suspend_lexical_environment`, then `resume_lexical_environment`, then visit the body. A class static block does start + suspend straight away, then the body (91438-91445).
5. **Always**, including on error: `end_lexical_environment`, then decrement the depth, then propagate the error (es2021 closure pattern).
6. If the environment is empty, change nothing. Otherwise, per `visitFunctionBody` (91277-91290):
   - a block body is merged after prologue directives, hoisted functions and hoisted vars (port `merge_statement_array`);
   - a concise body goes through `convertToFunctionBlock` (20665-20671): a `return` and a non-multiline block, **both ranged to the body**. es2021 omits those two ranges, so don't copy that part; the es2021 omission may itself be a small source-map divergence.
   - A missing body gets `createBlock(declarations)`.

The depth counter only needs to be correct at temp creation. Anything visited before step 2 (computed names, decorators, and class field initializers on ES2022+ targets, which open no scope) correctly lands in the enclosing scope.

## Decisive controls
Run each at System ES5, ES2015 and ESNext unless noted.
1. **Shadowing:** `export let x = 0; const v = x++; const g = () => x++;` should give module `_a` and `() => { var _a; … }`. If the arrow gets `_b` instead, the print finalizer is reserving the synthetic module string temps.
2. **Siblings:** two sibling arrows, each with its own `_a`.
3. **Nesting:** `function outer() { const v = x++; return () => x++; }`, non-exported, where outer and inner each get `_a`.
4. **Exported vs not:** `export function f() { return x++; } function g() { return x++; }`. Only `f` uses the module temp.
5. **Parameters** (ES2015 and ESNext): `const h = function (a = x++, {b} = {}, c = 1, ...r) {};`, where every defaulted or pattern parameter is lowered and rest is not.
6. **Computed names:** `class C { [x++]() { return x++; } get [x++]() { return x++; } }` and `({ [x++]: 1, m() { return x++; } })`. The name temp belongs to the enclosing scope and the body temp to the function. At ES5, the class IIFE becomes the enclosing scope.
7. **Constructor and static block:** a constructor `constructor() { this.v = x++; }`, a static block `static { v = x++; }` (ESNext), and a field `p = x++` (ESNext: module temp; ES2015: moved into the constructor).
8. **Prologue:** `function k() { "use strict"; function i() {} return x++; }`. The `var` goes after the directive and the hoisted `i`.
9. **Captured and parsed names:** `function g() { const _a = 1; return x++; }` should give `_b`. Also a parsed outer `_a` referenced inside a nested arrow.
10. **No temp needed:** `() => { x++; }` and `() => void x++` must not add a `var` or convert the body.
11. **Earlier-pass temps:** at ES2019, `(o) => o ?? x++`, to check ordering against the ES2020 temp in the same scope.
12. **Lowered functions:** async and generator functions at ES5/ES2015, where the lowered wrapper functions are the scopes.

**Out of scope, but it will need the same helper:** Rust System has no `visitDestructuringAssignment` port (113057-113059). The only arm in `visit_binary_expression` is export publication. So `export let x, y; function g(o) { [x, y] = o; }` goes untransformed today, where upstream flattens it with scoped temps.
