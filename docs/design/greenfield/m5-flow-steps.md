# M5: control-flow narrowing — steps

Parent design: checker-key-functions.md §4 (the complete port map —
this doc only sequences it); core-interfaces.md §5. The bind-time
graph exists since M2 stage 3.5. Prerequisite: M4 gate green.

Gate: T0 ≥ 50%; idempotence + jobs-independence invariants green.

## Stage 6.0: canaries [P]

Before touching the checker, snapshot 30 narrowing-heavy fixtures
(controlFlow/**, typeGuards/**, es2021/logicalAssignment/**) via
`cargo xtask conformance --files <list> --dump` — every stage
compares against these first (fast signal), full corpus after.

Commit: `m5 6.0: narrowing canary list`.

## Stage 6.1: FlowType + the walk skeleton [M]

- `FlowType` = Type | Incomplete(Type) per checker-key §4.1 — port
  the wrapper, not a boolean flag on the checker.
- `getFlowTypeOfReference` (70394) entry: reference key, declared/
  initial types, the flow-container logic.
- `getTypeAtFlowNode` (70420) from the full skeleton in checker-key
  §4.2: the backward loop, the antecedent-walk-on-None convention,
  the SHARED-node per-query cache (sharedFlowStart discipline), the
  Start outer-container resume rule, depth-2000 disable.
- Arms landed as stubs returning None/declared in this stage, filled
  next: assignment, call, condition, switch, labels, array mutation,
  ReduceLabel (port the antecedent-swap NOW — it is self-contained).

Replace M4's `get_flow_type_of_reference_stub` call site. Rate should
be unchanged (arms inert) — that null flip is the stage verification.

Commit: `m5 6.1: FlowType + getTypeAtFlowNode skeleton`.

## Stage 6.2: assignments + initial types [M]

`getTypeAtFlowAssignment` (70502) + `getInitialType`/`getAssignedType`
(69905) + `getNarrowableTypeForReference` + declared-vs-initial
distinction; the assignment-reduced-type rule
(`getAssignmentReducedType`: union declared types reduce by member
filter, never adopt the RHS). auto/evolving arrays
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
`getUnionOrEvolvingArrayType` (70759) incl. recombineUnknownType and
declared-type identity preservation. Subtype reduction in JOINs works
because M3 stage 4.8 landed.

Commit: `m5 6.3: branch/loop joins + fixpoint`.

## Stage 6.4: conditions + the narrowers [M]

`getTypeAtFlowCondition` (70614) then `narrowType` (71400) dispatch
per checker-key §4.5, then the sub-narrowers one commit each, tsc
order, with the facts model first:

1. `getTypeFacts` (69697) / `getTypeWithFacts` /
   `getAdjustedTypeWithFacts` — TypeFacts bits are generated (M0,
   table at 46297).
2. `narrowTypeByTruthiness` (incl. the discriminant-property path).
3. `narrowTypeByEquality` (71048) + `narrowTypeByBinaryExpression`
   (===/!==/==/!= incl. null/undefined special cases, assignment
   narrowing, `instanceof`, `in`).
4. `narrowTypeByTypeof` + switch-on-typeof.
5. `narrowTypeByDiscriminant` + `narrowTypeBySwitchOnDiscriminant` +
   `getTypeAtSwitchClause`.
6. `narrowTypeByCallExpression` (type predicates `x is T`,
   `asserts x`, assertion signatures) + `getTypeAtFlowCall` (70566)
   with `getEffectsSignature` (never-returning calls).
7. `narrowTypeByOptionality` + optional-chain/`??` containers.
8. The const-inlining rule (`if (c)` where `const c = <guard>` —
   inlineLevel ≤ 5) inside narrowType's Identifier arm.

Commit(s): `m5 6.4a-h: narrowers (+canary/rate per commit)`.

## Stage 6.5: isMatchingReference [M]

Port `isMatchingReference` (69448) + `getAccessedPropertyName`
DIRECTLY (checker-key §4.6): symbol-resolved identifiers, property/
element access with constant index forms, this/super/meta-property,
comma/assignment unwrapping. Every narrower from 6.4 consults it; do
NOT substitute a simplified reference-key model (that approximation
is a documented FN family in the first implementation).

Commit: `m5 6.5: isMatchingReference`.

## Stage 6.6: reachability [M]

`isReachableFlowNode` (70240) per checker-key §4.7 (lastFlowNode
single-entry cache + shared-node cache, never-call gates), consumed
by: unreachable-code reporting (7027 with the
within-unreachable-range suppression), 2367-family switch
exhaustiveness, implicit-return checks (2366/7030 family,
`checkAllCodePathsInNonVoidFunctionReturnOrThrow`), and
definite-assignment (2454, `isDefinitelyAssigned` walk family — port
alongside since it shares the machinery).

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
