# Checker foundations: the machinery under the four hot algorithms

Companion to checker-key-functions.md. That doc covers the four
load-bearing ALGORITHMS (relation, inference, overload, flow). This one
covers the FOUNDATIONAL MACHINERY they all sit on — the parts whose
DESIGN DECISIONS (not just control flow) are observable in diagnostics
and were the source of the subtlest divergences this project hit:
lazy type computation with cycle detection, the check-driver ordering
that makes resolution order observable, contextual typing, type
construction/normalization, widening, and instantiation.

Same conventions as checker-key-functions.md; same rule: PORT the tsc
source, probe when unsure. Line anchors are vendored tsc 6.0.3.

---

## 1. Lazy type computation + cycle detection — the memoization spine

Everything type-valued is computed on demand and memoized on the
symbol's links. The DESIGN DECISION that matters: tsc detects
resolution cycles with an explicit `pushTypeResolution`/
`popTypeResolution` stack and, on a cycle, marks the whole cycle's
results `false` so the caller falls back (usually to `any`/error) — it
does NOT just memoize a half-built type. Getting this wrong produces
either infinite recursion or a WRONG cached type that then poisons
everything downstream. The current tsrs has ad-hoc cycle guards per
subsystem (alias cache cycle_start, BaseType slot, etc.); a rebuild
unifies them into this one mechanism.

### 1.1 getTypeOfSymbol dispatch — tsc 56945

```rust
fn type_of_symbol(&mut self, sym: SymbolId) -> T {
    let cf = self.check_flags(sym);
    if cf.contains(DEFERRED_TYPE)   { return self.type_of_symbol_with_deferred_type(sym); }
    if cf.contains(INSTANTIATED)    { return self.type_of_instantiated_symbol(sym); }   // mapper applied
    if cf.contains(MAPPED)          { return self.type_of_mapped_symbol(sym); }
    if cf.contains(REVERSE_MAPPED)  { return self.type_of_reverse_mapped_symbol(sym); }
    let f = self.symbol_flags(sym);
    if f.intersects(VARIABLE|PROPERTY)                     { return self.type_of_variable_or_parameter_or_property(sym); }
    if f.intersects(FUNCTION|METHOD|CLASS|ENUM|VALUE_MODULE){ return self.type_of_func_class_enum_module(sym); }
    if f.contains(ENUM_MEMBER)   { return self.type_of_enum_member(sym); }
    if f.contains(ACCESSOR)      { return self.type_of_accessors(sym); }
    if f.contains(ALIAS)         { return self.type_of_alias(sym); }
    self.error_type
}
```

The dispatch is on SymbolFlags + CheckFlags (both bit-compatible in the
greenfield model — that is what makes this a verbatim port). The result
caches on `SymbolLinks.type_of_symbol` (each `type_of_*` worker sets it).

### 1.2 The resolution stack — tsc 55728 / 56642

```rust
// checker-wide state
resolution_targets: Vec<ResolutionTarget>,   // symbol/signature/type being resolved
resolution_results: Vec<bool>,               // parallel: still-true?
resolution_property_names: Vec<ResolutionKind>,  // Type|ResolvedBaseConstructorType|DeclaredType|...
resolution_start: usize,

fn push_type_resolution(&mut self, target: ResolutionTarget, kind: ResolutionKind) -> bool {
    if let Some(cycle_start) = self.find_resolution_cycle_start(target, kind) {
        // a cycle: mark every entry from the cycle start as NOT resolvable
        for i in cycle_start..self.resolution_results.len() { self.resolution_results[i] = false; }
        return false;   // caller must handle circularity (usually returns error/any)
    }
    self.resolution_targets.push(target);
    self.resolution_results.push(true);
    self.resolution_property_names.push(kind);
    true
}

fn find_resolution_cycle_start(&self, target: ResolutionTarget, kind: ResolutionKind) -> Option<usize> {
    for i in (self.resolution_start..self.resolution_targets.len()).rev() {
        // if an intermediate target ALREADY has its property resolved, no cycle through it
        if self.resolution_target_has_property(self.resolution_targets[i], self.resolution_property_names[i]) { return None; }
        if self.resolution_targets[i] == target && self.resolution_property_names[i] == kind { return Some(i); }
    }
    None
}
```

