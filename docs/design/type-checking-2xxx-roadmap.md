# 2XXX type-checking roadmap

This is the cross-workstream design for the 2XXX diagnostic band. The
2XXX codes are not one subsystem: they are the visible surface of type
construction, member access, assignability, call resolution, inference,
control-flow narrowing, declaration merging, and module/name lookup.

Use this document before starting any broad 2XXX fix, especially changes
touching `src/checker/relations.rs`, `src/checker/calls.rs`,
`src/checker/infer.rs`, `src/checker/shapes.rs`, or type identity in
`src/types.rs`.

Related focused docs:

- `archive/workstreams/relation-core-2.md`: archived mapped
  relation/member-access fixes and 2339 mining.
- `type-checking-2xxx-execution-plan.md`: readiness checks,
  dependency order, and stop conditions for central 2XXX behavior
  changes.
- `candidate-call-resolution.md`: transactional call candidate design
  for 2345/2554/2769/2349.
- `checker-key-functions.md`: tsc-faithful relation, inference,
  overload, and flow algorithms.
- `checker-foundations.md`: lazy resolution, contextual typing,
  construction/normalization, widening, instantiation, and member
  access.
- `architectural-debt.md`: known targeted debts such as 2403 identity,
  StringMapping, and inference widening order.
- `stall-playbook.md`: when local work has hit an architecture ceiling.

## Current Snapshot

Snapshot: `/tmp/fcc_after_yield_star_iterable.json`, after commit
`82d0b3b`. Refresh before acting; these numbers are a design baseline,
not a golden.

Gate-filtered 2XXX residue:

| Side | 2XXX total | Distinct 2XXX codes | Top codes |
|---|---:|---:|---|
| FP | 2,352 | 103 | 2322=406, 2339=311, 2403=216, 2345=201, 2554=51 |
| FN | 2,795 | 233 | 2322=415, 2339=208, 2345=145, 2300=141, 2411=89 |

Approximate subsystem buckets from the same snapshot:

| Bucket | FP | FN | Main codes |
|---|---:|---:|---|
| relation / assignability | 734 | 612 | 2322, 2403, 2411, 2741, 2344, 2415/2430 |
| member / name / access | 477 | 398 | 2339, 2538, 2536, 2708, 2503, 2307 |
| call / construct / overload | 385 | 243 | 2345, 2554, 2769, 2349, 2351, 2394 |
| grammar / declaration | 141 | 403 | 2300, 2364, 2369, 2695, 2804 |
| flow / operators | 68 | 78 | 2365, 2367, 2454, 2564 |
| other 2XXX | 547 | 1,061 | 2313, 2314, 2445/2446, 2461, 2493, 2698 |

The distribution matters: 2XXX work is now too large to treat as a list
of independent diagnostics. Most high-yield changes pass through a few
shared mechanisms.

## Verdict: Can We Keep Going As-Is?

Short answer: no for the central 2XXX cluster, yes for bounded
leaf-local work.

The current checker can still safely make progress when a fix is owned
by one narrow helper and the failure mode does not cross relation,
inference, and call selection. Examples:

- binding-pattern `TS7031` leaf classification;
- parser/grammar recovery fixes that preserve semantic reachability;
- a targeted `TS2403` redeclaration-compatibility compare;
- known nominality shape fixes before enabling assignable-side private
  checks;
- lib/harness name availability work.

The current checker should not continue accumulating broad local patches
for the central 2XXX families without deeper seams. The risky areas are:

- `relations.rs`: bool return, assignable/comparable-only modes, and a
  coinductive "cycle means true" shortcut cannot express tsc's
  `Ternary` maybe-stack or the five relation modes.
- `calls.rs`: a `CallCandidateTrial` exists, but it is still mostly a
  verdict object. It does not yet stage all candidate-local diagnostics,
  cache writes, contextual argument checks, or function-body effects.
- `infer.rs`: several tsc `getInferredType` rules are approximated, and
  widening still depends on surrounding cache/freshness behavior in
  enough places to create order sensitivity.
- `shapes.rs` / type identity: structural interning is useful, but tsc
  sometimes observes declaration/instantiation identity. Some cases have
  targeted workarounds; a broad identity-model change is not yet
  justified by current yield.

So the design is not "rewrite now." It is a strangler path: keep local
fixes where they are genuinely local, but add transactional and
relation-mode seams before changing behavior that can flip multiple
2XXX families at once.

## Ownership Map

Use this map to decide where a 2XXX divergence belongs before editing.

### Relation / Assignability

