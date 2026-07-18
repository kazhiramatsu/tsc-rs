# M5: control-flow narrowing — steps

Parent design: checker-key-functions.md §4 (the complete port map —
this doc only sequences it); core-interfaces.md §5. The bind-time
graph exists since M2 stage 3.5. Prerequisite: M4 gate green.

Gate: T0 ≥ 50%; idempotence + jobs-independence invariants green.

## Stage 6.0: canaries [P]

Before touching the checker, snapshot 30 narrowing-heavy fixtures
(controlFlow/**, expressions/typeGuards/**,
es2021/logicalAssignment/**) via
`cargo xtask conformance --files <list>` (mismatches land in
target/conformance/mismatches.json; a `--dump` convenience flag is
harness work if wanted) — every stage compares against these first
(fast signal), full corpus after.

Commit: `m5 6.0: narrowing canary list`.

## Stage 6.1: prelude + FlowType + the walk skeleton [M]

PRELUDE — two dependency groups every later stage consults, ported
FIRST in this stage:

- The union traversal utilities, as a unit: `forEachType` (69979),
  `someType` (69982), `everyType` (69985), `filterType` (69991),
  `mapType` (70028), `isTypeSubsetOf` (69962). They back
  getTypeWithFacts/getAssignmentReducedType/the narrowers/the JOINs
  and are scheduled in no earlier milestone.
- `isMatchingReference` (69448) + `getAccessedPropertyName` (69493),
  MOVED UP from the old 6.5 slot: 6.2's getTypeAtFlowAssignment opens
  with it (70504) and every 6.4 narrower consults it. Port DIRECTLY
  (checker-key §4.6): symbol-resolved identifiers, property/element
  access with constant index forms, this/super/meta-property,
  comma/assignment unwrapping. Do NOT substitute a simplified
  reference-key model (that approximation is a documented FN family
  in the first implementation).

THE SKELETON:

- `FlowType` = Type | Incomplete(Type) per checker-key §4.1 — port
  the wrapper, not a boolean flag on the checker. Include
  createFlowType's (70070) never→silentNeverType substitution inside
  incomplete wrappers (distinguishes "back-edge unresolved" from a
  real never).
- `getFlowTypeOfReference` (70394) entry: reference key, declared/
  initial types, the flow-container logic. Its postlude calls two
  later-stage pieces — `finalizeEvolvingArrayType` (6.2) and the
  NonNullExpression-parent `getTypeWithFacts` filter (6.4 item 1) —
  stub both as identity in this stage, ledgered, or the rate-neutral
  claim below fails under @strict.
- CALLER INTEGRATION is bigger than a stub swap — enumerate it:
  checkIdentifier's flow block (72150-72213) with
  `getControlFlowContainer` (71477), the constant-variable
  flowContainer-hoisting loop (`isConstantVariable`/
  `isPastLastAssignment`), the `assumeInitialized` computation and
  initialType selection (`getOptionalType`/`undefinedType`/
  `removeOptionalityFromDeclaredType`), auto-type noImplicitAny
  reporting and 2454 (see 6.6 note); plus `getNarrowedTypeOfSymbol`
  (72001 — destructured discriminated-union narrowing; its
  dependent-parameter arm reads `getInferenceContext(func)
  .nonFixingMapper` — GUARD it as an M6-deferred ledgered stub).
- `getTypeAtFlowNode` (70420) from the full skeleton in checker-key
  §4.2: the backward loop, the antecedent-walk-on-None convention,
  the SHARED-node per-query cache (sharedFlowStart discipline), the
  Start outer-container resume rule, depth-2000 disable.
- Arms landed as stubs returning None/declared in this stage, filled
  next: assignment, call, condition, switch, labels, array mutation,
  ReduceLabel (port the antecedent-swap NOW — it is self-contained).

Replace M4's `get_flow_type_of_reference_stub` call site. Rate should
be unchanged (arms inert + the two postlude stubs) — that null flip
is the stage verification.

Commit: `m5 6.1: prelude + FlowType + getTypeAtFlowNode skeleton`.

## Stage 6.2: assignments + initial types [M]

`getTypeAtFlowAssignment` (70502) + `getInitialType` (69905) /
`getAssignedType` (69861) + `getNarrowableTypeForReference` (71640) +
declared-vs-initial distinction; the assignment-reduced-type rule
(`getAssignmentReducedType`: union declared types reduce by member
filter; the RHS is never adopted EXCEPT a `never` RHS, which is
returned as-is (69675); fresh boolean literals are remapped via
getFreshTypeOfLiteralType; the worker is memoized under an
`A{id},{id}` getCachedType key). getTypeAtFlowAssignment also calls
`isReachableFlowNode` (70505/70526) — stub it returning true until
6.6, ledgered. auto/evolving arrays
(`getEvolvingArrayType`/`finalizeEvolvingArrayType`/`convertAutoToAny`)
port HERE, complete — `let x; ... x` and `var a = []; a.push(...)`
are corpus-common.

Commit: `m5 6.2: flow assignments + evolving arrays`.

## Stage 6.3: joins [M]

`getTypeAtFlowBranchLabel` (70653) with the empty-switch-clause
bypass and the declared-type short-circuit;
`getTypeAtFlowLoopLabel` (70694) — THE fixpoint, ported with its two
non-negotiables (checker-key §4.4): the flowTypeCache swap during
back-edge resolution, and never caching while incomplete.
`getUnionOrEvolvingArrayType` (70756) incl. recombineUnknownType and
declared-type identity preservation. Subtype reduction in JOINs works
because M3 stage 4.8 landed.

THE INVARIANTS NEED THREE M4-OWNED CALL-SITE EDITS — without them the
"non-negotiables" don't actually hold: (a) `checkExpressionCached`
(80580) saves/resets flowLoopStart + flowTypeCache around uncached
checks; (b) `getTypeOfExpression` (80895) writes flowTypeCache +
NodeFlags.TypeCached only when flowInvocationCount changed — this
cache IS what the loop-label swap invalidates; (c) getResolvedSignature
caching is guarded by `flowLoopStart === flowLoopCount` (77505) so
signatures resolved mid-loop are never cached. Edit all three when
this stage lands.

TWO SEAM EXTENSIONS LANDED WITH THIS STAGE (both tsrs-native, both
retire with their dependencies):
- **JOIN-SEAM catch** (walk dispatch, `[FLOW 6.3 JOIN-SEAM]`): an
  Unsupported unwind anywhere inside a join computation — an
  antecedent walk pulling a back-edge RHS, or the union's Subtype
  reduction relating members through an unported M6/M8 family —
  degrades to the 6.2 seam (flag + declared type) instead of
  containing the enclosing statement. Rationale: the 6.2 label stubs
  never computed any of this, so statements they let complete must
  not regress to containment (caught live: lib-esnext generator
  machinery under `yieldExpressionInControlFlow.ts` hits the
  mapped-type stub from remove_subtypes inside the loop fixpoint's
  union). The exit revert makes the final answer EXACTLY the 6.2
  stub's, so the FP=0 argument is inherited from 6.2, and the flag
  keeps the result out of flowLoopCaches.
- **flowLoopCaches seam guard**: a fixpoint whose query crossed a
  still-inert (or seam-caught) arm is answered but never cached —
  the memo outlives the query, and a later same-key query hitting it
  would skip the walk (and the flag), leaking the over-wide answer
  past the query-exit revert. Constant-off once 6.4 retires the flag.

`isExhaustiveSwitchStatement` (consumed by the branch bypass) stays a
conservative `false` stub — 6.6 owns the real computation; every
bypass walk crosses the still-inert switch-clause arm and reverts, so
the stub value is unobservable this stage.

Commit: `m5 6.3: branch/loop joins + fixpoint`.

## Stage 6.4: conditions + the narrowers [M]

`getTypeAtFlowCondition` (70614) then `narrowType` (71400) dispatch
per checker-key §4.5, then the sub-narrowers one commit each, tsc
order, with the facts model first:

1. `getTypeFacts` (69697) / `getTypeWithFacts` /
   `getAdjustedTypeWithFacts` — TypeFacts bits are generated (M0,
   table at 46297). Un-stub M4 5.5's non-null identity filter here.
2. `narrowTypeByTruthiness` (incl. the discriminant-property path).
3. `narrowTypeByEquality` (71048) + `narrowTypeByBinaryExpression`
   (===/!==/==/!= incl. null/undefined special cases, assignment
   narrowing, `instanceof`, `in`).
4. `narrowTypeByTypeof` + switch-on-typeof
   (`getSwitchClauseTypeOfWitnesses` 69948).
5. `narrowTypeByDiscriminant` + `narrowTypeBySwitchOnDiscriminant` +
   `getTypeAtSwitchClause` + the union key-property fast path
   (`getKeyPropertyName` 69612 / `getConstituentTypeForKeyType` 69625
   / `mapTypesByKeyProperty` 69587, lazy keyPropertyName/
   constituentMap, ≥10-constituent threshold — NOTE: M3's
   typeRelatedToSomeType also consumes this via
   getMatchingUnionConstituentForType 69630/65495; if M3 stubbed it,
   un-stub here).
6. `narrowTypeByCallExpression` (type predicates `x is T`,
   `asserts x`, assertion signatures) + `getTypeAtFlowCall` (70566)
   with `getEffectsSignature` (70194 — deps: `getTypeOfDottedName`
   70162, checkNonNullExpression, getResolvedSignature, the
   links.effectsSignature cache). Signature type-predicate
   MATERIALIZATION (`getTypePredicateOfSignature` 59765 + predicate
   construction from `x is T`/`asserts x` type nodes) is scheduled
   nowhere earlier — port it as part of this item. Known FN class
   until M6: generic assertion/predicate signatures resolve through
   the stubbed inference.
7. `narrowTypeByOptionality` (71440) + optional-chain/`??`
   containers.
8. The const-inlining rule (`if (c)` where `const c = <guard>` —
   inlineLevel < 5, strict) inside narrowType's Identifier arm,
   gated by `isConstantReference` (70374) + `isConstantVariable` +
   annotation-free initializered declaration.

COMPLETENESS: the arm inventory is the source region 70766-71460,
not the 8 items above — narrowTypeByBinaryExpression and
getTypeAtSwitchClause additionally dispatch to
`narrowTypeByConstructor` (71231), `narrowTypeByBooleanComparison`
(70891), `narrowTypeByPrivateIdentifierInInExpression` (71021),
`narrowTypeBySwitchOnTrue` (71192), the discriminant-property pair
(70833/70845), the shared worker `getNarrowedType`/`getNarrowedTypeWorker`
(71309, with its getCachedType N-key cache), and the matcher helpers
`containsMatchingReference`/`optionalChainContainsReference`/
`hasMatchingArgument`/`getReferenceCandidate` (69544/69553/69644/
69911) + `getCandidateDiscriminantPropertyAccess` (70766, has
binding-pattern arms — see getNarrowedTypeOfSymbol in 6.1).

Commit(s): `m5 6.4a-h: narrowers (+canary/rate per commit)`.

## Stage 6.5: isMatchingReference — MOVED INTO 6.1 [tombstone]

Moved to the 6.1 prelude: 6.2's getTypeAtFlowAssignment and every
6.4 narrower consult it, so sequencing it after them was a
dependency inversion. Content unchanged — see 6.1.

## Stage 6.6: reachability [M]

`isReachableFlowNode` (70240) per checker-key §4.7 (lastFlowNode
single-entry cache + shared-node cache, never-call gates; un-stub
6.2's true-stub), consumed by: unreachable-code reporting (7027 with
the within-unreachable-range suppression), switch exhaustiveness
(`isExhaustiveSwitchStatement`/`computeExhaustiveSwitchStatement`
78920/78933 — also consumed by 6.3's branch-label bypass; needs
getSwitchClauseTypeOfWitnesses + checkExpressionCached) and 7029
fallthrough (noFallthroughCasesInSwitch +
clause.fallthroughFlowNode), implicit-return checks (2366/7030
family, `checkAllCodePathsInNonVoidFunctionReturnOrThrow`, wrapped
in M4's addLazyDiagnostic slot), and definite-assignment: the real
family is `isSymbolAssignedDefinitely`/`isSymbolAssigned`/
`isPastLastAssignment`/`markNodeAssignments` (71480-71523,
symbol.lastAssignmentPos) — there is no tsc function named
`isDefinitelyAssigned`; 2454 itself falls out of checkIdentifier's
initialType logic (6.1 caller integration), not out of reachability.

ALSO OWNED HERE — the class-property flow-init family, escaped with
owner=M5 but scheduled in no earlier stage (the 6.2 review caught the
gap): `getFlowTypeOfProperty` (70153) / `getFlowTypeInConstructor`
(70118) / `getFlowTypeInStaticBlocks` (70136) behind the access.rs
`get_flow_type_of_access_expression` this-property arm and the
annotate.rs constructor/static-block-assigned property-type arms (the
four `[FLOW M5]` escapes). They ride reachability's stage because
their consumers are the definite-assignment/2565 family this stage
completes; retire the four escapes when they land.

Commit: `m5 6.6: reachability + its consumer checks`.

## Final gate

```sh
cargo xtask conformance                     # expect: T0 ≥ 50%
cargo xtask invariants --suite idempotence
cargo xtask invariants --suite jobs-independence
cargo xtask ledger check
```

## Expected failure modes

| Symptom | Diagnosis | Fix |
|---|---|---|
| Loop narrowing diverges or over-narrows | fixpoint invariants broken | Re-check flowTypeCache swap + incomplete-no-cache (checker-key §4.4, both marked non-negotiable) |
| Same fixture differs between runs | shared-flow cache outliving one reference query | sharedFlowStart resets per getFlowTypeOfReference |
| Narrowing leaks through function boundaries | Start outer-resume rule too broad | Only bare non-this refs resume outward; accesses and non-arrow this do not (checker-key §4.2) |
| `if (obj.kind === "a")` fails to narrow obj | isMatchingReference receiver matching incomplete | Port §4.6 fully; each gap is a known FN class |
| unknown stops recombining after narrowing | recombineUnknownType missing in the JOIN union | It lives in getUnionOrEvolvingArrayType, not in narrowType |
