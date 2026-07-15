# M4-end sweep (stage 5.9) — steps

The M4 final-gate sweep after 5.8e, per
[m4-checker-skeleton-steps.md](m4-checker-skeleton-steps.md) §"Final
gate", the residuals note at the end of
[m4-58-statement-extraction.md](m4-58-statement-extraction.md), and
the M4-close row of [definition-of-done.md](definition-of-done.md)
(T0 ≥ 35% All band — already met at 37.79% — plus **untagged escapes
0** and **stale 0**; first real go/no-go).

Inputs at sweep start (post-5.8e, `main` @634b25dc): manifest
entries 113 untagged (114 sites) + 44 owner="5.8" (46 sites) + 101
recovery + 74 stage-owned M5/M6/M7/M8. The sweep adjudicates the
first two groups to zero.

## Stage key and stale mechanics

The sweep is stage **5.9** (`parse_stage_key`: letterless `5.8`
owners expire the moment STAGE names any 5.9 value, because
`StageKey(4,8,MAX) <= StageKey(4,9,_)`). Therefore:

- Intermediate slices (5.9a-c) keep `tsrs2/STAGE` at `5.8e` — the
  44 owner="5.8" entries are being worked, not yet expired.
- The close slice (5.9d) bumps STAGE to `5.9`, which makes the
  stale gate ENFORCE that zero owner<=5.8 entries remain, and
  ratchets `[escapes] max_untagged` to 0. This is the designed
  forcing function, not an accident of ordering.
- Sites re-owned to M5/M6/M7/M8 survive the bump
  (`StageKey(5..8,0,0) > StageKey(4,9,_)`).

## Disposition vocabulary

Every one of the 157 entries gets exactly one:

1. **dead-guard (delete)** — the guarded shape is PROVEN
   unconstructible on both well-formed and malformed trees
   (constructibility proof from parser/binder sources, per the
   ledger ground rule: unreachability claims need a proof or a
   pin). The guard becomes `.expect("parser invariant: ...")` /
   `unreachable!(...)` carrying the proof citation; the manifest
   entry disappears and `max_untagged` ratchets down. This is NOT
   re-labeled "recovery": tsrs2 parse recovery substitutes PRESENT
   missing-Identifier/TemplateTail nodes (with parse diagnostics),
   never `None` children or foreign payloads, so calling these
   malformed-tree guards would be factually weaker than the
   evidence. tsc's twin reads the field unguarded — the panic IS
   the faithful port of tsc's implicit invariant. Empirical
   backstop, all part of `cargo xtask ci` at every slice gate:
   full-corpus conformance (5,908 fixtures / 48,719 diagnostic
   rows), the bind-corpus smoke (11,130 files parsed+bound, added
   to ci with this slice), and the invariants suite.
2. **recovery** — a permanent guard for input that IS reachable
   but for which no tsc semantics exist: malformed/parse-recovery
   trees, and the tsc-CRASH family (inputs on which the vendored
   tsc throws — Debug.fail transcriptions and implicit TypeError
   corners like createTypeReference-over-non-generic-interface;
   the corpus can never carry a golden for these, so the guard is
   FN-free forever). The distinction from dead-guard is
   REACHABILITY: a reachable crash-guard must stay graceful (an
   `expect` would panic on real input). Reason gains the strict
   marker vocabulary ("recovery node" / "parse recovery" /
   "parse-recovery", following the established Debug.fail
   wording); `max_recovery` bumps with a justification comment
   (the gate makes re-labeling a review surface by design).
3. **stage re-own (M5/M6/M7/M8)** — later-milestone semantics;
   reason gains the owner tag. Standing assignments: flow → M5;
   inference/overloads/speculation → M6; unused/grammar/suggestion
   → M7; conditional/mapped/infer evaluation, JSDoc/checkJs depth
   ([JSDOC] standing policy), display/nodeBuilder precision
   (T2-class), 2xxx tail → M8 (the skeleton final gate admits only
   M5-M8-stub residual classes).
4. **implement-5.9** — genuinely missing M4-band semantics; landed
   in 5.9b/5.9c, escape deleted by implementation.

## Sub-slices (one branch + PR each, per branch workflow)

- **5.9a `m4/5.9a-retag`** — the adjudication slice: 6-way audited
  disposition table (below) lands as (a) dead-guard deletions with
  proof-comment conversions, (b) recovery/stage reason re-tags,
  (c) this doc. Code semantics changes: none beyond guard
  conversion, EXCEPT trivially-safe error-type fallbacks
  adjudicated implement-now (e.g. the EWTA
  non-entity-name heritage arm → errorType per tsc 60372-60383).
  Manifest regenerated (`cargo xtask escapes --write-manifest`);
  `max_untagged` ratchets down to the implement-5.9 residue;
  `max_recovery` bumps by the re-tag count with per-family
  comments. STAGE stays 5.8e. Full `cargo xtask ci`.