Primary codes: `TS2322`, `TS2411`, `TS2741`, `TS2740`, `TS2344`,
`TS2415`, `TS2430`, `TS2442`, `TS2403`.

Owner modules:

- `src/checker/relations.rs`
- `src/checker/shapes.rs`
- `src/checker/relation_errors.rs`
- `src/types.rs` when identity or normalization is involved

Required design alignment:

- Use the archived `archive/workstreams/relation-core-2.md` notes for
  provenance on mapped local relation fixes.
- Do not add subtype or strict-subtype behavior by reusing assignable
  relation with ad hoc flags. That needs the relation-mode seam from
  `checker-key-functions.md` and `stall-playbook.md` section 2.1.
- Do not compare `TypeId` for tsc identity unless the tsc source uses
  identity at that exact point. Prefer structural relation/identity
  relation at diagnostic sites like 2403.

Safe current work:

- carrying original member symbols through inherited shapes;
- targeted private/protected nominality after shape identity is fixed;
- targeted `TS2403` redeclaration compare using a structural/identity
  relation instead of raw `TypeId` equality.

Deep-design trigger:

- a change touches `is_assignable_to`, `signature_related`, relation
  caches, union/intersection reduction, or type identity and produces
  NEW_FP in both `TS2322` and call-side `TS2345`/`TS2769`.

### Member / Name / Access

Primary codes: `TS2339`, `TS2538`, `TS2536`, `TS7053`, `TS2708`,
`TS2503`, `TS2307`, `TS2693`.

Owner modules:

- `src/checker/shapes.rs`
- `src/checker/symbols.rs`
- `src/checker/exprs.rs`
- parser/binder for namespace and module-symbol shape

Required design alignment:

- `TS2339` mining should follow the owner split in this roadmap. The
  archived `archive/workstreams/relation-core-2.md` notes remain useful
  provenance, but they are no longer the top-level design entry point.
- Member access must follow tsc's `getApparentType`,
  `getReducedApparentType`, `getPropertyOfType`, and indexed-access
  constraint paths. A local `prop_of_type` fallback to `any` is not an
  acceptable fix; it hides later `TS2322` and `TS2345` failures.
- Dotted namespace flattening is a front-end shape bug, not a relation
  bug. Fix parser/binder nesting instead of special-casing property
  access.

Safe current work:

- mining top `TS2339` receiver shapes and fixing one concrete path at a
  time;
- apparent-type additions that can be tied to a tsc source branch and
  focused probes;
- namespace/module fixes when probes show missing symbol shape.

Deep-design trigger:

- a proposed `TS2339` fix changes assignability or inference by making
  broad unknown/error/any fallbacks observable.

### Call / Construct / Overload

Primary codes: `TS2345`, `TS2554`, `TS2769`, `TS2349`, `TS2351`,
`TS2394`, `TS2558`.

Owner modules:

- `src/checker/calls.rs`
- `src/checker/infer.rs`
- `src/checker/relations.rs`
- contextual function/object-literal checking in `exprs.rs`

Required design alignment:

- Continue the existing `CallCandidateTrial` into a real transactional
  boundary. Today it records signature, mapper, arity, and some argument
  verdicts; it still needs candidate-local diagnostics, cache writes,
  contextual argument types, and commit/replay behavior.
- Do not tune overload selection by suppressing diagnostics from losing
  candidates.
- Do not broaden callable-union synthesis until signature-set identity
  is proved as in `candidate-call-resolution.md`.
- Inference fidelity work should wait until candidate-local contextual
  typing is strong enough that inference changes do not permanently
  poison expression caches.

Safe current work:

- candidate-trial refactors that are byte-identical;
- explicit type-argument application per candidate when the diagnostic
  fallback is preserved;
- spread expansion for single-signature calls if overload selection is
  unchanged.

Deep-design trigger:

- a fixture fix requires rechecking function expressions, mutating
  `param_ctx_types`, or dropping expression caches differently per
  overload candidate.

### Flow / Operators

Primary codes: `TS2365`, `TS2367`, `TS2454`, `TS2564`, plus nullable
access codes that are not all 2XXX.

Owner modules:

- `src/checker/flow/*`
- `src/checker/operators.rs`
- initialization and definite-assignment paths

Required design alignment:

- Flow work must keep the fact graph and resolver deterministic. Use
  `./verify.sh mf` for flow-affecting changes.
- Operator relation checks must not invent a separate mini-relation.
  If a binary/operator rule needs subtype/comparable behavior, route it
  through the relation design.

Safe current work:

- localized flow-edge parity such as `??=` or try/finally joins when
  probes isolate the edge;
