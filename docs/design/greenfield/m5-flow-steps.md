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

THE INVARIANTS NEED FOUR M4-OWNED CALL-SITE EDITS — without them the
"non-negotiables" don't actually hold: (a) `checkExpressionCached`
(80580) saves/resets flowLoopStart + flowTypeCache around uncached
checks; (b) `getTypeOfExpression` (80895) writes flowTypeCache +
NodeFlags.TypeCached only when flowInvocationCount changed — this
cache IS what the loop-label swap invalidates; (c) getResolvedSignature
caching is guarded by `flowLoopStart === flowLoopCount` (77505) so
signatures resolved mid-loop are never cached; (d) the
getEffectiveCallArguments spread-operand check (76324) branches on
the RAW flowLoopCount — mid-loop the operand runs UNCACHED
(checkExpression, not checkExpressionCached), or its resolvedType
memo outlives the fixpoint and feeds the post-loop re-resolution
that (c) forces. Edit all four when this stage lands. (The 6.3
review caught (d) missing: the vendored source has FOUR flowLoop
consumers outside the flow family itself, not three — enumerate by
grep, not from this list.)

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
  keeps the result out of flowLoopCaches. The rethrow/degrade split
  is the reason-string PREFIX: reasons prefixed `[FLOW M5] ` are the
  narrowable-containment gates and RETHROW (containment is the
  pre-6.3 statement-path outcome); M5-owned dependency stubs embed
  the tag parenthetically (`(... [FLOW M5])`) and degrade like the
  M6/M8 stubs — their statement-path containment stands untouched.
  (The 6.3 review caught `.contains` sweeping seven stub reasons
  into the rethrow set.)
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

LANDED (6.4a-h, branch m5/6.4-narrowers) with four recorded
deviations:

- **Exhaustiveness pulled FORWARD from 6.6** (isExhaustiveSwitch-
  Statement/computeExhaustiveSwitchStatement 78920/78933): the 6.3
  branch-label bypass consult became OBSERVABLE the moment 6.4e made
  the switch-clause arm live, and the conservative-false stub would
  over-widen exhaustive-switch joins (an FP face). The remaining 6.6
  consumers (7027, implicit returns) still land at 6.6. The
  links.isExhaustive cycle protocol and links.switchTypes live
  state-side (links writes are speculation-guarded).
- **Destructuring flow entry** (getFlowTypeOfDestructuring 55892 +
  getSyntheticElementAccess/getParentElementAccess 55896/55914)
  landed at 6.4b: the M4 identity stub was UNMASKED by the first
  live narrower (the retired arm-level flag had been partial-marking
  destructured positions). The checker cannot allocate synthetic
  nodes, so the factory chain is query DATA
  (FlowQuery::synthetic_props) and every reference-shaped probe of
  the walk dispatches through query-aware wrappers.
- **The seam flag does NOT go constant-off.** Remaining producers,
  all deliberate: the TS 5.5 body-inference predicate precondition
  (getTypePredicateFromBody is M6-adjacent; get_effects_signature
  flags candidates and deliberately does NOT memoize the uncertain
  no-effects verdict), the synthetic-reference
  generic-union-constraint guard, and parser-recovery shapes. The
  flowLoopCaches seam guard stays with it.
- **The [FLOW M5] failure-face gates do NOT retire here.** Unflagged
  answers are tsc-faithful modulo the 6.6 families — in particular
  the isReachableFlowNode true-stub's dead-code divergence, which
  the gates still shield (FP=0 depends on it). 6.6 owns their
  retirement (with the 2345 flip pin in lib.rs), leaving the flag
  channel as pure M6 deferral.

## Stage 6.5: isMatchingReference — MOVED INTO 6.1 [tombstone]

Moved to the 6.1 prelude: 6.2's getTypeAtFlowAssignment and every
6.4 narrower consult it, so sequencing it after them was a
dependency inversion. Content unchanged — see 6.1.

## Stage 6.6: reachability [M]

