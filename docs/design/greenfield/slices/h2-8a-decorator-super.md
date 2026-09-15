# A6-41-SUPER: standard-decorator static `super` forms and evaluation order (isolated candidate)

Status: **isolated research candidate**, prepared 2026-09-14 (third follow-up
2026-09-15, §5.2) in the worktree
`~/dev/tsc-rs-dec-super` (branch `draft/h2-8a-decorator-super`). Not production
admission, not an A6-41 or H2.8 completion claim. Root production is untouched;
profiles, ratchets, pins, hosted acceptance and the canonical runner are not part
of this candidate. The handoff that scoped it is
[h2-8a-decorator-super-claude-handoff.md](h2-8a-decorator-super-claude-handoff.md).

## 1. Base and restoration

- Evidence commit `6e298cda8dbf362f61ce6a8e690ec145270681df` (`work/h2-8-output-matrix`).
- Restoration (handoff §2, executed exactly): `class_fields.rs` ← retained lexical
  candidate v2; then `git apply --check` + `git apply` of comma printer, comma
  argument factory, literal property observation, list owner v18, UTF16 tests v4,
  static-accessor tests v2. attempt65 `prelaunch.inputs` **1014/1014** and
  `vendor_inputs` **109/109** SHA-256 exact; production SHA-256 after restoration
  matched the handoff (`standard_decorators.rs` 7872d2f6…, `class_fields.rs`
  c423c244…, `class_fields/downlevel.rs` 506ea986…).
