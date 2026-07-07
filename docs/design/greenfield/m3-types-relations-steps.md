# M3: types crate + relation engine — steps

Parent design: greenfield.md §4.2-4.3, §4.7 (data model, links,
relations); checker-foundations.md §4 (union/intersection
construction); checker-key-functions.md §1 (the engine) and §5 steps
1-4 (port order). Prerequisite: M2 gate green.

Gate: the relation pin suite (~200 oracle-probed cases) green.

## Stage 4.0: the relation pin harness FIRST [P]

Relations are not directly observable until the checker exists, so
M3 builds a probe bridge: a test-only entry that parses two type
annotations in a scratch program, resolves them through a MINIMAL
type-from-annotation path (stage 4.1 provides it), and asks the
relation engine. Ground truth comes from oracle probes of
`var t: Target = source_value;` / `declare var s: Source; var t:
Target = s;` fixtures — 2322 presence/absence IS the assignability
oracle. Build the generator:

```
xtask relpin gen pins/relations.toml   # [pair] source=".." target=".." expect=assignable|not
xtask relpin run                       # runs engine, prints disagreements
```

Seed `pins/relations.toml` with ~200 pairs covering: primitives ×
literals (fresh and regular), unions both sides, intersections,
object literals (width/optionality), functions (param bivariance vs
strictFunctionTypes contexts noted per pin), tuples (rest/optional
elements), recursive generic interfaces (the Maybe path: mutually
recursive `interface A<T> { next: B<T> }` pairs), deeply-expanding
generics (the depth limiter), template literals, index signatures,
`{}`/unknown/any/never edges. EVERY pin's `expect` value is filled by
running the oracle on the generated fixture, committed alongside.

Commit: `m3 4.0: relation pin harness + seeded pins`.

## Stage 4.1: types crate core [M]

Per greenfield §4.2-4.3 and core-interfaces §3:

- `Type`/`TypeData` arena; allocation id IS identity; interning maps
  ONLY for literals, unions, intersections, string mappings (keyed
  per greenfield §4.2 — `getTypeListId`-style member-id keys). Two
  structurally identical anonymous object types are DISTINCT.
- Intrinsics (any/unknown/never/void/undefined/null/string/number/
  boolean/bigint/symbol/object + error/silentNever/wildcard variants
  tsc distinguishes internally).
- Literal types as fresh/regular/wide triples (freshness is
  STRUCTURE, not a flag-cache: `fresh_of`, `regular`, `wide` links).
- Links tables with ONE-WRITE discipline + explicit `Resolving`
  sentinel variants + the checker-wide `speculation_depth: u32`
  assertion on every links write (greenfield §4.3 — this single rule
  is the transaction model).

Commit: `m3 4.1: type arena, intrinsics, literals, links`.

## Stage 4.2: union construction [M]

`getUnionType` (61505) per checker-foundations §4.1: flatten, dedup
by id, sort by id, literal-reduction rules, `origin` tracking, intern
by sorted member list. `UnionReduction::Subtype` STUBS to
Literal-reduction until stage 4.8 flips it on (record the stub in the
ledger comment) — nothing before overloads/JOINs consumes it.

Commit: `m3 4.2: getUnionType (Subtype reduction stubbed)`.

## Stage 4.3: intersection construction [M]

`getIntersectionType` (61789) — the eight ordered normalization steps
from checker-foundations §4.2, ported one commit if small enough,
otherwise two (steps 1-5, then 6-8). The identity-keyed
`typeMembershipMap` dedup falls out of the allocation-identity model;
assert it with a pin: two distinct written `{}` type literals do NOT
dedup.

Commit: `m3 4.3: getIntersectionType normalization`.

## Stage 4.4: relation scaffolding [M]

Per checker-key §1.5 + greenfield §4.7:

- `RelationKind` × 5, one cache each (`[RelCache; 5]`), never shared.
- `RelationResult` from the generated `RelationComparisonResult`
  (table at `_tsc.js` 3404).
- `getRelationKey` (67423): the exact key string incl. alias context,
  intersection state, and the `'*'` constraint-broadened prefix form.