Usage pattern (every lazy resolver that can recurse into itself):

```rust
fn type_of_variable_or_parameter_or_property_worker(&mut self, sym: SymbolId) -> T {
    // …fast paths…
    if !self.push_type_resolution(sym.into(), ResolutionKind::Type) {
        // circular: e.g. `const x = x` or a mutually-referential inferred type
        return self.report_circularity_error(sym);   // 2502/7022 + error type
    }
    let ty = /* compute from declaration (annotation or checkExpression of initializer) */;
    if !self.pop_type_resolution() {                  // popped false ⇒ a cycle was detected below
        return self.report_circularity_error(sym);    // or the "no-annotation implicit any" recovery
    }
    self.set_type_of_symbol(sym, ty);
    ty
}
```

Invariants:
- `resolution_property_names` distinguishes WHICH property is being
  resolved (Type vs DeclaredType vs ResolvedBaseConstructorType vs
  ResolvedReturnType vs ResolvedTypeArguments vs ...). A cycle exists
  only if the SAME (target, kind) pair recurs. This is why one symbol
  can be mid-resolution for its `Type` while safely resolving its
  `DeclaredType`.
- `pop_type_resolution` returns whether the entry it pops is still
  `true`; a `false` means someone deeper flagged the cycle, and the
  caller must NOT cache the (garbage) computed type.
- `resolution_start` is bumped during speculative/independent
  resolution passes so cycles don't leak across them.

Current-tsrs gap: unify the scattered cycle guards onto this. Until
then, the greenfield §4.3 "one-write links + resolving sentinel" rule
is the same idea expressed structurally.

---

## 2. The check driver — why resolution order is observable — tsc 87003

`checkSourceFileWorker` is the top of the tree and its ORDER is part of
the observable behavior (this project's entire "order-dependence" theme,
stall-playbook §2.2, comes from here):

```rust
fn check_source_file_worker(&mut self, file: NodeId) {
    if self.node_links(file).flags.contains(TYPE_CHECKED) { return; }
    self.check_grammar_source_file(file);
    // ...set up potentialThisCollisions / potentialNewTargetCollisions...
    for &stmt in self.statements(file) { self.check_source_element(stmt); }   // (A) eager pass, IN ORDER
    self.check_source_element(self.end_of_file_token(file));
    self.check_deferred_nodes(file);                                          // (B) deferred pass
    self.register_for_unused_identifiers_check(file);                        // (C) unused, LAST
    // ...check_unused_identifiers, external-module exports, this/new.target collisions...
    self.node_links_mut(file).flags |= TYPE_CHECKED;
}
```

Two-phase checking is the load-bearing decision:

### 2.1 Deferred nodes — tsc 86899

Function/arrow/method BODIES, class expressions, accessors, JSX, type
assertions, and untyped calls are NOT checked during the eager
statement pass — they are registered via `checkNodeDeferred` and run in
`checkDeferredNodes` AFTER all statements are checked eagerly.

```rust
fn check_node_deferred(&mut self, node: NodeId) {
    let file = self.source_file_of(node);
    if !self.node_links(file).flags.contains(TYPE_CHECKED) {
        self.node_links_mut(file).deferred_nodes.get_or_insert_default().insert(node);
    }
}
fn check_deferred_node(&mut self, node: NodeId) {
    self.instantiation_count = 0;   // reset per deferred node (instantiation budget)
    match self.node_kind(node) {
        Call|New|TaggedTemplate|Decorator|JsxOpening => self.resolve_untyped_call(node),
        FunctionExpr|Arrow|Method|MethodSig => self.check_function_expression_or_object_literal_method_deferred(node),
        GetAccessor|SetAccessor => self.check_accessor_declaration(node),
        ClassExpr => self.check_class_expression_deferred(node),
        Assertion|As|Paren => self.check_assertion_deferred(node),
        // ...JSX variants, void expr, binary, type parameter...
        _ => {}
    }
}
```