- **5.9b `m4/5.9b-relations`** — the type-system implementation
  slice: keyof/Index/IndexedAccess relation arms + identity
  halves, getSimplifiedType un-stub (shared prerequisite of the
  keyof-target arm and getConstraintOfIndexedAccess),
  computeBaseConstraint for generic tuples, variadic tuple
  relations, tuple rest index fallbacks,
  equality-comparability-over-intersection, tables tuple-twin
  demotion to test-only (retires 6 tables escapes). Oracle pins
  land with the commit.
- **5.9c `m4/5.9c-members`** — the members/signatures band,
  anchored by the two largest residual FN bands: Constructor
  signatures (getSignatureFromDeclaration classType arm) and the
  anonymous value-side member tail (enum/namespace/globalThis
  exports-as-members), plus the late-binding read-side catch-ups,
  member-table merges, accessor/ambient/prototype/export= typing
  arms, type-reference unknown-symbol arm, EWTA errorType, and
  the small probed arms (abstract-member 2513, enum-literal
  condition). See the implement-5.9c list in the table.
- **5.9d `m4/5.9d-modules-jsx`** — modules/jsx/interop band: both
  synthetic-default interop escapes, the aliased-JSX family ×8,
  super base constructors, import types, the display-presence
  fixes, Extract/Omit/Awaited arity trio, remaining calls/module
  arms, and the 2578 re-measure (the 5.8e deliberate FN) with a
  recorded keep/land decision. See the implement-5.9d list.
- **5.9e `m4/5.9e-close`** — untagged→0 and owner<=5.8→0 enforced:
  STAGE→5.9, `max_untagged = 0`, skeleton-steps final gate
  (conformance T0≥35% + FP=0, relpin, invariants idempotence,
  ledger check, MANUAL stub audit `grep -rn "M[5-8]-stub"
  crates/`), write `docs/NOTES-m4.md` (top 10 one-sided codes with
  owner guesses — seeds M5/M6 verification and the M8 backlog).
  M4 close go/no-go recorded against definition-of-done.

## Disposition table

Filled by the 5.9a audit: six parallel per-file auditors over all
157 entries (113 untagged + 44 owner="5.8"); the orchestrator
re-verified the shared lemmas (missing-node kind catalog, arena
kind/data coupling, tables-twin caller inventory, 5.8d alias
helpers) and resolved the one UNCERTAIN entry by oracle probe.
Totals: retire 72 / implement-5.9 53 / M8 28 / M6 2 / M5 1 /
recovery 1.

### Shared lemmas (cited below)

- **L-MISSING**: missing nodes are minted ONLY by
  parser `create_missing_node` (arena `alloc_missing`), and only
  with kinds Identifier / MissingDeclaration / CommaToken /
  TemplateTail / parse_expected_token token kinds. Parse recovery
  therefore substitutes PRESENT nodes (with parse diagnostics),
  never `None` children — mandatory-field `None` guards are dead
  on well-formed AND malformed trees.
- **L-COUPLING**: `alloc_node` derives the node kind from
  `data.kind()` (arena.rs) — kind/data divergence is
  unconstructible; "kind matched but wrong data" guards are dead.
  The codebase already trusts this as
  `unreachable!("kind/data agree")`.
- **L-PARENTS**: `finalize_tree` assigns parents to every
  non-root node; "no parent" guards on nodes that cannot be roots
  are dead.
- **L-TWIN**: the tables-side tuple-normalization twin has NO
  live caller (checker twins own every live path; tables copy is
  reached only from unit tests and one empty-tuple call that
  cannot hit the guarded arms).
- **L-BINDER**: `declare_symbol`/`add_declaration_to_symbol`
  attach a symbol to every declaration node (MISSING-named symbol
  for nameless shapes); "unbound declaration" guards on bound
  trees are dead.

### retire — 72 (proven-dead guards → expect/unreachable, 5.9a)

annotate.rs (27): the 25 malformed-payload guards in the
type-node/literal/template/signature families (L-MISSING + sole
constructors filling every mandatory field; incl. the two
payload-shape guards proven dead by lookahead gating — negated
non-numeric / non-minus prefix — and `unparsable numeric literal
text` dead by scanner output shapes) + `type operator {other:?}`
(parser emits only keyof/unique/readonly) + `get_this_type`
mid-cycle shell (classes are unconditionally GenericType; atomic
stamping; re-entrancy asserted). expr.rs (1): `try_get_this_type_at`
mid-cycle shell (same proof, class-parent-only route).

