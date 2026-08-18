# H2.5h-a / CS-2 — comment-scope root and core-pipeline packet

Design-gate packet for the first comment-scope production packet (step 2 of
the six-step plan in
[the H2.5h-a handoff](h2-5h-a.md#first-mandatory-design-packet-global-comment-scope)).
This document is the per-packet
[mandatory implementation-ready design gate](../post-h1-completion-slices.md#11-mandatory-implementation-ready-design-gate)
for CS-2 and authorizes exactly the production edits listed in §6. Its
machine check is the readiness envelope
`ratchets/fci-readiness/h2-5h-a-cs-2.v1.json` under the frozen shared
slice-readiness schema/checker
(`node .github/ci/slice-readiness.mjs --check h2-5h-a-cs-2`).

## 1. Identity, purpose, and boundary

- **Slice ID:** `h2-5h-a-cs-2` (packet CS-2 of the H2.5h-a comment-scope
  ladder). **Kind:** `foundation` — a byte-identical printer reshape that
  introduces dormant scope structure; it activates no new corpus behavior
  and admits no new runtime slice.
- **Purpose:** introduce the immutable
  `CommentEmissionScope`/`EmitContext` triple mandated by
  `E-COMMENT-SCOPE-H` at the printer root and core pipeline: the threaded
  context value gains the three independent scoped comment values
  (`container_pos`, `container_end`, `declaration_list_container_end`)
  while every currently qualified comment projection keeps byte-identical
  output.
- **Non-goals (owned by later packets):** migrating expression/list claim
  topology to the independent per-side tsc claim conditions (CS-3);
  migrating statement/declaration/class/JSX/parameter/transformed-node/
  substitution/notification routes and adding the
  `declaration_list_container_end` writer (CS-4); deleting the transitional
  contextless APIs (CS-5); the artifact-driven witness fixture suite and
  the zero-contextless-use audit (CS-6); any ES2015/Generators production
  work (forbidden before CS-6 green).
- **Prerequisites (all frozen):** the h2-5h-a packet envelope is `ready`;
  the comment-scope study and ten-family witness artifact are frozen; the
  owner graph, gap matrix, and disposition manifest are frozen at the
  hashes below.
- **Trusted base:** `6cf96880f5f69394d79546c7b924e7288dd53780`
  (origin/main, PR #455 merge).
- **Activation state:** before — `E-COMMENT-SCOPE-H` `planned`, no scope
  triple exists, `ExpressionEmissionContext` carries one optional paired
  container; after — the triple exists and is threaded through the root
  and core pipeline with `declaration_list_container_end` having no
  writer (typed dormant), `E-COMMENT-SCOPE-H` `active-unqualified`
  (candidate), `E-PRINTER-BASE`/`E-PRINTER-G` `active-unqualified`
  (modified, requalify at the CS-6 final validation ref).
- **Next owner:** CS-3 (expression and list route migration).
- **Authority artifacts (SHA-256 at the trusted base):**
  - `ratchets/h2-5h-a-comment-scope-witnesses.v1.json`
    `e0a1acf5334be55148e0eaeae2c77674d94a5f31411a762b4f67ceb77bd30251`
    (witnesses fingerprint
    `d4fb12ef7ab1080d4668b3ec6fc23563087d457e97746527b0e6fdb26180386f`);
  - `docs/design/greenfield/slices/h2-5h-a-comment-scope.md`
    `70582706258f4235de5bd2233a8726516b1165e68fb40076184632b30c9b9609`;
  - `ratchets/h2-5h-a-owner-graph.v1.json`
    `daf000fb260403f2701c9cbb2e858bab04b299d8208eb7b62b9683601016aaf2`;
  - `ratchets/h2-5h-a-gap-matrix.v1.json`
    `c6ee2462151c5306e05c6bec4eda877b33afda11668d0ef2e51e3333843cfa43`;
  - `ratchets/h2-5h-a-dispositions.v1.json`
    `d1165b7df30b69a8314b27e278ec48e05b1debc9b89c1a2ef73517d3614462cb`;
  - `docs/design/greenfield/slices/h2-5h-a.md`
    `373a4c4c852fa1a09bb31b1b4a01a83c189e3298034122aade46be747b5ed0be`;
  - `crates/emitter/src/printer.rs`
    `ece1dc03ed594ecc64c87607cb3e8bc37d25fb2fef7032986307cc5aee791b16`;
  - `crates/emitter/src/comment_cursor.rs`
    `be9db619edc2d2c43d9b43eaac66b2b816fc038cb8f56faf783839fffa1c49d2`.
  The pinned TypeScript is 6.0.3
  (`vendor/typescript-6.0.3`, source commit
  `050880ce59e30b356b686bd3144efe24f875ebc8`), identified inside the
  witness artifact's `typescript` block.

## 2. Required-reference table

| Row | Lifecycle before → after (this packet) | Validation ref / date | Current Rust symbols (visibility) | Pinned tsc authority | Role here |
|---|---|---|---|---|---|
| `E-COMMENT-SCOPE-H` | `planned` → `active-unqualified` (activate; candidate through CS-6) | design row; study + witnesses frozen 2026-08-17/18 | none yet; this packet lands `crate::comment_cursor::CommentEmissionScope` and `crate::printer::EmitContext` (both `pub(crate)`/private) | witness artifact `scope_graph` spans (§3) | the row this packet implements (root/core third of it) |
| `E-PRINTER-BASE` | `active-qualified` → `active-unqualified` (modified-requalify; returns at the CS-6 final validation ref) | `0653e10d` (2026-08-17) | `tsc_emitter::Printer` (pub); private `crate::printer::{EmissionPlan,ExpressionEmissionContext}` | n/a (integration boundary) | its hook-composition/no-checker-dependency/immutable-planning invariants are preserved premises; its named private context carrier is reshaped |
| `E-PRINTER-G` | `active-qualified` → `active-unqualified` (modified-requalify; same requalification) | `0653e10d` (2026-08-17) | private `crate::printer::{ParameterListParentheses,…}`; “`ExpressionEmissionContext` additions” | n/a | same carrier reshape; arrow/parameter phase order untouched |
| `E-COMMENTS-G` | `active-qualified`, `premise-unchanged` | `0653e10d` (2026-08-17) | `crate::comment_cursor::{CommentCursor,CommentResume,CommentResumeError}`, token-cursor family (`pub(crate)`) | n/a | frozen premise: cursor/resume semantics and the qualified H2.5g projections keep their scope; CS-2 adds a sibling type without touching them |
| `E-COMMENTS-H` | `planned` (activate later; untouched here) | design row | none | n/a | consumer of the completed scope model (H2.5h-b era) |
| `E-POSITIONS` | `active-qualified`, premise here (its H2.5h `modified-requalify` disposition concerns later wrapper producers) | `0653e10d` (2026-08-17) | wrapper/position machinery | n/a | wrapper comment-range ownership consumed unchanged |
| gap-matrix capability `comment-scope-threading` | `partial` (re-dispositioned by this packet's reviewed matrix amendment, §8) | frozen 2026-08-18 | anchors: `CommentCursor`, `token_owned_comment_phase_prefix`; asserted absences listed in §5 | n/a | the capability this packet begins closing |
| dispositions rows 34/35/36/37 (`E-PRINTER-BASE`, `E-PRINTER-G`, `E-COMMENTS-G`, `E-COMMENT-SCOPE-H`) | manifest amendment in this train (§8) | frozen 2026-08-18 | n/a | n/a | rows 34/35 `premise-unchanged` → `modified-requalify`; 36/37 unchanged |

Historical documents (H1 emitter notes, pre-H2.5g comment designs) are
rationale only and are cited nowhere in this packet as current fact.

## 3. Pinned upstream map

Authority: the machine-pinned `scope_graph` block of
`ratchets/h2-5h-a-comment-scope-witnesses.v1.json` (23-line occurrence
set over `vendor/typescript-6.0.3/lib/_tsc.js` plus seven anchored span
hashes), which froze §1 of
[the comment-scope study](h2-5h-a-comment-scope.md). The spans, with the
artifact's `span_id` as the stable identity (each carries
`slice_sha256` over the exact byte slice):

| span_id | Role | `_tsc.js` lines | Declaration |
|---|---|---|---|
| `printer-scope-state` | state | 116957–116959 | `var containerPos = -1; var containerEnd = -1; var declarationListContainerEnd = -1;` in `createPrinter` |
| `pipeline-emit-with-comments` | save/restore carrier (recursive) | 120978–120986 | `pipelineEmitWithComments(hint, node)` |
| `binary-trampoline-carrier` | save/restore carrier (iterative) | 118390–118468 | `createEmitBinaryExpression` `onEnter`/`onExit` |
| `emit-leading-comments-of-node` | set site | 121007–121032 | `emitLeadingCommentsOfNode(node, emitFlags, pos, end)` |
| `emit-trailing-comments-of-node` | restore site | 121033–121052 | `emitTrailingCommentsOfNode(node, emitFlags, pos, end, saved…)` |
| `for-each-leading-comment-to-emit` | guarded reader (leading) | 121219–121233 | `forEachLeadingCommentToEmit(pos, cb)` |
| `for-each-trailing-comment-to-emit` | guarded reader (trailing) | 121234–121238 | `forEachTrailingCommentToEmit(end, cb)` |

Call order and branch predicates (study §1.2–§1.4, frozen): the carrier
saves the triple, `emitCommentsBeforeNode` → `emitLeadingCommentsOfNode`
claims under `(pos > 0 || end > 0) && pos !== end` with independent
per-side skip conditions, `declarationListContainerEnd` is written only
for `VariableDeclarationList`; the child phase runs; the carrier restores
the triple from the saved values **before** the trailing walk, so a
node's trailing phase consults its parent's scope; the two readers guard
`pos !== containerPos` and
`end !== containerEnd && end !== declarationListContainerEnd` with `-1`
as the no-container sentinel. Synthetic comments bypass the triple.
There is no observable failure order: the tsc scope machinery cannot
throw; the only Rust failure surface is the existing
`PrinterError` plumbing, which this packet does not extend.

Function names alone are not identities: every span above is bound by
`slice_sha256` in the frozen artifact, and
`node crates/oracle/h2-5h-a-comment-scope-witnesses.mjs --check`
re-derives all of them from the vendored bundle.

## 4. Rust semantic map

| tsc object / field / sentinel / transition | Rust type & module (target state after CS-2) | Producer | Owner / updater | Consumer | Lifetime & invalidation | Identity / provenance observable? |
|---|---|---|---|---|---|---|
| `containerPos` (`-1` sentinel) | `CommentEmissionScope::container_pos()` view (`Option<CommentCursor>`), `crates/emitter/src/comment_cursor.rs` | `claim_container_unit` (from an established or inherited container) | the scope value itself (immutable; a new value per claim) | leading owned-prefix guards (§5 reader rows) | one emission subtree; replaced by the child's claimed copy, never mutated | no — guard input only |
| `containerEnd` (`-1` sentinel) | `CommentEmissionScope::container_end()` view (`Option<CommentCursor>`) | `claim_container_unit` | immutable value | `retains_end` trailing guards | as above | no |
| `containerPos`+`containerEnd` claimed pair (storage) | `CommentEmissionScope::container: Option<CommentRange>` — the H2.5g projection claims both sides as one unit, including the qualified inert states (synthesized or zero-width inherited ranges) that suppress the parent-end fallback while matching no guard; the paired unit is the exact carrier of that qualified behavior, and the two `-1` sentinels are the `None` views over it. **CS-3 splits this storage into independent per-side values when the per-side claim conditions land** (the view API is the stable consumer surface); until then a per-side split would have no producer and no witness coverage | `claim_container_unit` | immutable value | `container_unit()` round-trips the exact stored value into the deferred-comment stores | as above | no |
| `declarationListContainerEnd` (`-1` sentinel; sole writer = `VariableDeclarationList`) | `CommentEmissionScope::declaration_list_container_end: Option<CommentCursor>` (first-class field) | **none in CS-2** (dormant; CS-4 adds `claim_declaration_list_container` at the list route) | immutable value | `retains_end` includes it in the guard disjunction (always `None` here) | as above | no |
| save/restore of the triple around a child (both tsc carriers) | structural value threading: the parent's `EmitContext` is unchanged by the child's claims; the parent-scope value is what reaches a node's trailing phase | n/a | n/a | every route that today threads `declaration_context`/`expression_context` copies | scope of the borrow | no |
| iterative binary trampoline carrier | none — the Rust printer emits `BinaryExpression` through the same recursive dispatch (`emit_transformed_node_worker`), so the dual topology is one mechanism by construction | n/a | n/a | n/a | n/a | n/a |
| ambient container + syntax pair (`pipelineEmit` context) | `EmitContext { comments: CommentEmissionScope, syntax: ExpressionSyntaxContext }`, private in `crates/emitter/src/printer.rs`; replaces `ExpressionEmissionContext` (its `comment_container: Option<CommentRange>` field is deleted) | `EmitContext::file_root()` at the source-file root; `EmitContext::detached_transitional(...)` at the four legacy contextless APIs and the JSON route | immutable; `for_child`/`with_grammar` preserve `comments` while replacing `syntax`; `with_comments` installs a claimed scope | the whole dispatch (21 `expression_context:` signatures at the base) | one emission subtree | no |
| initial `-1/-1/-1` printer state | `CommentEmissionScope::empty()` (`pub(crate) const`), called only by the two `EmitContext` constructors | root construction | n/a | n/a | printer invocation | no |
| claim condition (established owner range, `pos !== end`, nonzero) | `Printer::comment_phase_established_container(owner) -> Option<CommentRange>` unchanged; its result (or the inherited active range) is applied with `EmitContext::with_claimed_container(active)` = `claim_container_unit` when `Some`, inherited scope when `None` — the exact `Option::or`-then-replace composition of today, with `declaration_list_container_end` preserved through every claim (constant `None` here) | per-node comment-phase owner (`ExpressionCommentPhaseOwner`) | n/a | the eight claim sites (§5) | per node | no |
| trailing guard (`end !== containerEnd && end !== declarationListContainerEnd`) | `CommentEmissionScope::retains_end(end: CommentCursor) -> bool` (true = the enclosing container or active declaration list owns this end; caller suppresses); replaces the deleted `Printer::comment_container_retains_end`, whose source/`Original`/nonempty checks are subsumed by the `container_end()` view returning `None` for every inert state | n/a | n/a | the four trailing-guard sites (§5) | per check | no |
| leading guard (`pos !== containerPos` projection) | unchanged H2.5g projections (`token_owned_comment_phase_prefix`, `parent_comment_container_owned_prefix[_for_owner]`), fed the ambient container as a range via `container_unit()` where a route threads it | n/a | n/a | leading-prefix sites (§5) | per check | no |

Representation notes fixed by this packet: both new types are
`#[must_use]`, `Clone, Copy, Debug, Eq, PartialEq`, and neither derives
nor implements `Default` (the handoff forbids a `Default` nested scope;
`ExpressionSyntaxContext` keeps its existing `Default` because it is not
comment scope). `CommentCursor` equality already carries the source
identity, so every cross-source comparison is fail-safe without new
error variants. `claim_container_unit` replaces the claimed unit and
preserves `declaration_list_container_end` — exactly tsc's non-list
claim shape projected onto the H2.5g paired carrier; the per-side
independent skip conditions of `emit-leading-comments-of-node` are
route semantics and land with the route migrations (CS-3/CS-4), not
here, because in the current qualified projection no reachable claim
sets one side without the other. The inert claimed states (synthesized
or zero-width ranges accepted from the inherited/deferred chain) are
qualified H2.5g behavior that tsc's claim gate would reject; they stay
representable and greppable inside the unit until CS-3 retires them
under the frozen witnesses.

## 5. Current local-gap matrix

Generated from the complete 68-site census of `comment_container` /
`ExpressionEmissionContext::NORMAL` in `crates/emitter/src/printer.rs` at
the pinned hash (grep census re-runnable with
`grep -n "comment_container\|ExpressionEmissionContext::NORMAL" crates/emitter/src/printer.rs`).

| Semantic row | Current Rust symbol (printer.rs lines at the pinned hash) | Class | Evidence / CS-2 step |
|---|---|---|---|
| single recursive emission topology (tsc's two carriers unified) | `emit_transformed_node_worker` recursion incl. `NodeData::BinaryExpression` arm (4977) | `already-exact` | no iterative carrier exists; recorded in §4; no edit |
| structural restore-before-trailing for the declaration family | `declaration_context` copies at 2543/2580/2650; trailing checks at 2633–2640 read the parent-scope value | `already-exact` | preserved verbatim through the type reshape (step 4) |
| ambient context type carrying comments + syntax | `ExpressionEmissionContext` (537–580), field `comment_container: Option<CommentRange>` | `partial-or-stale` — single wholesale-replaced paired range; no third value; `Default` derived | steps 2–3 reshape to `EmitContext`/`CommentEmissionScope`; focused unit contracts |
| root construction of the ambient scope | `ExpressionEmissionContext::NORMAL` at `print_transformed_source_file` 1308 | `partial-or-stale` — root is indistinguishable from the nine non-root constructions | step 4a: `EmitContext::file_root()`, sole zero-scope root |
| contextless nested entries | `NORMAL` at `emit_required_node` 7827, `emit_child_after_token` 7850, `emit_node_id` 8092 (also serving `print_json_source_file` 1119), `emit_identifier_name` 8165 | `partial-or-stale` — scope-dropping APIs; deletion owner CS-5, route migration CS-3/CS-4 | step 4b: constructed via `EmitContext::detached_transitional()` so every drop site is greppable; behavior unchanged |
| wrapper re-entry constructions (fresh syntax, preserved computed container) | `NORMAL.with_comment_container(active…)` at 8267/8274/8312/8473/8527 (+`with_comment_container` composition at 8368/8397) | `partial-or-stale` | step 4c: `EmitContext::for_wrapper(scope)` = fresh `NORMAL` syntax + explicit claimed scope; same values threaded |
| claim composition `established.or(inherited)` | 2541-2544 (`VariableStatement`), 2578-2581 (`VariableDeclarationList`), 2648-2651 (`VariableDeclaration`); `active_expression_comment_container` 10085 feeding 8237/8264/8296/8353/8382/8457/8496; `inherited_expression_comment_container` 10072 | `partial-or-stale` — wholesale `Option::or` on one range with no third-value transit | step 4c: `EmitContext::with_claimed_container(active)` applies `claim_container_unit` when `Some` and keeps the inherited scope when `None`; equivalence proof in §6 step 4 |
| trailing dedupe guard | `comment_container_retains_end` 10059; call sites 5576 (`child_trailing_comments_escape_active_container`; a present-but-inert container also suppresses the parent-end fallback there — preserved via `container_unit()` presence), 9171 (deferred per-node trailing — stays on `CommentRange`, no scope involvement), 11155, 11207 (list-end comments) | `partial-or-stale` — checks one range's end; no declaration-list disjunct | step 4d: `CommentEmissionScope::retains_end` for ambient sites; the per-node deferred check at 9171 uses the shared view helper `CommentEmissionScope::container_end_of(range)` over its `CommentRange` (per-node owner state, not ambient scope) — one source of truth, no dual guard |
| leading owned-prefix guards | `parent_comment_container_owned_prefix` 9205 / `_for_owner` 9061 with call sites 4866/4945/8852/9126; `token_owned_comment_phase_prefix` 9014 | `already-exact` for their qualified H2.5g cases; ambient callers feed them `container_unit()`, logic untouched | step 4e |
| pass-through container parameters | `emit_child_boundary_comments_before_terminator` (5595, used 2563/…), `emit_leading_comments_for_delimited_list_start_in_container[_with_space]` 8818/8834, list/element end-comment workers 11120/11167, `emit_delimited_expression_list` 5936/6037, `emit_call_arguments` 7791/7804, `emit_child_after_token_with_context_and_source_extent` 7919/7926, `emit_required_node_with_context_and_source_extent` 8004, 3175 | `partial-or-stale` — ambient state travels as `Option<CommentRange>` | step 4f: every ambient-container parameter becomes `CommentEmissionScope`; per-node owner ranges stay `CommentRange` (the type split is the review surface) |
| deferred-expression container plumbing | `DeferredExpressionSourceComments.container` (`ExpressionCommentContainer`), `expression_comment_container_range` 9093, `inherited_expression_comment_container` 10072 | `partial-or-stale` — resolves to the same single range | step 4c: the resolved active range feeds `with_claimed_container`; enum untouched (its variants are per-node owners, not ambient state) |
| `declarationListContainerEnd` writer + independent per-side claim conditions | none (asserted absences in the frozen gap matrix) | `missing` — **legal deferral**: outside CS-2's admitted scope; earliest owners CS-4 (writer, statement/declaration routes) and CS-3 (per-side expression claims); reachability guard = the field has no producer and `retains_end`'s third disjunct is constant-`None` (typed, not a silent fallback); adjacent-negative control = witness family `declaration-list-trailing-dedupe` stays oracle-only until CS-4 wires it | tracked in the amended gap matrix (§8) |
| `obsolete` after this packet | `ExpressionEmissionContext` (name), `with_comment_container`, `comment_container_retains_end` | `obsolete` — replacements named above (`EmitContext`, `with_claimed_container`/`with_comments`, `retains_end`+`container_end_of`); all former consumers are the sites in this table; `comment_phase_established_container` survives unchanged as the claim producer | deleted in the same commit; the workspace grep for the three names returning zero matches is part of acceptance |

No `shared-prerequisite` rows exist: every dependency of this packet is
already frozen. This matrix was generated before the Rust design below
was finalized; the post-implementation inventory is CS-6's gate.

## 6. Implementation sequence

**Allowed files:** `crates/emitter/src/comment_cursor.rs`,
`crates/emitter/src/printer.rs`,
`crates/emitter/tests/unit/lib/tests.rs` (the scope unit contracts join
the emitter's already-registered unit module: the H2.5g profile pins its
runtime-input closure at exactly 236 paths, so a **new** test file would
change that frozen identity — a profile-identity change this packet does
not own),
`.github/ci/contracts/h2-5h-a-dispositions.schema.json` (the disposition
count consts pinned by §8.2),
`crates/emitter/tests/source_comment_topology_contract.rs` (only if a
threading assertion needs a named helper; expected strings are never
edited), plus the §8 evidence/mint surface:
`crates/oracle/h2-5h-a-gap-matrix.mjs`,
`crates/oracle/h2-5h-a-dispositions.mjs`,
`ratchets/h2-5h-a-{gap-matrix,dispositions,foundation,comment-scope-witnesses,owner-graph,es2015-generators-witnesses}.v1.json`,
`ratchets/h2-5g-profile.v1.json` (pin rebind only),
`docs/design/greenfield/emitter-architecture.md`,
`docs/design/greenfield/slices/h2-5h-a.md` (step-5 amendment note only),
`docs/design/greenfield/slices/README.md` (index row),
`docs/design/greenfield/slices/h2-5h-a-cs-2.md`,
`ratchets/fci-readiness/h2-5h-a-cs-2.v1.json`,
`ratchets/fci-readiness/h2-5h-a.v1.json` (doc-digest re-pin only),
`ratchets/fci-packet-bootstrap.v1.json` (`allowedPacketIds += h2-5h-a-cs-2`).
**Forbidden:** every other path; in particular
`crates/emitter/src/builtins*`, `crates/emitter/src/transform.rs`,
`crates/checker/**`, `crates/xtask/**`, `.github/workflows/**`, and every
witness **generator observation** (re-mints must reproduce byte-identical
observations; only pin lines may differ, except the reviewed §8 rows).

Steps, in dependency order — each with its completion check:

1. **`CommentEmissionScope` in `comment_cursor.rs`.** Add the struct and
   the API of §4 (`empty`, `claim_container_unit`, `container_unit`,
   `container_pos`, `container_end`, `container_end_of`, `retains_end`),
   `#[must_use]`, no `Default`, doc comments citing
   `printer-scope-state`, `emit-leading-comments-of-node`,
   `emit-trailing-comments-of-node`,
   `for-each-trailing-comment-to-emit` span IDs. Unit contracts in the
   same change: an empty scope retains no end; a claimed scope retains
   exactly its claimed end cursor (source + position both required); a
   synthesized or zero-width claimed unit stays present
   (`container_unit()` `Some`) while both views and the guard return
   nothing; claiming preserves an (artificially constructed)
   declaration-list value while replacing the unit; the guard accepts a
   declaration-list end with no claimed unit. Check:
   `cargo test -p tsc-emitter comment` green.
2. **`EmitContext` in `printer.rs`.** Rename
   `ExpressionEmissionContext` → `EmitContext`; replace the
   `comment_container` field with `comments: CommentEmissionScope`; drop
   the `Default` derive and the `NORMAL` const; keep
   `for_child`/`with_grammar`/`grammar`/`carries_no_asi_left_edge`
   verbatim over the new field; add the three constructors
   (`file_root()`, `detached_transitional()`, `for_wrapper(scope)`) and
   `with_comments(scope)`. Precondition: step 1 merged into the same
   train. Check: `cargo check -p tsc-emitter` — every use site fails
   loudly; steps 3–4 fix them with no `#[allow]` and no fallback arm.
3. **Root wiring.** `print_transformed_source_file` (statement loop,
   line 1308) constructs `EmitContext::file_root()`; this is the only
   `file_root` caller. The statement-level leading/trailing comment walks
   surrounding the loop (1278–1311) stay contextless in this packet
   (CS-4 migrates them); the JSON route keeps `emit_node_id`
   (transitional). Check: focused topology suite unchanged.
4. **Site adaptation** (the §5 rows, all in `printer.rs`):
   a. root (done in step 3);
   b. the four contextless APIs construct `detached_transitional()`;
   c. claim sites: `comment_phase_established_container(owner)` is
      unchanged; every `established.or(inherited)` +
      `with_comment_container(active)` composition becomes
      `context.with_claimed_container(active)` =
      `claim_container_unit(range)` when `Some`, the inherited scope
      when `None` — equivalence with the replaced form: every computed
      `active` value includes the inherited container as its final
      fallback, so `None` occurs only when the inherited container was
      also absent, the claimed unit stores the identical `CommentRange`
      value, and the preserved third field is constant `None` in this
      packet because no writer exists; therefore every reachable value
      is identical;
   d. ambient trailing guards call `scope.retains_end(cursor)` and gate
      fallback suppression on `container_unit()` presence (preserving
      the qualified inert-container behavior); the per-node deferred
      check keeps its `CommentRange` and uses `container_end_of`;
   e. leading owned-prefix helpers keep their `CommentRange` parameters;
      ambient callers feed them `container_unit()`;
   f. ambient pass-through parameters retype to `CommentEmissionScope`;
      the deferred-comment stores receive `container_unit()`, which
      round-trips the exact stored value.
   Postcondition/check: workspace grep zero for
   `ExpressionEmissionContext`, `with_comment_container`, and
   `comment_container_retains_end`;
   `cargo test -p tsc-emitter` fully green with **zero expected-string
   edits** — the reshape is proven byte-identical by the existing suite,
   then by the corpus gate.
5. **Evidence and doc mint sequence** (§8): gap-matrix generator reviewed
   amendment → dispositions generator reviewed amendment → architecture
   map row edits → `h2-5h-a.md` step-5 amendment note → artifact re-mint
   chain in dependency order (`h2-5g-profile` → foundation →
   comment-scope witnesses → owner graph → gap matrix → dispositions →
   ES2015/Generators witnesses; each `--write` must byte-preserve every
   stored observation, and any observation drift aborts the packet) →
   h2-5h-a envelope doc-digest re-pin → CS-2 envelope + bootstrap.
   Check: the eight-command packet checker of `h2-5h-a.md` plus
   `node .github/ci/slice-readiness.mjs --check h2-5h-a-cs-2`, each run
   individually with per-command exit capture.
6. **Full local gate** at the final candidate:
   `taskpolicy -b nice -n 15 cargo xtask ci --baseline 6cf96880f5f69394d79546c7b924e7288dd53780`
   (output to a file, explicit exit-code check; the reviewed
   performance-ceiling resume protocol applies if the demoted run is red
   only on the wall-clock ceiling).

Error behavior: no new `PrinterError` variants, no new escapes, no
`unsafe`, no panics on reachable paths; every adapted site keeps its
existing error plumbing. Transform/pass composition is untouched: this
packet adds no transformer, changes no hook order, and neither reads nor
writes transform flags, node provenance, lexical receivers, generated
names, or source maps; printer expression context changes only as
specified above (the emitter-packet checklist rows of the design gate
are therefore each explicitly “unchanged by construction” except the
comment-scope rows implemented here).

## 7. Frozen witnesses

The CS-2 witness authority is the frozen ten-family artifact
`ratchets/h2-5h-a-comment-scope-witnesses.v1.json` (30 oracle-captured
cases, two fresh-process observations each; §1 hash above).
Reproduction commands:
`node crates/oracle/h2-5h-a-comment-scope-witnesses.mjs --check`
(re-derives every observation and the scope-graph pins from the pinned
bundle) and `--write` (full re-observation; must be byte-identical).

This packet consumes the witness set as follows: the `scope_graph` spans
are the upstream identities for every type and method comment written in
step 1; the family semantics fix the API shape (`retains_end`'s
disjunction, pair-claim, parent-scope trailing). Because CS-2 is
byte-identical by design, its output-level evidence is the unchanged
existing suite plus the corpus gate, not new witness fixtures; the
family-by-family Rust fixture consumption is explicitly owned later:
families `synthetic-wrapper-relocation`, `container-start-shared-child`,
`variable-declaration-list-trailing-dedupe`, and the flag/synthetic/
detached/zero-width families land with CS-3/CS-4 route migrations and
the CS-6 fixture suite, which byte-compares against the artifact's
stored `observation.writes` (never against transcribed strings — the
falsified draft output recorded in the handoff is the standing argument).
New oracle-captured expected strings are not introduced by this packet,
so no new capture commands exist here.

## 8. Evidence, ratchet, and documentation amendments (reviewed surface)

The printer reshape mechanically stales pinned evidence; this section is
the complete reviewed list. Anything beyond it that a `--check` reports
stale aborts the packet for a design amendment.

1. **Gap matrix** (`crates/oracle/h2-5h-a-gap-matrix.mjs`, capability
   `comment-scope-threading`, currently anchors
   `CommentCursor`/`token_owned_comment_phase_prefix` with asserted
   absences `declaration_list_container_end` in `printer.rs` and
   `container_end` in `comment_cursor.rs`): state stays `partial`;
   anchors gain `struct CommentEmissionScope` + `fn claim_container_unit`
   (comment_cursor.rs) and `struct EmitContext` + `fn file_root`
   (printer.rs); the two absences flip to: `claim_declaration_list_container`
   absent from `printer.rs` **and** from `comment_cursor.rs` (the CS-4
   writer API named in §4), keeping the matrix breakable by a premature
   writer landing; the note records packets 3–6 as the remaining owners.
   All other capabilities: pin-line re-mint only.
2. **Dispositions** (`crates/oracle/h2-5h-a-dispositions.mjs`, and the
   count consts in
   `.github/ci/contracts/h2-5h-a-dispositions.schema.json`, which the
   registered artifact-contract table enforces): rows 34
   (`E-PRINTER-BASE`) and 35 (`E-PRINTER-G`) `premise-unchanged` →
   `modified-requalify` with rationale “the private printer context
   carrier is reshaped by the comment-scope packets (CS-2); invariants
   preserved and requalified at the CS-6 final validation ref”, citing
   capability `comment-scope-threading`; every other row byte-identical
   apart from pins. Resulting counts: 14 premise-unchanged /
   17 modified-requalify / 10 activate / 4 future-owned-fail-closed /
   0 proven-unreachable / undispositioned 0.
3. **Architecture map** (`docs/design/greenfield/emitter-architecture.md`):
   `E-PRINTER-BASE` and `E-PRINTER-G` status → `active-unqualified`
   (modification note naming this packet and the CS-6 requalification);
   their symbol lists rename `ExpressionEmissionContext` → `EmitContext`;
   `E-COMMENT-SCOPE-H` status → `active-unqualified` with the landed
   symbols and “root/core (CS-2) landed; CS-3..6 open”. Row IDs are not
   renamed, added, or removed.
4. **Handoff** (`docs/design/greenfield/slices/h2-5h-a.md`): one
   amendment sentence in the step-5 bullet recording the CS-2 manifest
   amendment (new counts, this document as the owning packet); no other
   edit. Consequence: envelope doc-digest re-pin plus the two
   doc-pinning witness artifacts re-mint (pin lines only).
5. **Artifact re-mint chain** (dependency order, each `--write` then
   `--check`, observations byte-identical):
   `ratchets/h2-5g-profile.v1.json` → `h2-5h-a-foundation` →
   `h2-5h-a-comment-scope-witnesses` → `h2-5h-a-owner-graph` →
   `h2-5h-a-gap-matrix` → `h2-5h-a-dispositions` →
   `h2-5h-a-es2015-generators-witnesses`.
6. **Readiness artifacts:** re-pin `ratchets/fci-readiness/h2-5h-a.v1.json`
   `packet.sha256` (doc amendment); add
   `ratchets/fci-readiness/h2-5h-a-cs-2.v1.json` (status `ready`,
   predecessor `h2-5h-a` with its envelope-file receipt SHA-256, allowed
   paths = §6 list, proof commands = §9 acceptance commands);
   bootstrap `allowedPacketIds += h2-5h-a-cs-2`; index row in
   `docs/design/greenfield/slices/README.md`.

## 9. Acceptance

- **Focused tests** (edit-loop, written with the change):
  `cargo test -p tsc-emitter` — the new scope unit contracts plus the
  complete existing emitter suite (topology, factory-transform, printer
  oracle/foundation, token-cursor, contracts) with zero expected-value
  edits.
- **Deleted-API audit:**
  `grep -rn "ExpressionEmissionContext\|with_comment_container\|comment_container_retains_end" crates`
  returns nothing;
  `grep -c "detached_transitional" crates/emitter/src/printer.rs`
  equals the reviewed transitional-site count recorded in the PR body
  (constructor + four APIs), establishing the CS-5 deletion inventory.
- **Packet checker** (all run individually, exit captured):
  the eight `h2-5h-a.md` checker commands plus
  `node .github/ci/slice-readiness.mjs --check h2-5h-a-cs-2`.
- **Complete local gate:**
  `cargo xtask ci --baseline 6cf96880f5f69394d79546c7b924e7288dd53780`
  green at the final candidate head — conformance all/2xxx/syntactic with
  FP=0 and no set/integer-ratchet regression (T0 49024/49024 expected
  unchanged), A2/A5, invariants full-corpus, ledger, escapes
  (`--stale $(cat STAGE)`, no new sites), README-status freshness.
- **Fail-closed condition:** any observation drift in a re-minted witness
  artifact, any conformance byte change, or any stale row outside the §8
  list aborts the packet (design amendment + envelope re-pin before
  resuming).
- **Complete when:** all of the above are green at one head, the PR
  carries the gate summary, and the merge commit lands on `main`; CS-3
  then opens against this document as its predecessor.

## 10. Traceability and resources

| Upstream owner / invariant | Rust target | Focused test | Evidence |
|---|---|---|---|
| `printer-scope-state` triple + `-1` sentinel | `CommentEmissionScope` fields/views | scope unit contracts | gap-matrix anchors (§8.1) |
| `emit-leading-comments-of-node` claim (paired, CS-2-reachable form) | `claim_container_unit` + `with_claimed_container` over `comment_phase_established_container` | claim/preserve unit contracts + topology suite | witness artifact `scope_graph` |
| `emit-trailing-comments-of-node` restore-before-trailing | structural value threading (parent scope reaches trailing checks) | `variable declaration` topology tests unchanged; declaration-family sites 2543/2580/2650 | §5 rows |
| `for-each-trailing-comment-to-emit` guard disjunction | `retains_end` | retains-end unit contracts + list/element end-comment suite unchanged | §5 guard row |
| `for-each-leading-comment-to-emit` guard | unchanged H2.5g projections over `container_unit` | topology + delimited-list suites unchanged | §5 prefix row |
| both tsc carriers = one Rust route | recursive dispatch only | existing binary/topology tests | §4 row |
| root-only initial scope | `file_root` single caller | grep audit in §9 | envelope proof |
| E-PRINTER-BASE invariants (hooks, no checker dependency, immutable planning) | untouched `before/after_emit_node`, `substitute_node`, `EmissionPlan` call graph | `active_transform_contract`, `dependency_direction_contract` | architecture map note (§8.3) |

Resources: full-gate commands run demoted
(`taskpolicy -b nice -n 15`), Cargo build parallelism ≤ 2 in CI as
policy-pinned, the resume journal covers gate re-runs, and the
performance-ceiling exception re-runs at normal priority without raising
any ceiling. Single write owner: one implementer, serial commits on
`h2/5h-a-cs2`; no parallel file ownership needed.

## 11. Prohibitions

No fixture/case-ID or path-specific branches; no output text
substitution; no hand-authored expected output (all expected bytes come
from the frozen artifact or already-reviewed suite literals, none of
which change); no generic fallback converting an unknown branch into
success (`detached_transitional` is a named, greppable, to-be-deleted
constructor, not a silent default — `Default` is removed); no
inheritance of stale flags/state without the pinned justification in §4;
no edit to witness observations; no ES2015/Generators code.

## 12. Unresolved items

None. Every open question found during research was resolved into §4–§8
(carrier placement, claim/guard equivalences, deferral guards for the
declaration-list writer, evidence cascade). Should implementation reveal
a new owner, data-model decision, observable, or required file, the rule
of the design gate applies: stop, amend this packet, re-pin the
envelope, rerun `--check h2-5h-a-cs-2`, then resume.

## 13. Readiness summary

Authority hashes: §1 (eight artifacts/files + pinned TypeScript).
Reachable upstream rows: 7 pinned spans (§3), all mapped. Rust-map rows:
9 (§4). Local-gap rows: 13 (§5) — 3 `already-exact`, 8
`partial-or-stale` (each mapped to a §6 step and §9 test), 1 legal
deferred `missing` (guarded, CS-3/CS-4 owners named), 1 `obsolete` set
(replacements + consumers named). Witness rows: 10 families frozen, CS-2
consumption defined (§7). Architecture concerns/gaps touched: 6 rows +1
capability +2 manifest rows (§2, §8) — classifications: `E-COMMENT-SCOPE-H`
activate; `E-PRINTER-BASE`/`E-PRINTER-G` modified-requalify;
`E-COMMENTS-G`/`E-POSITIONS` premise-unchanged; `EA-GAP-MAPS-DECLS`
untouched (`future-owned-fail-closed` under H2.6/H2.7 as frozen).
Lifecycle transitions: `planned → active-unqualified` (E-COMMENT-SCOPE-H)
and `active-qualified → active-unqualified` ×2 with the CS-6
requalification path — all legal. Undispositioned rows: 0. Unresolved
rows: 0. Check command:
`node .github/ci/slice-readiness.mjs --check h2-5h-a-cs-2` (envelope
`ready`, this document's digest pinned), plus the §9 acceptance set.