`isReachableFlowNode` (70240) per checker-key §4.7 (lastFlowNode
single-entry cache + shared-node cache, never-call gates; un-stub
6.2's true-stub), consumed by: unreachable-code reporting (7027 with
the within-unreachable-range suppression), switch exhaustiveness
(landed EARLY at 6.4e — see the 6.4 landing note; only its 7027/
implicit-return consumers remain here) and 7029 fallthrough
(noFallthroughCasesInSwitch + clause.fallthroughFlowNode),
implicit-return checks (2366/7030 family,
`checkAllCodePathsInNonVoidFunctionReturnOrThrow`, wrapped in M4's
addLazyDiagnostic slot), and definite-assignment: the real family is
`isSymbolAssignedDefinitely`/`isSymbolAssigned`/
`isPastLastAssignment`/`markNodeAssignments` (71480-71523,
symbol.lastAssignmentPos) — there is no tsc function named
`isDefinitelyAssigned`; 2454 itself falls out of checkIdentifier's
initialType logic (6.1 caller integration), not out of reachability.

INHERITED FROM 6.4 (the landing note): once the true-stub retires,
the [FLOW M5] failure-face gates lose their last non-M6 shield and
retire HERE — flip lib.rs's
`loop_fixpoint_accumulates_widening_back_edge_types` pin to assert
[2345] with them, and re-evaluate every `[FLOW M5]`-reason escape row
(receiver/argument/return/assignment/declaration faces). The seam
flag (`traversed_inert_arm`) then narrows to pure M6 deferral
(body-inference predicate candidates).

ALSO OWNED HERE — the class-property flow-init family, escaped with
owner=M5 but scheduled in no earlier stage (the 6.2 review caught the
gap): `getFlowTypeOfProperty` (56222) / `getFlowTypeInConstructor`
(56210) / `getFlowTypeInStaticBlocks` (56193) behind the access.rs
`get_flow_type_of_access_expression` this-property arm and the
annotate.rs constructor/static-block-assigned property-type arms (the
four `[FLOW M5]` escapes). They ride reachability's stage because
their consumers are the definite-assignment/2565 family this stage
completes; retire the four escapes when they land.

Commit: `m5 6.6: reachability + its consumer checks`.

LANDED (6.6a-f, branch m5/6.6-reachability) with recorded deviations:

- **The worker landed exactly at 70240-70327** (single-entry memo
  written by the OUTER entry after the walk; the Shared arm's
  noCacheCheck reset falls through into the same iteration; the
  ReduceLabel arm invalidates the single-entry memo and reuses the
  6.3 override map, restored on unwind). Fallibility is tsrs-side:
  the Call arm's effects consult Errs on M6 body-inference
  candidates (get_effects_signature's None-query contract — no flag
  channel exists outside a query, and an unflagged "reachable" past
  an undecided asserts-false/never candidate would be a
  2534/2366-family FP face; the Err also keeps both reachability
  memos unwritten).
- **7027 landed error-face-only**: the binder's Unreachable node
  flag + bind-time bookkeeping were already complete; the checker
  side (checkSourceElementUnreachable aggregation + the
  withinUnreachableCode save/restore + the flow consult) runs under
  every option value, but addErrorOrSuggestion's suggestion face
  rides the unmodeled suggestion channel — only
  `allowUnreachableCode: false` reports (11 corpus fixtures, 30
  semantic rows).
- **functionHasImplicitReturn's flip un-hid a latent M4 gap**:
  effective_return_type_node lacked every TYPE-node
  signature-declaration arm (FunctionType/ConstructorType/Call/
  ConstructSignature/MethodSignature/IndexSignature/SetAccessor) —
  tsc's getEffectiveReturnTypeNode (16768) reads `.type`
  generically, so a never-typed CALLABLE PARAMETER hid its
  annotation from the effects consult (2366 FP,
  neverReturningFunctions1 f12) and type-node-declared PREDICATES
  from getTypePredicateOfSignature's annotation read.
