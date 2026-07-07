# Core interfaces: the data contracts

The authoritative reference for the objects every phase reads and
writes. greenfield.md §4 gives the DESIGN rationale (identity,
interning, links); the other docs give ALGORITHMS. This doc is the
CONTRACT: the actual tsc interface definitions (public fields from
`typescript.d.ts`, internal fields from `_tsc.js` usage), each with its
Rust mapping and which fields are load-bearing for parity.

Rule of thumb: a field is load-bearing (must match tsc) if it is
OBSERVABLE — it affects a diagnostic's code/span/message, or it changes
resolution order, type identity, or caching. Fields that are pure
internal bookkeeping (perf caches) can differ. Every enum below uses
tsc's EXACT numeric values (greenfield §3 codegen) so masks port verbatim.

Vendored tsc 6.0.3. Line anchors: `typescript.d.ts` (Dxxxx) for public
interfaces, `_tsc.js` (Jxxxx) for internal shapes.

---

## 1. Node — the AST atom — D4305

```ts
interface Node extends ReadonlyTextRange {   // ReadonlyTextRange = { pos, end }
    kind: SyntaxKind; flags: NodeFlags; parent: Node;
}
// internal (attached during parse/bind/check):
//   id, symbol, localSymbol, flowNode, locals, nextContainer, emitNode, …
```

Rust:

```rust
struct Node {
    kind: SyntaxKind,          // #[repr(u16)], tsc values — parser dispatch ports verbatim
    flags: NodeFlags,          // bit-compatible; incl. ThisNodeHasError / ThisNodeOrAnySubNodesHasError
    pos: u32, end: u32,        // FULL start (incl. leading trivia) .. end
    parent: NodeId,
    data: NodeData,            // the kind-specific payload (see below)
}
```

Load-bearing:
- **`pos`/`end`**: `pos` is the FULL start (with leading trivia); the
  ERROR span uses `getStart` (skips trivia) via `getErrorSpanForNode`.
  Getting the trivia boundary wrong shifts every diagnostic column.
- **`flags: NodeFlags`**: `ThisNodeHasError` (parse-error gating, set by
  `parseErrorBeforeNextFinishedNode` → finishNode, syntax-and-binder §2.1),
  `Let`/`Const`/`Using` (block-scope semantics), `AwaitContext`,
  `DisallowInContext`, `Ambient`, `JavaScriptFile`, `HasImplicitReturn`,
  `HasExplicitReturn` (return-path checks). All observable.
- **`kind`**: everything dispatches on it. `#[repr(u16)]` with tsc's
  numbering.

Side tables (NOT fields — greenfield §4.3 links): `NodeLinks` holds
`resolvedType`, `resolvedSignature`, `resolvedSymbol`, `flowNode`,
`deferredNodes`, per-node `NodeCheckFlags`. `node.symbol` /
`node.locals` are on the node in tsc but belong in `SymbolLinks`-style
side tables in the arena model.

`NodeData` is the tagged union of the ~180 node kinds (Identifier,
CallExpression, ...). Its variants are the parser's output; each
carries the child `NodeId`s and any literal payload. This is the
largest enum in the codebase; generate the SyntaxKind↔variant mapping.

---

## 2. Symbol — the named entity — D6533

```ts
interface Symbol {
    flags: SymbolFlags;
    escapedName: __String;               // name with a leading-underscore escape scheme
    declarations?: Declaration[];
    valueDeclaration?: Declaration;
    members?: SymbolTable;               // instance members / namespace members
    exports?: SymbolTable;               // module/namespace exports
    globalExports?: SymbolTable;
}
// internal: id, mergeId, parent, exportSymbol, constEnumOnlyModule,
//           isReferenced, isReplaceableByMethod, links (SymbolLinks)
```

Rust:

```rust
struct Symbol {
    flags: SymbolFlags,            // bit-compatible; the merge masks are computed from these
    name: String,                 // escapedName (keep tsc's __String escape for __proto__ etc.)
    declarations: Vec<NodeId>,
    value_declaration: NodeId,     // INVALID if none; first value decl wins
    members: SymbolTable,          // IndexMap<Name, SymbolId> — ORDERED (iteration is observable)
    exports: SymbolTable,
    parent: SymbolId,
    export_symbol: SymbolId,       // local↔export linking (syntax-and-binder §3.2)
    merged_into: SymbolId,         // getMergedSymbol chases this
}
```

Load-bearing:
- **`flags: SymbolFlags`**: drives `getTypeOfSymbol` dispatch
  (checker-foundations §1.1), the binder merge (includes/excludes,
  syntax-and-binder §3.1), and accessibility. Bit-compatible.
