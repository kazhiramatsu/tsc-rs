Don't pre-reserve: TS names wrapper temps in the order they are hoisted into the wrapper `var` list, so pre-reserving would swap the names whenever the postfix comes first. The typed-identity refactor is the right fix and stays within system.rs. This round was read-only apart from Node probes against the vendored 6.0.3 compiler (`transpileModule`, `module: System`).

## What TS actually does
My round-67 example produces no temp: `var [a] = f()` becomes `a = f()[0]`. So I probed shapes that do produce temps, at ES5, ES2015 and ES2019:

| Source (after `export let x = 0;`) | TS wrapper `var` and temps |
|---|---|
| `const v = x++; var [a, b] = f();` | `var x, _a, v, _b, a, b;`, postfix `_a`, destructuring `_b` |
| `var [a, b] = f(); const v = x++;` | `var x, _a, a, b, _b, v;`, destructuring `_a`, postfix `_b` |
| `const v = x++; let {c, d} = f();` and the reverse order | same pattern: whichever comes first is `_a` |
| `const v = x++; const u = x++; var [a, b] = f();` | `_a`, `_b`, `_c` in order |
| `const v = x++; const w = f() ?? 1;` (ES5 and ES2019) | nullish `_a`, postfix `_b` |

**Rule:** a temp's name is decided when the wrapper `var` list prints, in hoist order.
- The ES2020 nullish temp always comes first because its root-level `var _a;` is a custom prologue that System hoists before anything else.
- At ES5 the destructuring temp is an ES2015 typed temp. At ES2015 it is System's own flattening temp. Either way the order rule is the same.

Pre-reserving future typed temp names would give `_b` for the postfix and `_a` for the destructuring when the postfix comes first. That removes the collision but still gets the names wrong.

## Recommended: keep temp identity typed through the wrapper hoist
With all wrapper temps typed (FinalizerTraversal), the print finalizer names them in `var`-list print order, which matches the table above. Temps local to `execute` get their own scope and can shadow wrapper temps (`AllowShadow`), which reproduces TS's `for-of-postfix` output. Everything stays in system.rs; no printer change is needed.

1. **Hoist storage.** Replace `hoisted_names: Vec<String>` with an ordered list of either a plain name or a typed binding.
   - `hoist_name_node`: if the identifier has a generated binding, push a fresh reference built with `generated_binding_of_identifier`, i.e. `TargetBinding::from_existing` plus `write_generated_metadata`. Stop inserting that text into the `generated_bindings` map.
   - Parsed and plain names stay as text.
2. **System's own temps become typed.** Use `TargetBinding::allocate` (not `allocate_planned`) and push the typed reference into the hoist list at the moment of allocation. This covers:
   - `SystemUpdateTemp::Module` (hold the binding, not a string);
   - `ensure_system_binding_identifier` (system.rs:2177-2183, currently `create_identifier(&temp)`);
   - the module arm of `FlattenHost::allocate_flatten_temp`.

   Remove `next_temp_name`/`temp_ordinal` for temps.
3. **Wrapper `var` list.** Build typed entries as fresh identifiers carrying the binding metadata, and text entries with `create_identifier`.
4. **Module names keep the text map.** `exports`/`context`/`generated_module_names` and the default class/function names in `generated_declaration_names` still go through `generated_bindings` and `unique_generated_name`. Their reservation set is no longer used for temps. Restore `collect_identifier_texts` for it, which is what those names were validated with, and drop the r67 `collect_system_reserved_names` narrowing.

**Hazards to audit:**
- **Rebuilding a typed temp from its text.** Two typed temps can share a provisional spelling, so any `create_identifier(&text)` that recreates one would merge two identities. Check every `create_identifier(&…)` in system.rs.
  - The temp sites at 2177/2183 are covered by item 2.
  - Lines 1009, 1051, 1091 and 2380/2388 build local and default names. Confirm none of them can be an earlier-pass temp, and where one can, clone the declaration's name node instead.
  - In `transform_hoisted_variable_statement`, build initialization assignments from the visited declaration-name node, never from text.
- **Planned authoritative temps from earlier passes**, such as class-fields private temps. The finalizer keeps their spelling, and System's typed temps must avoid them in the wrapper scope. The finalizer's scope reservation should handle this, but confirm it with a control.
- **Loop-variable temps (`_i`)** use their own counter under the LoopVariable policy. Keep that policy when re-hoisting them.

**Controls:**
- Your four statement-order cases, using temp-producing forms (`var [a, b] = f()`, `let {c, d} = f()`), at ES5, ES2015 and ESNext, with comments on and off.
- The two-postfix case (`_a/_b/_c`).
- Nullish before and after a postfix at ES5 and ES2019.
- `for-of-postfix`.
- Class-fields interplay at ES2021: `class C { #x = 1 }` plus `export let y = 0; const v = y++;`, to check a planned private temp against a System temp.

## Unary predicate helper
Upstream `isLeftHandSideExpressionKind` (12210-12248) includes `MissingDeclaration`. The printer's list (printer.rs:12854-12888) matches upstream except that it lacks that one kind, and `is_unary_expression_kind` (12890-12901) matches exactly. Put the pair into one shared local helper that includes `MissingDeclaration`, apply it after `skip_partially_emitted_expressions`, and use it in:
- the new unary arms;
- `parenthesize_operand_of_prefix_unary` (factory.rs:3984);
- the printer.

Also replace the two `unwrap()`s in the PartiallyEmitted ancestor walk with `TransformError`.