- operator diagnostic span/message fixes that do not alter relation
  semantics.

Deep-design trigger:

- a flow/operator fix depends on declaration identity, subtype
  reduction, or relation recursion behavior.

### Grammar / Declaration

Primary codes: `TS2300`, `TS2364`, `TS2369`, `TS2370`, `TS2374`,
`TS2393`, `TS2695`, `TS2804`.

Owner modules:

- parser and binder
- declaration merge and symbol uniqueness paths
- statement/expression grammar checks in checker

Required design alignment:

- Parser recovery and semantic gating belong with the front-end design;
  archived retrofit notes live in
  `archive/workstreams/parse-error-gate.md`.
- Declaration merge fixes must preserve binder symbol identity because
  later 2XXX relation/access diagnostics consume those symbols.

Safe current work:

- exact grammar checks with tsc source anchors;
- duplicate declaration and symbol-merge fixes that are proven with
  targeted fixtures.

Deep-design trigger:

- a grammar fix changes which declarations enter the binder or changes
  symbol identity used by relation/member access.

## Cross-Cutting Invariants

These invariants must hold across all 2XXX workstreams.

### One Relation Query Must Mean One tsc Relation

Do not keep adding boolean flags to `is_assignable_to` for subtype,
strict-subtype, identity, or comparable behavior. tsc has separate
relations with separate caches. The current `erase_generic_sigs` plus
`comparable_cache` is a useful two-relation precedent, not the final
model.

When a fix needs relation-mode fidelity, introduce:

```rust
enum RelationKind {
    Identity,
    Subtype,
    StrictSubtype,
    Assignable,
    Comparable,
}
```

Start with a byte-identical wrapper that routes all existing assignable
queries through `RelationKind::Assignable` and comparable queries
through `RelationKind::Comparable`. Only after that should behavior
change.

tsc also keeps a sixth auxiliary map, `enumRelation`, used only by
`isEnumTypeRelatedTo`. It is deliberately out of scope for this enum;
model enum compatibility caching separately if mining ever surfaces it.

### Candidate Checking Must Be Transactional

Overload/call candidate probing must not permanently mutate:

- expression type caches;
- `param_ctx_types`;
- `checked_decls`;
- diagnostics;
- contextual-this/function-body state.

The existing `CallCandidateTrial` is the right direction, but it is not
yet the full boundary. Treat any `TS2345`/`TS2769` fix that depends on
candidate order as blocked on this boundary.

### Inference Fidelity Comes After Candidate Isolation

`getInferredType` changes can flip:

- `TS2345` at the call site;
- `TS2322` downstream through inferred return/variable types;
- `TS2769` by changing overload applicability;
- `TS2339` when inferred object/union shapes change.

Port inference rules only when candidate-local contextual typing is
strong enough to make probes explainable. Before that, prefer
byte-identical scaffolding and focused fixes with no broad widening
change.

### Type Identity Is a Last-Resort Lever

Structural interning is not globally wrong; it is a performance and
simplicity advantage. The observed identity-related damage is real, but
not yet large enough to justify changing anonymous type identity across
the whole checker.

Prefer targeted identity fixes first:

- `TS2403`: compare redeclaration types structurally/with identity
  relation instead of raw `TypeId` equality.
- inherited private/protected props: preserve original member symbols.
- anonymous declaration identity: leave documented until a fresh mining
  pass shows it blocking a larger 2XXX family.

### Never Hide a Deep Type Error With `any`

Using `any` to make a local `TS2339`/`TS2345`/`TS2322` disappear often
creates matching output in one fixture while suppressing the diagnostic
that another fixture expects. Any fallback to `any` in 2XXX work must be
traceable to the tsc source branch being mirrored.

## Design Decision Matrix

Use this matrix before editing.

| Observation | Action |
|---|---|
| One fixture, one helper, no unrelated 2XXX movement | Local fix is acceptable. |
| NEW_FP appears in another 2XXX family after a focused fix | Treat as missing architecture seam, not as noise. |
| Probe differs between micro-fixture and full fixture | Suspect order dependence or candidate/cache mutation. Design first. |
| Fix needs subtype/strict-subtype/identity/comparable distinction | Add relation-kind seam first. |
| Fix needs overload candidate-specific contextual typing | Extend `CallCandidateTrial` first. |
| Fix requires a broad `prop_of_type` fallback | Mine receiver shapes and mirror tsc member access instead. |
| Fix requires changing structural interning | Prefer targeted diagnostic-site compare; full identity migration only after threshold. |

