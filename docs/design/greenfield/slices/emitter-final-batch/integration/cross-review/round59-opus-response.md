The scope implementation matches upstream, and I found no defects in it. Your correction is right: in `() => void x++` the operand is visited as a value, not discarded, so upstream allocates a local temp there. For destructuring assignment, the shared flattener can be reused with three default-implemented hooks that leave the ES2015 caller unchanged. This was a code read only, with no Cargo and no edits.

## Scope implementation review

**Statement-level discarding matches `visitorWorker` (113036-113068).**
- ExpressionStatement visits its expression as discarded.
- Parenthesized and PartiallyEmitted expressions pass the flag through.
- A nested ForStatement uses `visitForStatement(node, false)`: the initializer and incrementor are discarded, while the condition and body are visited normally.
- A comma BinaryExpression has no special case upstream, so `(f(), x++)` still allocates a temp. Rust matches through `visit_binary_expression`; worth one control.
- `() => void x++` goes through `visitEachChild` → `visitor`, so it allocates. That fixes my round-58 example.

**`visit_function_like` follows the `visitEachChild` order.**
- Modifiers, decorators, name and type parameters are visited in the enclosing scope.
- Then: start the environment → IN_PARAMETERS → visit parameters → lower defaults (ES2015+ with VARIABLES_HOISTED_IN_PARAMETERS) → clear the flag → suspend → resume → visit the body.
- The environment is always ended and the depth decremented before the error is propagated.
- The static block matches 91438-91445.
- The exported / non-exported split in `transform_hoisted_function` matches 112576-112600.

**Ranges:**
- A concise body becomes a `return` statement and a non-multiline block, both ranged to the body (`convertToFunctionBlock`, 20665-20671). An existing block body keeps its multiline setting.
- `lower_parameter_default` matches 91197-91276:
  - Binding-pattern parameters: the alias replaces the parameter name, and a `var pattern = alias === void 0 ? init : alias` initialization statement is added. There are no emit flags on this arm.
  - Initializer arm: `NO_SOURCE_MAP` is set on the assignment name; `NO_SOURCE_MAP | NO_COMMENTS` is added to the initializer; the assignment gets `NO_COMMENTS`; the block gets `SINGLE_LINE | NO_TRAILING_SOURCE_MAP | NO_TOKEN_SOURCE_MAPS | NO_COMMENTS`; both are ranged to the parameter.
  - The parameter array is updated in place, so its range and trailing comma survive, as in 91192.
- `merge_statement_array` matches `mergeLexicalEnvironment`. The var statement goes after hoisted functions and the initialization statements after hoisted vars, with the splices in upstream's order. `is_hoisted_variable_statement` has the same meaning as `isHoistedVariable`.

**Typed identity:**
- `create_function_temp_reference` bypasses the spelling-keyed map, which is correct.
- The binding-pattern alias is a `TargetBinding::allocate` temp. That matches `getGeneratedNameForNode(parameter)`, which falls to the default `makeTempVariableName` case. Because it prints first in the parameter list, the finalizer names it before the body `var`, as upstream does.
- Module postfix temps are unchanged strings.

**Minor points (no action needed):**
- Parameters are visited outside the `arrays` memo. That memo only avoids re-visiting a shared array, so nothing depends on it here.
- With no body, Rust merges into an empty block where upstream does `createBlock(declarations)`. Both produce the same statement order, and a body-less function can't allocate temps anyway.

## Destructuring-assignment port

### Entry point (system.rs, `visit_expression` BinaryExpression arm)
Port 113106-113141:
- If the node is a destructuring assignment (`=` with an object or array literal on the left) and `has_exported_reference_in_destructuring_target(left)` is true, call `flatten_destructuring_assignment(self, node, FlattenLevel::All, !value_is_discarded, false)`. Otherwise use the current path.
- The binary arm must now receive `value_is_discarded`.
- Port the check's recursion exactly:
  - a non-compound `=` recurses into its left side;
  - **array** `SpreadElement` recurses into its expression;
  - object and array literals recurse into every member;
  - a shorthand property checks its name; a property assignment checks its initializer;
  - an Identifier is true when `get_referenced_export_container(Reference)` is a same-source SourceFile, the same test `exports_for_identifier` uses;
  - **everything else is false.** That includes an object `SpreadAssignment` (`({...x} = o)`) and any alias-only target (`let z; export {z as w}; [z] = a`). Upstream neither flattens nor publishes those.
- Use the optional `parse_tree_resolver_node` here, not the `require_…` variant: synthesized identifiers must count as false, not raise an error.

### Hooks in `FlattenHost` (flatten_destructuring.rs)
All three have defaults, so `Es2015Visitor` and the unit-test driver don't change.

