The smallest safe U1 change is a typed target role, threaded only through the binding→assignment converters. The generic flattener passes `Existing`; only the `using` entry passes `ExpressionReference`, so no other flattener changes, and you don't have to prove the BindingElement arms unreachable. For A, every one of the four sites can take its marker from state the transform already computes. No re-serialization and no broad predicate is needed. This was read-only code reading at 5e6a4cfa1.

## U1: restricting the declaration-reference projection

**Call graph** (`flatten_destructuring.rs`):
- `convert_to_assignment_pattern` (:1414) → `convert_to_object_assignment_pattern` / `convert_to_array_assignment_pattern` → `make_object_assignment_pattern` / `make_array_assignment_pattern` (:1205 / :1144) → `convert_to_object_assignment_element` / `convert_to_array_assignment_element` (:1314 / :1264).
- Nested patterns recurse through `convert_to_assignment_element_target` (:1488) back into `convert_to_assignment_pattern`.
- The generic flattener reaches the same `make_*` functions through `create_flatten_object_pattern` / `create_flatten_array_pattern` (:1075 / :1089), with `FlattenPatternKind::Assignment`.

**Why the generic path is untouched in practice.** Upstream, `flattenDestructuringAssignment` builds its patterns from elements of an *assignment* pattern. Those are object-literal elements or expressions, so the converters take their identity arm (`cast(...)`). In native terms, `require_not_binding_shape` then `Ok(element)`. The BindingElement arms, the only place the projection would go, run only when a binding pattern is converted, and today that means es_next `hoist_variable_statement`.

That is an invariant, though, not a type guarantee. I did not audit every shared-flattener caller for an Assignment-kind flatten over a binding pattern. So the fully safe, still minimal form is the typed role:

```rust
#[derive(Clone, Copy)]
pub(super) enum AssignmentTargetRole { Existing, ExpressionReference }
```

- **Thread it through** `convert_to_assignment_pattern`, `convert_to_object_assignment_pattern` and `convert_to_array_assignment_pattern`, `make_object_assignment_pattern` and `make_array_assignment_pattern`, the two `convert_to_*_assignment_element` functions, and `convert_to_assignment_element_target`. Nested patterns must pass the role down.
- **Callers:**
  - `create_flatten_object_pattern` / `create_flatten_array_pattern` (:1075 / :1089) pass `Existing`, which keeps today's behaviour byte for byte.
  - es_next :1093 passes `ExpressionReference`.
- **Positions projected under `ExpressionReference`** (identifier targets only):
  - the name of a shorthand property (:1386);
  - the object spread name (:1335), which is the `rest` target that ES2018 flattening later assigns;
  - the array spread name (:1285);
  - the identifier returned by `convert_to_assignment_element_target` for property-assignment and array-element targets.
- **Never projected:** property names, initializers or defaults, and nested pattern nodes (these get the role passed down instead).
- **Projection itself:**
  - Apply it only when the target is a parse-tree name: an `Identifier` whose original has a real position and no `generated_binding_id`. Generated names keep today's behaviour, and CommonJS substitution skips them anyway.
  - Make it `clone_node(name)`, set its original to `name`, keep `name`'s text range, and add `InternalEmitFlags::DECLARATION_NAME_REFERENCE`.
  - **Keep all existing emit flags.** Do *not* call es_next's `project_parsed_assignment_target(VariableInitializerClone)` as-is: it clears `LOCAL_NAME`, `EXPORT_NAME` and `INTERNAL_NAME`. That clearing mirrors `hoistInitializedVariable`'s clone of a plain identifier. Upstream's pattern converters reuse `element.name` with its flags unchanged. Factor out only the "add the internal reference flag to a clone" step.
- **Why clone rather than flag the parsed node:** the parsed node's metadata would leak into any later clone of it. `hoist_binding_identifier` clones the same name for `var x` / `exports.x = void 0`; today that happens before the conversion, but only by call order.
- **Global classification stays as it is.** `is_non_reference_identifier_node` is unchanged; the projected clone opts out through the flag it already honours.

**Checks:** the `using` rows (ES5 object and nested-rest, the ES2015 nested-rest `rest`), plus the ES2015 object shorthand rows that pass today. Run the full System `using` set, because the node identity changes even though System never reads the flag. Add `export let {x: y}` and `export let [x, ...r]` at ES5 and ES2015 × CommonJS. Include an Existing-role control: an ordinary `({a, ...r} = s)` flatten whose output must not change.

## A: marker source at each of the four sites