For central 2XXX behavior changes, also apply
`type-checking-2xxx-execution-plan.md`. This roadmap explains ownership
and direction; the execution plan gives the preflight gates for deciding
whether a behavior edit is ready.

## Sequencing

This sequence keeps local yield and deep refactors aligned.

### Phase 0: Keep Leaf-Local Work Moving

Proceed with workstreams whose behavior does not cross 2XXX seams:

- `destructuring-parameter-implicit-any.md`;
- parse-error semantic gate: design parked in
  `archive/workstreams/parse-error-gate.md` — archived for shelf
  hygiene, not completed; revive the doc when scheduling it;
- lib-gap 2304: parked in `archive/workstreams/lib-gap-2304.md`, also
  not completed; mostly buys the raw metric, so check gate visibility
  before scheduling;
- focused grammar/declaration checks.

These reduce noise and make later 2XXX mining cleaner.

### Phase 1: 2XXX Mining Ledger

Before any broad 2XXX implementation, generate a fresh snapshot and
write a `docs/design/NOTES-<date>-2xxx.md` ledger with:

- top 20 FP and FN codes;
- top files per code;
- owner bucket from this roadmap;
- whether each top code is local, relation, call, member, flow, or
  parser/binder.

Do this with the full JSON, not memory. The distribution moves after
every successful workstream.

### Phase 2: Safe Targeted Fixes

Work items that can proceed without new deep seams:

- shape-symbol fixes for inherited private/protected members, using the
  archived relation-core notes only as provenance;
- targeted `TS2403` redeclaration compare;
- `TS2339` receiver-shape mining where each cluster maps to one tsc
  member-access branch;
- parser/binder dotted namespace shape if mining confirms it remains a
  top member-access bucket.

Each should be its own gate-clean commit.

### Phase 3: Transactional Call Candidate Boundary

Extend `CallCandidateTrial` until it can stage:

- contextual argument types;
- candidate diagnostics;
- expression/cache writes;
- `param_ctx_types` writes;
- function-body checking effects for context-sensitive arguments.

Keep early stages byte-identical. Only after the boundary is real should
2345/2554/2769 inference and overload ranking behavior change.

### Phase 4: Relation-Kind Dark Launch

Phases 3 and 4 are both no-behavior scaffolds and are order-independent
of each other; the numbering is not a dependency. Start with whichever
seam the next mined cluster needs first.

Introduce `RelationKind` and per-kind cache plumbing without behavior
change:

- `Assignable` routes to current assignable behavior;
- `Comparable` routes to current comparable behavior;
- `Identity`, `Subtype`, and `StrictSubtype` can exist as unsupported
  or assignable aliases during the dark launch, but no call site should
  rely on their distinct behavior yet.

This stage is valuable even before the full `Ternary` maybe-stack. It
stops new work from adding more ad hoc relation flags.

### Phase 5: Ternary Relation Engine When Justified

Only start the `Ternary` maybe-stack migration when mining shows a
relation recursion/cache ceiling:

- repeated `TS2322`/`TS2345` regressions from relation changes;
- order-dependent relation results;
- recursive generic fixtures where the current coinductive true shortcut
  is implicated;
- no local cluster ≥80 diagnostics remains.

Use the dark-launch style from `stall-playbook.md`: introduce the new
engine beside the old one, measure disagreement, then flip one seam at a
time.

### Phase 6: Inference Fidelity

After candidate isolation, port more of tsc's inference:

- `getInferredType`;
- `getCovariantInference`;
- literal-widening and top-level return-position rules;
- missing priority bits only when probes demand them.

Run call fixtures and assignment fixtures together because inference
changes are expected to move both `TS2345` and `TS2322`.

### Phase 7: Identity Model Only If Threshold Is Met

Do not broaden type identity unless a fresh ledger shows identity-model
divergence blocking a large family. The first broad candidate would be
anonymous declaration identity, but it must be preceded by a byte-level
display audit because TypeId ordering can change union/intersection
text.

## Required Verification

For any 2XXX behavior change:

```sh
cargo build --release
cargo test --release
./verify.sh golden-check
```

Additional gates:

- flow/operator changes: `./verify.sh mf`;
- pure refactor/dark-launch stages: byte-identical full-corpus diff
  before behavior is enabled;
- relation/call/inference changes: focused probes for `TS2322`,
  `TS2339`, `TS2345`, `TS2554`, and `TS2769` buckets before the full
  gate.

Commit only at 0 NEW_FP. NEW_FN must be root-caused, and for 2XXX it is
usually a sign that one subsystem is now exposing another missing tsc
rule.