WHY this matters for parity:
- A function body is checked with the contextual type it received
  during the eager pass (contextual typing, §3) already resolved — this
  is what makes `const f: (x: number) => void = x => x.toFixed()` work:
  the eager pass records the contextual signature, the deferred pass
  checks the body against it. Porting expression checking WITHOUT the
  eager/deferred split mis-orders these and changes diagnostics.
- The eager pass is where SIGNATURES, DECLARED TYPES, and TOP-LEVEL
  expression types are established; the deferred pass consumes them.
- `checkExpression` (80960) is the dispatch both passes use; a function
  expression checked eagerly returns its type but its BODY check is
  deferred (`checkFunctionExpressionOrObjectLiteralMethod` eager vs
  `...Deferred`).

Current-tsrs note: tsrs has a deferred-ish model (the CFG resolver
already defers some work), but a rebuild should make the eager/deferred
split explicit and total, matching checkDeferredNode's kind list — it
is the cheapest way to get resolution order right by construction
instead of chasing order-dependence FNs.

---

## 3. Contextual typing — the inference/object-literal driver — tsc 73471

`getContextualType(node, flags)` answers "what type is this expression
expected to be?" It drives: inference (contextual return type,
inferTypeArguments §2.3 of the other doc), object-literal member
checking, function-expression parameter typing, and array-literal
element typing. It is a large dispatch on the PARENT node kind. The
current tsrs has `check_expr(e, Some(ctx))` threading a contextual type,
but not the full `getContextualType` parent-walk — extending it is where
a lot of inference-quality FPs/FNs live.

```rust
fn get_contextual_type(&mut self, node: NodeId, flags: ContextFlags) -> Option<T> {
    if self.node_flags(node).contains(IN_WITH_STATEMENT) { return None; }
    // pushed contextual types (from checkExpressionWithContextualType) win first
    if let Some(i) = self.find_contextual_node(node, /*include_caches*/ flags.is_empty()) {
        return Some(self.contextual_types[i]);
    }
    match self.node_kind(self.parent(node)) {
        VariableDecl|Parameter|PropertyDecl|PropertySig|BindingElement =>
            self.contextual_type_for_initializer_expression(node, flags),      // the declared annotation
        Arrow|ReturnStatement => self.contextual_type_for_return_expression(node, flags),
        Yield  => self.contextual_type_for_yield_operand(self.parent(node), flags),
        Await  => self.contextual_type_for_await_operand(self.parent(node), flags),
        Call|New => self.contextual_type_for_argument(self.parent(node), node),  // the PARAMETER type
        Assertion|As => /* const-assert ? recurse : type from the annotation */,
        Binary => self.contextual_type_for_binary_operand(node, flags),          // =, ||, &&, ??
        PropertyAssignment|Shorthand => self.contextual_type_for_object_literal_element(self.parent(node), flags),
        SpreadAssignment|NonNull|Paren => self.get_contextual_type(self.parent(node), flags),  // pass through
        ArrayLiteral => /* element contextual type by index + spread handling */,
        Conditional => self.contextual_type_for_conditional_operand(node, flags),
        TemplateSpan => self.contextual_type_for_substitution_expression(...),
        Satisfies => Some(self.type_from_type_node(self.assertion_type(self.parent(node)))),
        Jsx* => /* JSX attribute/element contextual types */,
        _ => None,
    }
}
```

Design decisions to preserve:
- **Pushed contextual types take priority** (`find_contextual_node`):
  `checkExpressionWithContextualType` (used by inference §2.3, argument
  checking §3.2 of the other doc) PUSHES a contextual type onto a stack
  before checking, so a call argument being inferred sees the parameter
  type as context. The stack is popped after. tsrs's ctx threading is
  the un-stacked version of this.
- **`ContextFlags`** (Signature, NoConstraints, Completions,
  SkipBindingPatterns) change the answer — e.g. inferTypeArguments asks
  with `SkipBindingPatterns` when every type param has a default. Port
  the flags; they gate real behavior.
