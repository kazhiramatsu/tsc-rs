# M4: checker skeleton — expressions + statements, declared types — steps

Parent design: ../checker-foundations.md §1-§7 (this milestone IS
that doc, sequenced); ../checker-key-functions.md §3 (resolveCall,
ported here with inference stubbed). Prerequisite: M3 gate green.

Scope rule (greenfield §12): everything EXCEPT flow narrowing (M5)
and type-argument inference (M6). An identifier's type is its
declared/flow-initial type; generic calls without explicit type
arguments instantiate their type parameters per the stub in stage
5.7. Gate: T0 ≥ 35%.

From this milestone on, run `cargo xtask conformance` after EVERY
stage and record the rate in the commit body; the ratchet activates
at the first stage that produces diagnostics.

Ledger rule (M3 review lesson — four wrong verdicts traced to false
unreachability claims): any ledger comment declaring an arm
unreachable/DEAD in the current milestone must cite a
constructibility argument or a pin that would catch the arm going
live.

## Stage 5.0: checker state + the resolution spine [M]

- EXTEND the existing `CheckerState` (checker/src/state.rs — M3
  built it; it already carries binder/options/tables/links/relations/
  `speculation_depth`; do NOT create a second struct): add files, the
  diags sink, and the resolution machinery below (greenfield §4.3 —
  all links writes assert speculation_depth is 0 or route through a
  transaction).
- `pushTypeResolution`/`popTypeResolution` (55728) + the
  (target, kind) cycle stack per checker-foundations §1.2, with the
  circularity reporting pattern (2502/7022 family) at each consumer.
