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
tests. The full state-surface inventory the transaction must cover is
Stage 7.0t below. Design and land that before 7.1; the
alternative — candidate state leaking through links or blanket
panics mid-resolution — is the exact failure mode the M4 5.7a
deferred re-check protocol only papered over for calls.

Gate: T0 ≥ 58%. Inference moves 2345/2322/2769/2339 together — run
the full gate per stage, not just call fixtures.

## Stage 7.0t: speculation scoped-transaction — STATE-SURFACE INVENTORY [M]

Spec input for the `begin_speculation()` transaction (the START
PRECONDITION above). Transcribed from the 2026-07-19 M4 review
(m4-review-2026-07-19.md 付録A, items B34/B35) — the engine-band
agents' full-state audit. Legend: (a) = today's Unsupported-unwind
restoration, (b) = M6 rollback requirement, (c) = residue risk.

**Already closed by the `m6/0-start-bar` slice (read the code, not
the risk column, for these):** A1 relation-key forcing
(get_relation_key is fallible and constraint-forcing; the `*`
broadest-key Maybe recheck is live), A4 resolved_return_type
(`seal_signature_return_type`: `??=` + speculation assert), B10
(union_property_cache / subtype_reduction_cache /
set_symbol_is_discriminant / ty_mut_cached_equivalent_base_type all
assert speculation_depth == 0), B15 (parameters of context-sensitive
signatures never cache their type).

### A. Transient stacks (must be at entry depth after any unwind — all in UnwindSnapshot unless noted)

| Field (state.rs) | today | M6 need | risk |
|---|---|---|---|
| `resolution_targets/results/property_names` (659-661) + `resolution_start` (664) | every pusher pops via captured-result pattern; snapshot-checked | save/restore lengths + `resolution_start`; results VALUES below the mark are mutated by cycle flagging — truncate is enough since entries above mark are popped | LOW |
| `contextual_type_nodes/types/is_cache` (502-504) | push/pop paired | truncate to mark | LOW |
| `inference_context_nodes/contexts` (509-510) | exists-but-empty (M6 payload pending) | THE new M6 stack — define rollback with it | — |
| `contextual_binding_patterns` (495) | paired | truncate | LOW |
| `awaited_type_stack` (474) | paired | truncate | LOW |
| `active_type_mappers(+caches)` (306-307) | pushed frames popped on Err (instantiate.rs:1091-1096) | truncate stack; ALSO port `clearActiveMapperCaches` on inference fixing; entries in SURVIVING outer frames computed during a failed candidate stay structurally true — but under a mutable inference mapper they are stale unless cleared | **HIGH** |
| `variance_handler_stack` (303) | popped on Err (engine.rs:1692-1699) | truncate | LOW |
| `class_interface_declared_in_progress` (320), `type_parameter_defaults_in_progress` (324) | popped before `?` | truncate | LOW |
| `flow_loop_stack` (527) + `flow_loop_start` (521) | paired; snapshot-checked | truncate/restore | LOW |
| `shared_flow` (336) | truncate-before-`?` (flow.rs:317) — **NOT snapshot-checked** | truncate to mark; add to UnwindSnapshot | MED |
| `reduce_label_overrides` (343) | restore-before-return (flow.rs:1856-1863) — **NOT snapshot-checked** | snapshot map or forbid across speculation | MED |
| `exhaustive_switch_computing` (551) | remove-before-`?` (narrow.rs:2352) — **NOT snapshot-checked** | must be empty across the boundary | MED |
| RelationChecker maybe stack (engine.rs:707-720) | per-call local struct; Err arm pops its frame's keys (1707-1718); every early return precedes the pushes | nothing — cannot leak by construction | NONE |

### B. Counters / cursors / flags