- **getApparentTypeOfContextualType** (73424) is the version used for
  member/element access into a contextual type (it applies apparent-type
  and instantiation); object/array literal checking calls it.

---

## 4. Type construction & normalization

The type CONSTRUCTORS are where identity and reduction decisions live —
observable through display, relations, and narrowing.

### 4.1 getUnionType — reduction — tsc 61505

Unions are built with a `UnionReduction` mode (None / Literal /
Subtype). Members are flattened, deduped by id, sorted by id, and:
- **Literal** reduction: collapse fresh literals to a single regular
  literal per value; drop subsumed literals when the base is present.
- **Subtype** reduction (`removeSubtypes`): drop any member that is a
  subtype of another (needs the Subtype relation, checker-key-functions
  §1.5). This is why union display and narrowing JOINs depend on
  Subtype.

The DESIGN DECISION: unions are interned by their sorted member-id list
(`unionTypes` map, greenfield §4.2). Two unions with the same members
are the SAME object — but the reduction mode is part of the key path,
so `A | B` reduced differently can differ. Port `getTypeListId` keying.

### 4.2 getIntersectionType — normalization — tsc 61789

The most intricate constructor (cited throughout the operator sweep and
the fx2/fx4 documented FN). Its steps, in order (each observable):
1. `addTypesToIntersection` flattens + computes an `includes` flag mask.
2. Never absorption; `{}`-nullish folding under strictNullChecks;
   DisjointDomains → never (string & number = never, etc.).
