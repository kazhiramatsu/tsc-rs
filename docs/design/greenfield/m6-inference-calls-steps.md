# M6: inference + overload completion — steps

Parent design: checker-key-functions.md §2 (inference) and §3 (the
re-run machinery); core-interfaces.md §6 (InferenceInfo contract).
Prerequisite: M5 gate green. This milestone REPLACES exactly one
M4 stub (`infer_type_arguments`) and activates the CheckMode plumbing
M4 ported inert.

**START PRECONDITION (external review 2026-07-14,
[definition-of-done.md](definition-of-done.md) checkpoint table): a
speculation scoped-transaction API must exist BEFORE any stage here
lands.** Today the links contract is only "speculative writes panic"
(links.rs assert_writable) and speculation_depth is raised solely by
its unit tests; candidate trials during overload/inference need a
production `begin_speculation()` guard whose drop/abort rolls back
the contextual/inference stacks, temporary caches, and collected
diagnostics, with commit-on-success — plus failed-candidate rollback
tests. Design and land that (a 7.0-adjacent stage) before 7.1; the
alternative — candidate state leaking through links or blanket
panics mid-resolution — is the exact failure mode the M4 5.7a
deferred re-check protocol only papered over for calls.

Gate: T0 ≥ 58%. Inference moves 2345/2322/2769/2339 together — run
the full gate per stage, not just call fixtures.

## Stage 7.0: canaries [P]