- **`declarations` order + `valueDeclaration`**: overload order,
  first-declaration reporting anchors (unused sweep), and which
  declaration a diagnostic points at. Order is observable.
- **`members`/`exports` are ORDERED tables** (IndexMap): property
  iteration order shows up in union/intersection member synthesis and
  display. Never a plain HashMap.
- **`escapedName`**: tsc escapes names starting with `__` (and a few
  reserved) so user `__proto__` doesn't collide with internal
  `__computed`/`__constructor`. Port the escape or name lookups collide.

Internal symbol names (D6549 `InternalSymbolName`): `__call`,
`__constructor`, `__new`, `__index`, `__export`, `__global`,
`__missing`, `__type`, `__object`, `__computed`, `default`, ... — these
are the keys call/construct/index signatures and computed members are
stored under. Port the constants.

---

## 3. Type — the type object — D6646

```ts
interface Type {
    flags: TypeFlags;
    symbol: Symbol;                      // the declaring symbol (INVALID for intrinsics)
    aliasSymbol?: Symbol;
    aliasTypeArguments?: readonly Type[];
    pattern?: DestructuringPattern;
}
interface FreshableType extends Type { freshType; regularType; }   // literal fresh/regular pair
```

Internal (the ones that matter, from `_tsc.js`): `id` (allocation
identity — the whole point), `objectFlags` (`ObjectFlags`: Reference,
Instantiated, ObjectLiteral, FreshLiteral, Anonymous, Mapped, ...),
`widened` (widening cache), and per-kind payload
(`types` for unions/intersections, `target`+`resolvedTypeArguments` for
references, `value` for literals, `members`/`callSignatures`/... for
resolved object types).

Rust (greenfield §4.2 gives the full `TypeData`):

```rust
struct Type {
    flags: TypeFlags,          // bit-compatible — the single most-used mask in the checker
    object_flags: ObjectFlags,
    symbol: SymbolId,
    alias: Option<(SymbolId, Box<[TypeId]>)>,
    data: TypeData,            // Intrinsic | Literal{fresh/regular/wide} | Union | Intersection
                               // | ObjectAnon{decl} | ObjectReference | Interface | TypeParameter
                               // | IndexedAccess | Conditional | Mapped | TemplateLiteral
                               // | StringMapping | …
}
```

Load-bearing (all of it):
- **`flags: TypeFlags`** is consulted on nearly every checker line;
  bit-compatible so `t.flags & 1024 /* StringLiteral */` ports verbatim.
- **allocation `id` = identity**: interning ONLY where tsc interns
  (literals/unions/intersections/stringMappings). Two written `{}` are
  distinct (fx2/fx4; greenfield §4.2, checker-foundations §4.2).
- **`objectFlags & FreshLiteral`**: excess-property checking hinges on
  it (checker-foundations §5).
- **`aliasSymbol`/`aliasTypeArguments`**: display AND some relation
  keys (`getRelationKey` includes alias context). Observable in message
  text and in cache hits.
- **freshType/regularType pair**: literal widening (structural, not
  cache-order — stall-playbook §2.2).

---

## 4. Signature — the callable — D6852

```ts
interface Signature {
    declaration?: SignatureDeclaration | JSDocSignature;
    typeParameters?: readonly TypeParameter[];
    parameters: readonly Symbol[];
    thisParameter?: Symbol;
}
// internal: resolvedReturnType, resolvedTypePredicate, minArgumentCount,
//   resolvedMinArgumentCount, flags (SignatureFlags: HasRestParameter,
//   HasLiteralTypes, Abstract, IsInnerCallChain, …), target+mapper
//   (for instantiated signatures), compositeSignatures, …
```

Rust (the current tsrs `Signature` in `src/types.rs` is close — see the
`from_method` field relation-core-1 added):

```rust
struct Signature {
    type_params: Vec<SymbolId>,
    params: Vec<ParamInfo>,       // name, type, optional, decl span
    min_args: u32,
    rest: Option<TypeId>,         // rest element type (HasRestParameter)
    this_param: Option<TypeId>,
    ret: TypeId,                  // resolved lazily (decl_key ⇒ infer from body)
    predicate: Option<PredInfo>,  // `x is T` / `asserts x`
    flags: SignatureFlags,        // bit-compatible; incl. Abstract
    from_method: bool,            // strictVariance keys on TARGET decl kind (checker-key §3.2)
    // instantiated: target + mapper (greenfield §4.8)
}
```

Load-bearing: `min_args`/`rest` (arity checks, overload resolution
checker-key §3), `predicate` (narrowing), `from_method` (variance),
`type_params` (erase-generics + inference). `IndexInfo` (D6873): `{
keyType, type, isReadonly }` — the string/number/symbol index sigs.