- **getFlowTypeInConstructor/InStaticBlocks use a this-rooted
  synthetic chain**: the 6.4b encoding grew a root discriminator
  (`FlowQuery::synthetic_this_root`) — `reference` holds the real
  CONTAINER (file identity + tsc's setParent(reference, container)
  flow-container), `synthetic_props` the single accessed name, and
  the chain-grounding matchers test ThisKeyword kind exactly like
  tsc's isMatchingReference this arm (69460). The cache key uses
  the ThisKeyword arm's "0|…" base. Private names ground on the
  `__#…@` description exactly as the factory's
  createPrivateIdentifier does.
- **The [FLOW M5] gates retired into a FLAG-EXACT registry, not
  nothing**: deleting the syntax-probe gates surfaced the seam's
  blind spot — a JOIN crossing an unported M6/M8 dependency (live
  case: Record<K,V>'s mapped-type instantiation inside `in`
  narrowing) seam-reverts the query to the DECLARED type, and a
  report face consuming that deliberately-wide answer FPs (7053 on
  controlFlowInOperator, 18048 on controlFlowOptionalChain). The
  replacement records flagged queries per reference node
  (CheckerState::flow_inert_answer_nodes, cleared per file) and the
  nine report faces (unknown/nullable/void receiver, property miss,
  element ladder, argument, assignment, return, declaration
  initializer) contain ONLY when the probed operand's answer was
  actually seam-reverted — the ~1100-line PR-#6 syntax-probe family
  (FlowGuardCertainty/flow_guards_narrow_reference/…) deleted whole.
  The registry retires with the seam flag's last producers (M6
  body-inference, M8 mapped/generator stubs through the JOIN-SEAM).
- **Both remaining parenthetical M5 stubs resolved**: the
  isConstantReference binding-pattern arm went LIVE (its
  isSomeSymbolAssigned dependency had landed with the
  definite-assignment family), and the narrowTypeByEquality
  intersection-operand containment retired (operands carry real
  flow-narrowed types since 6.4).
- The definite-assignment family (isSymbolAssignedDefinitely/
  isSymbolAssigned/isPastLastAssignment/markNodeAssignments) needed
  NO work — it landed complete at 6.2; this stage only re-verified
  the hashes.
- The 2345 flip pin landed as specified (loop_fixpoint… asserts
  [2345], no partial marks) plus its sibling
  (speculative_overload… asserts [2769]).