`speculation_depth` (203): the transaction's own guard — RAII
increment/decrement. `instantiation_depth` (311): restored on Err;
snapshot-checked. `instantiation_count` (312): monotone per element,
reset at the three entry points (check.rs:344 / check.rs:1764 /
expr.rs:102 = tsc 86551/86921/80965) — do NOT restore on rollback
(tsc doesn't). `inline_level` (539): NOT snapshot-checked.
`in_variance_computation` (295) / `variance_type_parameter` (291):
save/restore exists. `flow_analysis_disabled` (331): one-way latch
even in tsc — leave. `is_inference_partially_blocked` (247):
M6-owned, part of the transaction. `suggestion_count` (454): **must
be snapshot/restored or consumption-gated under speculation** — tsc
consumes the did-you-mean budget only on reporting paths.

### C. Caches — permanent-truth (keep across failed speculation, keyed structurally)

`relations` (5 maps + enum_relation) — keep (A1 key collisions are
FIXED); entries referencing speculation-minted types stay internally
consistent (types are permanent interned objects; tsc doesn't roll
relation caches back either). `subtype_reduction_cache`,
`cached_types`, `tables.instantiation_*`, `links.alias_instantiations`,
`links.union_property_cache`, `Signature.instantiations`/
`erased_signature_cache`/`optional_call_signature_cache`/
`isolated_signature_type`, `decorator_context_override_type_cache`,
`undefined_properties`, `evolving_array_types`/`final_array_types`,
`flow_loop_caches`, `flow_node_reachable`, `last_flow_node(+reachable)`,
`switch_types_cache`/`exhaustive_switch_cache`/`effects_signature_cache`/
`resolved_type_predicates`, deferred-global memos,
`init_global_type_probes`, `jsx_implicit_import_containers`,
`js_assignment_containment_indexes`, `unresolved_package_root_cache`,
`merged_symbols` — all compute-once truths; no rollback needed
PROVIDED no M6 inference-placeholder type can be an input key (in tsc
the only inference-sensitive cache is the active-mapper cache).
**Verify this property for each new M6 cache.**

Links tables: write-once/protocol slots; the four Resolving-sentinel
protocols each have paired Err-reverts (variances / simplified /
resolvedSignature call protocol / aliasTarget) — verified. Late-bind
revert annotate.rs:3460-3467; members retracts
annotate.rs:3117/3263/4360/5354. The A4/B10 discipline bypasses are
CLOSED (asserts in place), so "no permanent writes under speculation"
now holds mechanically — any new raw cache field must join the assert
net before 7.1 lands.

### D. Diagnostics sinks (transaction must truncate)

`diagnostics` (570) — truncate to mark (push-dedupe is order-safe
under truncation). `visible_global_diagnostics` (575).
`partially_checked_ranges` (580) + `partial_check_records` (584) — a
speculative containment permanently marks a range (affects
@ts-expect-error exemption) → must roll back.
`elaborated_satisfies_expressions` (587), the five `potential_*`
per-file accumulators (431-435) — speculative pushes must roll back
or double-drain. `deferred_nodes` (420) — tsc DOES still check nodes
registered under speculative checkMode; verify against tsc
checkNodeDeferred before deciding to roll back. `suggestion_count` —
see B.

### E. Interners / arenas (append-only, never rolled back — garbage is safe)

`tables` types (object-flag memos:
COULD_CONTAIN_TYPE_VARIABLES_COMPUTED / IS_UNKNOWN_LIKE_UNION_COMPUTED
written only after the fallible region — verified;
IDENTICAL_BASE_TYPE_CALCULATED is the exception = review B1, still
open), `signatures` (196; the A4 raw-field bypass is closed),
`members` (197; staged-publication in-place mutation has Err retract
twins), `mappers` (262), `widening_contexts` (482),
`inference_context_arena` (added at 7.1: context MUTATIONS also
deliberately survive failed trials — chooseOverload reuses the SAME
context across its re-run, 76842-76844, so trial-time candidate
accumulation is tsc semantics, not rollback debt; the
inference-context NODE STACK stays A-class checkpoint-managed).

### Additional M6-start requirements (review B35)

- Port `clearActiveMapperCaches` (tsc 73624-73629, invoked from
  getInferredType 69310) WITH the inference port.
- Decide ONE revert-twin convention:
  `revert_node_enum_values_computed` (links.rs:861-864) asserts
  speculation_depth while every other revert twin deliberately does
  not — an unwind DURING speculation panics there via
  evaluate.rs:112. Resolve before 7.1.
- Extend the speculation_depth assert net to any remaining raw
  Signature fields and the D-sinks above as the transaction lands.
- `suggestion_count` budget needs rollback (see B).

Field line numbers are review-HEAD (43432368) state.rs coordinates —
re-derive when the struct shifts; the FIELD LIST is the contract.

Commit: `m6 7.0t: speculation transaction` (API + rollback tests
BEFORE 7.1).

**LANDED (this slice) — decisions of record:**
- API: `begin_speculation()` → `SpeculationCheckpoint` +
  `commit_speculation`/`rollback_speculation` + the `speculate`
  closure wrapper (speculate.rs; checkpoint is `#[must_use]` with a
  debug drop-guard, LIFO-asserted). Boundary ordering rule: the
  wrapper rolls back BEFORE re-propagating Err, so outer Err-revert
  twins always fire at entry depth.
- Revert-twin convention (B35, resolved): twins never assert
  speculation_depth — a revert restores pre-write state, always
  legal. `revert_node_enum_values_computed` lost its assert AND its
  depth parameter (now matches every other twin's signature); the
  evaluate.rs:112 panic path is gone, and twins that fire INSIDE a
  speculative region (depth > 0) are correct by design.
- D sinks are TRANSACTION-managed (truncate/restore on rollback,
  keep on commit) — no asserts on sink pushes. deferred_nodes
  VERIFY item resolved: KEEP across rollback (checkNodeDeferred
  86899-86908 registers unconditionally). instantiation_count and
  the flow_analysis_disabled latch also deliberately survive.
- Assert-net extension (B35): the three raw Signature caches —
  `instantiations`, `erased_signature_cache`,
  `optional_call_signature_cache` — now assert depth == 0 at their
  write sites (same message as links assert_writable). The net is
  the 7.4 WIRING INVENTORY: when live trials exercise a site that
  is a category-C permanent truth (tsc writes it during trials),
  relax THAT site with the evidence in hand — no blanket relaxation
  here.
- exhaustive_switch_computing: begin debug-asserts empty (the
  inventory claim) and the checkpoint clone-restores it anyway;
  reduce_label_overrides is clone-snapshot/restored.
- active_type_mappers(+caches): truncate-to-mark only; surviving-
  frame cache staleness under a mutable inference mapper stays 7.3's
  clearActiveMapperCaches (tsc clears at fixing, not on candidate
  failure).
- B34 census widening: UnwindSnapshot (check.rs) now also checks
  shared_flow, reduce_label_overrides, exhaustive_switch_computing,
  inline_level (file-end baseline zeros included).

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

**LANDED (this slice) — decisions of record:**
- Contexts are arena-allocated (`inference_context_arena`,
  E-class — see 7.0t list) with `InferenceContextId` as tsc object
  identity; the 47401-47402 node stack's payload swapped from the
  uninhabited placeholder to the id (stack stays A-class).
- `InferenceInfo.type_parameter` is the TypeParameter TYPE (TypeId),
  NOT its symbol — mapper sources compare type identities
  (getMappedType 63341). core-interfaces §6's SymbolId sketch is
  corrected in place.
- Deferred mappers: `DeferredMapperTargets::{InferenceFixing,
  InferenceNonFixing}(ctx)` dispatch over the context's CREATION-TIME
  capture (`mapper_sources`/`mapper_infos` — tsc's
  makeDeferredTypeMapper sources snapshot and thunk-captured info
  objects, 68258-68278). AMENDED post-review: infos live in their own
  E-class arena (`InferenceInfoId` on CheckerState) so they carry tsc
  object identity — the fixing thunk's isFixed test-and-set rides the
  captured info while clearCachedInferences/getInferredType read the
  LIVE slots, making mergeInferences (80836 `target[i] = source[i]`,
  a slot-id rewrite at 7.4) tsc-exact by construction, including the
  post-merge split (detached capture stays fixed; the fresh live row
  starts unfixed, reopening the 68710 candidate gate). The 7.1
  dynamic-lookup equivalence proof is superseded; the split is pinned
  by test (fixing_dispatch_consults_creation_capture_after_slot_
  replacement).
- `compare_types` is a closed enum (`CompareTypesFn`); only the
  default `Assignable` is constructible until the 7.5 head rebuild
  routes the relation-frame worker (64507) and M8 the conditional
  infer-source context (66368).
- undefined-exactness: `candidates`/`contra_candidates`/`priority`/
  `intra_expression_inference_sites`/`inferred_type_parameters` are
  Option (present ⇒ tsc-created; present candidate vecs are
  non-empty); `implied_arity: Option<usize>`.
- Frontier stubs (escapes owner=M6): `infer_types` → 7.2,
  `get_inferred_type` → 7.3. Production-unreachable: every
  production pushInferenceContext site still passes None until 7.4.
- Consumer wrappers activated 1:1 this slice:
  checkExpressionWithContextualType ORs Inferential for Some
  contexts + clears (not drains) sites (80566-80569);
  instantiateContextualType's real body + instantiateInstantiableTypes
  (73459-73470, NEW port) + hasInferenceCandidatesOrDefault +
  context_flags parameter (only getApparentTypeOfContextualType
  passes real flags; 78813/80570/80726 pass void 0);
  getContextualThisParameterType's 72666 mapper read;
  contextuallyCheckFunctionExpressionOrObjectLiteralMethod's 79174
  mapper instantiation (its Inferential arm = 7.4 escape);
  instantiateTypeWithSingleGenericCallSignature's M4 `unreachable!`
  replaced by the live 80753-80766 probe (getSingleSignature
  75896-75909 NEW port) with the generic body a 7.4 escape.
- Gate evidence: canaries byte-stable at baseline (T0 141/740,
  FP=0, FN=599, mismatches=33); 725 checker tests (13 new pins:
  creation shape, clone independence/flag-OR, inferred-part filter,
  deferred dispatch identity+frontier, fixing order
  (clear-while-unfixed → fix → resolve), sites lazy-add/drain/
  mid-drain-unwind/undrained-clear, outer-return-mapper merge+cache,
  returnMapper branch incl. the 73453 boolean-literal filter,
  Signature-branch non-fixing consult, arena-vs-stack rollback).

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
  inferenceContext.inferredTypeParameters)` (76844). mergeInferences
  is a LIVE-slot id rewrite (`inferences[i] = source_slot`) — the
  mapper capture keeps the creation-time infos, so tsc's post-merge
  split (detached capture fixed, fresh live row unfixed) holds by
  construction (7.1 post-review identity model, pinned by test).
- Context-sensitive argument detection (`isContextSensitive` 63832)
  and the deferred body interaction (M4's driver already defers
  bodies; the re-run is what types their parameters).
- Re-entry protocol (M4-review F7): get_resolved_signature's exit
  write is wrote_sentinel-conditional where tsc 77500-77507 restores
  links.resolvedSignature UNCONDITIONALLY per frame (result, or the
  prior cached value mid-fixpoint) — a re-entrant mid-fixpoint frame
  can leak an inner failure stash over the outer frame's Resolving
  sentinel. Re-derive the port protocol against 77500-77507 when the
  re-run lands.
- Observed while pinning slice 5 (2026-07-20): under
  exactOptionalPropertyTypes, overload selection accepts a candidate
  whose optional-property undefined-vs-string mismatch the
  single-signature path correctly rejects (probed: declared
  `{ a: number; c?: undefined }` argument against overloads
  `{ a; c?: string }` → number / `unknown` → string — port picks #1,
  tsc picks #2). An applicability-relation eOPT gap in
  choose_overload; re-probe when this stage replaces the selection
  loop.

Commit: `m6 7.4: inferTypeArguments + chooseOverload re-run`.

## Stage 7.5: consumers cleanup [M]

Ripple sites that were declared-type-only until now: contextual
tuple/array element inference in literals, generic
constructor/`new` inference, tagged templates, JSX element type
resolution's call path, `satisfies` interplay, and the
2769 failure-path candidate choice (getCandidateForOverloadFailure
with real instantiated candidates). Also owned here (the M3 code
markers say M6; no other milestone schedules it):
`isValidBigIntString` (18973) for bigint template-literal
placeholders (isValidTypeForTemplateLiteralPlaceholder's bigint arm
is a live Unsupported in structural.rs; the annotation-literal
full-radix `parsePseudoBigInt` half landed early — M4-review A14,
slice 1). Also owned here (escaped at M5 6.4f with owner="M6";
scheduled in no earlier stage — the M5 post-close review flagged the
missing doc backing): `compareTypePredicateRelatedTo` (64606) + the
compareSignaturesRelated predicate arm (64577-64592, incl. the
1224/1226-family reporting) — retire structural.rs's
`type_predicate_signature_relation_gate` with it. Decision table to
restore (M4-review B7 — the gate over-contains three of four cells;
its escape reason is accurate only for the both-sides cell; verified
against 64575-64591): related path — the predicate arm sits inside
!ignoreReturnTypes AND after the void/any-target early return;
compareTypePredicateRelatedTo runs ONLY when BOTH sides carry
predicates; target-only: an identifier/this predicate reports the
1224 family and fails, an asserts-form target alone falls through
silently; source-only is a plain return-type comparison (a predicate
signature's return type is boolean); identical path —
compareTypePredicatesIdentical when either side carries one but ONLY
inside !ignoreReturnTypes; every ignoreReturnTypes cell (union
matching, callback pre-gates) consults no predicate at all. The three
port gate sites carry matching notes.

Also here (M4-review B8): rebuild compareSignaturesRelated's HEAD in
tsc order when porting instantiateSignatureInContextOf — tsc
(64487-64514) decides same-reference identity, the top-signature
pair, and sourceHasMoreParameters arity BEFORE generic instantiation,
where the port's early gate contains all of it; make
signature_related_to honor its erase parameter via the live
get_erased_signature (erased sides carry no type parameters, retiring
most of the gate's fire surface); and add signaturesRelatedTo's
same-target-reference PAIRWISE arm (instantiations of one reference
target compare index-to-index, never N×M). The stale inline cite
"66727-66730" at the gate was corrected to 64505-64514 (slice 5).

Re-probe the M4 NOTES top-10 list; retire entries this milestone
fixed.

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