---

## 5. FlowNode — the CFG node — J42404

```ts
// createFlowNode(flags, node, antecedent) → { flags, id, node, antecedent }
// FlowLabel additionally: { flags, antecedents: FlowNode[] }
```

Rust (tsrs `flow_graph.rs` already implements this — Tier-2):

```rust
enum FlowNode {
    Start { container: Option<NodeId>, outer: Option<(FlowId, Span)> },
    Assignment { node: NodeId, antecedent: FlowId },
    Call       { node: NodeId, antecedent: FlowId },
    Condition  { true_or_false: bool, node: NodeId, antecedent: FlowId },  // TrueCondition/FalseCondition
    SwitchClause { node: SwitchClauseData, antecedent: FlowId },
    BranchLabel { antecedents: Vec<FlowId> },     // JOIN
    LoopLabel   { antecedents: Vec<FlowId> },     // JOIN + back-edge
    ReduceLabel { target: FlowId, antecedents: Vec<FlowId>, antecedent: FlowId },  // try/finally
    ArrayMutation { node: NodeId, antecedent: FlowId },
    Unreachable,
}
```

`FlowFlags` (bit-compatible: Unreachable=1, Start=2, BranchLabel=4,
LoopLabel=8, Assignment=16, TrueCondition=32, FalseCondition=64,
SwitchClause=128, ArrayMutation=256, Call=512, ReduceLabel=1024,
Referenced=2048, Shared=4096) drive the `getTypeAtFlowNode` dispatch
(checker-key §4.2) and `isReachableFlowNode` (§4.7). **`Shared`** marks
nodes reached by multiple paths — the shared-flow cache keys on it.

---

## 6. InferenceInfo / InferenceContext — J68300

```ts
// createInferenceInfo(tp) → {
//   typeParameter, candidates, contraCandidates, inferredType,
//   priority, topLevel: true, isFixed: false, impliedArity }
```

Rust (the current tsrs `InferenceInfo` LACKS `topLevel`/`isFixed` —
adding them is a relation-core prerequisite, checker-key §2.1):

```rust
struct InferenceInfo {
    type_parameter: SymbolId,
    candidates: Vec<TypeId>,          // covariant
    contra_candidates: Vec<TypeId>,   // contravariant
    inferred_type: Option<TypeId>,
    priority: InferencePriority,      // bit flags; lowest wins
    top_level: bool,                  // inferred at a top-level position ⇒ widen literals
    is_fixed: bool,                   // pinned by a prior fixing pass
    implied_arity: Option<u32>,
}
struct InferenceContext {
    inferences: Vec<InferenceInfo>,
    signature: Option<SignatureId>,
    flags: InferenceFlags,            // NoDefault, AnyDefault, SkippedGenericFunction
    mapper: MapperId, non_fixing_mapper: MapperId,
    return_mapper: Option<MapperId>,
    compare_types: RelationFn,        // subtype/assignable comparator used during inference
}
```

`InferencePriority` (D6885+): NakedTypeVariable=1, SpeculativeTuple=2,
SubstituteSource=4, HomomorphicMappedType=8, ..., ReturnType=128, ...
— `PriorityImpliesCombination` = a mask (getCovariantInference unions
vs supertypes on it, checker-key §2.1). The current tsrs has 5 of ~14;
add levels only when a probe demands one.

---

## 7. Diagnostic — the OUTPUT contract — D6923

```ts
interface DiagnosticRelatedInformation {
    category: DiagnosticCategory;    // Warning=0, Error=1, Suggestion=2, Message=3
    code: number;
    file: SourceFile | undefined;
    start: number | undefined;       // UTF-16 offset
    length: number | undefined;
    messageText: string | DiagnosticMessageChain;
}
interface Diagnostic extends DiagnosticRelatedInformation {
    reportsUnnecessary?: {};          // faded (unused-identifier)
    reportsDeprecated?: {};
    source?: string;
    relatedInformation?: DiagnosticRelatedInformation[];
}
interface DiagnosticMessageChain {
    messageText: string; category; code; next?: DiagnosticMessageChain[];
}
```

Rust (the current tsrs `Diagnostic` in `src/diagnostics/mod.rs` matches
this shape):

```rust
struct Diagnostic {
    file: Option<FileId>, start: u32, length: u32,   // start = tsc UTF-16 offset (see below)
    message: MessageChain,                           // code+category+args, nested `next`
    related: Vec<RelatedInfo>,
    category_override: Option<Category>,             // suggestion-band moves
}
```

