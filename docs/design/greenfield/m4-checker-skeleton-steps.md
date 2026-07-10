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
- `getTypeOfSymbol` dispatch (56945) per checker-foundations
  §1.1, with the workers: variable/parameter/property (annotation →
  `getTypeFromTypeNode`; initializer → checkExpression + widening),
  function/class/enum/module, enum member, accessors, alias.
- `getTypeFromTypeNode` (63196) for the annotation grammar: keyword
  types, references (arity check 2314 lives here; the type-parameter
  DEFAULTS path `fillMissingTypeArguments` 59545 calls instantiateType
  — completes at 5.2), unions/intersections, literals, arrays/tuples,
  functions/constructors, typeof queries, indexed access, keyof,
  parenthesized, PLUS the arms the worker (63199) actually has:
  this-type, type predicates (trivial: asserts→void else boolean),
  ExpressionWithTypeArguments (→ type reference; 5.3 heritage needs
  it), import types, TypeOperator's unique/readonly,
  optional/rest/named-tuple members, infer-type (goes with the
  conditional stub). Conditional/mapped TYPES may return stubs THAT
  ARE LEDGERED as such and error-free (checked in M8's long tail) —
  but prefer porting them now if the corpus rate demands.
  Template-literal types are NOT in that stub list: M3 4.1 already
  builds them (relation arms + pins are live).

FORWARD-DEPENDENCY NOTE (the compile-order reality): 5.1's bodies
call machinery from later stages — initializer typing → checkExpression
(5.5); `getWidenedTypeForVariableLikeDeclaration` (56552) →
getWidenedType + reportImplicitAny (5.6); typeof queries
(getTypeFromTypeQueryNode 60596) → checkExpressionWithTypeArguments
(77963, 5.5) + getWidenedType (5.6); keyof → getIndexType (62016) →
getPropertiesOfType (5.3); indexed access → getIndexedAccessType
(62552) → getReducedApparentType (59098, 5.3). Laziness makes the
RUNTIME order safe (nothing drives these until the 5.4 driver), but
each call site is wired to a ledgered temporary stub at 5.1 and
un-stubbed by its owning stage — same discipline as every other stub
in this plan.

Commit(s): `m4 5.1a-c: symbol typing + type-from-annotation`.

## Stage 5.2: instantiation [M]

`instantiateType` (63675 — NOT 63315, which is the `instantiateTypes`
list helper) per checker-foundations §6: `TypeMapper` as the CLOSED
enum of SIX mapper kinds (Simple/Array/Deferred/Function/Composite/
Merged — flags.rs TypeMapKind; never a HashMap),
`couldContainTypeVariables` memoized fast path, depth-100/count-5M
guards emitting the real 2589, instantiation caches on links.
Port the whole instantiation FAMILY here, not just the type walker:
`instantiateTypes` (63315), `instantiateSignature` (63411),
`instantiateSymbol` (63436), `instantiateIndexInfo` (63829),
`getSignatureInstantiation` (59886) — 5.3's instantiated-reference
member tables and 5.7's explicit type arguments consume them.
`instantiation_count` resets at tsc's THREE entry points:
checkExpression, checkSourceElement, checkDeferredNode (wired in
stages 5.4 + 5.5).

Commit: `m4 5.2: instantiateType + TypeMapper`.

## Stage 5.3: member resolution [M]

Per checker-foundations §7: `getApparentType` (59093) full chain
(primitives → wrapper interfaces is how `"x".length` works),
`resolveStructuredTypeMembers` (58679) for interfaces (declaration
merge + heritage — that means the base-type resolvers land HERE:
`getBaseTypes` 57218, `resolveBaseTypesOfClass` 57252,
`resolveBaseTypesOfInterface` 57319; 5.8's 2415/2417 and M3's
deferred `getSingleBaseForNonAugmentingSubtype` depend on them),
anonymous types, instantiated references, unions/
intersections; `createUnionOrIntersectionProperty` (59101) incl.
`getTargetSymbol` identity for nominal private/protected;
`getPropertyOfType`, index-info lookup, `getReducedApparentType`
(59098, checker-foundations §4.3).

UN-STUB the M3 normalization stubs here (they were ledgered against
this stage): `getReducedType` (59287, real discriminant reduction),
`getSingleBaseForNonAugmentingSubtype` (needs getBaseTypes),
`getSimplifiedType` (62455 — resolveStructuredTypeMembers itself
calls it, and the 5.1 indexed-access arm needs it real).

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
precedent) — it is not yet in types/src/flags.rs. Add the OTHER
inlined const enums M4 consumes in the same codegen commit, none of
which are in flags.rs today: `TypeSystemPropertyName` (5.0
pushTypeResolution), `IndexFlags` (5.1 keyof), `SignatureKind`
(5.3/5.7 getSignaturesOfType), `WideningKind` (5.6 reportImplicitAny),
`IterationUse`/`IterationTypeKind` (5.8 iteration protocol),
`MemberOverrideStatus` (5.8 overrides).

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
optional chaining, private names; `checkNonNullExpression` 74990 and
the 2531/2532/2533 + 18047/18048/18049 families live here — but its
TypeFacts filter (`getTypeWithFacts`/`getAdjustedTypeWithFacts`) is
M5-owned: stub it as identity, ledgered M5-stub; identical behavior
when strictNullChecks is off) → assertions/as/satisfies →
template expressions → unary/binary operators (the operator table:
arithmetic 2362/2363, comparison via comparable relation, equality,
in/instanceof, logical, assignment incl. 2322 reporting +
`getRegularTypeOfObjectLiteral` at assignment positions). BINARY
EXPRESSIONS ARE A STATE MACHINE: tsc checks them with an explicit
work-stack trampoline (`createCheckBinaryExpression` 79810, wired as
`var checkBinaryExpression = ...` at 46480) precisely for deep
chains — port that shape, NOT a recursive checkBinaryExpression
(this repo's 50k-term-chain constraint is tested; M1/M2 already paid
this debt once) →
conditional `?:` (union of branches; no narrowing yet) →
await/yield (impl-checker-2xxx §5 rows 12-13: `getAwaitedType`,
async return checking) → JSX for .tsx (impl-checker-2xxx §5b —
ATTRIBUTE-table checking only in this stage; JSX element/component
CALL resolution routes through resolveCall and lands with/after
5.7) → arrow/function expressions (signature from annotation via
`getContextualSignature`; body checking DEFERRED; return-type
inference from body for un-annotated functions —
`getReturnTypeFromBody`, un-narrowed).

