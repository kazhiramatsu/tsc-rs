No blocker. There is one required follow-up before trusting the new local-name path (`derived_from` is lost), one control to add, and the regression battery. This is a code read of the current diff only; nothing was built.

## Required follow-up: `generated_binding_of_identifier` drops `derived_from`
`TargetBinding::from_existing` (target_bindings.rs:140-180) rebuilds every naming policy from the identifier's metadata:
- the numbered base, preferred base and role suffix;
- file-level optimistic, private temp, planned-authoritative and loop variable;
- whether the name is reserved in nested scopes.

It does not rebuild `derived_from`, even though `write_generated_metadata` writes it (line 518) and the finalizer reads it (line 1632). System's own numbered names are the ones that carry it (`allocate_numbered_derived`, around system.rs:3336). They reach the new `create_local_name_reference` whenever a default or generated class or function name is created through `create_identifier(&name)` (system.rs:1000) and then referenced again. The declaration identifier then has `derived_from` and the export/assignment reference does not, so the finalizer sees the same binding ID with different naming facts.

**Fix, either of:**
- read `metadata.generated_binding_derived_from()` in `generated_binding_of_identifier` and add it to the rebuilt binding (e.g. a `with_derived_from` setter);
- or build references with `clone_node(name)`. That is upstream's `getLocalName`/`hoistVariableDeclaration(cloneNode(name))`, and it keeps all the metadata.

Either way the declaration and every reference then carry identical binding facts. The print-order marker is also not rebuilt, but it only applies to substitution clones and never to a wrapper declaration.

## Your three invariants
1. **Execute-local `_a` shadowing the wrapper `_a`:** holds. Wrapper temps are declared at the wrapper `var` list, and the `execute` for-of temp is first seen inside the `execute` scope. The finalizer uses `AncestorBindingPolicy::AllowShadow`, and System temps come from `TargetBinding::allocate`, which is not reserved in nested scopes. That gives TS's `_a` in both scopes.
2. **Earlier-pass and later-pass wrapper temps distinct, named in hoist order:** holds for FinalizerTraversal temps. `hoist_name_node` now pushes the typed binding at the moment it is hoisted, and `allocate_update_temp` pushes System's own temps in allocation order. The wrapper `var` list is printed before `execute` and the hoisted functions, so the finalizer's first event for each binding is its declaration position. This reproduces the round-68 probe table in both statement orders.
3. **Private planned temps not clobbered:** holds as far as the policy goes. Class-fields private temps keep their preferred base and private domain through `from_existing`. Legacy-decorator metadata temps (legacy_decorators.rs:3449) are planned-authoritative `_a`-style names, so their spelling is fixed. If one lands in the wrapper after a System temp, the finalizer keeps it and moves the System temp instead. That can reverse TS's print-order names. It won't collide, but it may not match.
   - **Add a control:** ES2015 System with `experimentalDecorators` and `emitDecoratorMetadata`, a decorated class using a union-typed property, with `export let x = 0; const v = x++;` both before and after it. Decide from the TS output whether the legacy temp's planned policy needs revisiting.

## Remaining places that recreate an identifier from its text
None of the `create_identifier(&…)` sites left in system.rs can be an earlier-pass temp:
- **Lines 787, 1083 and 3677:** plain hoisted names, and import or import-equals locals, which are parsed.
- **Lines 1000, 1511, 3269 and 3567-3881:** module-owned names: `generated_declaration_names`, the import binding module names, exports, context, setter parameters and helpers.
- **Lines 4082-4301:** constructors that take text for synthetic names.

Temps now go through `create_generated_reference`. Declaration leaves and publications use `clone_node(leaf.name)` (around line 1958) or the existing name nodes. Class and function locals use `create_local_name_reference`, which needs the fix above. Removing the typed-name insertion from the `generated_bindings` text map is safe because nothing else rebuilt those names from text. Restoring `collect_identifier_texts` for module-name reservation is correct.

## Other changes
- **Factory unary checks:** now use `is_unary_expression_kind` and `is_left_hand_side_expression_kind` after `skip_partially_emitted_expressions`. That matches upstream 12207-12266, including `MissingDeclaration`. The printer uses the same helpers for its operand and left-side-of-access checks, and upstream `parenthesizeLeftSideOfAccess` also uses `isLeftHandSideExpression`, so adding `MissingDeclaration` there is consistent.
- **`unwrap()` removal:** the PartiallyEmitted ancestor walk now uses `ok_or` errors.
- **Carried over from round 67, unchanged:**
  - the `QuestionDotToken` line-break gap;
  - the CommonJS import-first early return;
  - the ES2017 name pass-through;
  - the es2021 ranges on the `return` and block;
  - the text-range-only replacement statement.

## Mandatory regression runs
- **System:** the full 842-case suite with the prior 788 byte-identical, including the new multi-element/postfix/nullish/private-field orders, plus the legacy-decorator control above.
- **Factory parentheses, which touch every transform:**
  - h2_2b namespace/enum;
  - async372 and the 1076 export-destructuring comments;
  - ES2015 class/arrow/generator, ES2017, ES2019/ES2021 nullish, and the legacy and standard decorator suites (including the System decorator helper and map fixtures), through the full emitter witness sets;
  - the qualified global tests, transpile routes, and the CommonJS export controls.