- **The full-corpus FP sweep after retirement (27 rows) split
  five-and-three**: the old syntax gates had been swallowing
  UNRELATED latent divergences (their subtree probe treated property
  -NAME identifiers as narrowable references, silencing whole
  object-literal initializers). Five REAL fixes came out: (1)
  resolved object-literal properties come from the TABLE (last-wins,
  computed-name members excluded — the duplicate-member 2322 face);
  (2) member elaborations anchor at the PROPERTY NAME with
  deep-first nested elaboration (generateObjectLiteralElements
  64448); (3) reportUnmatchedProperty's private arm (a source-class
  `#name` twin reports the 2322 head, never 2741); (4)
  getContextualThisParameterType accepts EVERY assignment operator
  (isAssignmentExpression — the `??=` prototype-method 2683 face);
  (5) getTypeOfPropertyOfType works over any STRUCTURED type (the
  OBJECT-only guard broke the awaited-unwrap of overload-failure
  Promise INTERSECTIONS — spurious 2322 beside the 2769). Plus the
  effects-probe return-type read grew an in-progress cycle guard
  (mutual recursion through functionHasImplicitReturn — tsc's
  equivalent memoizes noTypePredicate, so in-progress answers
  FALSE). Three families stayed deferred with precise owner-tagged
  containments: computed-key destructuring assignments (the PR
  -#41094 evaluation-order family, M6), failing array-literal
  relations with a spread element (elaborateArrayLiteral
  tupleization, M6), and never-narrowed receiver reports over
  unreduced intersection members (getReducedType never-reduction,
  M6).

REVIEW FIXES (6.6g, branch fix/m5-6.6-review) — the post-merge 6.6
review (six-way tsc cross-check + oracle probes) closed:

- **Four flag-exact containment omissions** (live FPs outside the
  corpus, all fed by the M6 body-inferred-predicate seam): the
  binding-pattern initializer row, the merged-declaration initializer
  row AND its derived-type 2403 face, the empty-pattern non-null
  face, plus SUBTREE consults (`flow_answer_is_seam_reverted_within`)
  for compound return/declaration operands — restoring the retired
  subtree gates' containment strength at flag-exact precision.
- **getTypeOfPropertyOfType guardless** (55803): the STRUCTURED
  pre-guard degraded primitive receivers' apparent-type members
  (`({ length: n } = "abc")` manufactured a 2322).
- **reportUnmatchedProperty tsc-exact**: the private-twin arm probes
  the FIRST unmatched property BEFORE the count dispatch, keyed to
  the source class's OWN members; the walk and 2741/2739 displays run
  over the getSingleBaseForNonAugmentingSubtype-substituted pair
  (64809/65250 — the substitution existed but the reporter received
  the originals).
- **Elaboration parity**: elaborateError's paren/const-as/`=`,comma
  recursion arms (63968-63983), array-element deep-first rows
  (64406-64407), satisfies STOPS the member descent (no tsc arm),
  and the 2418 computed-property message (64449).
- **The lapsed 2775/2776 M4 residual ported** (77639-77646) — its
  first canary exposed getTypeOfDottedName's private lookup
  reconstructing the mangled key in the wrong id space
  (privateNamesAssertion FP); the key now recovers from the class's
  own members table.
- **SetAccessor return annotations** reach both signature-return
  readers (16768 generic `.type` read — the bare-return 7030 face);
  **computed shouldPreserveConstEnums** (isolatedModules ‖
  verbatimModuleSyntax, 18157) with the conformance mapper keys;
  the **never-2339 containment narrowed to intersection-bearing
  declared types** (the double-typeof never face reports like tsc);
  the flow-side **7008 prints declaration names** (never the
  `__#<id>@` mangling); stale [FLOW M5] comments and off-by-a-few
  tsc cites cleaned.

Gates: T0 53.4636→53.4840% (+10), 2xxx 75.0131→75.0606% (+10),
FP=0 all bands; checker tests 629→646 (17 new oracle-verified pins);
escapes 226/0/0/112 (+4 seam-containment sites, all owner-tagged).

## M5 close (branch m5/close)

The two close blockers the 6.6g review left, then the final gate:

- **checkTypePredicate tail ported** (81206-81253) — the last
  `owner="M5"` escape. The 2677 assignability face rides tsc's
  leadingError as a containingMessageChain, and checkTypeRelatedTo
  wraps errorInfo under that chain UNCONDITIONALLY (64890-64896):
  every relation-failure path (generic 2322, no-common-properties
  2559, missing-property, readonly 4104) lands the SAME outer code
  2677 at node.type with no arguments — so the head-only slice
  needs no display rendering and none of check_type_assignable_to's
  head overrides (oracle-pinned: the weak-target shape reports
  2677, never a bare 2559). Direction is predicate→parameter
  (width subtyping: extra predicate members are fine). Plus the
  1229 rest-reference row, the 1225 cannot-find-parameter row, and
  the 1230 checkIfTypePredicateVariableIsDeclaredInBindingPattern
  walk (81269-81288, recursive over nested patterns). 15
  oracle-pinned unit shapes (2 tests), escapes 226 → 225.
  typeGuardFunctionErrors' remaining 1228×3 (constructor/accessor
  return positions) are contained by the pre-existing
  overload-band parse-recovery escape, not this family.