Upstream injects metadata only when `some(getTypeMetadata(...))`. The native plans already encode exactly that.

1. **Class declaration location (:819) and class decoration statement (:1498).**
   - **Source:** `constructor_metadata.is_some()` in `visit_class_declaration` (:651). `ConstructorMetadataPlan::for_class` is `Some` exactly when the class would inject metadata: `emitDecoratorMetadata`, and the class or a constructor parameter is decorated (the `HasClassOrConstructorParameterDecorators` fact), and a constructor with a body exists. For a class, `getOldTypeMetadata` yields only `design:paramtypes`, and only in that case.
   - **Threading:** it's `Copy`, so compute `let class_metadata = constructor_metadata.is_some();` before the `.map(...)` at :657 consumes it. Pass the flag into `transform_decorated_class_declaration(..., class_metadata)` and `create_class_decoration_statement(assignment, current, class_metadata)`. Both are reached only when `has_constructor_decoration` holds.
   - **Composition rule** over the current class node's modifiers (after the ts transform: export and default kept, `abstract`/`declare` already gone):
     - skip only the *leading* run of export/default modifiers;
     - among the rest, if a non-decorator modifier exists, the range starts at the end of the last one;
     - otherwise it starts at `declaration.pos`, i.e. the whole node.
   - **Consequences:**
     - `@dec export default class` → past `default`;
     - `export default @dec class` → whole node;
     - `export @dec class` → whole node;
     - `@dec export class` → past `export`.
2. **Member decoration statement (:1592).**
   - **Source:** `!metadata.is_empty()`. Compute it before `decorators.extend(metadata)` consumes the vector. The vector comes from `metadata_by_original.remove(owner)`, so it is non-empty exactly for the metadata owner.
   - **Pre-existing mismatch:** this site passes the *original* member's `original_modifiers`. Upstream uses the transformed member, where `public`, `private`, `protected`, `readonly` and `override` are already gone. With `@dec public get p()`, native currently moves the range past `public`, but upstream moves past `@dec` (without metadata) or uses the whole node (with metadata).
   - **Fix:** use the current `member` together with its `modifiers` (:1543). For an accessor owner, the current member corresponds to the owner.
   - **Witnesses:** add `@dec public get` and `@dec override get`.
3. **`finish_class_element` (:4344), called from :522 / :529 / :536 / :559.**
   - **Source:** `metadata_by_original` is local to `prepare_class_member_plan`. Record owners in a transform field `metadata_owner_members: BTreeSet<NodeId>`, inserted next to `metadata_by_original.insert` (:1029). Look it up with `get_original_node(original).node()`.
   - **Why the set is populated in time:** `metadata_decorator_owner` returns `Some` only when `owner == original` (:3024 / :3032). So the owner's metadata is computed in its own loop iteration, before `prepare_and_visit_class_element` visits that member and reaches `finish_class_element`.
   - **Paired accessors:** yes, this covers the non-owner member exactly.
     - Upstream `injectClassElementTypeMetadata` injects only into `firstAccessorWithDecorators` (`classElementOrClassElementParameterIsDecorated`, `_tsc.js:14697`).
     - The non-owner accessor keeps its own modifiers, so upstream `moveRangePastModifiers` runs as usual on it, and the set lookup correctly reports no metadata for it.
     - A pure predicate would have to re-derive `metadata_accessor_owner` together with the `emitDecoratorMetadata` and "legacy-decorated" gates. The set cannot drift from the emitted metadata, so prefer it.
   - **Element composition rule:** decorators, then metadata, then the remaining non-decorator modifiers of the current node (`static`, `accessor` and `async` survive; TS-only modifiers are already elided). If any non-decorator modifier survives, the range starts after the last one; otherwise it is the whole node. Properties and methods still short-circuit to `name.pos`, so passing the marker for them is harmless.

The helper change itself is small: `move_range_past_modifiers(declaration, modifiers, injected: InjectedMetadata)`. `None` keeps the current logic; `Class` and `Element` apply the composition rules above.

**Witnesses** (mint expected output from upstream):
- **Class:** `constructor() {}` with no parameters; a class with no constructor; a class with only constructor-parameter decorators; `@dec abstract class`; the four export/default orders above.
- **Accessor:** decorator on the getter only; on the setter only; on the second accessor of a pair; `@dec static get`; `@dec public get`; `@dec override get`.
- **Controls:** `emitDecoratorMetadata: false`, and method and property members.
- **Matrix:** ES5, ES2015 and ES2022 × CommonJS and System × both comment settings.