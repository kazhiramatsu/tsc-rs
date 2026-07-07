# 2XXX execution plan and readiness checks

This document is the preflight design for entering the hardest 2XXX
type-checking work. It does not replace the roadmap. It answers the
operational question that the roadmap intentionally leaves broad:

> Is the current design strong enough to start changing central 2XXX
> behavior, and what must exist before each deep change?

Verdict: the existing north-star design is the right direction, but the
central 2XXX work should not begin as direct behavior edits. It needs a
short sequence of byte-identical seams, mining artifacts, and rollback
rules first. Local helper fixes can continue; relation, overload,
inference, and identity changes need the gates below.

## Existing Design Audit

The current design set is coherent:

| Document | What it gets right | Gap for the next phase |
|---|---|---|
| `greenfield.md` | tsc architecture is observable, so semantic parity must mirror tsc in checker-visible places. | Too broad to decide which retrofit seam comes first. |
| `checker-foundations.md` | Names the foundational mechanisms: lazy resolution, check order, contextual typing, type construction, widening, instantiation, member access. | Needs execution gates so these do not become an all-at-once rewrite. |
| `checker-key-functions.md` | Correctly identifies relation, inference, overload, and flow as load-bearing algorithms. | Needs a dependency order for introducing those algorithms into the current checker. |
| `candidate-call-resolution.md` | Correctly chooses candidate-scoped call resolution as the safe model for `TS2345`, `TS2554`, `TS2769`, `TS2349`. | The current `CallCandidateTrial` is only partial; state writes and diagnostics are not yet transactional. |
| `type-checking-2xxx-roadmap.md` | Correctly separates leaf-local work from central 2XXX seams and rejects more ad hoc relation flags. | Needs a stricter "ready to edit behavior" checklist. |
| `stall-playbook.md` | Gives the right migration style: dark launch, byte-identical scaffolds, one seam at a time. | Needs to be applied mechanically to every 2XXX branch. |

Conclusion: the direction is sound. The main design risk is sequencing,
not architecture choice. The worst move would be to start porting deeper
relation or inference behavior before candidate state, relation kind,
and measurement seams are in place.

## Hard Constraints

These constraints apply to every central 2XXX change.

1. Behavior and scaffolding must be separate commits.
   A seam commit should be byte-identical under the full golden suite.

2. A 2XXX behavior change must name its owner before editing:
   relation, member/access, call/overload, inference, flow/operator,
   declaration/binder, or type identity.

3. A relation behavior change must not add another mode flag to
   `is_assignable_to`. It needs the relation-kind seam first.

4. A call or overload behavior change must not rely on candidate order
   until candidate diagnostics and writes are isolated.

5. An inference behavior change must wait until candidate-local
   contextual typing is explainable for the probes it touches.

6. A member-access fix must not use a broad `any` fallback unless that
   exact fallback is anchored to the tsc source branch being ported.

7. Type identity is not a general-purpose fix. Use targeted comparison
   at the diagnostic site first, and change anonymous identity only when
   mining proves the broader model is the blocker.

8. `NEW_FP` / `NEW_FN` is not a stop signal. It is a triage signal.
   Classify whether the new movement came from the intended owner seam.
   Continue when the cause is understood and the direction is still
   toward tsc semantics.

## Evidence Ladder

Before changing central 2XXX behavior, collect evidence in this order:

1. Fresh full golden snapshot.
2. Top FP/FN codes and top files for the target code.
3. Owner classification for each top cluster.
4. A focused probe that reproduces the smallest semantic branch.
5. A tsc source anchor for the branch being mirrored.
6. A byte-identical scaffold when a shared seam is needed.
7. The behavior change with focused probes plus full golden-check.
8. A post-change ledger entry explaining any `NEW_FP` / `NEW_FN`.

Skipping steps 4-6 is only acceptable for leaf-local fixes whose owner
does not cross relation, call, inference, member access, or type
identity.

## Dependency Order

The central 2XXX work should use this order unless fresh mining proves a
local cluster dominates.