- **STAGE reconciliation** — the marker moves `5.9` → `M5`, NOT a
  6.x token: parse_stage_key has no 6.x arm by design (M5+ escapes
  are owner-tagged at milestone granularity; 6.x lives only in doc
  stage numbering). The stale rule is `owner <= STAGE`, so `M5`
  makes the whole M5 band due (now vacuously green: zero M5 owners
  remain) while M6 owners stay live — the exact analogue of `5.9`
  marking the whole-M4 band due through M5. `escapes --stale M5`:
  sites=225 stale=0 untagged=0 recovery=112.

## Final gate

```sh
cargo xtask conformance                     # expect: T0 ≥ 50%
cargo xtask invariants --suite idempotence
cargo xtask invariants --suite jobs-independence
cargo xtask ledger check
```

CLOSED 2026-07-19 (definition-of-done M5 bar: flow landed with
idempotence + jobs-independence still green):

| Gate | Result |
|---|---|
| conformance all | T0 **53.5248%** (26,240/49,024) FP=0 — bar ≥ 50% |
| conformance 2xxx | T0 **75.0986%** (15,809/21,051) FP=0 |
| conformance syntactic | T0 99.8219% (2,242/2,246) FP=0 |
| invariants idempotence | ok (programs=275) |
| invariants jobs-independence | ok (programs=275) |
| ledger check | entries=1601 stale=0 |
| escapes --stale M5 | sites=225 stale=0 untagged=0 recovery=112 |
| checker tests | 648 |

M5 total (6.1 → close): T0 42.7403% → 53.5248%, 2xxx 52.9714% →
75.0986%. Next: M6 speculation transaction (convergence plan §4;
the M6-start bar is the scoped-transaction API + failed-candidate
rollback tests precondition).

## M5 post-close review (branch fix/m5-post-close-review)

A six-agent full-milestone review (line-diffed against the vendored
tsc + tsrs2-executed probes) after the close. Fixes landed:

- **The lapsed strictPropertyInitialization swap** — the M4 doc's
  "the M5 swap lifts it" promise (m4-58 §2564 band) was neither
  landed nor re-owned, invisible to `escapes --stale` (silent
  no-call suppressions carry no owner row). Landed:
  isPropertyInitializedInConstructor (85517-85525) +
  isPropertyInitializedInStaticBlocks (85502-85516) as
  this-rooted synthetic queries (flowContainer OMITTED — the "-1"
  key slot, unlike the 5619x family), wired into the 2564
  constructor face, the 2612 fifth disjunct (85370 — the probe's
  declared type is the DERIVED CLASS type, tsc quirk preserved),
  and the 2729 static-block leg (48024-48040). DEVIATION: under
  `strictNullChecks: false` tsc 6.0.3 CRASHES in the static-block
  probe (48036 is not strict-gated; getOptionalType 67853 asserts —
  reproduced); the pre-swap declared-type reduction stays for that
  regime only.
- **Seam-registry face coverage** (the review's executed FP faces):
  arithmetic-operand (2362 probe FP), operator, and
  iteration/not-iterable faces now consult the flag-exact registry;
  the assignment/argument faces' node-identity probes grew composite
  coverage. All via a BOUNDED probe
  (`flow_answer_is_seam_reverted_in_composite`): descends only
  type-embedding composites (object/array literals + carriers,
  conditional branches, paren/non-null) — the full-subtree walk
  over-contained (it swallowed yieldExpressionInControlFlow's
  tsc-real `o = yield* o` 2322, an A1 identity regression). The
  6.6g return/declaration consults keep the full walk (shipped).
- **Static-private assertion targets** (`S.#m(v)` — executed
  2775+2322 double FP): the mangled-key recovery searches the
  class's exports table too (statics land there; the binder mangles
  both flavors identically).
- **The predicate relation gate reads effective_return_type_node**
  — the 5-kind TYPE-node match let FunctionDeclaration/Expression/
  Arrow/MethodDeclaration predicate signatures compare as plain
  booleans (unledgered silent FN; tsc-probed 2322 divergence). Now
  a containment superset of tsc's predicate arm; the target-only
  split restores with compareTypePredicateRelatedTo (registered in
  m6 7.5).