Snapshot 40 fixtures — actual corpus paths:
types/typeRelationships/typeInference/**,
expressions/functionCalls/typeArgumentInference*.ts,
expressions/contextualTyping/**, overload-heavy
(es6/templates/taggedTemplate*, expressions/functionCalls/**).
Same snapshot procedure as M5 6.0.

Commit: `m6 7.0: inference canary list`.

## Stage 7.1: inference data model [M]

`InferenceInfo` / `InferenceContext` from core-interfaces §6 PLUS
three fields §6 omits (fixed there too, this list is authoritative):
`intra_expression_inference_sites` (68286/68290 — populated by
object/array-literal/JSX checking, DRAINED inside the fixing mapper
before is_fixed is set, cleared by checkExpressionWithContextualType
80557), `inferred_type_parameters` (80804 — consumed by
chooseOverload via getSignatureInstantiation, stage 7.4), and the
`outer_return_mapper` cache slot (createOuterReturnMapper 63385).
Also: `top_level`, `is_fixed` (SET exclusively inside
makeFixingMapperForContext 68258 when the fixing mapper resolves a
type parameter on demand — it is mapper machinery, not an
inferFromTypes arm), `implied_arity`, `contra_candidates`, the
`InferencePriority` bit set (generated), the fixing/non-fixing
mapper pair (both Deferred mappers), and `compare_types`. Context
creation: `createInferenceContext` (68238);
`cloneInferenceContext` (68241) serves the OUTER-context NoDefault
mapper inside inferTypeArguments (75951) and createOuterReturnMapper
— the chooseOverload RE-RUN reuses the SAME context (76842-76844;
cloning there would discard fixed inferences — the failure-modes
row 2 is the correct statement).

Commit: `m6 7.1: InferenceInfo/InferenceContext`.

## Stage 7.2: inferTypes / inferFromTypes [M]

The candidate collector (68637/68646), ported arm by arm in tsc
order, priorities attached exactly as the source sets them: the
NoInfer gate FIRST (`isNoInferType` 60427 — Substitution type with
Unknown constraint aborts all inference; the NoInfer intrinsic→
Substitution mapping must exist for it to fire), identical
types, unions/intersections both sides (the naked-type-variable
ordering), literals, template literals + string mappings
(inferTypesFromTemplateLiteralType 68575), index/keyof, conditional
types (inferToConditionalType 69011), mapped-type homomorphic
inference (inferToMappedType 68972; ReverseMapped needs
createReverseMappedType 68398 + getTypeOfReverseMappedSymbol +
inferReverseMappedType 68441 + the mapped-type accessors),
object/signature structural inference
(`inferFromProperties`, `inferFromSignatures` with bivariance rules),
array/tuple element inference, contra-candidate collection at
contravariant positions, `top_level` cleared at CANDIDATE-RECORD
time via `isTypeParameterAtTopLevel(originalTarget, …)` (68732 — not
a flag threaded down the descent; threading one diverges),
priority comparison (LOWER wins; equal priorities accumulate).

ARM DISPOSITIONS (same discipline as M3 4.6): the conditional-type
and mapped/ReverseMapped arms are DORMANT for as long as M4 5.1 left
those type kinds as M8-ledgered stubs — port the arms against
source, ledger them dormant, and pin them when the constructors go
live (M8 at the latest; earlier if M4 chose to port them). The
template-literal arm is LIVE (M3 builds the type kind); the
string-mapping arm goes live with M4 5.1/5.2 (`Uppercase<...>` is an
intrinsic ALIAS reference — needs generic alias instantiation, which
M3's annotation path lacks).

Verify per commit against canaries; expect 2345-family movement only
after 7.4 wires results in.

Commit(s): `m6 7.2a-d: inferFromTypes arms`.

## Stage 7.3: resolving inferences [M]

- `getCovariantInference` (69263) — the FULL widen-literals condition
  from checker-key §2.1 including `hasPrimitiveConstraint`,
  `isTypeParameterAtTopLevelInReturnType`, and the
  PriorityImpliesCombination union-vs-common-supertype split
  (Subtype relation from M3 4.8 feeds getCommonSupertype).
- `getContravariantInference` (69260, intersection-vs-common-subtype
  split — `getCommonSubtype` 67662 is scheduled nowhere earlier; port
  it here alongside M3's getCommonSupertype).
- `getInferredType` (69271) — the constraint clamp EXACTLY as the
  checker-key §2.2 skeleton: ReturnType-priority inferences FILTER to
  the compatible part; others go never → fallback → instantiated
  constraint. Defaults instantiate with the backreference mapper
  (63381) merged with the nonFixingMapper;
  NoDefault/AnyDefault flags honored (NoDefault → silentNeverType,
  which carries NonInferrableType so it can never become a candidate).
- CACHE WIRING + INVALIDATION (the milestone table's "generics
  instantiation caches" — otherwise unscheduled):
  `signature.instantiations` keyed by getTypeListId (59902-59910);
  getInferredType calls `clearActiveMapperCaches()` (73624, at 69310)
  to invalidate M4 5.2's active-mapper instantiation caches;
  `reverseHomomorphicMappedCache` (68387); `clearCachedInferences`
  (68279) on every candidate/topLevel mutation. Stale
  non-fixing-mapper instantiations across candidate accumulation are
  a silent-wrong-type source — port the invalidation discipline, not
  just the caches.

Commit: `m6 7.3: covariant/contravariant inference + clamp`.

## Stage 7.4: inferTypeArguments + the re-run [M]

- `inferTypeArguments` (75938): the contextual-return pre-inference
  is TWO passes, not one (75944-75961 — the piece most
  first-implementation quality gaps traced to; checker-key §2.3 had
  them conflated, corrected there): (a1) ReturnType-priority
  inference against the contextual type instantiated through
  `cloneInferenceContext(outerContext, NoDefault)`'s mapper, skipped
  when isFromBindingPattern, generic contextual signatures routed
  through getSignatureInstantiationWithoutFillingInTypeArguments;
  (a2) a FRESH `returnContext = createInferenceContext(...)` (75957)
  doing priority-None inference from the contextual type under
  `createOuterReturnMapper(outerContext)` (63385) — `context.
  returnMapper` comes from cloneInferredPartOfContext of THAT
  context (75960), NOT from the ReturnType-priority pass. Outer
  context comes from `getInferenceContext` (73599) walking the
  inference-context NODE STACK — push/pop it alongside M4 5.5's
  contextual-type stack. Then PHASE (b) per-argument inference via
  `checkExpressionWithContextualType` (80557) + `getTypeAtPosition`,
  rest args via `getSpreadArgumentType` (76002).
- DELETE the M4 stub; `chooseOverload`'s inference path now runs the
  real thing, INCLUDING the SkipContextSensitive /
  SkipGenericFunctions first pass and the NORMAL-mode RE-RUN before
  committing a candidate (checker-key §3.2 — the plumbing M4 laid;
  the re-run reuses the SAME InferenceContext, stage 7.1).
- The SkipGenericFunctions CONSUMER side (scheduled nowhere else —
  without it higher-order generic inference degrades silently):
  `skippedGenericFunction` (80816, sets SkippedGenericFunction on the
  context), checkExpression's higher-order path (80760-80815:
  getUniqueTypeParameters 80843, hasOverlappingInferences,
  mergeInferences, `context.inferredTypeParameters = ...` 80804,
  instantiateSignatureInContextOf 75910), and chooseOverload's
  consumption via `getSignatureInstantiation(candidate, ...,
  inferenceContext.inferredTypeParameters)` (76844).
- Context-sensitive argument detection (`isContextSensitive` 63832)
  and the deferred body interaction (M4's driver already defers
  bodies; the re-run is what types their parameters).

Commit: `m6 7.4: inferTypeArguments + chooseOverload re-run`.

## Stage 7.5: consumers cleanup [M]

Ripple sites that were declared-type-only until now: contextual
tuple/array element inference in literals, generic
constructor/`new` inference, tagged templates, JSX element type
resolution's call path, `satisfies` interplay, and the
2769 failure-path candidate choice (getCandidateForOverloadFailure
with real instantiated candidates). Also owned here (the M3 code
markers say M6; no other milestone schedules them): full-radix
`parsePseudoBigInt` (18909 — M3 ported the decimal slice only,
annotate.rs) and `isValidBigIntString` (18973) for bigint
template-literal placeholders
(isValidTypeForTemplateLiteralPlaceholder's bigint arm is a live
Unsupported in structural.rs). Re-probe the M4 NOTES top-10
list; retire entries this milestone fixed.

Commit: `m6 7.5: inference consumers (+rate)`.

## Final gate

```sh
cargo xtask conformance      # expect: T0 ≥ 58%
cargo xtask ledger check     # zero M6-stub entries remain
```

## Expected failure modes

| Symptom | Diagnosis | Fix |
|---|---|---|
| Literal type args where oracle widens (or inverse) | widen-literals condition simplified | The condition is FOUR clauses (checker-key §2.1); port, don't paraphrase |
| Callback parameter types wrong only in overloads | re-run skipped or run against the un-instantiated candidate | Re-run uses the SAME InferenceContext, NORMAL mode, then re-instantiates (checker-key §3.2) |
| 2345 fixed but new 2322 downstream | inference result leaked into caches during a failed candidate | Candidate probing must not write links (speculation_depth discipline, greenfield §4.3) |
| Constraint violations infer `never` where oracle keeps part | ReturnType-priority FILTER branch missing | getInferredType clamp has three outcomes, not one |
| Context-sensitive arg checked twice with different types | isContextSensitive classification diverges | Port the tsc predicate; it gates the whole two-pass scheme |