- The restoration is committed as the candidate base
  (`13f5767ece720108f41e46fb50a713283ec429be`, "Restore v18 decorator candidate
  base for A6-41-SUPER"); its tracked diff is also saved as
  `target/h2-8a-decorator-super/base/evidence-to-restored-base.tracked.patch`.
  Every later change is a separate diff on top of that commit.

## 2. Source owners (pinned `_tsc.js`, whole SHA-256 1c59e77a…)

| Source | Lines | Rust owner (this candidate) |
| --- | --- | --- |
| `updateState`, `enterClass`/`exitClass`, `enterClassElement`, `enterName`, `enterOther`, `shouldVisitNode` | 98973–99060 | `DecoratorReceiverFrame::{Class,ClassElement}` now carry `class_super`; `update_receiver_state`, `enter_receiver_class(class_this, class_super)`, `enter_receiver_class_element` |
| `visitor` / `discardedValueVisitor` | 99061–99148, 99183–99202 | `visit_with_value_use(id, ExpressionValueUse)`; `visit_discarded_value`; arms for expression statement, `for`, parenthesized (discarded), comma list, binary, prefix/postfix unary, call, tagged template, property/element access, arrow function, class static block |
| `transformClassLike` | 99319–99616 | `transform_class_like`: class IIFE lexical environment (`start_lexical_environment` … `merge_lexical_environment`), class decorators transformed **before** `prepare_class_super`, `createClassInfo` `__runInitializers` request before class decorators |
| `partialTransformClassElement`, `visitReferencedPropertyName`, `visitComputedPropertyName`, `injectPendingExpressions` | 99831–99944, 100345–100380, 100511–100545 | `prepare_decorators_and_computed_names` (every member decorators assignment pending in member order; undecorated non-inlineable computed names absorb pending; leftovers emitted after `_metadata`) |
| `visitPropertyDeclaration` (lexical environment + IIFE) | 100041–100150 | `visit_property_initializer_in_environment`, `create_decorated_initializer`, `wrap_initializer_with_environment` |
| `visitCallExpression`, `visitTaggedTemplateExpression`, `visitPropertyAccessExpression`, `visitElementAccessExpression` | 100154–100201 | `visit_call_expression`, `visit_tagged_template_expression`, `visit_property_access_expression`, `visit_element_access_expression`, `create_super_property_get` |
| `visitBinaryExpression`, `visitPreOrPostfixUnaryExpression`, `visitCommaListExpression` | 100249–100344 | `visit_binary_expression`, `lower_super_assignment`, `visit_pre_or_postfix_unary_expression`, `lower_super_update`, `visit_comma_list_expression`, `split_super_key_for_read_write` |
| `visitDestructuringAssignmentTarget` … `visitAssignmentPattern` | 100394–100488 | `visit_destructuring_assignment_target`, `visit_assignment_pattern`, `visit_array_assignment_element`, `visit_assignment_element`, `visit_assignment_rest_element`, `visit_object_assignment_element`, `visit_assignment_property`, `visit_assignment_rest_property` |
| `createReflectGetCall`, `createReflectSetCall`, `createAssignmentTargetWrapper` | 24601–24606, 24754–24783 | `create_reflect_get_call`, `create_reflect_set_call`, `create_assignment_target_wrapper` |
| `expandPreOrPostfixIncrementOrDecrementExpression` | 27499–27519 | `expand_pre_or_postfix_increment_or_decrement_expression` |
| `visitCommaListElements`, `isSimpleInlineableExpression`, `isCompoundAssignment`, `getNonAssignmentOperatorForCompoundAssignment`, `isSuperProperty`, `isLeftHandSideExpression`, `isAssignmentExpression` | 91306–91317, 93030–93060, 14608, 17090–17120 | same-named helpers on `StandardDecoratorVisitor` |
| `visitEachChildOfClassStaticBlockDeclaration`, `visitEachChildOfArrowFunction`, `visitFunctionBody`, `convertToFunctionBlock`, `hoistVariableDeclaration`, `endLexicalEnvironment`, `mergeLexicalEnvironment` | 91438–91445, 116104–116200 | `visit_class_static_block_declaration`, `visit_arrow_function`, `visit_function_body`, `convert_to_function_block`, `hoist_declaration_for`, `materialize_lexical_environment`, `merge_lexical_environment` |
| class-fields `transformPublicFieldInitializer` (static, ES2022+), `addPropertyOrClassStaticBlockStatements` | 96603–96630 area, `if (isStatic(property) && !shouldTransformPrivateElementsOrClassStaticBlocks) continue;` | `downlevel.rs::materialize_public_static_field_block` (dependency fix, see §5) |
| **Follow-up 2026-09-15:** `visitParameterList` (`VariablesHoistedInParameters` arm), `addDefaultValueAssignmentsIfNeeded`, `addDefaultValueAssignmentForBindingPattern`, `addDefaultValueAssignmentForInitializer`, `hoistVariableDeclaration` (flag set) | 91168–91262, 116104–116116 | `visit_arrow_function` (flag check after the parameter visit), `lower_arrow_parameter_defaults`, `lower_arrow_parameter_default` (decision 14) |
| `visitParameterDeclaration` | 100202–100225 | `visit_parameter_declaration` (decision 15) |
| `createStringLiteralFromNode`, `getTextOfIdentifierOrLiteral`, printer `getLiteralTextOfNode` / `getTextOfNode` | 21535–21543, 120467–120479, 13045–13075 | `create_string_literal_from_node` (text source on the literal; printer branch already ported), used by `super_property_key`, `create_context_name_literal`, `create_access_object`, `create_set_function_name_block` (decision 16) |
| `createESDecorateClassElementContextObject`, `createESDecorateClassElementAccessHasMethod`, `createESDecorateClassElementAccessGetMethod` / `SetMethod`, `partialTransformClassElement` (`propertyName` for literal names) | helpers section (`createESDecorate*`), 99862–99872 | `create_context_name_literal`, `create_access_object` (`is_literal_member_name` → element access on the literal-from-node), decision 16 |
| `getHelperVariableName` | 99214–99221 | `helper_variable_base_name` (decision 17) |
| es2018 plan-based `flattenDestructuringAssignment` return (`inlineExpressions`) | 93251–93328 (`return factory.inlineExpressions(expressions)`) | `es2018.rs::flatten_destructuring_assignment` no longer re-ranges the result (decision 18) |
| **Second follow-up:** `isNamedEvaluationSource`, `isNamedEvaluation`, `isAnonymousFunctionDefinition`, esDecorators `isAnonymousClassNeedingAssignedName`, `transformNamedEvaluation` (+ `getAssignedNameOfIdentifier`, `getAssignedNameOfPropertyName`, `finishTransformNamedEvaluation`), the named-evaluation arms of `visitParameterDeclaration`, `visitBindingElement`, `visitVariableDeclaration`, `visitPropertyAssignment`, `visitExportAssignment`, `visitBinaryExpression` | 15937–15984, 93761–93800, 93849–93905, 93950, 100226–100228, 100202–100206, 100249–100262, 100376–100393, 100495–100499 | `record_named_evaluation`, `record_property_assignment_named_evaluation`, `is_anonymous_class_needing_assigned_name`, `is_proto_setter`, `NamedEvaluationName`, `inferred_class_name_sources`; arms in `visit_with_value_use` / `visit_parameter_declaration` / `visit_binary_expression` (decision 19) |
| `createMethodDescriptorObject` / accessor descriptor `__setFunctionName` names, printer `emitPrivateIdentifier` → `getTextOfNode` | 100585–100595, 120467–120479 | `create_set_function_name(name_source)`; printer `PrivateIdentifier` source-spelling reuse and text-source branch (decision 20) |

| **Third follow-up (2026-09-15):** `addDefaultValueAssignmentForInitializer` reached from the class-fields transform's own `visitParameterList`, `convertToFunctionBlock` via `visitFunctionBody` | 91239–91262, 91277–91291, 20665–20672 | `downlevel.rs::lower_parameter_default` (assignment and block take the parameter's range, the assigned name and initializer `NoSourceMap`), `install_function_bindings` (the return statement and block take the concise body's range) (decision 25) |
| `transformPrivateIdentifierInInExpression` | 96117–96126 | `downlevel.rs::visit_binary_expression` (`in` arm): `setOriginalNode` only, no text range on the `__classPrivateFieldIn` call (decision 27) |
| `transformClassMembers` member assembly (class-this assignment block, named-evaluation helper block, synthetic constructor, synthetic static block, remaining members) | 97218–97231 | `downlevel.rs::arrange_synthetic_members`, used by `install_instance_operations` (returns the synthetic constructor) and `install_private_static_pending_block` (decision 28) |
| `visitClassStaticBlockDeclaration` (drops a relocated static block from the member pass), `transformPublicFieldInitializer` / `transformPrivateFieldInitializer` (drop a relocated static field), `addPropertyOrClassStaticBlockStatements` after `transformClassMembers`, `compareEmitHelpers` (stable priority sort, request order otherwise) | 97130–97135, 96294–96370, 96971–97045, 97432–97443, 26023–26030 | `downlevel.rs::visit_relocated_static` / `request_relocated_static_helpers` over `TransformationContext::defer_emit_helper_requests` / `resume_emit_helper_requests` (`transform.rs`); the printer's helper sort no longer runs `order_private_field_helpers` (`helpers.rs`, removed) (decision 26) |

## 3. Design decisions

1. **State.** `classSuper` is carried next to `classThis` in the class and
   class-element frames and derived by the same `updateState` rule (element
   frame, or the third predecessor of a name frame). The `_classSuper` identity
   is the identifier `prepare_class_super` places in the heritage clause; each
   use is a fresh identifier of the same text (tsc reuses one node).
2. **Order.** Class decorators are transformed before the heritage expression
   is visited (both in the enclosing frame), matching `transformClassLike`.
   `__runInitializers` is requested where `createClassInfo` requests it, so a
   decorated class nested in a class decorator cannot request `__esDecorate`
   first. Members are still visited in one source-order pass: constructors
   hold no receiver, and Rust allocates generated names at visit time, so a
   second constructor pass would reorder nothing observable (witness
   `phase-order/constructor-nested-anonymous`).
3. **Value use.** `ExpressionValueUse::{Required,Discarded}` is threaded only
   through the tsc `discardedValueVisitor` edges (expression statement, `for`
   initializer/incrementor, comma left operand and non-final comma-list
   elements, parentheses in a discarded position). A partially emitted
   expression is deliberately **not** a discarded edge (tsc re-enters
   `visitor`), so `(super.x = v) as T;` keeps its result temp.
4. **Memo policy.** The node memo keeps only required-value lowerings. A
   discarded-value visit of a mode-sensitive kind (unary update, binary, comma
   list, parenthesized) neither reads nor writes the memo. Every reachable
   producer visits an expression node once; a shared or synthesized node
   reached twice would otherwise observe the temp-less lowering. No global
   cache rewrite and no clone removal were made.
5. **Temps and environments.** Super temps are `TargetBinding::allocate`
   bindings (finalizer-named `_a`, `_b`, … per printer scope, `_i`/`_n`
   skipped); hoisted temps join the context's lexical environment
   (`hoistVariableDeclaration`, `NoNestedSourceMaps`). Environments exist for
   the class IIFE, every static block, every property initializer (IIFE when
   non-empty), and every arrow function (expression bodies convert to a
   single-line block with the body's text range, as `convertToFunctionBlock`).
   The arrow's environment follows `visitParameterList`: active (InParameters)
   while the parameters are visited, suspended until `visitFunctionBody`
   resumes it. Suspending before the parameters made a decorated class
   expression inside a parameter default fail with "lexical environment is
   suspended" (three `retained-lexical-environments/*/concise-arrow-binding-parameter`
   commands of the 530); the complete 530 replay caught it and the order was
   corrected before the final runs.
   Ordinary functions/methods/accessors/constructors open `other` frames with
   no receiver, so no super temp can be hoisted inside them; they keep the
   generic visit. The former `DecoratorDefinitionBindings::temporaries` list is
   gone: decorator receiver caches and computed-name cache temps hoist into the
   class IIFE environment, whose `var` statement is merged first (before
   `let _outerThis = this`), as `mergeLexicalEnvironment` does.
6. **Ranges.** Reads: `Reflect.get(...)` takes original node and text range of
   the `super` token. Assignment: the getter read takes the left operand,
   the compound binary and the `Reflect.set` take the whole node, the result
   temp identifiers take the node's range. Update: `Reflect.get` and
   `Reflect.set` take the node, the expansion assignment takes the operand.
   Destructuring wrappers take the target node. Setter-wrapper parameters are
   unhoisted temps (`createTempVariable(undefined)`).
7. **Pending expressions.** Every member decorators assignment queues in
   member order; a referenced (decorated) computed name or an undecorated
   non-inlineable computed name absorbs the queue entries after the
   class-fields boundary; the v18 emulation block for decorated computed fields
   is kept; leftovers print right after `const _metadata` in the decoration
   block, one statement each.
8. **Error paths.** A `TransformError` aborts the whole standard-decorator
   transform (the visitor and its frames are dropped); no environment or frame
   restoration is attempted on `Err`.
9. **`_outerThis`.** Applied only to the pending expressions that become
   leading-block statements (tsc `thisVisitor`); class decorators and pending
   expressions injected into a computed name keep `this` (witness
   `decorator-super-extra/*/phase-order/outer-this-and-cache-temp`, tsc probe
   `holder.make()` shapes). Computed-name cache temps are
   `getGeneratedNameForNode(ComputedPropertyName)` temps, i.e. reserved in
   nested scopes, so a temp hoisted inside a nested function scope skips them.
10. **Anonymous heritage.** `safeExtendsExpression`: an anonymous class or
    function expression (or arrow) as the base becomes `(0, base)` so the
    `_classSuper` declaration performs no named evaluation.
11. **Helper request order.** `__runInitializers` is requested where
    `createClassInfo` requests it (before class decorators); `__propKey` where
    `visitReferencedPropertyName` requests it (before the class-level
    `__esDecorate` / `__setFunctionName` requests).
12. **Class-fields dependencies (owner: `class_fields/downlevel.rs`).**
    (a) ES2022+: a public static field with an initializer under set semantics
    stays in the class as `static { this.x = init; }`
    (`transformPublicFieldInitializer` /
    `addPropertyOrClassStaticBlockStatements` skip statics when
    `!shouldTransformPrivateElementsOrClassStaticBlocks`), never a trailing
    statement and never an alias-forcing static operation — this fixes every
    class in a file flagged `TransformPrivateStaticElements`, including the
    undecorated `Base` and nested decorated class expressions.
    (b) ES2015: with `emitNode.classThis` (the standard-decorator IIFE shape)
    `createClassTempVar` makes no `classAliases` entry, so source references
    to the class name inside relocated static initializers are not
    substituted (`new Derived()`, `Derived.f()`, `__classPrivateFieldGet(A, …)`).
    (c) ES2015: `ClassWasDecorated` (the parsed class carries decorators)
    suppresses `NeedsClassSuperReference` — a nested class's `super` inside a
    decorated class's static block no longer allocates `(_a = _classSuper)`.
    (d) ES2015: the class-fields super lowering takes the same original
    node / text range rules as the standard-decorator lowering (`Reflect.get`
    of a read carries the `super` token; update expansion carries operand /
    node ranges; the result temp carries the node range), and the update
    sequence is parenthesized by the call-argument rule (with its range)
    rather than an explicit range-less pair.
    (e) A decorated computed field now carries tsc's injected name
    `[(…pending, _a = __propKey(k))]` (decision 7). Both class-fields paths
    therefore resolve the field's key through
    `findComputedPropertyNameCacheAssignment` over the comma sequence: the
    ES2022 visitor's `computed_name_binding` reuses the cache identifier's
    generated binding (`generated_binding_of_identifier`) instead of hoisting
    a second temp, and the downlevel's `plan_public_field_name` reads the
    cache assignment at the end of the sequence (through the computed
    name's parenthesized wrapper).
13. **Decorated computed fields.** v18 emitted `[_a]` for a decorated field
    and kept the cache assignment in a separate static block (the
    class-field emulation block). The 530 frozen commands contain no plain
    decorated computed field (only two `accessor` ones), so that shape was
    unwitnessed; the candidate emits tsc's shape and lets class-field
    lowering fold the key evaluation into the next computed name or emit it
    before the class (extra witnesses `phase-order/decorated-field-*`).

14. **Arrow parameter defaults that hoist (follow-up).** tsc's
    `visitParameterList` sets `InParameters` while an arrow's parameters are
    visited; a super update / compound assignment in a default calls
    `hoistVariableDeclaration` (result temp, expand temp), which turns on
    `VariablesHoistedInParameters`, and for targets >= ES2015
    `addDefaultValueAssignmentsIfNeeded` moves every default of that list
    into the body: `p = init` becomes `if (p === void 0) { p = init; }`
    (name clones with NoSourceMap, block SingleLine|NoTrailingSourceMap|
    NoTokenSourceMaps|NoComments, both ranged to the parameter), a binding
    pattern becomes a generated parameter `_a` plus
    `var pattern = _a === void 0 ? init : _a;` (one `TargetBinding` shared by
    the parameter and both reads, finalizer-named like every other auto temp).
    The statements join the arrow's environment through
    `add_initialization_statement` and `visit_function_body` merges them after
    the hoisted `var`s; an expression body is converted to a block. A nested
    arrow starts its own environment, so its hoist never moves the outer
    defaults (`param-default/nested-arrow-in-default`); a read-only default
    hoists nothing and keeps the native default. The port lives on
    `visit_arrow_function` only: non-arrow functions reset `classThis` /
    `classSuper`, so no super form can hoist inside their parameters.
15. **`visitParameterDeclaration` (follow-up).** Every parameter reached by
    the visitor is rebuilt without modifiers / question token / type; when
    the name or the initializer changed, the updated parameter takes the
    original's comment range, text range and source-map range
    (`moveRangePastModifiers` is the node itself after the TypeScript
    transform) and its **name loses its trailing source map**. The missing
    `NoTrailingSourceMap` produced one extra mapping (the end of `a` in
    `(a = super.x) => a`) on every parameter-default witness, including the
    read-only control. The named-evaluation arm (an anonymous decorated
    class as the default) is not ported and not witnessed.
16. **`createStringLiteralFromNode` text sources (follow-up).** Every string
    literal tsc builds from a name node keeps that node as `textSourceNode`,
    and the printer's `getLiteralTextOfNode` re-reads the identifier's
    source spelling (`super.\u0078` → `Reflect.get(_classSuper, "\\u0078", …)`,
    a tsc quirk the port must reproduce) or the literal's original token
    (`'m 2'` verbatim). The port's `create_string_literal_from_node` sets
    `string_literal_text_source` (the printer branch existed for
    destructuring / module names) and is now used for: the super property
    key of read / call / tag / assignment / update / destructuring targets,
    the decorator context `name:` (identifier, private, string, numeric
    names), the access object's `has` key, the `get` / `set` element access
    of a string / numeric member name (`obj['m 2']`, `obj["1"]` — tsc treats
    such names as computed), and the `__setFunctionName` class name of a
    declared class (`"\\u0044erived"` below ES2022). Names assigned by named
    evaluation (`const \u0044erived = @dec class {}`) still use the text
    (unmeasured, §6).
17. **Helper variable names for literal member names (follow-up).**
    `getHelperVariableName` uses the member name text only for identifiers,
    private names and string literals that are identifier text; `'m 2'` and
    `1` fall back to `member` (`_static_member_decorators`). The port derived
    the helper name from the decoded plan name (`_static_m 2_decorators`);
    `helper_variable_base_name` now follows tsc.
18. **es2018 flattened-assignment range (follow-up, `es2018.rs`).** tsc's
    `flattenDestructuringAssignment` returns `inlineExpressions(expressions)`
    unchanged: a single flattened assignment keeps the element range
    `emitAssignment` gave it, and a synthesized comma chain carries no range
    or original. The plan-based es2018 lowering re-ranged the result to the
    whole destructuring assignment, which moved the trailing mapping of
    `({ ...super.list } = value)` (rest element wrapped by the setter object)
    from the rest element's end to the assignment's end — the two
    `es2015/*/object-target/rest` rows of §5. The re-ranging is removed; the
    shared ES2015 flattener never had it.

19. **Named evaluation of anonymous decorated class expressions (second
    follow-up).** The port inferred an assigned class name only for variable
    declarations (`record_variable_class_name`) and only when the initializer
    was the bare class expression. tsc applies
    `isNamedEvaluation(node, isAnonymousClassNeedingAssignedName)` →
    `transformNamedEvaluation` in every source the esDecorators visitor
    handles: parameter and binding-element defaults (identifier names, no
    rest), variable declarations, `=` / `&&=` / `||=` / `??=` assignments to
    an identifier, property assignments (identifier / string / numeric /
    private names; a computed name whose expression is a non-identifier
    literal; any other computed name rewritten to `[_a = __propKey(k)]` with
    the hoisted temp as the name) and export assignments (`"default"`),
    skipping the initializer's outer expressions (parentheses, partially
    emitted expressions). The class must be anonymous, decorated or
    member-decorated, and without an explicitly assigned name. The port
    records the assigned name (`inferred_class_names`) and its source
    (`inferred_class_name_sources`: the name node for
    `createStringLiteralFromNode`, the temp binding, or `"default"`) before
    visiting the initializer; `transform_class_like` installs it through the
    existing `__setFunctionName` block, now with the source spelling
    (`__setFunctionName(_classThis, "\\u0061")`, `__setFunctionName(_classThis, _a)`).
20. **Private names with escapes (second follow-up).** The private
    method / accessor descriptors' `__setFunctionName(function …, "#\\u006d")`
    and the decorator context `name: "#\\u006d"` take the private identifier
    as text source; the printer's text-source branch and its
    source-spelling reuse for transformed names now accept
    `PrivateIdentifier` (tsc's `getLiteralTextOfNode` / `getTextOfNode` treat
    identifiers and private identifiers alike). `printer.rs` is therefore a
    fifth production file of the candidate (own patch).

21. **Helper request timing around the member passes (second follow-up).**
    The port requested every class-level helper (`__esDecorate`,
    `__runInitializers`, a declared name's `__setFunctionName`) after the
    first member pass, before the members' bodies were visited. tsc requests
    a member decorator's `__esDecorate` during the first pass
    (`partialTransformClassElement`), the class decorators' `__esDecorate`
    and `__runInitializers` after the second pass (_tsc.js:99494/99520), and
    a declared class's `__setFunctionName` below ES2022 only in the
    class-fields transform; `__esDecorate` / `__runInitializers` carry
    priority 2 and sort ahead of the unprioritized helpers, so only the
    request order *within* each group is observable. A decorated class
    nested in a member body (`param-default-member-decorated-class`:
    `__runInitializers` from the inner class's `createClassInfo` precedes
    the outer class decorators' `__esDecorate`) and a `__propKey` requested
    inside a static block (`property-computed-expression` below ES2022:
    ahead of the outer declared name's `__setFunctionName`) exposed the
    difference. The class-level requests of a class without decorated
    members and the declared-name `__setFunctionName` now follow the second
    member pass.
22. **Reserved `__propKey` temp (second follow-up).**
    `getGeneratedNameForNode(ComputedPropertyName)` is an auto temp reserved
    in nested scopes; the class IIFE's `_metadata` temp therefore skips `_a`
    (`var _b;` in tsc). The property-assignment temp of decision 19 is
    allocated with `TargetBinding::allocate_reserved_in_nested_scopes`.
23. **ES2021 logical-assignment result range (second follow-up,
    `es2021.rs`).** tsc's `transformLogicalAssignment` returns the created
    `left ?? (left = right)` binary expression without a text range or
    original; the port re-ranged it to the assignment, adding a trailing
    mapping at the assignment's end (`assignment-logical` at ES2015). The
    re-ranging is removed (own patch `candidate-es2021-logical-range.patch`).
24. **Private auto-accessor storage spelling (second follow-up).**
    `createAccessorPropertyBackingField` names the storage through
    `getGeneratedPrivateNameForNode(name, …, "_accessor_storage")`, i.e. from
    the private name's source text (`#\\u0061_accessor_storage`, lowered to
    `_Derived_\\u0061_accessor_storage`); the port used the decoded name.
    `private_name_source_spelling` feeds `allocate_private_storage`.

25. **Class-fields parameter-default and concise-body ranges (third
    follow-up, `class_fields/downlevel.rs`).** The undecorated
    `param-default-undecorated-control` rows at ES2015 differed only in the
    source map: the class-fields transform's own `visitParameterList` path
    (`lower_function_parameter_defaults`, reached when a temp is hoisted
    while the parameters are visited) built `if (a === void 0) { a = init; }`
    with `NoSourceMap` on the whole assignment and no text range on the
    assignment or the block, and `install_function_bindings` converted the
    arrow's concise body without ranging the return statement. tsc's
    `addDefaultValueAssignmentForInitializer` puts `NoSourceMap` on the
    assigned name and the initializer only and gives the assignment and the
    single-line block the parameter's range (mappings at `{`, at `a` and at
    the end of `init`, all to the parameter), and `convertToFunctionBlock`
    ranges both the return statement and the block to the body (`return`
    and its end map to the body expression). `lower_parameter_default` and
    `install_function_bindings` now mirror the standard-decorator port of
    the same two functions (decision 14 / `convert_to_function_block`).
26. **Helper request order of relocated statics (third follow-up,
    `class_fields/downlevel.rs`, `transform.rs`, `printer.rs`, `helpers.rs`).**
    Below ES2022 tsc drops static blocks and static field initializers from
    the member pass (`visitClassStaticBlockDeclaration` returns nothing,
    `transformPublicFieldInitializer` / `transformPrivateFieldInitializer`
    elide the field) and visits them only in
    `addPropertyOrClassStaticBlockStatements`, after every member body and
    the constructor. Helpers requested there (`__classPrivateFieldIn` from a
    decorator context's `has: obj => #m in obj`, `__classPrivateFieldGet`
    from `this.#m()`) therefore follow the helpers of private method bodies
    and instance initializers, while two relocated statics keep their own
    order. The port plans statics in member order; the w5 lane S commit
    `6e4516d38` compensated with a printer-side reordering
    (`order_private_field_helpers`: below ES2022 move `__classPrivateFieldIn`
    behind the last get/set helper), which is right for a static block
    ahead of a private method body but wrong when the `in` helper precedes
    the get helper inside the relocated statics themselves — the
    `private-name` rows (decoration block: `has` before `get`, then the
    user's `this.#m()`) and any plain class whose static blocks use `#x in`
    before `this.#x`. The plan now parks the requests made while it visits a
    relocated static (`visit_relocated_static` →
    `TransformationContext::defer_emit_helper_requests`, a nesting stack of
    buffers deduplicated by name) and re-issues them after the instance
    operations (`request_relocated_static_helpers` →
    `request_emit_helper`, which keeps an earlier position for a helper
    requested in the meantime); nested classes inside a static block
    re-issue into the enclosing buffer. The printer's helper order is the
    stable priority sort of `compareEmitHelpers` alone; the reordering is
    removed. At ES2022 and later nothing is relocated and the visit runs
    unparked. Witnesses: `decorator-super-followup3` (§4).
27. **Lowered private `in` keeps no text range (third follow-up,
    `class_fields/downlevel.rs`).** The `helper-order` witnesses exposed two
    extra mappings on every lowered `#x in obj` (the start and end of the
    `__classPrivateFieldIn(...)` call mapped to the binary expression):
    `transformPrivateIdentifierInInExpression` only sets the original node
    on the helper call, so the call itself emits no mappings and the
    receiver keeps its own; the port also copied the text range
    (`set_original_and_range`). Original node only now; every target band
    that lowers the expression (ES2015 relocation, ES2022+
    `TransformPrivateStaticElements`) shares the arm.
28. **Synthetic member order at ES2022+ (third follow-up,
    `class_fields/downlevel.rs`).** `transformClassMembers` rebuilds the
    member list when it synthesizes a constructor (instance fields under set
    semantics) or the pending-expressions static block (private static
    definitions of a `TransformPrivateStaticElements` class): the class-this
    assignment block, the named-evaluation helper block, the synthetic
    constructor, the synthetic static block, then every other member in
    order. The port inserted the synthetic constructor at index 0 (ahead of
    the standard-decorator transport blocks) and placed the pending block
    right after the transport blocks even when a synthetic constructor
    existed. `arrange_synthetic_members` applies the tsc order in both
    places; `install_instance_operations` now returns the constructor it
    synthesized so the pending block can follow it. Below ES2022 the
    transport blocks are not retained members, so nothing changes there.
    Witnesses: `decorated-instance-get-then-static-in` (the constructor
    behind the transport blocks) and
    `decorated-instance-then-private-static-method` (the pending block
    behind the synthetic constructor) at `es2022/set` and `esnext/set`.

## 4. Witnesses

Manifest generator `scripts/generate-decorator-super-inputs.mjs` →
`crates/compiler/tests/fixtures/decorator-super-inputs.json`: 112 variants ×
(ES2015, ES2022, ESNext) × (set, define) = **672** cases, IDs
`decorator-super/<target>/<set|define>/<family>/<variant>`, families read (5),
call-tag (9), assign (7), compound (12), logical (8), update (11), discard (13),
array-target (8), object-target (8), receiver (12), phase-order (6), handoff (10),
fault (3). Observer `scripts/observe-decorator-super.mjs` (two matching
Program executions per case, complete command tuple) →
`crates/compiler/tests/fixtures/decorator-super.json`: **670 complete
observations, 2 upstream exceptions** (`esnext/set/handoff/static-private-accessor`
and `static-public-accessor`: tsc `Debug Failure: Undeclared private name for
property declaration` in `transformPrivateFieldInitializer`; recorded, no credit).
Native replay: `crates/compiler/tests/decorator_super_contract.rs` →
`integration/h2_8a_decorator_super.rs` (own comparator over the shared
`assert_completed_observation`; two program constructions per pass; failed cases
re-run once; complete captures per pass; write-failure injected through a
recording `OutputSink`). The fixture is loaded at run time so one binary built
before the change replays before and after (`fixture_sha256` is recorded in
every capture). Runtime control `scripts/decorator-super-runtime-control.mjs`
executes upstream and native JavaScript with the same event log.

Extra controls (`--extra`): `decorator-super-extra-inputs.json` → 4 variants ×
6 = **24** cases, IDs `decorator-super-extra/<target>/<mode>/<family>/<variant>`
(`phase-order/outer-this-and-cache-temp`, `handoff/heritage-anonymous-class`,
`handoff/heritage-anonymous-function`, `handoff/heritage-named-class`), observed
into `decorator-super-extra.json` (24 complete, 0 exceptions) and replayed by
`decorator_super_extra_forms_match_complete_typescript_observations`. They live
apart from the primary set so the binary built at the restored base keeps
replaying the frozen 672; their own "before" replay uses a second binary built
from the restored-base worktree `~/dev/tsc-rs-dec-super-base` (commit
13f5767ec + the same test files).

Follow-up witnesses (`--followup`, 2026-09-15):
`decorator-super-followup-inputs.json` → 26 variants × 6 = **156** cases, IDs
`decorator-super-followup/<target>/<mode>/<family>/<variant>`:
`param-default` (10: prop/elem postfix and prefix updates, compound, binding
pattern, second parameter, static-block IIFE, read-only control, discarded
postfix in a comma, nested arrow), `unicode-name` (15: `super.\u0078` read /
assign / compound / update / call / tag / object- and array-target / logical,
`\u{78}`, an escaped decorated member name, an escaped class name, a
single-quoted and a numeric decorated member name) and `phase-order` (1: the
class-declaration shape of the decorated computed field whose decorator uses
lexical `this`). Observed into `decorator-super-followup.json.zst` (156
complete, 0 diagnostics, 0 exceptions) and replayed by
`decorator_super_followup_forms_match_complete_typescript_observations`
(`TSC_RS_DECORATOR_SUPER_FOLLOWUP_CASE_SET`). Its "before" is the candidate at
`after-10` bytes (the clippy repairs of §7 are behavior-neutral): binary
`decorator_super_contract.before-followup`, run `before-followup`.

Direct controls for the value-use memo (`scripts/observe-decorator-super-direct.mjs`
→ `crates/emitter/tests/fixtures/decorator-super-direct.json`, replayed by
`crates/emitter/tests/decorator_super_direct_contract.rs`): 7 shapes × (ES2015,
ES2022) × (set, define) = **28** cases, IDs
`decorator-super-direct/<target>/<mode>/<shape>`. No Program input reaches these
shapes (no transform ahead of `transformESDecorators` emits a
`CommaListExpression` or shares an expression node), so both sides inject them
into the parsed tree — tsc through a `customTransformers.before` transformer
of the complete `program.emit`, the port by rebuilding the parsed arena before
`get_script_transformers` runs — and compare the emitted JavaScript text (two
repetitions): a synthetic comma list as a discarded statement, as a used call
argument (assignment first / assignment last / nested parenthesized comma),
and one expression node shared by two statements in the orders
discarded→used, used→discarded and a postfix update discarded→used. The memo
policy of §3 (required-value lowering memoized per node id, discarded visits
of the mode-sensitive kinds bypass the memo) reproduces tsc for every shape.
A node shared by two *required* contexts would be memoized once where tsc
allocates a second temp; no producer creates that shape, so it stays a
recorded reasoning, not a control.

Second follow-up witnesses (`--followup2`):
`decorator-super-followup2-inputs.json` → 27 variants × 6 = **162** cases,
IDs `decorator-super-followup2/<target>/<mode>/<family>/<variant>`:
`named-evaluation` (21: parameter defaults — decorated, member-decorated,
undecorated control, escaped name, hoist + named, parenthesized; variable —
plain, escaped, parenthesized; assignment — plain, escaped, `??=`; property —
plain, escaped, string, numeric, computed literal, computed expression;
binding element — plain, escaped; export default expression) and
`private-name` (6: escaped private method / field / getter / setter /
auto-accessor, plain control). Observed into
`decorator-super-followup2.json.zst` (162 complete, 0 diagnostics, 0
exceptions; the member-decorated variant names its method `m3` so it does
not conflict with `Base.m`) and replayed by
`decorator_super_followup2_forms_match_complete_typescript_observations`
(`TSC_RS_DECORATOR_SUPER_FOLLOWUP2_CASE_SET`). Its "before" is the
`after-13` candidate (binary `decorator_super_contract.before-followup2`,
run `before-followup2`).

The direct controls grew to 8 shapes × 4 = **32** cases: the added
`shared-node-used-twice` shape (one super assignment node shared by two
required-value call arguments) is a **recorded divergence**: tsc lowers the
node twice (`var _a, _b;`, `_b` in the second lowering) while the port's memo
returns the first lowering for the second required visit; the test pins the
port's output as exactly that rewrite of tsc's text (`var _a;`, `_a` twice)
and gives it no credit. No producer ahead of `transformESDecorators` shares an
expression node, so the memo stays as designed (memoization/identity audit
across producers remains a parent A6-41 item).

Third follow-up witnesses (`--followup3`, 2026-09-15):
`decorator-super-followup3-inputs.json` → 8 variants × 6 = **48** cases, IDs
`decorator-super-followup3/<target>/<mode>/helper-order/<variant>`: plain
classes `plain-static-in-then-method-get` (a static block's `#x in this`
ahead of a private method body's `this.#x`), `plain-static-blocks-in-then-get`
(two static blocks, `in` first — the shape the removed printer reordering
inverted), `plain-static-field-get-then-block-in` (a static field initializer
ahead of a static block), `plain-instance-get-then-static-in` (an instance
initializer `Derived.#x` ahead of a static block), and
`plain-nested-class-in-static-block` (a private method's `#x in this` ahead of
a static block whose nested class requests the get helper); decorated classes
`decorated-static-in-then-method-get` (a decorated public method, a static
block `#x in this`, then an undecorated private method body — the shape the
w5 reordering was written for), `decorated-instance-get-then-static-in` and
`decorated-instance-then-private-static-method` (an instance field, so the
ES2022 set-mode constructor is synthetic, next to a private static method
whose definition lands in the pending-expressions static block — the member
order of decision 28). Observed into `decorator-super-followup3.json.zst`
(48 complete, 0 diagnostics, 0 exceptions; the instance variants read the
static field as `Derived.#x` and declare it first, because `this.#x` in an
instance initializer is 2339 and a later static declaration is 2729 under
`useDefineForClassFields` at ES2022+)
and replayed by `decorator_super_followup3_forms_match_complete_typescript_observations`
(`TSC_RS_DECORATOR_SUPER_FOLLOWUP3_CASE_SET`). tsc's observed orders confirm
decision 26: get before in wherever a member body or instance initializer
requests it, in before get when both live in relocated statics, and at
ES2022 / ESNext the member-pass order (the decorated shapes keep `in` first
there; `decorated-instance-get-then-static-in` at `es2022/define` is get
first). Beyond the helper order the set also exposed decisions 27–28 (the
lowered `in` range, the synthetic member order). Its "before" is the
`after-16` candidate production rebuilt with the same test files in the
restored-base worktree (`before-followup3`).

Fixture storage: the observation fixtures are zstd-compressed
(`decorator-super.json.zst` 84 KB for 15.9 MB of JSON, `-extra` 19 KB,
`-followup` 28 KB, `-followup2` 34 KB; level 19, written by the observer
through `node:zlib`).
The replay decodes them at run time (`zstd` dev-dependency of
`tsc-rs-compiler`, already in the lock file), every `fixture_sha256` in a
capture is the decoded JSON's SHA-256 (the primary fixture's decoded hash is
the one the earlier receipt pinned for `decorator-super.json`), and the receipt
pins both the compressed file and the decoded hash. The input manifests stay
plain JSON.

## 5. Results (final candidate bytes, runs `after-18`; the `after-16` column is the second follow-up's record)

Every count is a distinct population and must not be summed. Receipt:
`ratchets/h2-8a-decorator-super-design-experiment.v1.json` (pins production,
fixture (compressed and decoded), script, test and patch SHA-256, every run's
prelaunch/terminal records, per-case classifications, the runtime-control
summaries and the adjacent emitter suite result).

| Population | Before (restored base) | After (candidate, `after-16`) | Final (`after-18`, third follow-up) |
| --- | --- | --- | --- |
| Complete commands, primary 672 (670 complete + 2 upstream exceptions, no credit) | 152 exact ×2, 518 repeated failures (actual exit 101) | **670 exact ×2**, 0 failures (actual exit 0) — the two `es2015/*/object-target/rest` rows of the earlier `after-10` runs are closed by decision 18 | **670 exact ×2**, 0 failures (actual exit 0) |
| Complete commands, extra controls 42 | 7 exact ×2, 35 failures | **42 exact ×2**, 0 failures (actual exit 0) | **42 exact ×2**, 0 failures |
| Complete commands, follow-up witnesses 156 (before = the `after-10` candidate bytes) | 31 exact ×2, 125 repeated failures (`param-default` 10/60, `unicode-name` 15/90, `phase-order` 6/6) | **156 exact ×2**, 0 failures (actual exit 0) | **156 exact ×2**, 0 failures |
| Complete commands, second follow-up witnesses 162 (before = the `after-13` candidate) | 38 exact ×2, 124 repeated failures | **150 exact ×2**, 12 repeated failures (all adjacent owners, §5.1 / §6) | **162 exact ×2**, 0 failures (the 12 ES2015 rows closed by decisions 25–26) |
| Complete commands, retained 530 (existing owner test) | 530 exact ×2, 0 failures (actual exit 0) | **530 exact ×2**, 0 failures (actual exit 0) — 0 regressions | **530 exact ×2**, 0 failures — 0 regressions |
| Complete commands, third follow-up witnesses 48 (before = the `after-16` candidate production rebuilt with the same test files) | — | 26 exact ×2, 22 repeated failures (`before-followup3`: 8 JavaScript differences — the inverted helper order of `plain-static-blocks-in-then-get` / `plain-nested-class-in-static-block` at ES2015 and the synthetic member order of the two decorated instance variants at `es2022/set` / `esnext/set` — and 14 map-only rows, the lowered `in` range) | **48 exact ×2**, 0 failures (actual exit 0) |
| Runtime control (second follow-up) | — | 115 / 115 agree (20 not executable upstream, 27 skipped) | 115 / 115 agree |
| Runtime control (third follow-up) | — | — | 40 / 40 agree (8 skipped: esnext/define) |
| Runtime control (same-runtime event logs, primary) | — | 553 / 553 agree (117 not executable: esnext/define native decorators, the no-emit fault) | 553 / 553 agree |
| Runtime control (extra) | — | 35 / 35 agree (7 not executable) | 35 / 35 agree |
| Runtime control (follow-up) | — | 130 / 130 agree (26 not executable: esnext/define native decorators) | 130 / 130 agree |
| Direct controls (`decorator_super_direct_contract`, JavaScript text) | — | **28 exact ×2** + 4 recorded divergences ×2 (synthetic comma lists, shared nodes; §4) | unchanged (28 + 4) |
| Direct metadata controls | — | the 4 static-accessor printed-output controls and the 8 template-provenance controls stay in the emitter contract suite (§7) | unchanged |

### 5.1 Second follow-up (`after-16`)

The 162 `decorator-super-followup2` witnesses were replayed on the `after-13`
candidate first (`before-followup2`: 38 exact — the undecorated / plain
controls and every esnext/define case; every named-evaluation and
private-name row failed) and on the final bytes (`after-16`: **150 exact
×2**). The intermediate `after-15` bytes (decisions 19–20 only) reached
136/162; decisions 21–24 closed `param-default-member-decorated-class`
(helper order), `assignment-logical` at ES2015 (es2021 range),
`property-computed-expression` at ES2015 (helper order and the reserved
temp) and `private-accessor-escape` (storage spelling). The 12 rows still
differing are ES2015-only and belong to adjacent owners recorded in §6:
`named-evaluation/param-default-undecorated-control` (set, define: source map
of the class-fields / ES2015 parameter-default lowering of an undecorated
class) and `private-name/{private-method,private-field,private-getter,
private-setter,private-plain-control}` (set, define: the downlevel's
`__classPrivateFieldGet` request preceding `__classPrivateFieldIn`; the same
rows are exact at ES2022 and ESNext, and the escaped names themselves print
correctly). The four recorded `shared-node-used-twice` divergences of the
direct controls are counted separately (§4).

Intermediate follow-up runs: `after-11-followup` (146/156: the 10
literal-member-name rows failed on the helper variable name, decision 17) and
`after-12-{followup,new,extra}` (156/156, 670/670, 42/42 at bytes that still
carried a debug assertion in `visit_parameter_declaration`; the `after-12`
530 replay executed the emitter `contracts` binary by mistake and was
discarded, and the `after-12` emitter suite exposed the assertion on erased
parameter decorators); the `after-13` emitter suite (`runs/after-13-emitter-suite/`)
is green for the library and contracts targets and red only for the first
version of the direct-control test, which rebuilt the class ahead of the
TypeScript transform (its `lexical_scope_owner` walks the parsed `parent`
chain); the test now injects after `transformTypeScript` and the clean record
is `runs/after-13b-emitter-suite/`. Before/after classification
(`scripts/decorator-super-compare.py`): primary 670 exact; extra 42 exact;
follow-up 156 exact.

The `after-14` / `after-15` chains preceded `after-16` (a doc-comment
repair between `after-14` and `after-15`, decisions 21–24 between `after-15`
and `after-16`); every population was replayed again at the final bytes.
Receipt SHA-256 at these bytes (superseded by the third follow-up's receipt,
§5.2): `63dcd9f31536e66b7afc03dc62ec10c3b0c4873a8334891aa91f0ad57f230c62`.
Binaries executed (SHA-256) are recorded per run in the receipt
(`runs.*.binaries`); the follow-up before-binary is
`decorator_super_contract.before-followup`
(`cd28661f0a26f17bf183ef418b95c3b84c1d3f78ebe901e8e3588ea611f49cd6`).

### 5.2 Third follow-up (`after-17` / `after-18`)

The 12 rows of §5.1 were replayed first on the `after-17` bytes (decisions
25–26 only): `decorator-super-followup2` **162 exact ×2**, and the whole
chain (`run-after-17-chain.sh`: clippy, `followup2` 162/162, `followup`
156/156, primary 670/670 + 2 exceptions, extra 42/42, retained 530/530,
emitter suites 494 / 452 / 1) was green at those bytes. The third witness
set (`decorator-super-followup3`, §4) was then observed and replayed on the
same production (`after-17b`, the test set added to the binary): its
JavaScript was exact on every ES2015 row, and its source maps and the two
`es2022/set` / `esnext/set` decorated instance rows exposed decisions 27–28.
Those two fixes changed production, so every population was replayed again
at the final bytes (`run-after-18-chain.sh` then `run-followup3.sh`): the
"Final" column of §5, all with actual exit 0, plus `after-18b-followup3`
**48 exact ×2** (binary `decorator_super_contract.after-18b`, the same
production SHA-256 as the `after-18` chain per `prelaunch.json`) against
`before-followup3` 26 / 22 (binary `decorator_super_contract.before-followup3`
built in `~/dev/tsc-rs-dec-super-base` from the `after-16` production —
its `prelaunch.json` production hashes equal the after-16 receipt's — with
the final test files). Before/after classification
(`scripts/decorator-super-compare.py`): primary 670 exact, extra 42, follow-up
156, second follow-up 162, third follow-up 48. Evidence assembly
`finalize-4.sh` (run once more after `before-followup3` had finished; the
first pass had read that run's captures while it was still writing).
Receipt SHA-256 at the final bytes:
`1cd9b70839fe032933cc5ddc2cd3f79c2f4685c9e1568879da56e650c245bfbc`.

## 7. Test suite

Adjacent emitter suites on the final bytes
(`target/h2-8a-decorator-super/runs/after-18-emitter-suite/`, actual exit
0; the `after-16` and `after-17` suites recorded the same counts): library
**494 passed / 0 failed**, contracts **452 passed / 0 failed**, direct
controls **1 passed** (28 exact ×2 + 4 recorded divergences ×2). `cargo fmt
--all -- --check` is clean; `cargo clippy -p tsc-rs-emitter -p
tsc-rs-compiler --tests -- -D warnings` is clean at the final bytes
(`logs/clippy-after-18.log`, with the third follow-up test set; the three
candidate-v2 `class_fields.rs` findings — two unnecessary `as i32` casts,
one type complexity — are repaired in the visitor patch).

## 6. Remaining rows (recorded, not closed)

Closed by the 2026-09-15 follow-up (kept here as the record of what §6
listed before it): the es2018 object-rest wrapper range (decision 18, the two
`es2015/*/object-target/rest` rows), parameter defaults inside arrows in
static initializers (decisions 14–15, `param-default` family), unicode-escaped
identifier property names (decision 16, `unicode-name` family), the
decorated computed field with a lexical-`this` decorator and no later computed
name (the arrow form was already exact in the extra witnesses; the
class-declaration shape is now witnessed too), and the memo policy proof for
shared / synthetic comma-list nodes (direct controls, §4).

Closed by the second follow-up (§5.1): named evaluation of anonymous
decorated class expressions in every esDecorators source (decision 19),
assigned class names and private names with escapes (decisions 19–20), and
the shared node reached twice in required-value mode (measured as a recorded
divergence of the memo policy, §4).

Closed by the third follow-up (§5.2): the ES2015 parameter default of an
undecorated class expression (`named-evaluation/param-default-undecorated-control`,
both modes — the class-fields downlevel's own `addDefaultValueAssignmentForInitializer`
/ `convertToFunctionBlock` ranges, decision 25) and the ES2015
`__classPrivateFieldIn` / `__classPrivateFieldGet` order of every
`private-name` row (the relocated-static helper request order, decision 26,
which also retires the w5 printer reordering `order_private_field_helpers`;
its own witnesses are `decorator-super-followup3`, which in turn closed the
lowered `in` range and the ES2022+ synthetic member order, decisions 27–28).

Still open:

- **Shared node in two required contexts** stays a recorded divergence
  (memo policy); closing it means re-lowering per visit like tsc, which is
  the memoization/identity audit of the parent slice.
- **A6-41 parent items untouched:** FileLevel census / global-name oracle
  overhaul, memoization/identity audit across producers.

## 8. Deliverables and reapplication

All paths are relative to the worktree; nothing beyond the restored-base
commit `13f5767ec` is committed (the candidate is the working tree), and root
production is untouched.

Production candidate patches (diffs against `13f5767ec`; apply in this order
with `git apply --check` then `git apply` from a worktree restored per
handoff §2):

1. `h2-8a-decorator-super.candidate-standard-decorators.patch` —
   `crates/emitter/src/builtins/standard_decorators.rs` (A6-41-SUPER owner:
   §2 rows, decisions 1–11, 13, and the follow-up decisions 14–17).
2. `h2-8a-decorator-super.candidate-class-fields-downlevel.patch` —
   `crates/emitter/src/builtins/class_fields/downlevel.rs` (dependency rows
   §3.12 a–e, ES2015 side).
3. `h2-8a-decorator-super.candidate-class-fields-visitor.patch` —
   `crates/emitter/src/builtins/class_fields.rs` (dependency row §3.12 e,
   ES2022 side: `computed_name_binding` reuses the cache identifier's
   generated binding; plus the three clippy repairs of §7: two `as i32`
   casts removed, the `VisitedFunctionParts` alias).
4. `h2-8a-decorator-super.candidate-es2018-flatten-range.patch` —
   `crates/emitter/src/builtins/es2018.rs` (decision 18, the flattened
   destructuring assignment keeps its element range).
5. `h2-8a-decorator-super.candidate-printer-private-name.patch` —
   `crates/emitter/src/printer.rs` (decision 20: private-identifier text
   sources and source-spelling reuse).
6. `h2-8a-decorator-super.candidate-es2021-logical-range.patch` —
   `crates/emitter/src/builtins/es2021.rs` (decision 23).
7. `h2-8a-decorator-super.candidate-helper-request-order.patch` —
   `crates/emitter/src/transform.rs` (`defer_emit_helper_requests` /
   `resume_emit_helper_requests`) and `crates/emitter/src/builtins/helpers.rs`
   (`order_private_field_helpers` removed) (decision 26; the downlevel side
   is in patch 2 with decisions 25, 27 and 28, the printer side in patch 5).
8. `h2-8a-decorator-super.candidate-witness-tooling.patch` — the test
   targets `crates/compiler/tests/decorator_super_contract.rs` +
   `integration/h2_8a_decorator_super.rs` (five sets, zstd loader) and
   `crates/emitter/tests/decorator_super_direct_contract.rs` (with its
   fixture `decorator-super-direct.json`), the five input manifests, the
   `zstd` dev-dependency (`crates/compiler/Cargo.toml`, `Cargo.lock`), and the
   `scripts/*decorator-super*` / `generate-decorator-super-inputs.mjs` /
   `observe-decorator-super.mjs` / `observe-decorator-super-direct.mjs`
   tools. The observation fixtures `decorator-super.json.zst` (670 + 2),
   `decorator-super-extra.json.zst` (42), `decorator-super-followup.json.zst`
   (156), `decorator-super-followup2.json.zst` (162) and
   `decorator-super-followup3.json.zst` (48) are regenerated from the
   manifests with the observer (`--write`, two matching Program executions
   per case) and are kept in the worktree.

Evidence directory: `target/h2-8a-decorator-super/` — `runs/<label>/`
(`prelaunch.json`, `run.log`, `terminal.json`, `captures/`, `analysis.json`),
`bin/` (every replayed binary), `logs/` (builds, clippy), `observe/`,
`runtime-control*.json`, `base/evidence-to-restored-base.tracked.patch`,
`run-final.sh` / `run-final-2.sh`, `run-after-17-chain.sh` /
`run-after-18-chain.sh` / `run-followup3.sh` (third follow-up),
`finalize.sh` / `finalize-2.sh` / `finalize-3.sh` / `finalize-4.sh`,
`extract-patches.sh`. The receipt pins all of it. The restored-base worktree
`~/dev/tsc-rs-dec-super-base` holds the before-binary of the extra controls;
the follow-up set's before-binary (`bin/decorator_super_contract.before-followup`)
is the `after-10` candidate plus the clippy repairs.

Existing fixtures, past patches and receipts are untouched; no ratchet,
profile, pin, admission, hosted-acceptance or canonical-runner surface was
changed.