### A. Measurement and Ledger

Refresh the full JSON before starting a broad branch. Create a dated
`docs/design/NOTES-<date>-2xxx.md` only when the branch is large enough
to need running notes. The ledger must include:

- top FP/FN codes;
- top files for each target code;
- owner bucket;
- whether the issue is local or shared;
- expected seam dependency;
- current and post-change FP/FN movement.

### B. No-Behavior Scaffolds

Add scaffolds before semantic edits:

- `RelationKind` facade:
  `Assignable` delegates to current `is_assignable_to`; `Comparable`
  delegates to current comparable behavior; `Identity`, `Subtype`, and
  `StrictSubtype` exist but no call site relies on distinct behavior.

- Relation cache container:
  introduce per-kind cache shape without changing results. Existing
  assignable/comparable cache entries should keep their current meaning.

- Candidate speculation boundary:
  extend `CallCandidateTrial` so it can stage contextual argument types,
  diagnostics, cache writes, and function-body effects. The first commit
  may only record these values; it should not change candidate choice.

- Diagnostic/cache transaction primitive:
  create a small explicit boundary for speculative checking. It should
  either use overlays or snapshot only the affected maps. It must cover
  `expr_type_cache`, `param_ctx_types`, `checked_decls`, diagnostics,
  and contextual function-body state touched by candidate probes.

### C. High-Confidence Local 2XXX Fixes

While the scaffolds are landing, keep harvesting clusters that do not
need the deep seams:

- targeted `TS2403` redeclaration compatibility compare;
- inherited private/protected member symbol preservation;
- `TS2339` receiver-shape mining with one tsc member-access branch per
  fix;
- parser/binder namespace and duplicate-declaration shape fixes;
- grammar checks with exact tsc anchors.

These changes should stay small enough that `NEW_FP` / `NEW_FN` can be
explained by one owner.

### D. Call Candidate Behavior

After the candidate boundary exists, change call behavior in this order:

1. Explicit type arguments applied per overload candidate.
2. Spread expansion into effective argument slots.
3. Candidate-local contextual argument types.
4. Failure-candidate diagnostic selection.
5. Callable union signature set synthesis.
6. Overload ranking passes that depend on subtype relation.

Do not mix inference widening changes into these steps. If a call fix
needs subtype ranking, stop at the call boundary and add the
relation-kind seam first.

### E. Inference Fidelity

Only after call candidate state is isolated:

1. `InferenceInfo.top_level` and `is_fixed` data.
2. `getCovariantInference` literal widening rules.
3. `getInferredType` covariant/contravariant/default/constraint clamp.
4. Contextual return-type inference before argument inference.
5. Missing inference priority bits, one probe at a time.

Every inference change should run both call and assignment fixtures
because the same inferred type can move `TS2345`, `TS2769`, `TS2322`,
and `TS2339`.

### F. Ternary Relation Engine

Do not start the full `Ternary` maybe-stack merely because 2XXX is
hard. Start it only when evidence shows the current boolean relation is
the blocker:

- repeated relation changes regress both assignment and call families;
- relation results differ by cache/order;
- recursive generic probes implicate the coinductive true shortcut;
- local 2XXX clusters no longer explain the largest movement;
- subtype/strict-subtype behavior cannot be added safely through the
  relation-kind facade.

When this threshold is met, use a dark launch:

1. Keep public bool APIs.
2. Run old bool relation and new ternary relation on selected top-level
   assignable/comparable queries.
3. Record disagreements in a deterministic debug ledger.
4. Port one structured relation arm at a time.
5. Flip one call site class only after disagreement is understood.

### G. Type Identity

Identity changes come after targeted diagnostics prove insufficient.
The first identity work should be diagnostic-site specific:

- `TS2403` declaration compatibility;
- inherited private/protected origin symbols;
- type parameter and instantiated anonymous type display/equality probes.

Only consider a broad anonymous-declaration identity migration when a
fresh ledger shows identity is blocking a large repeated family and the
targeted sites no longer explain it.

