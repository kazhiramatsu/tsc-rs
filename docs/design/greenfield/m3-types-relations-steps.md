# M3: types crate + relation engine — steps

Parent design: greenfield.md §4.2-4.3, §4.7 (data model, links,
relations); checker-foundations.md §4 (union/intersection
construction); checker-key-functions.md §1 (the engine) and §5 steps
1-4 (port order). Prerequisite: M2 gate green.

Gate: the relation pin suite (~200 oracle-probed cases) green.

INSTANTIATION BOUNDARY (governs every stage below): M3 contains NO
`instantiateType`/`TypeMapper`, no `getTypeFromTypeNode`, no member
instantiation and no `getApparentType` — those are M4 stages 5.1-5.3.
Everything M3 pins must be constructible WITHOUT a mapper. Engine
arms whose input types cannot exist before M4 are ported or stubbed
dead with ledger notes; their pin rows land in M4 and re-run through
this same relpin harness (the pin file grows per milestone, it is
never M3-frozen). Variance measurement moved to M4 for the same
reason (see stage 4.7 tombstone).

## Stage 4.0: the relation pin harness FIRST [P]

Relations are not directly observable until the checker exists, so
M3 builds a probe bridge: a test-only entry that parses two type
annotations in a scratch program, resolves them through a MINIMAL
type-from-annotation path (stage 4.1 provides it), and asks the
relation engine. Ground truth comes from oracle probes of
`var t: Target = source_value;` / `declare var s: Source; var t:
Target = s;` fixtures. Classification rule: ANY semantic diagnostic
on the fixture = not related — NOT "2322 present". Assignment
failures root as 2322 but ALSO as 2353 (excess property), 2739/2740
(missing properties), 2559/2560 (weak type / no properties in
common); the fixture contains no other error source, so
presence-of-any-error is the correct oracle. Comparable pins use
`s as Target` fixtures the same way (2352-family presence). Build
the generator:

```
xtask relpin gen pins/relations.toml   # regenerates fixtures + oracle expects
xtask relpin run                       # runs engine, prints disagreements
# [pair] source=".." target=".."
#        relation="assignable"|"comparable"   (default assignable)
#        options={ strictFunctionTypes=false, ... }  (optional; emitted
#          as harness `// @option:` directives so oracle+engine agree)
#        expect="yes"|"no"                    (filled by gen, committed)
```

Seed `pins/relations.toml` with ~200 pairs covering: primitives ×
literals (fresh and regular), unions both sides, intersections,
object literals (width/optionality), functions (param bivariance vs
strictFunctionTypes on/off per pin), tuples (rest/optional/readonly
elements), recursive interfaces (the Maybe path: mutually recursive
NON-GENERIC `interface A { next: B }` / `interface B { next: A }`
pairs — these exercise the maybe-stack fully without instantiation),
template literals, index signatures, `{}`/unknown/any/never edges,
undefined/null under strictNullChecks on AND off, comparable-relation
pairs (literal↔base both directions, disjoint primitives). EVERY
pin's `expect` value is filled by running the oracle on the generated
fixture, committed alongside. DEFERRED TO M4 pin rows (need
instantiation/declared-type machinery): generic references
(`A<T>`/`B<T>` mutual recursion), deeply-expanding generics (the
depth limiter can only fire through deferred-reference expansion),
variance-driven reference pairs, enums.

Commit: `m3 4.0: relation pin harness + seeded pins`.

## Stage 4.1: types crate core [M]

Per greenfield §4.2-4.3 and core-interfaces §3:

- `Type`/`TypeData` arena; allocation id IS identity; interning maps
  ONLY where tsc declares one (`_tsc.js` 46988-47009). The ones M3
  needs: literal values (string/number/bigint/enum maps), unions
  (`unionTypes`, `getTypeListId`-style member-id keys) plus the
  2-union fast-path cache `unionOfUnionTypes` (61512 — its key DOES
  include `getAliasId`; relation keys do NOT, see stage 4.4),
  intersections (`intersectionTypes`), string mappings, tuple TARGET
  shapes (`tupleTypes` — arity/ElementFlags/readonly key), template
  literals (`templateLiteralTypes`). NO map is keyed structurally on
  anonymous object members: two structurally identical anonymous
  object types are DISTINCT.
- Intrinsics (any/unknown/never/void/undefined/null/string/number/
  boolean/bigint/symbol/object + error/silentNever/wildcard variants
  tsc distinguishes internally, + `missingType` and the
  undefined/null WIDENING variants — `getUnionTypeWorker` reads all
  three directly at 61540-61562).
- Literal types as fresh/regular/wide triples (freshness is
  STRUCTURE, not a flag-cache: `fresh_of`, `regular`, `wide` links).
- Links tables with ONE-WRITE discipline + explicit `Resolving`
  sentinel variants + the checker-wide `speculation_depth: u32`
  assertion on every links write (greenfield §4.3 — this single rule
  is the transaction model).
- The relpin probe bridge's MINIMAL type-from-annotation path — an
  explicitly scoped slice of M4 5.1/5.3, each fn ledgered as a
  partial port: keyword types, literal type nodes, parenthesized,
  union/intersection type nodes, array/tuple type nodes (tuple
  targets + ElementFlags), type literals and NON-GENERIC interface
  references (members read from binder tables; no heritage, no type
  arguments), function/constructor type nodes
  (`getSignatureFromDeclaration` for annotation-only signatures),
  template literal type nodes, index signatures. NOTHING requiring a
  TypeMapper. M4 5.1 replaces this with the full `getTypeFromTypeNode`
  port.

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
  tsc's SIXTH map `enumRelation` (47455, symbol-pair keyed, consumed
  only by `isEnumTypeRelatedTo` 64673) is deliberately NOT a
  RelationKind (checker-key §1.5); M3 stubs `isEnumTypeRelatedTo`
  (enum declared types are M4) — ledger the stub, enum pins arrive
  with M4.
- `RelationResult` from the generated `RelationComparisonResult`
  (table at `_tsc.js` 3404).
- `getRelationKey` (67423): `sourceId,targetId` (ids SWAPPED so the
  smaller comes first for identity relation only), `:intersectionState`
  suffix when nonzero, and the generic-reference form
  (`getGenericTypeReferenceRelationKey` 67394) with the `'*'`
  constraint-broadened prefix and `=N` type-parameter backrefs. NO
  alias context — alias ids belong to `unionOfUnionTypes` keys, not
  relation keys (greenfield §4.7 misstates this; the source is
  authoritative).
- `Ternary` values from codegen (False=0, Unknown=1, Maybe=3,
  True=-1 — already generated in types/src/flags.rs) and the
  truthiness convention (`& 1`-style checks in ports read as tsc
  wrote them).

Commit: `m3 4.4: relation caches + getRelationKey + Ternary`.

## Stage 4.5: the engine core [M]

Port in this order, each against its cited source:

1. `isTypeRelatedTo` (64762) — entry fast paths, fresh→regular
   normalization, the comparable reversed-simple rule (KEEP all three
   occurrences: 64773 here and TWICE inside isRelatedTo at
   65150/65197 — checker-key §1.1's "unionOrIntersectionRelatedTo"
   location for the third is wrong, corrected there).
2. `checkTypeRelatedTo` (64842) — state setup, error-node handling
   (error CAPTURE only at M3; chain shaping is T2 work).
3. `isRelatedTo` (65147) — the reversed-comparable entry check on the
   ORIGINAL types (65150), normalization, simple rules (65197), the
   fresh-object-literal excess-property check (`hasExcessProperties`
   65347, CALLED at 65201 — it lives HERE, not in
   propertiesRelatedTo) and the weak-type common-property check
   (65208, `isWeakType` 67285 / `hasCommonProperties` 67298), then
   union/intersection dispatch: `unionOrIntersectionRelatedTo` 65414
   + the typeRelatedToSomeType / eachTypeRelatedToSomeType /
   someTypeRelatedToType / eachTypeRelatedToType family (65466-65629,
   incl. `typeRelatedToDiscriminatedType` 66523 for discriminated
   union targets) with the skip-caching small-union rule (<4
   constituents bypass recursiveTypeRelatedTo, 65237).
   NORMALIZATION DEPENDENCY DISPOSITIONS (`getNormalizedType` 64807):
   fresh→regular = stage 4.1; `getNormalizedTupleType`/
   `createNormalizedTupleType` = PORT (tuple pins with rest elements
   hit it); `getReducedType` (59287) = STUB returning input, ledgered
   (discriminant reduction is M4 5.3's `getReducedApparentType`);
   `getSingleBaseForNonAugmentingSubtype` = STUB (base types are M4);
   `getSimplifiedType` (62455) = STUB (indexed access unconstructible
   in M3); Substitution unwrap = STUB (same reason).
4. `recursiveTypeRelatedTo` (65725) — THE maybe-stack, ported from
   the full skeleton in checker-key §1.2 with all four invariants
   (Maybe never cached mid-recursion; commit-on-unwind; the
   relation_count complexity budget; depth-100 overflow with the real
   diagnostics: 2321 Excessive_stack_depth / 2859
   Excessive_complexity — NOT 2589, which is instantiation depth and
   belongs to M4 5.2's instantiateType). The cache-hit
   variance-replay branch (instantiateType against
   reportUnmeasurable/reportUnreliable mappers, 65744-65750) is only
   reachable during variance measurement — stub it dead with a ledger
   note until M4.
5. `isDeeplyNestedType` (67465, maxDepth default 3) +
   `getRecursionIdentity` (67507) — checker-key §1.3 skeleton.
   Unpinnable in M3 (expansion requires deferred-reference
   instantiation); port against source, pins arrive in M4.

Verify continuously: `xtask relpin run` — the recursion/Maybe pins
flip green here.

Commit(s): `m3 4.5a-e: relation engine core`.

## Stage 4.6: structuredTypeRelatedTo [M]

The body (65872; worker 65942), one family per commit, tsc's arm
order (checker-key §1.4). Full arm inventory with M3 disposition —
LIVE = pinnable now; DEAD = port the dispatch skeleton but the input
types are unconstructible before M4, ledger each and verify via M4
pin rows:

- identityRelation early branch + the *IdenticalTo family
  (`propertiesIdenticalTo` 66911, `signaturesIdenticalTo` 67082,
  `indexSignaturesIdenticalTo` 67196, `compareProperties` 67536,
  `isMatchingSignature` 67559, `compareSignaturesIdentical` 67574) —
  LIVE (identity dispatch on object pins).
- type-parameter arms incl. the comparable constraint walk (66109) —
  DEAD (type-parameter types need generic declarations).
- Index (keyof) / IndexedAccess / Conditional / Substitution / Mapped
  arms (`mappedTypeRelatedTo` 66508, `getApparentMappedTypeKeys`
  65930) — DEAD; port arm ORDER now so the dispatch is pinned.
- TemplateLiteral + StringMapping target arms — LIVE (template
  literal pins).
- `propertiesRelatedTo` (66766) — LIVE; includes the TUPLE arm
  (target-tuple arity/rest/readonly ElementFlags logic, 66771+) that
  tuple pins exercise. NOTE: the excess-property and weak-type checks
  are NOT here — they live in isRelatedTo (stage 4.5 item 3).
- `signaturesRelatedTo` (66934) — LIVE; the real per-signature worker
  is `compareSignaturesRelated` (64487) + 
  `compareTypePredicateRelatedTo` (64606), OUTSIDE the
  checkTypeRelatedTo closure — port them with this family (method
  bivariance vs strictFunctionTypes contravariance — signature kind
  comes from the declaration, core-interfaces §4 `from_method`).
- `indexInfosRelatedTo` (67167) + `membersRelatedToIndexInfo` (67108)
  — LIVE (index-signature pins).
- `relateVariances` (66488) for references — DEAD: variance
  measurement (stage 4.7 tombstone) is M4. Port the call site
  returning "fall through to structural", ledgered.

Commit(s): `m3 4.6a-e: structural arms (+pin counts)`.

## Stage 4.7: variance measurement — MOVED TO M4 [tombstone]

`getVariances`/`getVariancesWorker` (67306/67312) depend on
`createMarkerType` (67360) → `makeUnaryTypeMapper` +
`instantiateTypes` + `getDeclaredTypeOfSymbol` + the
`resolutionTargets` stack — all M4 5.0-5.3 machinery, so this stage
cannot exist in M3 (the original placement was a scoping error).
Ported in M4 (stage 5.3b in m4-checker-skeleton-steps.md), together
with the `VarianceFlags` codegen seed (a const enum INLINED in
`_tsc.js` — add a `SourceEnum` entry like Ternary's; it is NOT yet in
types/src/flags.rs) and the unmeasurable/unreliable marker
propagation into relation results. Until then `relateVariances` stays
the stage-4.6 ledgered stub, and the generic-reference /
deeply-expanding / variance pin rows wait in M4.

## Stage 4.8: Subtype/StrictSubtype activation [M]

The engine already dispatches on RelationKind; now: port the
subtype-specific simple rules, flip `UnionReduction::Subtype` in
`getUnionType` from the stage-4.2 stub to a real
`removeSubtypes` (61368) port, and add `getCommonSupertype` (67650).
Pins: union
display-order fixtures and literal-union reduction pairs
(oracle-probed via variable-hover is unavailable — use assignability
consequences and error-message text pins instead, generated the same
way as stage 4.0).

Commit: `m3 4.8: Subtype relation + union subtype reduction`.

## Final gate

```sh
cargo xtask relpin run        # expect: 0 disagreements over ~200 pins
cargo xtask ledger check
cargo xtask conformance       # no-regression only: M3 wires no new
                              # diagnostics, [t0] ratchet must be unchanged
```

## Expected failure modes

| Symptom | Diagnosis | Fix |
|---|---|---|
| Recursive pins flap depending on pin ORDER | a Maybe result got cached mid-recursion | Re-check reset_maybe_stack commit conditions (checker-key §1.2 invariant 1) |
| Distinct `{}` literals compare related where oracle says no | structural interning crept in | Only tsc's interning maps may exist (stage 4.1 list) — none keyed on anonymous object structure |
| Deep generic pins overflow where oracle succeeds (M4 rows) | isDeeplyNestedType counting by raw type instead of recursion identity | Port getRecursionIdentity, not a depth counter |
| Fresh-literal pins differ | fresh→regular normalization missing at an entry | It happens at isTypeRelatedTo entry AND inside specific arms — follow the source, not symmetry intuition |
| excess-property/weak-type pins pass structurally but miss errors | the two checks were ported into propertiesRelatedTo | They live in isRelatedTo (65201/65208) BEFORE dispatch — stage 4.5 item 3 |
| Cache hit-rate ~0 and pins slow | getRelationKey missing intersection state or the generic-reference form | The key string is part of the contract (NO alias context); diff against tsc's for a sample pair by instrumenting the oracle |