- `Ternary` values from codegen (False=0, Unknown=1, Maybe=3,
  True=-1) and the truthiness convention (`& 1`-style checks in ports
  read as tsc wrote them).

Commit: `m3 4.4: relation caches + getRelationKey + Ternary`.

## Stage 4.5: the engine core [M]

Port in this order, each against its cited source:

1. `isTypeRelatedTo` (64762) — entry fast paths, fresh→regular
   normalization, the comparable reversed-simple rule (KEEP all three
   occurrences across the engine, checker-key §1.1).
2. `checkTypeRelatedTo` (64842) — state setup, error-node handling
   (error CAPTURE only at M3; chain shaping is T2 work).
3. `isRelatedTo` (65147) — normalization + simple rules +
   union/intersection dispatch (`unionOrIntersectionRelatedTo` with
   its skip-caching small-union rule).
4. `recursiveTypeRelatedTo` (65725) — THE maybe-stack, ported from
   the full skeleton in checker-key §1.2 with all four invariants
   (Maybe never cached mid-recursion; commit-on-unwind; the
   relation_count complexity budget; depth-100 overflow with the real
   2321/2589-family diagnostics).
5. `isDeeplyNestedType` (67465) + `getRecursionIdentity` — checker-key
   §1.3 skeleton.

Verify continuously: `xtask relpin run` — the recursion/Maybe pins
flip green here.

Commit(s): `m3 4.5a-e: relation engine core`.

## Stage 4.6: structuredTypeRelatedTo [M]

The body (65872), one family per commit, tsc's arm order
(checker-key §1.4): type-parameter/indexed-access/conditional/
substitution arms; then `propertiesRelatedTo` (excess-property check
BEFORE structural for fresh literals; weak-type common-property
check); `signaturesRelatedTo` (method bivariance vs
strictFunctionTypes contravariance — signature kind comes from the
declaration, core-interfaces §4 `from_method`); `indexInfosRelatedTo`;
`relateVariances` for references.

Commit(s): `m3 4.6a-e: structural arms (+pin counts)`.

## Stage 4.7: variance measurement [M]

`getVariances`/`getVariancesWorker` for references and aliases —
required by `relateVariances` and later by inference. Includes the
`VarianceFlags` inlined enum (M0 codegen) and the unmeasurable/
unreliable marker propagation into relation results.

Commit: `m3 4.7: variance computation`.

## Stage 4.8: Subtype/StrictSubtype activation [M]

The engine already dispatches on RelationKind; now: port the
subtype-specific simple rules, flip `UnionReduction::Subtype` in
`getUnionType` from the stage-4.2 stub to a real
`removeSubtypes` port, and add `getCommonSupertype`. Pins: union
display-order fixtures and literal-union reduction pairs
(oracle-probed via variable-hover is unavailable — use assignability
consequences and error-message text pins instead, generated the same
way as stage 4.0).

Commit: `m3 4.8: Subtype relation + union subtype reduction`.

## Final gate

```sh
cargo xtask relpin run        # expect: 0 disagreements over ~200 pins
cargo xtask ledger check
```

## Expected failure modes

| Symptom | Diagnosis | Fix |
|---|---|---|
| Recursive-generic pins flap depending on pin ORDER | a Maybe result got cached mid-recursion | Re-check reset_maybe_stack commit conditions (checker-key §1.2 invariant 1) |
| Distinct `{}` literals compare related where oracle says no | structural interning crept in | Only the four interning maps may exist (greenfield §4.2) |
| Deep generic pins overflow where oracle succeeds | isDeeplyNestedType counting by raw type instead of recursion identity | Port getRecursionIdentity, not a depth counter |
| Fresh-literal pins differ | fresh→regular normalization missing at an entry | It happens at isTypeRelatedTo entry AND inside specific arms — follow the source, not symmetry intuition |
| Cache hit-rate ~0 and pins slow | getRelationKey missing alias/intersection context | The key string is part of the contract; diff against tsc's for a sample pair by instrumenting the oracle |