- **Composite body-inference probe** — a union callee whose FIRST
  member is annotated cloned that declaration into the composite
  signature, so a LATER unannotated candidate escaped the seam flag
  (unflagged over-wide answer); the probe now sweeps every
  constituent.
- **isDiscriminantProperty's `!isGenericType` term** (69565) landed
  (the "generic types are M4" comment had lapsed); executed probes
  showed no observable divergence on the four narrower faces, but
  the memoized verdict also feeds M3's relation-side
  discrimination.
- Hygiene: the `(M5 6.3/6.4 seam)` partial-mark reason strings and
  stale stub-era comments (checkIdentifier header, flow.rs/state.rs
  seam docs, functions/widen/expr notes) renamed to the M6/M8
  producer set; the 5619x line cites above corrected;
  `noFallthroughCasesInSwitch` added to the harness directive
  vocabulary (was expansion-rejected; 7029 corpus-untestable).

### Declined post-close findings (reviewed, deliberately not fixed)

Written 2026-07-19 with the M4-review docs slice — PR #36's body
claimed this list was already recorded here; it was not (M4-review
B33). Live-bug status of all five re-verified negative 2026-07-19;
each row names its re-check trigger.

- **A1 — synthetic-query cache-key unification**: the
  strictPropertyInitialization synthetic this-queries omit
  flowContainer (the `"-1"` key slot, tsc-exact) while the 5619x
  family keys its container slot explicitly. No collision today —
  the key tuples stay distinct via their container-slot components —
  but the two conventions coexist. Re-check when M6 adds any third
  synthetic-query flavor: pick ONE convention at that point instead
  of a third variant.
- **A2 — nested-query seam-flag non-propagation**: a nested flow
  query's seam-revert flag does not propagate into the outer query's
  answer. Unproducible today — constructing a nested flagged answer
  needs an M6 body-inference or M8 JOIN-SEAM producer. Re-check at
  M6 body inference and at the M8 JOIN-SEAM retirement of the 6.6f
  registry.
- **D3 — getNarrowedTypeOfSymbol registration keying**: the review
  flagged the registration keying as divergence-prone once
  currently-unproducible producers exist; like A2 it cannot be
  exercised until the M6/M8 dependencies land. Re-check alongside
  A2 at M6 body inference.
- **E5 — effects-signature predicate memo (M6 landmine)**: declined
  as ALREADY DEFUSED — get_effects_signature's
  body-inferred-predicate probe (narrow.rs) flags the declared-type
  revert and does NOT memoize the non-final verdict (a memo hit
  would leak the unflagged wide answer; the loop-fixpoint pin
  caught exactly that live). M6's getTypePredicateFromBody port
  retires the probe.
- **C5 — checkMode dropped on the overload-failure path**:
  get_candidate_for_overload_failure receives check_mode and drops
  it (`let _ = check_mode;`, calls.rs) — tsc threads it into
  inferSignatureInstantiationForOverloadFailure's argument walk.
  Inert while the M6 stub runs no argument walk; the M6 inference
  port MUST thread check_mode through this path when it replaces
  the stub fill.

## Expected failure modes

| Symptom | Diagnosis | Fix |
|---|---|---|
| Loop narrowing diverges or over-narrows | fixpoint invariants broken | Re-check flowTypeCache swap + incomplete-no-cache (checker-key §4.4, both marked non-negotiable) |
| Same fixture differs between runs | shared-flow cache outliving one reference query | sharedFlowStart resets per getFlowTypeOfReference |
| Narrowing leaks through function boundaries | Start outer-resume rule too broad | Only bare non-this refs resume outward; accesses and non-arrow this do not (checker-key §4.2) |
| `if (obj.kind === "a")` fails to narrow obj | isMatchingReference receiver matching incomplete | Port §4.6 fully; each gap is a known FN class |
| unknown stops recombining after narrowing | recombineUnknownType missing in the JOIN union | It lives in getUnionOrEvolvingArrayType, not in narrowType |