1. **`fn allocate_flatten_temp(&mut self, hoist: bool) -> Result<TargetBinding, TransformError>`.** The default is the current free-function body (flatten_destructuring.rs:461-474); all five call sites switch to `host.allocate_flatten_temp(..)`. Assignment flattening always hoists. System's override:
   - **Inside a function scope:** `TargetBinding::allocate` plus `context.hoist_variable_declaration(create_function_temp_reference(..))`. That is the same path postfix uses, so both share the function's environment in call order.
   - **At module level:** `name = next_temp_name(); push_hoisted_name(&name); binding = TargetBinding::allocate_planned(context, name.clone())`, then `self.generated_bindings.insert(name, binding.clone())`, so the wrapper's `var` identifier carries the same identity.
     - Planned spelling is authoritative, so the finalizer keeps System's unique, source-ordered name. No typed name can collide with a string temp, and the ordinal sequence behind the existing controls stays the same.
     - `allocate_planned` does not reserve the name in nested scopes, so nested temps may still shadow it, as upstream's do.
   - Implement `generated_bindings()` for System as `unreachable!`, with a comment that the override owns allocation. The allocator is the trait's only caller.
2. **`fn complete_flattened_assignment(&mut self, target: TransformNode, assignment: TransformNode) -> Result<TransformNode, TransformError>`**, default `Ok(assignment)`. Call it in `emit_binding_or_assignment`'s non-callback Assignment arm, **after** `set_text_range` and `set_original_node` on the plain assignment.
   - That is a small reorder in the shared file. It changes nothing for the identity default, and it keeps `original` on the assignment, where upstream puts it.
   - Here `target` is the **pre-visit** pattern element. Upstream's print-time `substituteBinaryExpression` sees `node.left` before its child is substituted, so import-first `getExports` keys off the identifier even when the printed left becomes `m_1.a`.
   - System's override: if `target` is an Identifier, apply `exports_for_identifier(target)`, which already covers the generated, file-level-reserved, LOCAL_NAME and synthesized gates. Wrap with `create_export_call_with_name` for each export, in order. That function already sets NO_COMMENTS on the value and takes the value's comment range, like `createExportExpression`.
   - Keep `use_assignment_completion = false`: upstream passes no callback.
   - No-substitution ownership is automatic. The flattener only calls `host.visit_expression` on its inputs (right side, targets, initializers, computed keys), never on the assignments it creates, so nothing publishes twice. The visited left still gets `substitute_import_identifier`.
3. **`fn request_flatten_helper(&mut self, helper: EmitHelper) -> Result<(), TransformError>`.** The default is `context().request_emit_helper(helper)`; replace the calls at 980 and 1032.
   - System's override does not attach the helper, matching the round-49/51 binding path.
   - Upstream System never calls `readEmitHelpers` (112146 only moves helpers that are already attached). So a late `__read`/`__rest` is referenced but never defined in this file.
   - **Needs checking:** it stays in the shared `emitHelpers` array, and the next source file's first `readEmitHelpers` (94082) would attach it there. Add a two-file control. If TS shows the helper carried over, model that as a carry in the program context. Neither the binding path nor this path models it today.

At ES5, ES2015 flattens destructuring before System, and System keeps publishing the resulting plain assignments through `visit_binary_expression`. That path is unchanged, so the new flatten effectively runs at ES2015 and above.

### Controls to add to yours
Run at System ES2015 and ESNext, plus ES5 where noted.
1. `({...x} = o)` and alias-only `[z] = a`: not flattened and not published.
2. Mixed `let z; export {z as w}; export let x; [x, z] = a`: flattened, and `z` publishes `w` through its exported binding.
3. `for ([x] of arr) {}`: the for-of initializer is not a BinaryExpression, so there is no flatten and no publication.
4. Chained `[x] = [y] = a`, and needs-value `f([x] = a)` / `const v = ([x] = a)`.
5. Import re-export target: `import {a} from "m"; export {a}; [a] = arr`. Expected `exports_1("a", m_1.a = …)`.
6. `[o.p, x] = a` (property target, no publication) and `[x = y++] = a` (a postfix temp inside the flatten, to check module ordinal order).
7. Inside a function: `function g(o) { const v = x++; [x, y] = o; return ([x] = o); }`, where local temps are shared with postfix and named in call order.
8. Swap collision `[x, a] = [a, x]`, and `({[k()]: x} = o)` needing a temp for the computed key.
9. Object rest `({a, ...r} = o)` under ESNext and importHelpers: `__rest` requested late, plus the two-file carry control.
10. An ES5 compound pattern, to show the existing ES2015-flattened publication is unchanged.
11. Comments: `[/*c*/ x] = a` and `({ /*c*/ x } = o)`.