operators.rs (17): the "without data" family ×13 (L-COUPLING;
kind-dispatched call sites) + `property access without receiver`
(all four constructors fill expression) + `constructor without a
class parent` (L-PARENTS) + `checkBinaryLikeExpressionWorker
unknown operator` (closed parser operator domain ×
exhaustive worker arms) — the Debug.fail transcription is dead,
unlike the reachable-crash family below.

evaluate.rs (9): enum-evaluator mandatory-field guards
(L-MISSING; every constructor fills name/operands/head/spans;
recovery missing-TemplateTail still carries TemplateTailData).

access.rs (2): `private member without declaring class`
(accessibility mask strips PRIVATE unless symbol parent is a
CLASS — the exact condition making get_declaring_class Some;
synthetics never carry CONTAINS_PRIVATE: hardcoded
CONTAINS_PUBLIC) + `protected-instance report without containing
type` (props findable only through resolved base constraints).
Both conditioned on Contains*-flag propagation staying unported —
noted in the conversion comments.

check.rs (1): `non-generic reference display` (reference targets
are GenericType-or-TupleTarget by producer enumeration;
TupleTargets exit earlier symbol-less).

structural.rs (2): `unparsable numeric member name`
(is_numeric_name admits only f64-parsable digit strings) +
[tables] see L-TWIN. instantiate.rs (3): mapped-type
missing/unbound guards (sole MappedType constructor fills
type_parameter; L-BINDER). relate.rs (1 entry, 2 sites): enum
member without declaration (binder grants ENUM_MEMBER only at
declaration binding; L-BINDER). variance.rs (1): symbol-flags
guard (all three call chains guarantee CLASS/INTERFACE/ALIAS).

types/tables.rs (6): the create_normalized_tuple_type arm family
+ single-rest collapse + get_tuple_target_type (L-TWIN) — these
convert during the 5.9b twin demotion, not 5.9a.

calls.rs (1): `checkTypeArguments under a head message` (head
producers — decorators — skip type-argument collection entirely).
jsx.rs (1): `string-literal tag type without a string payload`
(sole STRING_LITERAL producers always store Literal data; string
enum literals carry their value).

### implement-5.9 — 53 (escape deleted by implementation)

**5.9b relations/constraints band** (11 + the 6 tables retires):
keyof targets / keyof sources / indexed-access targets relation
arms; identity halves for Index/IndexedAccess/StringMapping
(Conditional/Substitution halves narrow to M8); getSimplifiedType
un-stub (shared prerequisite: keyof-target arm +
getConstraintOfIndexedAccess); computeBaseConstraint for generic
tuples; variadic tuple elements in relations; tuple rest index
fallbacks; equality comparability over intersection operands;
tables twin demotion to test-only.

**5.9c members/signatures band** (anchors: the two largest
residual FN bands in M4): `signature declaration kind` Constructor
arm — every `new C(...)` against an explicit constructor is
currently contained (getSignatureFromDeclaration classType arm:
class type parameters, parameter properties, instance-type
return); `anonymous members for symbol flags` value-side tail —
every `E.A` / `N.x` / `ns.x` property access on
enum/namespace/globalThis objects is currently contained
(exports-as-members + merged-globals). Plus: early/late member
table merge (combineSymbolTables→mergeSymbol; merge_symbol landed
5.8d); late index signature over early __index (cloneSymbol);
late-bound computed-name index-signature reads (write side landed
5.8e — M7-stub option foreclosed); instantiated accessor writes
(one-line instantiate of target write type); const-enum object
property lookup (pass skipObjectFunctionPropertyAugment into
existing get_property_of_type_ex); prototype-property arm
(getTypeOfPrototypeProperty); worker declaration kind re-routes
(Class/Function/Enum/EnumMember); export= value type; accessor
this-parameter from getter signature; ambient property base-class
type ×2 (getTypeOfPropertyInBaseClass; undefined-base fallthrough
removes over-containment); variable-like circularity annotation
(kind-generic effective_type_annotation_node); type reference to
symbol flags (unknown-symbol → errorType per tsc 60380-60405);
EWTA non-entity-name heritage → errorType (tsc 60372-60383);
getDeclaredTypeOfAlias (3-line resolveAlias wiring); abstract
member without declaring class — PROBED: vendored tsc emits
TS2513 with "class 'any'" (no Debug.fail crash) on
`interface I { abstract m(): void }` + ctor-typed heritage, so
the arm implements to match; enum-literal condition non-Literal
payload (tsc-faithful `!!undefined → false`).