## Branch Decision Matrix

Use this matrix when choosing the next implementation candidate.

| Dominant observation | First move |
|---|---|
| `TS2322`, `TS2411`, `TS2741`, `TS2344` dominate | Relation/member-shape mining; add relation-kind seam before behavior that needs subtype/identity/comparable separation. |
| `TS2345`, `TS2554`, `TS2769`, `TS2349` dominate | Candidate boundary first; behavior only after speculative writes are staged. |
| `TS2339`, `TS2538`, `TS2536` dominate | Member/access mining; fix apparent/reduced/indexed access paths without widening to `any`. |
| `TS2403` dominates | Targeted redeclaration compare before global identity work. |
| Micro probe passes but full fixture fails | Suspect cache/order/candidate mutation; build scaffold or transaction first. |
| One local helper explains the whole diff | Local fix is acceptable; still run full golden-check. |
| New movement crosses relation and call families | Treat as shared seam evidence, not noise. |

## Readiness Checklists

### Before Relation Behavior

- A probe identifies the needed tsc relation: identity, subtype,
  strict-subtype, assignable, or comparable.
- Existing assignable/comparable results are routed through a
  relation-kind facade.
- The change does not add a boolean mode flag to `is_assignable_to`.
- Relation cache keys and invalidation are named in the design note.
- Full golden movement is classified by relation-owned codes.

### Before Call Behavior

- `CallCandidateTrial` records enough data to explain why the candidate
  succeeded or failed.
- Candidate probes do not permanently write `expr_type_cache`,
  `param_ctx_types`, `checked_decls`, diagnostics, or function-body
  contextual state unless selected.
- Explicit type-argument and inferred-type paths are separated in the
  trial record.
- Failure diagnostics are selected from trials, not emitted during
  probing.

### Before Inference Behavior

- Candidate-local contextual typing is stable for the target probes.
- The target rule is anchored to tsc `inferTypeArguments`,
  `getCovariantInference`, or `getInferredType`.
- Assignment and call golden movement are inspected together.
- Literal widening/freshness side effects are named.

### Before Member/Access Behavior

- The receiver shape is mined from the target fixture.
- The fix maps to `getApparentType`, `getReducedApparentType`,
  `getPropertyOfType`, `getIndexType`, or namespace/binder shape.
- No broad `any` fallback is introduced.
- Any relation call added by member access is justified separately.

### Before Type Identity Behavior

- A targeted compare has been tried or ruled out.
- The tsc branch observes identity at this site.
- The change cannot be modeled by preserving original symbols,
  declaration links, or alias/instantiation metadata at a narrower site.
- A rollback plan exists because identity changes can move many codes.

## Stop Conditions

Stop a branch and write design before continuing when any of these are
true:

- the fix needs a new relation mode but `RelationKind` is absent;
- overload success depends on a candidate that was checked after a
  different candidate mutated state;
- an inference fix requires changing cache invalidation or freshness;
- a member-access fix would return `any` for an unmodeled shape;
- a type-identity fix changes allocation/interning policy globally;
- full-suite movement cannot be assigned to one owner bucket.

These stop conditions do not mean abandon the branch. They mean insert
the missing seam and continue with better isolation.

## Answer to "Can We Go With the Current Design?"

Yes, if "current design" means the combined direction from
`greenfield.md`, `checker-foundations.md`, `checker-key-functions.md`,
`candidate-call-resolution.md`, `type-checking-2xxx-roadmap.md`, and
`stall-playbook.md`.

No, if "current design" means directly editing the current bool
relation, overload path, or inference shortcuts until the target
diagnostic matches. That path will keep producing unrelated 2XXX
movement because the checker-visible seams are not isolated enough.

The next safe implementation candidates are therefore:

1. byte-identical `RelationKind` facade;
2. byte-identical expansion of `CallCandidateTrial` into a real
   speculative boundary;
3. targeted local 2XXX clusters that do not cross those seams.
