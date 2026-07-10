# M4: checker skeleton — expressions + statements, declared types — steps

Parent design: checker-foundations.md §1-§7 (this milestone IS that
doc, sequenced); checker-key-functions.md §3 (resolveCall, ported here
with inference stubbed). Prerequisite: M3 gate green.

Scope rule (greenfield §12): everything EXCEPT flow narrowing (M5)
and type-argument inference (M6). An identifier's type is its
declared/flow-initial type; generic calls without explicit type
arguments instantiate their type parameters per the stub in stage
5.7. Gate: T0 ≥ 35%.

From this milestone on, run `cargo xtask conformance` after EVERY
stage and record the rate in the commit body; the ratchet activates
at the first stage that produces diagnostics.

## Stage 5.0: checker state + the resolution spine [M]

- Checker struct: files, options, binder output, type tables, links
  tables, diags sink, and `speculation_depth` (greenfield §4.3 — all
  links writes assert it is 0 or route through a transaction).
- `pushTypeResolution`/`popTypeResolution` (55728) + the
  (target, kind) cycle stack per checker-foundations §1.2, with the
  circularity reporting pattern (2502/7022 family) at each consumer.
- `error_at`-family helpers taking `&'static DiagnosticMessage` only.

Commit: `m4 5.0: checker state + resolution stack`.

## Stage 5.1: name resolution + symbol typing [M]

- `resolveName`: the scope walk over binder locals with meaning masks
  (SymbolFlags-based), including the suggestion-free error path
  (2304-family with spelling suggestions DEFERRED to M8 — emit the
  plain form tsc uses when no suggestion is found; note it in the
  ledger as a partial port).
- `getTypeOfSymbol` dispatch (near 56911) per checker-foundations
  §1.1, with the workers: variable/parameter/property (annotation →
  `getTypeFromTypeNode`; initializer → checkExpression + widening),
  function/class/enum/module, enum member, accessors, alias.
