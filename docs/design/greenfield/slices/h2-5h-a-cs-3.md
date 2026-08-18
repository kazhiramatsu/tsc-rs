# H2.5h-a / CS-3 — comment-scope expression and list route packet

Design-gate packet for the second comment-scope production packet (step 3
of the six-step plan in
[the H2.5h-a handoff](h2-5h-a.md#first-mandatory-design-packet-global-comment-scope)),
under the
[mandatory implementation-ready design gate](../post-h1-completion-slices.md#11-mandatory-implementation-ready-design-gate).
Machine check: envelope `ratchets/fci-readiness/h2-5h-a-cs-3.v1.json`
(`node .github/ci/slice-readiness.mjs --check h2-5h-a-cs-3`).

## 1. Identity, purpose, and boundary

- **Slice ID:** `h2-5h-a-cs-3`. **Kind:** `foundation` — the printer's
  expression/list comment routes move onto tsc's per-side claim
  predicates; no new corpus behavior is admitted, and the conformance
  ratchet enforces corpus byte-identity.
- **Purpose:** replace the CS-2 transitional claimed-unit storage with
  the independent per-side `containerPos`/`containerEnd` values, land
  the exact flag-aware per-side claim conditions of
  `emitLeadingCommentsOfNode` on the expression and list routes (Call,
  New, Array, Object, Spread, the delimited-list engine, and the
  deferred/wrapper/no-ASI expression spine), and retire the
  present-but-inert container states in favor of tsc's inheritance
  semantics.
- **Non-goals (owned later):** statement, declaration, class, JSX,
  parameter, transformed-node, substitution, and notification route
  migration plus the `declaration_list_container_end` writer (CS-4);
  deletion of the four `detached_transitional` contextless entries
  (CS-5); the artifact-driven full-pipeline fixture suite and
  zero-contextless audit (CS-6); mixed per-side source positions on one
  comment range (deferral row in §5); any ES2015/Generators production
  work.
- **Prerequisites:** CS-2 merged (`9e6235bc`, PR #456) with its envelope
  `ready`; the ten-family witness artifact and the scope study frozen.
- **Trusted base:** `9e6235bc300a68a1bf4961aebc12a29bb19a78ee`
  (origin/main, PR #456 merge).
- **Activation state:** before — the scope stores one claimed unit,
  claims are whole-unit with inert synthesized/zero-width states, and
  route claim conditions ignore emit flags; after — per-side storage
  and predicates on the expression/list routes, no inert states
  anywhere, statement-family sites keeping their current flagless
  paired-claim semantics through the new storage (their flag-aware
  migration is CS-4). `E-COMMENT-SCOPE-H`, `E-PRINTER-BASE`,
  `E-PRINTER-G` remain `active-unqualified` (requalify at CS-6);
  `E-COMMENTS-G` moves `active-unqualified` in this packet (§8).
- **Next owner:** CS-4.
- **Authority artifacts (SHA-256 at the trusted base):**
  `crates/emitter/src/printer.rs`
  `9c3fdf49ba0f8725211c526c2fadad5ff83af518f4163f65a2d560518efe7b01`;
  `crates/emitter/src/comment_cursor.rs`
  `3a3339c24a0ef0b0ff2f5d5e0a76fdef3f2b56ab231e077c0099e95b759e1037`;
  `docs/design/greenfield/slices/h2-5h-a-cs-2.md`
  `3b0efdeb6a866650a230ef72cbfa40dee845f106bcd4fd8af99e4366b0bd3c75`
  (predecessor packet; its §8 chain-walk method is normative here);
  `ratchets/h2-5h-a-comment-scope-witnesses.v1.json`
  `637efd50acde9a016a4fe3695baa9ff258c67fdc101dff0544e4d0b2ae91c1f3`
  (fingerprint
  `953d1bb33ace4064e8910026e6b819709db9d2f3fe86fca74a24d9ea320a86aa`);
  `ratchets/h2-5h-a-gap-matrix.v1.json`
  `58aca26f9e6e7d4011a12e90f691ff8d390a9797d22f571f0b44a2ddb9b31712`;
  `ratchets/h2-5h-a-dispositions.v1.json`
  `9ce6e7d01edcdb104d06241d0d2d8fdc8f47988a752819dc898a0077f89f825a`;
  `docs/design/greenfield/slices/h2-5h-a.md`
  `4b79e4ab0ec2737b2885542fbc5da23c5118fe6e1e931e61dde02e8d82e241b8`;
  pinned TypeScript 6.0.3 as identified in the witness artifact.

## 2. Required-reference table

| Row | Lifecycle before → after | Validation ref | Current Rust symbols | Role here |
|---|---|---|---|---|
| `E-COMMENT-SCOPE-H` | `active-unqualified` (unchanged; CS-3 lands the route third) | CS-2 landed 2026-08-18 | `crate::comment_cursor::CommentEmissionScope`, `crate::printer::EmitContext` | the row under implementation |
| `E-PRINTER-BASE` / `E-PRINTER-G` | `active-unqualified` (unchanged) | last qualified `0653e10d` | `EmitContext` and pipeline | invariants preserved premises |
| `E-COMMENTS-G` | `active-qualified` → **`active-unqualified` (modified-requalify)** | `0653e10d` (2026-08-17) | `SourceLeadingCommentPhaseVisit`, the qualified expression/list projections (`token_owned_comment_phase_prefix`, list-start family, element/list-end workers), `CommentCursor`/`CommentResume` | **this packet edits the row's qualified route projections**; cursor/resume semantics untouched; requalifies at the CS-6 final validation ref |
| `E-POSITIONS` | premise, unchanged | `0653e10d` | wrapper/position machinery | wrapper comment ranges consumed as-is |
| dispositions row 36 (`E-COMMENTS-G`) | `premise-unchanged` → `modified-requalify` (§8 amendment) | frozen 2026-08-18 | n/a | the manifest follows the map |
| gap-matrix `comment-scope-threading` | `partial` (re-anchored, §8) | frozen 2026-08-18 | anchors incl. `claim_container_unit` | `claim_container_unit` anchor is REPLACED by the per-side producer anchors; absences unchanged (`claim_declaration_list_container` stays CS-4's) |

## 3. Pinned upstream map

Same authority as CS-2 §3: the witness artifact's `scope_graph` spans.
CS-3 consumes, at full predicate precision,
`emit-leading-comments-of-node` (`_tsc.js:121007-121032`, slice hash
`ce6bf342a94094cccc4bf56debcb99390c8e232705263609dfcf068589284ebb`):

```text
skipLeading  = pos < 0 || (flags & NoLeadingComments)  || kind === JsxText
skipTrailing = end < 0 || (flags & NoTrailingComments) || kind === JsxText
if ((pos > 0 || end > 0) && pos !== end) {
  containerPos = pos   iff  !skipLeading  || (pos >= 0 && NoLeadingComments)
  containerEnd = end   iff  !skipTrailing || (end >= 0 && NoTrailingComments)
  (declarationListContainerEnd: VariableDeclarationList only — CS-4)
}
```

plus the two guarded readers (`for-each-leading/trailing-comment-to-emit`)
and the restore ordering already structural since CS-2. Derived facts the
Rust design must reproduce:

- For a claim range representable in Rust (`SourceRange::Original`, both
  positions present), a side goes unclaimed **iff** the node is
  `JsxText` without that side's suppression flag; a suppression flag
  *claims while suppressing emission*. A `Synthesized` range or a
  zero-width/at-zero range (`(pos > 0 || end > 0) && pos !== end` false)
  claims **nothing — the enclosing scope stays active** (inheritance).
  The CS-2 inert states modeled the qualified H2.5g behavior instead;
  retiring them is this packet's one semantic delta (§5, §7).
- tsc additionally allows mixed per-side positions (`pos = -1` with a
  real `end`, via `setCommentRange`). Rust's `CommentRange` wraps
  `SourceRange`, which is `Original(both)` or `Synthesized` — the mixed
  state is **unrepresentable by type**, giving the §5 deferral its
  structural guard.

## 4. Rust semantic map

| tsc object / transition | Rust target (after CS-3) | Producer / consumer | Notes |
|---|---|---|---|
| `containerPos` | `CommentEmissionScope::container_pos: Option<CommentCursor>` (real field) | `claim_sides`; leading owned-prefix guards | view methods stay; storage becomes per-side |
| `containerEnd` | `CommentEmissionScope::container_end: Option<CommentCursor>` (real field) | `claim_sides`; `retains_end` | |
| `declarationListContainerEnd` | field unchanged, still writer-less | CS-4 | absence `claim_declaration_list_container` still enforced by the gap matrix |
| per-side claim application | `CommentEmissionScope::claim_sides(pos: Option<CommentCursor>, end: Option<CommentCursor>) -> Self` — each `Some` side replaces, each `None` side inherits; declaration-list field preserved | route claim sites | replaces `claim_container_unit` (deleted) |
| the predicate table (§3) | `Printer::established_container_sides(owner: ExpressionCommentPhaseOwner) -> (Option<CommentCursor>, Option<CommentCursor>)` — outer gate + per-side flag/JsxText conditions over `owner.range`/`owner.flags`/`owner.kind` | expression/list claim sites | `comment_phase_established_container` (range form, flagless) SURVIVES for the statement-family sites only, renamed `statement_paired_container_claim` to mark the CS-4 migration surface |
| enclosing-scope capture for deferred children | `ExpressionCommentContainer::{Node(TransformNode), Scope(CommentEmissionScope)}` — `Scope` replaces the `Range` variant; `Node` stays the lazy parent claim, resolved at consumption through `established_container_sides` over the parent's owner | deferred stores 7976/7983/8061 and `nested` | the deferred value IS the saved scope; both consumers become the two guarded readers |
| deferred leading guard (`pos !== containerPos`) | resolved scope's `container_pos()` equality against the owner start (replacing the range/start comparison inside `parent_comment_container_owned_prefix_for_owner` for the deferred path) | `emit_deferred_expression_leading_comments` | the prefix-resume construction (trivia skip) is unchanged |
| deferred trailing guard | resolved scope's `retains_end(owner_end)` | `emit_deferred_expression_trailing_comments` | replaces `container_end_of` on a bare range there |
| active-claim composition | `active_expression_comment_scope(deferred, ctx, owner) -> CommentEmissionScope` = (deferred scope if stored else `ctx.comments()`) `.claim_sides(established_container_sides(owner))` | the 8422/8451 applications, `for_wrapper` sites, `nested` | replaces `active_expression_comment_container`/`inherited_expression_comment_container` returning ranges |
| inert states | **deleted** — a `Synthesized`/zero-width established or inherited value claims nothing and inherits | escape checks 5633 gate becomes `container_end().is_some()` | §7 verification |
| `container_unit()` round-trip | deleted with the unit; the deferred stores capture `ctx.comments()` directly | | |

Statement-family sites (2601/2638/2708 and their pass-throughs 2627/2683/
2697) keep byte-identical semantics: `with_comments(match
statement_paired_container_claim(owner) { Some(range) =>
ctx.comments().claim_sides(pos(range), end(range)), None =>
ctx.comments() })` — full pairs only, no flags, exactly today's values;
their flag-aware migration is CS-4's.

## 5. Current local-gap matrix

Census basis: the CS-2 packet's 68-site map re-verified at the trusted
base (`grep -n "with_claimed_container\|for_wrapper(\|container_unit()\|claim_container_unit\|comments()" crates/emitter/src/printer.rs`).

| Semantic row | Current symbol (lines at base) | Class | CS-3 step |
|---|---|---|---|
| per-side storage | `CommentEmissionScope.container: Option<CommentRange>` | `partial-or-stale` (paired unit) | step 1 splits the fields |
| per-side claim predicates | none (`with_claimed_container` is whole-unit, flagless) | `missing` | step 2 `established_container_sides` + unit contracts per predicate row |
| inheritance on unclaimable ranges | inert-presence semantics at `claim_container_unit` + the 5633 presence gate + list-end workers | `partial-or-stale` — qualified H2.5g behavior, not tsc's | steps 3–4; §7 witness families 1/2/6/10 |
| deferred capture as scope | `ExpressionCommentContainer::Range(CommentRange)` + `container_for_parent` + range-based consumers 10120 region | `partial-or-stale` | step 5 |
| expression/list route claims (Call 7861-era `emit_call_arguments`, delimited engine 5903-era, Spread arm, wrapper/no-ASI spine 8324-8581) | claim sites listed in §6 | `partial-or-stale` | step 6 |
| statement-family paired claims | 2601/2638/2708 | `already-exact` for this packet (semantics preserved through new storage; renamed producer marks CS-4's surface) | step 7 |
| mixed per-side source positions (`pos=-1,end=X` comment ranges) | unrepresentable: `SourceRange` is `Original`-both or `Synthesized` | `missing` — **legal deferral**: outside admitted scope; earliest owner = the H2.5h-b metadata packet that first needs `setCommentRange` with partial positions; reachability guard = the type cannot express it, so landing a producer requires a type change that breaks this row visibly; adjacent-negative control = witness family `emit-flag-suppression` pins the flag-suppressed-yet-claimed neighbor | tracked in §8 matrix note |
| `retains_end`/`container_pos_of`/`container_end_of` views | comment_cursor.rs | `already-exact` (survive as the stable API) | — |
| `obsolete` after CS-3 | `claim_container_unit`, `container_unit`, `EmitContext::with_claimed_container`+`container_unit`, `active_expression_comment_container`, `inherited_expression_comment_container`, `expression_comment_container_range`, `ExpressionCommentContainer::Range` | `obsolete` — replacements in §4 | deleted same-commit; grep-zero in §9 |

No `shared-prerequisite` rows. `undispositioned = 0`.

## 6. Implementation sequence

**Allowed files:** `crates/emitter/src/comment_cursor.rs`,
`crates/emitter/src/printer.rs`,
`crates/emitter/tests/unit/lib/tests.rs`,
`crates/emitter/tests/source_comment_topology_contract.rs` (expected
strings may change ONLY under §7's witness rule), plus the CS-2 §8
evidence surface verbatim (oracle generators/ratchets/schema consts/
harness pins/architecture map/`h2-5h-a.md` note/envelopes/bootstrap/
index) with `docs/design/greenfield/slices/h2-5h-a-cs-3.md` and
`ratchets/fci-readiness/h2-5h-a-cs-3.v1.json` added. **Forbidden:**
everything else, notably `crates/emitter/src/builtins*`, checker, xtask,
workflows, witness observations.

Steps (each with its completion check):

1. **Storage split** (comment_cursor.rs): fields
   `container_pos`/`container_end`; `claim_sides`; views become field
   reads; `contract_scope` gains the per-side form; delete
   `claim_container_unit`/`container_unit`. Check:
   `cargo test -p tsc-rs-emitter --lib` for the scope contracts
   (updated same-commit: per-side replacement, None-side inheritance,
   declaration-list preservation, cross-source guard).
2. **Per-side producer** (printer.rs):
   `established_container_sides(owner)` implementing §3's table;
   `statement_paired_container_claim` (renamed flagless survivor).
   Unit contracts: one row per predicate case — both-claimed,
   JsxText-unflagged (neither), JsxText+NoLeading (pos claimed),
   JsxText+NoTrailing (end claimed), flag-suppressed-but-claimed,
   synthesized none, zero-width none, at-zero-start none.
3. **Scope-level composition**: `active_expression_comment_scope` +
   `EmitContext::with_comments` application at 8422/8451; `for_wrapper`
   takes the composed scope; the 5633 presence gate reads
   `container_end().is_some()`.
4. **Inert retirement**: delete the inert acceptance path; the escape
   checks and both list-end workers now see inheritance. Check: topology
   suite — every diff vs the CS-2 baseline must satisfy §7.
5. **Deferred capture**: `ExpressionCommentContainer::Scope`; stores at
   7976/7983/8061 capture `expression_context.comments()`; `nested`
   takes the composed scope; consumers per §4. Check: factory-transform
   + topology suites.
6. **Route sweep**: `emit_call_arguments`, `emit_delimited_expression_list`,
   Spread arm, wrapper/no-ASI spine — claim sites move to
   `established_container_sides`; pass-through `comments()` threading is
   already in place from CS-2. Check: full emitter suite.
7. **Statement-family preservation**: the three sites re-expressed per
   §4's paired form; byte-identity asserted by the unchanged
   `variable`-family topology tests and the corpus gate.
8. **Ledger + evidence**: every new/edited `tsc-port` header carries
   `tsc-hash` (ledger derivation); then, with printer.rs byte-final,
   replay the CS-2 §8 chain walk (`walk.sh`/`battery.sh`/
   `patch_consts.py`; 1a-qualification before 1a-profile; pin-only bulk
   verification; historical base-failing checks untouched), the §8
   amendments below, and the envelope/bootstrap/index additions.
9. **Full local gate** at the final candidate
   (`cargo xtask ci --baseline 9e6235bc300a68a1bf4961aebc12a29bb19a78ee`,
   demoted per [[load-control]] discipline: `taskpolicy` on every line,
   maintenance escalation when the user is present; identical env on
   every resume; normal-priority resume for the perf ceiling only).

Error behavior: no new `PrinterError` variants, escapes, or panics.
Transform/pass composition, hooks, provenance, names, source maps:
untouched. The emitter-packet checklist rows are each unchanged by
construction except the comment-scope rows above.

## 7. Frozen witnesses and the output-change rule

Authority: the frozen ten-family artifact (§1 hash; reproduction
`node crates/oracle/h2-5h-a-comment-scope-witnesses.mjs --check|--write`).
CS-3 consumes families `synthetic-wrapper-relocation`,
`container-start-shared-prefix`, `delimited-list-starts`,
`emit-flag-suppression`, and `zero-width-and-not-emitted` as the
semantic authority for the inheritance/per-side behavior;
`declaration-list-trailing-dedupe` stays CS-4;
`binary-trampoline-parity`, `synthetic-comments-alongside-source`,
`detached-comment-reroute`, `type-node-trailing-repetition` are
adjacent controls that must stay byte-stable.

**Output-change rule (fail-closed):** the conformance ratchet pins the
whole corpus, so corpus output cannot change at all. For the focused
suites: a topology/unit expected value may change **only** when the new
value is derivable from a frozen witness case's stored
`observation.writes` bytes for the same construct (cited in the test's
comment by `case_id`), and every unchanged family byte-compares as
before. An output change with no covering witness case aborts the
packet: freeze the missing witness first (generator amendment = reviewed
observation change), then resume. Expected values are never authored
from reasoning about tsc.

## 8. Evidence, ratchet, and documentation amendments

1. **Gap matrix** generator: capability `comment-scope-threading`
   anchors swap `fn claim_container_unit` →
   `fn claim_sides` (comment_cursor.rs) and add
   `fn established_container_sides` (printer.rs); state stays `partial`;
   absences unchanged; note records CS-3 landed, CS-4..6 remaining.
2. **Dispositions** generator + schema consts: row 36 `E-COMMENTS-G`
   `premise-unchanged` → `modified-requalify` ("its qualified
   expression/list comment projections are re-expressed on the per-side
   scope by CS-3; cursor/resume semantics preserved; requalifies at the
   CS-6 final validation ref", capability `comment-scope-threading`);
   counts 14→13 / 17→18; schema consts follow.
3. **Architecture map**: `E-COMMENTS-G` status → `active-unqualified`
   with the modification note; `E-COMMENT-SCOPE-H` text records the
   per-side landing; row IDs unchanged.
4. **Handoff** `h2-5h-a.md`: one amendment sentence (CS-3 manifest
   amendment, new counts) → envelope re-pin + doc-pinning witness
   re-mints, as in CS-2.
5. **Chain walk**: the CS-2 §8.5b–5d list verbatim (omissions →
   emit-qualification → transition → 1a-qualification → 1a-profile →
   ladder → 3c/3d → 5g profile + schema consts → 5g-qualification
   (`--write` only; observation reuse) → six H2.5h-a artifacts → harness
   pins), printer.rs byte-final first, bulk pin-only verification, and
   the base-classification rule for anything unexpected.
6. **Readiness:** new envelope `h2-5h-a-cs-3` (`ready`, predecessor
   `h2-5h-a-cs-2` with its envelope-file receipt), bootstrap
   `allowedPacketIds += h2-5h-a-cs-3`, index row.

## 9. Acceptance

- `cargo test -p tsc-rs-emitter` fully green; every changed expected
  value carries its witness `case_id` citation per §7.
- Deleted-API audit:
  `grep -rn "claim_container_unit\|container_unit\|with_claimed_container\|active_expression_comment_container\|inherited_expression_comment_container\|expression_comment_container_range" crates`
  returns nothing.
- Packet checker: the eight `h2-5h-a.md` commands +
  `node .github/ci/slice-readiness.mjs --check h2-5h-a-cs-2` +
  `--check h2-5h-a-cs-3`, run individually.
- Complete local gate green at the final head — conformance
  49024/49024 all bands, FP=0, no ratchet regression, invariants
  full-corpus, A2/A5, escapes/ledger, README freshness.
- Fail-closed: any witness observation drift, any corpus byte change,
  any un-witnessed focused-output change, or any stale artifact outside
  §8's list aborts for a packet amendment.
- Complete when green at one head, PR carries the gate summary, merge
  commit lands; CS-4 opens against this document.

## 10. Traceability and resources

| Upstream invariant | Rust target | Test | Evidence |
|---|---|---|---|
| §3 per-side predicate table | `established_container_sides` | predicate-row unit contracts | gap-matrix anchor |
| None-side inheritance | `claim_sides` | scope unit contracts | §8.1 |
| unclaimable ⇒ enclosing scope active | inert retirement at escape/list-end sites | topology families 1/2/10 comparisons | witness artifact |
| flag-suppressed-yet-claimed | producer flag arms | `emit-flag-suppression` alignment | witness artifact |
| deferred value = saved scope | `ExpressionCommentContainer::Scope` + two readers | deferred/topology suites | §8.1 |
| statement-family byte-identity | `statement_paired_container_claim` | unchanged variable topology tests + corpus | ratchet |

Resources: [[load-control]] discipline throughout; single writer, serial
commits on `h2/5h-a-cs3`; the chain walk budgeted once.

## 11. Prohibitions

As CS-2 §11, plus: no expected value authored without a witness
`case_id`; no flag semantics invented beyond §3's table; no
`declaration_list_container_end` producer.

## 12. Unresolved items

None. The CS-2 draft's open question (deferred-store adaptation) is
resolved in §4: the deferred value is the saved enclosing scope
(`Scope` variant), because both of its consumers are exactly tsc's two
guarded readers; the `Node` variant remains the lazy parent claim
resolved through the same per-side producer. Should implementation
surface a new observable or data-model fact, the amend rule applies.

## 13. Readiness summary

Authority hashes: §1 (seven artifacts/files + pinned TypeScript).
Upstream rows: the §3 span at predicate precision + the two readers.
Rust-map rows: 11 (§4). Gap rows: 9 (§5) — 2 `already-exact`, 4
`partial-or-stale`, 2 `missing` (one implemented in step 2, one legal
type-guarded deferral), 1 `obsolete` set. Witness rows: 10 families, 5
consumed + 4 adjacent controls + 1 deferred (CS-4). Architecture
impact: `E-COMMENTS-G` modified-requalify (map + manifest amendments);
others unchanged. Undispositioned 0. Unresolved 0. Check:
`node .github/ci/slice-readiness.mjs --check h2-5h-a-cs-3` + §9.