3. Any absorption (with wildcard/error variants).
4. Non-strict nullable collapse.
5. `removeRedundantSupertypes` when a primitive + its literal coexist.
6. **The 2-member type-variable + primitive constraint collapse** (the
   rule the operator sweep's `comparable_dir` mirrors): `T & primitive`
   where T's constraint is primitive-or-empty-object either returns T
   (constraint is a strict subtype), never (disjoint), or a
   constrained type variable.
7. Union distribution (cross-product) when a member is a union.
8. Otherwise intern by member-id list (`intersectionTypes` map).

**The load-bearing identity fact** (greenfield §4.2, stall-playbook
§2.3): `typeMembershipMap` dedupes by type IDENTITY. Two structurally
identical anonymous `{}` types are DISTINCT entries — which is why tsrs
(structural interning) cannot reproduce unknownControlFlow fx2/fx4. A
rebuild's allocation-identity model makes this fall out for free.

### 4.3 getReducedType — discriminant reduction — tsc 59287

Removes never-typed discriminant members and reduces unions of object
types that have become impossible. Member access and narrowing go
through `getReducedApparentType` (getReducedType ∘ getApparentType ∘
getReducedType). Port it; it is why a narrowed discriminated union
resolves its members correctly.

---

## 5. Widening & fresh literals — tsc 68013 / 67923

Two coupled decisions the current tsrs models with `fresh`/`regular`
pairs (keep that; greenfield §4.2 makes it structural):

- **getWidenedType** (68013): only runs when `RequiresWidening`
  object-flag is set. `any|nullable → any`; object literal →
  `getWidenedTypeOfObjectLiteral` (widen each property, memoized on a
  widening context so `{a: null, b: null}` widens consistently); union →
  widen members then re-union with Subtype reduction if any member
  becomes `{}`; caches on `type.widened` when context-free.
- **getRegularTypeOfObjectLiteral** (67923): strips the FreshLiteral
  object flag (the counterpart of literal `regularType`). Called at
  assignment/return positions and in `SkipContextSensitive` arg checks
  (checker-key-functions §3.2) — the fresh-vs-regular distinction drives
  excess-property checking (fresh object literals get excess-property
  errors; regular ones don't).

Design decision: widening is DECIDED BY the RequiresWidening flag +
position, never by cache arrival order (the freshness-de-ordering
principle, stall-playbook §2.2). getWidenedLiteralType (checker-key
§2.1) is the literal-level version used in inference.

---

## 6. Instantiation & TypeMapper — tsc 63675

```rust
fn instantiate_type(&mut self, ty: T, mapper: MapperId) -> T {
    if !self.could_contain_type_variables(ty) { return ty; }   // fast reject: cache couldContain per type
    if self.instantiation_depth == 100 || self.instantiation_count >= 5_000_000 {
        self.error(Diagnostics::Type_instantiation_is_excessively_deep_and_possibly_infinite);  // 2589
        return self.error_type;
    }
    let key = (self.type_id(ty), self.alias_id(...));
    // per-active-mapper cache: instantiation memoized against the CURRENT mapper
    if let Some(c) = self.active_mapper_cache(mapper).get(key) { return c; }
    self.instantiation_count += 1; self.instantiation_depth += 1;
    let result = self.instantiate_type_worker(ty, mapper);   // dispatch on TypeData
    self.instantiation_depth -= 1;
    result
}
```

Design decisions:
- **TypeMapper is a closed enum of kinds** (Simple `t→u`, Array
  `[t..]→[u..]`, Function, Composite `f∘g`, Merged, Deferred), NOT a
  `HashMap<SymbolId,TypeId>` (the current tsrs `Mapper`). Composite and
  deferred mappers are where subtle divergence hides — a HashMap cannot
  express `apply g then f` without eagerly composing, which changes when
  type variables resolve. Port the mapper-kind enum.
- **`couldContainTypeVariables`** (memoized per type) is the fast-path
  guard that makes instantiation affordable — a type with no free
  variables instantiates to itself.
- **Depth 100 / count 5M** guards produce the real 2589 diagnostic.
- `instantiation_count` resets per deferred node (§2.1) — the budget is
  per-node, not global.

---

## 7. Member access — getApparentType + resolveStructuredTypeMembers

- **getApparentType** (59093): the type you actually read members from.
  Instantiable (type param) → base constraint or unknown; then mapped →
  apparent-mapped; reference → this-argument-substituted; intersection →
  combined apparent members; PRIMITIVES → their global wrapper
  interface (`string` → `String`, `number` → `Number`, etc. — this is
  how `"x".length` resolves); `object` → empty object; index →
  `string|number|symbol`; non-strict `unknown` → empty object. Port the
  whole chain; every arm is a member-access behavior.
- **resolveStructuredTypeMembers** (58679): computes an object type's
  members/call-sigs/construct-sigs/index-infos (the tsrs `Shape`). For
  interfaces it merges declarations + heritage; for anonymous types it
  resolves the type-literal; for instantiated types it maps a target
  shape. The tsrs `shape_of_type`/`build_iface_shape` is this — the
  relation-core-1 class-method-overload-merge bug lived exactly here.
- **createUnionOrIntersectionProperty** (59100): how a property is
  synthesized across union/intersection members (optional-flag
  combination, `getTargetSymbol` identity for the nominal-privacy
  check that the archived relation-core notes identified). Port
  `getTargetSymbol` here — it is
  the origin-symbol accessor the assignable-side nominality work
  (archive/workstreams/relation-core-2-steps.md STAGE N) depends on.

---

## 8. Where these sit in the porting order

These foundations are PREREQUISITES for checker-key-functions §5:

- §1 (lazy resolution + cycle stack) and §6 (instantiation) are needed
  before ANY type-valued computation → build them in M2/M3 (greenfield
  §12) alongside the types crate.
- §2 (check driver + deferred) frames the whole checker → M4.
- §4 (union/intersection construction) is needed by relations and
  narrowing → M3, before checker-key-functions §1.
- §3 (contextual typing) and §5 (widening) gate inference quality →
  M6, with checker-key-functions §2.
- §7 (apparent type + members) gates member access and the relation
  engine's structural arm → M3/M4.

None of these is a "hot path to optimize"; they are the SEMANTIC
FOUNDATION whose DECISIONS (cycle handling, resolution order,
contextual dispatch, identity-based intersection dedup, flag-driven
widening, mapper-kind instantiation) must match tsc or the four hot
algorithms compute the right thing on the wrong inputs.