- `getTypeFromTypeNode` (63196) for the annotation grammar: keyword
  types, references (arity check 2314 lives here), unions/
  intersections, literals, arrays/tuples, functions/constructors,
  typeof queries, indexed access, keyof, parenthesized. Conditional/
  mapped/template-literal TYPES may return stubs THAT ARE LEDGERED
  as such and error-free (checked in M8's long tail) — but prefer
  porting them now if the corpus rate demands.

Commit(s): `m4 5.1a-c: symbol typing + type-from-annotation`.

## Stage 5.2: instantiation [M]

`instantiateType` (near 63315) per checker-foundations §6:
`TypeMapper` as the CLOSED enum of five mapper kinds (never a
HashMap), `couldContainTypeVariables` memoized fast path, depth-100/
count-5M guards emitting the real 2589, instantiation caches on
links, `instantiation_count` reset per deferred node (wired in stage
5.4).

Commit: `m4 5.2: instantiateType + TypeMapper`.

## Stage 5.3: member resolution [M]

Per checker-foundations §7: `getApparentType` (59093) full chain
(primitives → wrapper interfaces is how `"x".length` works),
`resolveStructuredTypeMembers` (58679) for interfaces (declaration
merge + heritage), anonymous types, instantiated references, unions/
intersections; `createUnionOrIntersectionProperty` (59100) incl.
`getTargetSymbol` identity for nominal private/protected;
`getPropertyOfType`, index-info lookup, `getReducedApparentType`
(checker-foundations §4.3).

Commit(s): `m4 5.3a-b: apparent type + member resolution`.

## Stage 5.3b: variance measurement (moved from M3 4.7) [M]

Now that instantiation (5.2) + declared types (5.1) + the resolution
stack (5.0) exist: `getVariances`/`getVariancesWorker` (67306/67312)
for references and aliases, `createMarkerType` (67360) with the
marker type parameters, the unmeasurable/unreliable out-of-band
marker propagation into RelationComparisonResult, and un-stubbing the
two M3 ledgered stubs: `relateVariances` (66488, the stage-4.6 stub)
and recursiveTypeRelatedTo's cache-hit variance-replay branch
(65744-65750). Prereq: add `VarianceFlags` to the M0 codegen
`SourceEnum` seed (const enum inlined in `_tsc.js`, Ternary
precedent) — it is not yet in types/src/flags.rs.

EXTEND pins/relations.toml with the M3-deferred rows: generic
references (mutually recursive `interface A<T> { next: B<T> }`
pairs), deeply-expanding generics (the depth limiter +
getRecursionIdentity finally fire), variance-driven reference pairs
(in/out modifier cases included), enums (un-stub
`isEnumTypeRelatedTo` + the `enumRelation` symbol-pair map here).
`cargo xtask relpin run` green over the widened suite is part of this
stage's exit, and stays green through the M4 gate.

Commit(s): `m4 5.3b: variance + M3-deferred relation pins`.

## Stage 5.4: the check driver [M]

`checkSourceFileWorker` (87003) per checker-foundations §2: the
grammar-checks slot (populated in M7; the hook exists now), the eager
statement pass IN SOURCE ORDER via `checkSourceElement` (86546), the
deferred-nodes pass (`checkNodeDeferred`/`checkDeferredNode` near
86916 — port the FULL kind list now; deferred bodies are what make
contextual typing ordering correct), and the end-of-file bookkeeping.
Program-level: options diagnostics gate semantics
(core-interfaces §8), files check in program order, final
`compareDiagnostics` sort + dedup (M0's diags crate).

Commit: `m4 5.4: check driver (eager/deferred)`.

## Stage 5.5: expression checking, non-call arms [M]

`checkExpression` (80960) dispatch, porting arms in tsc order, each
with its tsc-named worker:

literals (fresh types) → identifiers (`getResolvedSymbol` +
declared-type; the FLOW CALL SITE exists but returns declared type —
a single function `get_flow_type_of_reference_stub` that M5 replaces)
→ this/super → array literals (contextual element types, spreads,
tuple inference OFF until M6 — literal tuple contexts use the
declared path) → object literals (fresh object types,
excess-property checking via the relation's fresh handling, computed
names, spread via `getSpreadType`) → property/element access
(checkPropertyAccessExpression 75069: 2339-family reporting,
optional chaining, private names) → assertions/as/satisfies →
template expressions → unary/binary operators (the operator table:
arithmetic 2362/2363, comparison via comparable relation, equality,
in/instanceof, logical, assignment incl. 2322 reporting +
`getRegularTypeOfObjectLiteral` at assignment positions) →
conditional `?:` (union of branches; no narrowing yet) →
await/yield (impl-checker-2xxx §5 rows 12-13: `getAwaitedType`,
async return checking) → JSX for .tsx (impl-checker-2xxx §5b) →
arrow/function expressions (signature from annotation via
`getContextualSignature`; body checking DEFERRED; return-type
inference from body for un-annotated functions —
`getReturnTypeFromBody`, un-narrowed).

Contextual typing arrives WITH this stage per checker-foundations §3:
`getContextualType` (73471) parent-walk + the pushed-context stack +
`ContextFlags`, because object/array literals and function
expressions consume it immediately.

Commit(s): `m4 5.5a-e: expression arms (+rate per commit)`.

## Stage 5.6: widening [M]

Per checker-foundations §5: `getWidenedType` (68013) driven by the
RequiresWidening object flag, `getWidenedTypeOfObjectLiteral` with a
widening context, `getRegularTypeOfObjectLiteral` (67923), literal
widening at declaration sites, and the 7005/7006/7034-family
implicit-any reporting hooks (reportImplicitAny — port the report
sites the corpus exercises; the inference-driven refinements land in
M6).

Commit: `m4 5.6: widening + implicit-any reporting`.

## Stage 5.7: calls with stubbed inference [M]

Port the REAL structure now so M6 only swaps one function:
`getEffectiveCallArguments` (76295), `reorderCandidates` (75768),
`resolveCall` (76579) and `chooseOverload` (76763) per checker-key §3
— including arity checking (2554/2555 family), explicit type-argument
checking (2344 via constraint checks), `getSignatureApplicabilityError`
(76194) with relation-based argument checks, construct/new paths,
`resolveUntypedCall`, and the failure-path candidate selection (T0
codes only; chain shaping is T2).

THE STUB: `infer_type_arguments` returns each type parameter's
default if present, else its constraint, else `unknown` — one
function, marked `/// M6-stub` in the ledger. The subtype overload
pass RUNS (the relation exists since M3); context-sensitive re-run
plumbing (CheckMode bits) is ported but inert until M6 fills
inference.

Commit: `m4 5.7: resolveCall/chooseOverload (inference stubbed)`.

## Stage 5.8: statements + declarations [M]

`checkVariableDeclaration` (83600) family (annotation-vs-initializer
2322, destructuring declarations), control statements (if/while/for
conditions checked, for-in/for-of element typing via the iteration
protocol — port `getIterationTypesOfIterable` now, it is
load-bearing far beyond loops), return checking against the enclosing
signature, throw, switch (2678 comparability of case clauses),
classes (heritage 2415/2417 via relations, member overrides,
parameter properties, index signature checks), interfaces (2320/2411
member compatibility), enums (the constant evaluator ports from tsc's
`evaluate` — checker-foundations-adjacent; anchor `createEvaluator`
19382), modules/namespaces, import/export checking (2305/2307-family
minus module RESOLUTION — single-program files only, as the harness
provides them), declaration-merging checks (2403 with the identity
relation, 2717).

Commit(s): `m4 5.8a-d: statements + declaration checks (+rate)`.

## Final gate

```sh
cargo xtask conformance          # expect: T0 ≥ 35%
cargo xtask relpin run           # widened suite (incl. 5.3b rows) still 0
cargo xtask invariants --suite idempotence
cargo xtask ledger check         # M6-stub entries are the ONLY allowed stubs
```

Then write `docs/NOTES-m4.md`: top 10 one-sided codes with owner
guesses — it seeds M5/M6 verification and the M8 backlog.

## Expected failure modes

| Symptom | Diagnosis | Fix |
|---|---|---|
| Cascade errors after one 2304 | errorType not silencing downstream arms | Port tsc's errorType short-circuits per arm, not a blanket suppression |
| Object-literal assignments over-report 2353/2322 | freshness lost (regular where fresh expected, or vice versa) | Fresh at creation; regular ONLY via getRegularTypeOfObjectLiteral at the tsc-cited positions |
| Rate collapses when deferred pass lands | deferred kind list wrong → bodies checked twice or never | Match checkDeferredNode's kind list exactly (near 86916) |
| Generic calls all error 2345 | the M6 stub used `never`/error instead of constraint/unknown fallback | The stub's fallback order is default → constraint → unknown |
| `"x".length` fails | apparent-type primitive→wrapper arm missing | checker-foundations §7 chain, every arm |