COMPLETENESS CHECKLIST: the dispatch switch at 81011
(checkExpressionWorker) is the arm inventory — the prose above is
not exhaustive. Arms it adds that need explicit dispositions:
ClassExpression (mandatory — 5.4's deferred-kind list includes
checkClassExpressionDeferred), NonNullExpression (77960),
ExpressionWithTypeArguments (77963), MetaProperty, SpreadElement,
PrivateIdentifier, QualifiedName, regex/paren/typeof/delete/void/
omitted; CallExpression's import-call split (checkImportCallExpression)
and TaggedTemplateExpression route to 5.7.

Contextual typing arrives WITH this stage per checker-foundations §3:
`getContextualType` (73471) parent-walk + the pushed-context stack +
`ContextFlags`, because object/array literals and function
expressions consume it immediately. EXCEPTION: its
CallExpression-parent arm (getContextualTypeForArgumentAtIndex) needs
getResolvedSignature — return undefined there until 5.7 activates it.

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
default if present, else its constraint, else `unknown` — marked
`/// M6-stub` in the ledger (NOTE: tsc's real no-inference fallback
is default → unknown; the constraint step is an M4-only enrichment —
say so in the ledger so M6's swap doesn't accidentally preserve it).
The stub SURFACE is inference-context construction at BOTH call
sites, not one function: chooseOverload's `createInferenceContext`
(68238) + `inferenceContext.inferredTypeParameters` reads, AND the
overload-failure path (getCandidateForOverloadFailure 76871 →
inferSignatureInstantiationForOverloadFailure). The subtype overload
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
cargo xtask ledger check         # allowed residual stubs, by class:
                                 #   M5-stub  (flow: get_flow_type_of_reference_stub,
                                 #             non-null TypeFacts identity filter)
                                 #   M6-stub  (inference surfaces per 5.7)
                                 #   M8-stub  (conditional/mapped type nodes per 5.1)
                                 # nothing else — M3's normalization stubs
                                 # must be GONE (un-stubbed in 5.3)
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
