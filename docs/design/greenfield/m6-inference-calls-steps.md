# M6: inference + overload completion — steps

Parent design: checker-key-functions.md §2 (inference) and §3 (the
re-run machinery); core-interfaces.md §6 (InferenceInfo contract).
Prerequisite: M5 gate green. This milestone REPLACES exactly one
M4 stub (`infer_type_arguments`) and activates the CheckMode plumbing
M4 ported inert.

Gate: T0 ≥ 58%. Inference moves 2345/2322/2769/2339 together — run
the full gate per stage, not just call fixtures (checker-key §2 note).

## Stage 7.0: canaries [P]

Snapshot 40 fixtures: typeInference/**, typeArgumentInference/**,
contextualTyping/**, overload-heavy (taggedTemplates, functionCalls).
Same dump procedure as M5 6.0.

Commit: `m6 7.0: inference canary list`.

## Stage 7.1: inference data model [M]

`InferenceInfo` / `InferenceContext` VERBATIM from core-interfaces §6
— including `top_level`, `is_fixed`, `implied_arity`,
`contra_candidates`, the `InferencePriority` bit set (generated), the
fixing/non-fixing mapper pair, and `compare_types`. Context creation
(`createInferenceContext`, cloneInferenceContext for the re-run).

Commit: `m6 7.1: InferenceInfo/InferenceContext`.

## Stage 7.2: inferTypes / inferFromTypes [M]

The candidate collector (68637/68646), ported arm by arm in tsc
order, priorities attached exactly as the source sets them: identical
types, unions/intersections both sides (the naked-type-variable
ordering), literals, template literals + string mappings,
index/keyof, conditional types, mapped-type homomorphic inference
(ReverseMapped), object/signature structural inference
(`inferFromProperties`, `inferFromSignatures` with bivariance rules),
array/tuple element inference, contra-candidate collection at
contravariant positions, `top_level` clearing on descent, fixing
(`is_fixed`) when a candidate is consumed by a dependent inference,
priority comparison (LOWER wins; equal priorities accumulate).

Verify per commit against canaries; expect 2345-family movement only
after 7.4 wires results in.

Commit(s): `m6 7.2a-d: inferFromTypes arms`.

## Stage 7.3: resolving inferences [M]

- `getCovariantInference` (69263) — the FULL widen-literals condition
  from checker-key §2.1 including `hasPrimitiveConstraint`,
  `isTypeParameterAtTopLevelInReturnType`, and the
  PriorityImpliesCombination union-vs-common-supertype split
  (Subtype relation from M3 4.8 feeds getCommonSupertype).
- `getContravariantInference` (intersection-vs-common-subtype split).
- `getInferredType` (69271) — the constraint clamp EXACTLY as the
  checker-key §2.2 skeleton: ReturnType-priority inferences FILTER to
  the compatible part; others go never → fallback → instantiated
  constraint. Defaults instantiate with the backreference mapper;
  NoDefault/AnyDefault flags honored.

Commit: `m6 7.3: covariant/contravariant inference + clamp`.

## Stage 7.4: inferTypeArguments + the re-run [M]

- `inferTypeArguments` (75938): PHASE (a) contextual-return
  pre-inference producing the returnMapper (checker-key §2.3 — the
  piece most first-implementation quality gaps traced to), then PHASE
  (b) per-argument inference via
  `checkExpressionWithContextualType` + `getTypeAtPosition`, rest
  args via `getSpreadArgumentType` (76002).
- DELETE the M4 stub; `chooseOverload`'s inference path now runs the
  real thing, INCLUDING the SkipContextSensitive /
  SkipGenericFunctions first pass and the NORMAL-mode RE-RUN before
  committing a candidate (checker-key §3.2 — the plumbing M4 laid).
- Context-sensitive argument detection (`isContextSensitive`) and the
  deferred body interaction (M4's driver already defers bodies; the
  re-run is what types their parameters).

Commit: `m6 7.4: inferTypeArguments + chooseOverload re-run`.

## Stage 7.5: consumers cleanup [M]

Ripple sites that were declared-type-only until now: contextual
tuple/array element inference in literals, generic
constructor/`new` inference, tagged templates, JSX element type
resolution's call path, `satisfies` interplay, and the
2769 failure-path candidate choice (getCandidateForOverloadFailure
with real instantiated candidates). Re-probe the M4 NOTES top-10
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