**5.9d modules/jsx/interop band**: BOTH synthetic-default interop
escapes (getTypeWithSyntheticDefaultImportType +
resolveESModuleSymbol cloning — one shared machinery set:
createDefaultPropertyWrapperForModule + cloneTypeAsModuleType +
syntheticType memo); the aliased-JSX family ×8 (all reduce to
5.8d's resolve_symbol_ex / resolve_alias / get_symbol_flags_of;
gates the canonical `import * as React` fixture shapes);
index-signature-only JSX.IntrinsicElements (tolerate the None
symbol per tsc getApplicableIndexSymbol); super base constructors
(helpers all landed — getInstantiatedConstructorsForTypeArguments
consumed at class.rs:674 already); import types
(getTypeFromImportTypeNode over 5.8d module machinery);
getSymbolForExpression private-identifier arm (both helpers
landed 5.5d); getDeclarationSpaces alias target (resolveAlias +
recursion); readonly-entity alias receiver (namespace-import 2540
tail); exactOptionalPropertyTypes argument-head variant
(different error CODE — diagnostic presence, not display);
checkImportMetaProperty ES-module-kind arms + globalImportMeta
(Node16 arm stays on the impliedNodeFormat escape family);
destructuring into an element access (synthetic-access marker
mirroring tsc SyntheticExpression; corpus-hit
declarationsAndAssignments.ts:139); identically-named-type
display ×2 + origin-union display (whole diagnostics are
suppressed today — bounded qualified-name fallback / origin
unwrap + keyof rendering; escalate leftovers to the M8-tagged
structured tail); global type-alias arity guards
Extract/Omit/Awaited (one shared getGlobalTypeAliasSymbol port:
2317 + unknownSymbol fallback; adjudicated as one family — the
memory's "Extract/Omit arity guards" residual row); unique-symbol
index head (fullyQualifiedName in the 2339 chain head). Plus the
2578 re-measure with a recorded keep/land decision.

### M8 re-own — 28

Conditional/infer/mapped families (skeleton final-gate M8-stub
class): the three get_type_from_type_node arms + conditional
sources/targets + mapped-type targets relation arms +
mapped-readonly write 2542 + member resolution for
Mapped/ReverseMapped object flags. JSDoc/checkJs band
([JSDOC] standing policy; definition-of-done exclusion):
getJSDocTypeAssertionType; binary expando onEnter; JS container
object type; assignment-declaration value type; widen ESSymbol JS
arm; widenTypeInferredFromInitializer JS arms;
getContextualType parenthesized JSDoc arms;
getContextualTypeForAssignmentDeclaration JS kinds;
isJSConstructor probes ×2; type alias without
TypeAliasDeclaration; reportImplicitAny JSDoc arms; type node
kind {other} (the five JSDoc-flavored type nodes in TS positions,
8020 band); entityNameToString JSDoc/JSX kinds. Display/T2:
operator-error identically-named display (message-only twin of
the check.rs pair — those two implement in 5.9d because they
suppress whole diagnostics; this one is message-only);
reference display with outer type parameters. Pragma/options
tail: @jsx-family pragma comment (processCommentPragmas unported
since M1); jsxFactory-family options (parseIsolatedEntityName
missing); react-jsx implicit import container (pragma half
decisive). UTF-16 stranded surrogate (WTF-16 literal storage — a
representation project).

### M6 — 2, M5 — 1, recovery — 1

M6: assertion-deferred stash guard (the exact stash/defer
atomicity seam the M6 speculation transaction owns — external
review precondition); symbol-less tuple reference display ("the
full tuple renderer is M6" per in-code precedent + manifest
siblings). M5: isConstantReference binding-pattern arm
(getNarrowedTypeOfSymbol family, m5-flow-steps 6.1/6.6). recovery:
createTypeReference over a non-generic interface — REACHABLE
(thisless non-generic interface as JSX.ElementType) but the
vendored tsc throws TypeError there (undefined instantiations
map), so no golden can exist; reclassed to the permanent
crash-guard family with the "parse-recovery" marker wording
(Debug.fail-transcription precedent), NOT convertible to expect.

## Gates (every slice)

Unchanged from 5.8: `cargo xtask ci` green on the branch before PR
and before merge — fmt, clippy -D warnings, build, tests, relpin,
conformance all + 2xxx with FP=0 + integer-ratchet non-regression,
invariants, ledger check, `escapes --stale $(cat STAGE)` including
untagged/recovery ceilings. Merge commit only.