- `error_at`-family helpers taking `&'static DiagnosticMessage` only.
- GLOBAL-TYPE BOOTSTRAP (no other stage owns it; the resolvers are
  LAZY, so the mechanism lands here and each global starts resolving
  the moment 5.1's declared types exist — keeping 5.1's array arm and
  5.3's apparent chain unblocked instead of inverting their
  dependency): the initializeTypeChecker slice (88732) that binds
  globals — `globalArrayType` (88788),
  `globalObjectType`/`globalFunctionType`/...,
  `globalReadonlyArrayType` (88863 — note the `|| globalArrayType`
  fallback) — over `getGlobalType` (60663) / `getGlobalTypeSymbol`
  (60635) / `getGlobalSymbol` (60650, a locationless resolveName) /
  `getGlobalTypeOrUndefined` (60898), plus the deferredGlobal* memo
  resolvers (pattern at 60679). Also materialize the init-block types
  M3 skipped: `emptyGenericType` (47170) and `anyFunctionType` (47179
  — intersect.rs's vacuous exclusion becomes real). noLib semantics
  STAY the default: conformance runs WITHOUT lib files today, so
  globals resolve only when the fixture declares them, and an
  undeclared global falls back per `getTypeOfGlobalSymbol` (60604):
  program-level 2318 + emptyGenericType (arity > 0) /
  emptyObjectType. The M3 probe-world rule (apparent(primitive) =
  emptyObjectType under noLib) is thereby the CORRECT M4 behavior,
  not a shortcut; it changes only if/when lib loading exists.
  Consumers this un-blocks: array `T[]` type nodes (5.1's annotate.rs
  arm), `"x".length` (5.3's apparent chain), the excess-property
  check's globalObjectType arm, the single-rest tuple collapse
  `[...T[]]` → Array<T> (getTupleTargetType 61146-61148 — a live
  M4Dependency escape in tables.rs), and the array-source relation
  arms (66432-66438).

Commit: `m4 5.0: checker state + resolution stack + global bootstrap`.

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
- `resolveEntityName` (49292) for qualified type names — the M3
  annotation slice takes plain identifiers only; its qualified-name
  arm is a live Unsupported in annotate.rs.
- Generic type-ALIAS instantiation: `getTypeAliasInstantiation`
  (60261) on the type-reference path (consumes 5.2's instantiateType
  — same forward-dependency discipline as the note below). The
  "(M4 5.1)" markers in annotate.rs/structural.rs/unions.rs point
  here, and M6 7.2's string-mapping arm assumes it exists.
- REST-PARAMETER SIGNATURES: the full `getSignatureFromDeclaration`
  (59569) port, superseding the M3 annotation-only slice — rest
  params are signature construction, not relation work; landing this
  retires the five live Unsupported arms in the arity-helper family
  (getTypeAtPosition/getParameterCount/getMinArgumentCount/
  hasEffectiveRestParameter, 78233-78341).
- Type parameters become CONSTRUCTIBLE here, so in the SAME commit:
  un-stub the union-side `removeConstrainedTypeVariables` (61450,
  called from getUnionTypeWorker at 61551) AND getIntersectionType's
  step-6 type-variable collapse (61821-61839, checker-foundations
  §4.2 step 6 — the intersect.rs arm returns Unsupported today), and
  remove the twin `unreachable!()` guards (unions.rs + the tables
  twin) that panic the moment a constrained type variable reaches
  union construction. Neither is scheduled anywhere else.
- TWIN RULE (invariant from here on): ALL checker-side union
  construction routes through the checker twin `get_union_type_ex`
  (unions.rs), NEVER `tables.get_union_type` — only the checker twin
  runs the relation-dependent reductions
  (`removeStringLiteralsMatchedByTemplateLiterals` 61434, subtype
  reduction).

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

StringMapping goes LIVE here: `getStringMappingType` (62119) +
`getStringMappingTypeForGenericType` (62154). `Uppercase<...>` etc.
are intrinsic ALIAS references (5.1's getTypeAliasInstantiation
routes them); landing these flips the M3-DEAD StringMapping relation
arms live (the "(M4 5.2)" Unsupported markers in structural.rs and
unions.rs, plus tables.rs isPatternLiteralType's dead arm). Pin rows
land in 5.3b.

Commit: `m4 5.2: instantiateType + TypeMapper`.

## Stage 5.3: member resolution [M]

Prereq: the 5.0 global-type bootstrap — this stage's apparent chain
(`globalStringType` for `"x".length` etc.) reads globals that 5.0's
lazy resolvers bind.

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

Union MEMBER synthesis is this stage's work too:
`resolveUnionTypeMembers` (58224) with `getUnionSignatures` (58055) +
`getUnionIndexInfos` (58210) — retires structural.rs's two live
"(M4)" Unsupported arms (union signature / union index-info
resolution). So is TUPLE member synthesis: the per-index/`length`
property synthesis (61160-61185) that M3's createTupleTargetType port
elided, and `createNormalizedTupleType`'s (61213) M4Dependency
escapes in tables.rs — union/never variadic distribution, array-like
variadic elements, variadic-in-rest-window, and the 2799/2800
tuple-too-large diag site (61240-61246). Late-bound computed-name
members: `lateBindMember` (57662) /
`getResolvedMembersOrExportsOfSymbol` (57712) + the late-bindable
index-signature arm (60018-60049; annotate.rs and engine.rs carry
live Unsupported arms) — port here, or re-mark those arms
`/// M7-stub` explicitly if the corpus rate doesn't demand them.

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
marker propagation into RelationComparisonResult. AS-LANDED
CORRECTION (supersedes "un-stub the two M3 ledgered stubs"): M3 left
NO relateVariances stub — the whole reference arm (66420-66431) is
comment-elided in structural.rs, so port `relateVariances` (66488)
and its call-site arm FRESH. And the cache-hit variance-replay branch
(65738-65750) needs more than un-stubbing: EXTEND the relation-cache
entry format to persist the ReportsUnmeasurable/ReportsUnreliable
bits — the M3 engine's cache writes store SUCCEEDED/FAILED only,
while tsc accumulates `propagatingVarianceFlags` (65804-65808) into
every write (65853, 65865). Prereq: add `VarianceFlags` to the M0
codegen `SourceEnum` seed (const enum inlined in `_tsc.js`, Ternary
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
`isEnumTypeRelatedTo` + the `enumRelation` symbol-pair map here) —
PLUS the five other categories the pin-file header defers
(pins/relations.toml): arrays `T[]` (5.0 bootstrap + 5.1 array arm),
StringMapping (5.2), primitives vs objects-with-required-members
(5.3 apparent types), tuple-to-object (5.3 tuple member synthesis),
rest-parameter signatures (5.1's getSignatureFromDeclaration). Each
row lands with the stage that makes it constructible; ALL are green
by this stage's exit. `cargo xtask relpin run` green over the widened
suite is part of this stage's exit, and stays green through the M4
gate.

AS-LANDED NOTES (2026-07-11): the stage split into two commits —
`m4 5.3b-i` (enums: declared types + the shared constant evaluator +
isEnumTypeRelatedTo; latent fixes: tables regular-of-union gained
mapType's no-change identity, fresh literal twins copy the symbol,
isConstantVariable reads COMBINED node flags) and `m4 5.3b` (variance
proper). Marker measurement RELIES on bare type-parameter relations,
so the structural source-TypeVariable arm (66291, constraint chase)
and target-TypeParameter comparable loop (66098) un-escaped here as
prerequisites, together with the adjacent same-block arms: alias
variance fast path (66081), single-element generic tuples (66095),
readonly-array/array targets (66432), generic-tuple constraint
(66439), array-source tuple walks in propertiesRelatedTo (66772).
The rest-parameter pin category forced the getNonArrayRestType family
(getRestTypeAtPosition/getRestOrAnyTypeAtPosition/
getNameableDeclarationAtPosition) and compareSignaturesRelated's
reportUnreliableMarkers parameter; the single-rest tuple collapse to
(readonly) Array lives in the checker's createTupleType wrapper (the
tables twin keeps its escape for normalization-internal callers).
SymbolLinks.variances maps tsc's undefined/emptyArray/array tri-state
onto LinkSlot Vacant/Resolving/Resolved; the handler chain is an
explicit Base/Propagating frame stack on CheckerState. ReportsUnmeasurable
has NO live producer until M6/M8 (reportUnmeasurableMapper's callers
are mappedTypeRelatedTo and inference) — the bits persist and replay,
only Unreliable fires today (template arm 66279, rest-parameter probe
64519, measurement-mode typeArgumentsRelatedTo). The ForCheck marker
pair (47216-47218) waits for 5.4's checkTypeParameterDeferred.

Commit(s): `m4 5.3b-i: enums — declared types, the constant evaluator,
isEnumTypeRelatedTo` + `m4 5.3b: variance + M3-deferred relation pins`.

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

AS-LANDED NOTES (2026-07-12): checker/src/check.rs. The dispatch
landed with the FULL kind list; besides the driver spine, the live
arms are checkBlock (statement recursion), the three declaration arms'
checkTypeParameters slices (interface/alias/class), checkTypeParameter
(2716/2344/2368 + the checkNodeDeferred registration),
checkTypeParameters' inline-lazy closures (2744/2706/2300),
checkTypeReferenceNode + checkTypeArgumentConstraints (explicit
type-argument 2344), and checkTypeParameterDeferred (2636/2637 over
the ForCheck marker pair). All other arms are no-op escapes named
after their tsc worker and owner stage (grep source_element_stub);
deferred-node arms other than TypeParameter are unreachable!() until
5.5/5.7 land their checkNodeDeferred registrations. Discovered facts:
(1) the 2716-lands-on-the-second-parameter ordering REQUIRES the
checkTypeReferenceNode forcing pass (checkSourceElement(node.default)
runs before hasNonCircularTypeParameterDefault) — and that recursion
re-enters the SAME reference node, so getTypeFromTypeReference's tail
links write is tsc-unguarded OVERWRITE semantics (the one sanctioned
write-twice site, links.overwrite_type_reference_resolution). (2)
checkTypeAssignableTo landed as a HEAD-message slice (code/span/args
exact, chain tail elided as T2) over a fallible typeToString slice
(markers render super-/sub- + varianceTypeParameter per 51535;
display failure drops the diagnostic rather than mis-printing). (3)
The FP=0 gate forced three honest failure-band gates in
onFailedToResolveSymbol: default-lib names (lib_globals.rs — the
oracle checks WITH libs, we bind none, so lib-name misses are
architecture artifacts, not observables), an all-meanings re-probe
(alias resolution/alternate-code arms unported), and
declare-global-bearing programs (augmentation binding unported); plus
getSuggestedLibForNonExistentName's static table (2583/2584 args) and
the getExportsOfSymbol globalThis special case (real bug found by the
2694 FP). (4) File-less diagnostics are EXCLUDED from per-file output:
tsc's init-band 2318s precede the getDiagnosticsWorker global
snapshot, so per-file semantics never surface them — our lazy-global
architecture would otherwise leak them as FPs. (5) plainJSErrors
(program layer) + checkJs:false skip-all landed with the driver;
aggregate output is sortAndDeduplicateDiagnostics'd like
getPreEmitDiagnostics. addLazyDiagnostic runs callbacks inline — the
eager identity (checkSourceFileWithEagerDiagnostics 87104) is this
program's only mode. NodeCheckFlags joined the codegen seeds;
currentNode landed as driver state (2589 now node-anchored). The
unreachable-code slice (86763) is elided whole to M5
(suggestion-band by default + isReachableFlowNode).

Commit: `m4 5.4: check driver (eager/deferred)`.

## LIB-LOADING DECISION POINT (raised by 5.4's FP burn-down) [!]

5.4 exposed the structural tension the 5.0 noLib decision deferred:
the conformance oracle checks every program WITH its target's default
libs (harness resolve_program_libs feeds programJson.libs; the oracle
host loads them), while this engine binds none — so every new wave of
checker-driven FORCING mints diagnostics tsc's world never shows.
5.4's wave (name resolution: 2304/2583 on Date/Promise/Partial/...)
was absorbed by honest failure-band gates (lib_globals.rs + the
re-probe/augmentation gates), at FN cost. The 5.5 wave will NOT be
gateable the same way: expression checking forces PROPERTY access on
lib-typed values (`"x".length`, array methods, Promise members), and
under noLib the apparent-type chain falls back to emptyObjectType —
the 2339-family failures are property-level, not name-level, and
suppressing them means gating the heart of the 2xxx band. The M4 exit
gate (T0 >= 35%) is unlikely to be reachable under noLib.

The architecture already leaves the door open:
- Lib files are the program PREFIX, so for a fixed lib set the
  node/array/symbol id bases of every lib file are IDENTICAL across
  programs — parse+bind ONCE per lib set and share the immutable
  SourceFiles/Binders (the id-base design makes the cache exact, not
  approximate). Fixture files then bind on top with the usual bases.
- Goldens collect diagnostics for FIXTURE files only (driver.mjs
  iterates programJson.files, not libs), so lib files never need
  CHECKING — only resolvability. skipLibCheck semantics are moot;
  laziness bounds typing cost to what fixtures actually force.
- Unsupported holes (mapped types in Partial, conditionals in
  ReturnType) stay honest FN escapes when forced — no FP exposure.

Retires on landing: lib_globals.rs whole, the 2583-dead-branch note,
most of the failure-band FN cost, and the per-wave gate treadmill.
Decide BEFORE starting 5.5; the recommendation as of 5.4 is to land
lib loading first (it is program plumbing, not checker semantics, and
every later stage's rate reads become honest against it).

DECIDED (2026-07-12): executed as its own staged insert —
m4-lib-loading-steps.md (L1 lib corpus gate → L2 plumbing → L3
per-lib-set cache → L4 measurement + lib_globals retirement). The
oracle contract turned out SIMPLER than feared: the oracle host runs
noLib:true with the harness-expanded libs as ordinary prepended ROOTS,
so <reference lib> is inert program-wide, tsc's default-lib bucket
ordering is unreachable, and getSourceFiles order == ProgramJson.libs
order ++ fixtures (empirically pinned) — the engine consumes the libs
list as given.

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
`getRegularTypeOfObjectLiteral` at assignment positions — 5.6 owns
that port, this stage only calls it). BINARY
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

RE-ENTRANCY TRAP (from 5.4): node resolvedType caches are write-once
(panic on rewrite) everywhere EXCEPT the TypeReference arm, whose tail
became tsc's unguarded overwrite at 5.4
(links.overwrite_type_reference_resolution) because the
type-parameter-default recursion (getResolvedTypeParameterDefault
59043) re-enters the node mid-computation. Any type-node check* arm
landing here that FORCES ITS OWN node (getTypeFromTypeNode(node)
before/around child recursion, the checkTypeReferenceNode shape)
inherits the same requirement the moment the node can sit in a
type-parameter default subtree — `interface P<T = Q[]>` variants
re-enter through the inner reference today, but a self-forcing
checkArrayType/checkUnionOrIntersectionType/checkTypeOperator arm
moves the double-write onto ITS node kind. At each arm's landing:
either route its cache write through overwrite semantics (tsc's
shape) or prove the arm's nodes unreachable from default subtrees.
The write-once panic is the tripwire — it fails loud, not wrong.


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

## Unsupported channel in M4

The CheckResult2/Unsupported channel STAYS through M4 — it is the
probe/conformance escape valve; retiring it is not an M4 goal. The
gate rule: every arm currently marked plain "(M4)" is either
implemented by its owning stage or re-marked with an explicit
M5/M6/M7/M8-stub class by gate time. The previously-unowned arms and
their owners (details in the owning stages):

- rest-parameter signatures (five Unsupported arms in structural.rs's
  arity-helper family, 78233-78341) → 5.1
  (`getSignatureFromDeclaration` 59569).
- union signature synthesis + union index-info synthesis
  (structural.rs) → 5.3 (`resolveUnionTypeMembers` 58224).
- late-bound computed-name members (annotate.rs/engine.rs) → 5.3, or
  an explicit M7-stub marker (stage 5.3's item decides).
- qualified type names (annotate.rs) → 5.1 (`resolveEntityName`
  49292).

## Final gate

```sh
cargo xtask conformance          # expect: T0 ≥ 35%
cargo xtask relpin run           # widened suite (incl. 5.3b rows) still 0
cargo xtask invariants --suite idempotence
cargo xtask ledger check         # span/hash freshness only — it has
                                 # NO stub-class support
grep -rn "M[5-8]-stub" crates/   # the stub audit is MANUAL (optional
                                 # xtask work: a `ledger check --stubs`
                                 # mode). Allowed residual classes:
                                 #   M5-stub  (flow: get_flow_type_of_reference_stub,
                                 #             non-null TypeFacts identity filter)
                                 #   M6-stub  (inference surfaces per 5.7)
                                 #   M7-stub  (late-bound members, only if 5.3
                                 #             took that option)
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