Load-bearing (this IS the conformance metric):
- **`start`/`length`**: tsc offsets are UTF-16 code units; tsrs stores
  byte offsets that are MONOTONIC with tsc's for ordering, and converts
  at the boundary. The comparison tiers (T0 line/col, T2 span) depend on
  the conversion matching. Keep the byte↔UTF-16 map exact.
- **`code` + `category`**: T0 compares code; T1 adds category (the
  suggestion band — knowledge-base §2). `category_override` is how
  unused/emit-suppressed diagnostics move bands.
- **`messageText` chain**: T2/T3 parity. Built by `chainDiagnosticMessages`;
  generated message table (D `diagnosticMessages.json`, greenfield §3).
- **`relatedInformation`**: T3. "declared here" (2728), overload
  related-info, etc.
- **Ordering**: `compareDiagnostics` sorts by file, start, length, code,
  message — the T4 floor. tsrs applies the same final sort.

---

## 8. CompilerOptions — the behavior gate — D7014

The subset the corpus exercises (each gates real behavior; the harness
parses them from `// @option:` directives — syntax-and-binder-adjacent):

```rust
struct CompilerOptions {
    strict: Option<bool>,                         // master switch for the strict family
    strict_null_checks, strict_function_types,
    strict_property_initialization, strict_bind_call_apply,
    no_implicit_any, no_implicit_this,
    use_unknown_in_catch_variables: Option<bool>,
    no_unused_locals, no_unused_parameters: Option<bool>,
    no_implicit_returns, no_fallthrough_cases_in_switch: Option<bool>,
    exact_optional_property_types: Option<bool>,
    target: Option<String>,   // ScriptTarget — gates lib layer + downlevel
    lib: Option<Vec<String>>,
    module: Option<String>,   // ModuleKind — gates module semantics
    // …emit options mostly irrelevant to diagnostics, EXCEPT:
    no_emit, preserve_const_enums, emit_declaration_only: Option<bool>,  // suggestion-band emit
}
```

- `strict` expands to the family unless a member is set explicitly —
  port the expansion (`getStrictOptionValue`).
- `target` gates lib inclusion (archived notes:
  `archive/workstreams/lib-gap-2304.md`) AND downlevel diagnostics
  (es5-async, the U6 root cause B,
  `archive/workstreams/u6-unused-fp.md`).
- `no_emit`/`preserve_const_enums`/`emit_declaration_only` interact with
  the suggestion-band emit-marking (knowledge-base §1).

`Diagnostic` also flows from `getOptionsDiagnostics` (bad option combos)
— gated BEFORE semantic diagnostics in the driver (checker-foundations
§2), a check the current tsrs already mirrors.

---

## 9. Program / check entry — the API contract

The public surface every consumer (harness, tests, CLI) uses:

```rust
// crates/checker public API
fn check_program(files: &[InputFile], options: &CompilerOptions) -> (Vec<Diagnostic>, ExitCode);
struct InputFile { name: String, text: String }   // name includes the lib file(s)
```

tsc's `Program`/`TypeChecker` (D6015/D6173) expose hundreds of methods
for the language service; a batch conformance checker needs only the
diagnostic-collection path (`getSemanticDiagnostics` +
`getSyntacticDiagnostics` + `getSuggestionDiagnostics` per file,
concatenated and sorted). The greenfield harness (§7) drives exactly
this; do NOT build the full TypeChecker surface — it is unobservable in
the conformance metric and pure cost.

---

## 10. The observability table (what MUST match vs what MAY differ)

| Interface field | Must match tsc? | Why |
|---|---|---|
| `Node.pos/end`, `kind`, error flags | YES | span/columns, gating, dispatch |
| `Node.id` | no | allocation-local |
| `Symbol.flags`, `declarations` order, `members` iteration order | YES | dispatch, merge, member synthesis order |
| `Symbol.id`/`mergeId` | no | allocation-local |
| `Type.flags`, allocation identity, `objectFlags` FreshLiteral, `aliasSymbol` | YES | relations, display, excess-property, cache keys |
| `Type` internal caches (`widened`, resolved shapes) | no | memoization |
| `Signature.min_args/rest/predicate/from_method` | YES | arity, narrowing, variance |
| `FlowNode.flags`, graph shape | YES | narrowing, reachability |
| `InferenceInfo.priority/topLevel/isFixed` | YES | inference result |
| `Diagnostic.code/category/span/chain/related/order` | YES | THE metric (tiers T0–T4) |
| `CompilerOptions` (behavior-gating subset) | YES | every gated diagnostic |

The rule the whole design turns on: **identity, order, and the
diagnostic shape are observable and must match; allocation ids and
memoization caches are free.** Everything in the algorithm docs is in
service of computing the "must match" columns correctly.
