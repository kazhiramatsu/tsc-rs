# H2.5h-a / CS-4 — comment-scope statement-family routes and the declaration-list writer

Design-gate packet for the third comment-scope production packet, under
the mandatory implementation-ready design gate
(../post-h1-completion-slices.md). Front-run in the `h2/5h-a-cs4`
worktree while CS-3 and gate-tax 2 closed; re-pinned at the CS-4
train's design-gate pass on 2026-08-20 (trusted base `54bbbc03`, the
§12 items closed below). Machine check:
`node .github/ci/slice-readiness.mjs --check h2-5h-a-cs-4`.

## 1. Identity, purpose, and boundary

- **Slice ID:** `h2-5h-a-cs-4`. **Kind:** `foundation` — the printer's
  remaining comment routes move onto tsc's per-side claim predicates
  and the `declaration_list_container_end` writer lands; no new corpus
  behavior is admitted, and the conformance ratchet enforces corpus
  byte-identity.
- **Purpose:** (a) migrate the statement-family claim sites
  (`VariableStatement`, `VariableDeclarationList`,
  `VariableDeclaration`) from the CS-3 transitional flagless
  paired-claim (`statement_paired_container_claim`) onto the exact
  flag-aware per-side predicate (`established_container_sides`);
  (b) land tsc's only `declarationListContainerEnd` producer — inside
  the end-side claim, keyed on `VariableDeclarationList` — activating
  the already-landed `retains_end` dedupe reader; (c) thread the
  remaining contextless emission callers (declaration, class, JSX,
  parameter, transformed-node, substitution, and notification routes)
  through `EmitContext` so every route inherits and claims through the
  one threaded scope.
- **Non-goals (owned later):** deletion of the four
  `detached_transitional` contextless entries and every dual nested
  API (CS-5); the artifact-driven full-pipeline fixture suite and
  zero-contextless audit (CS-6); mixed per-side source positions
  (`pos=-1,end=X`) on one comment range (H2.5h-b metadata packet, per
  the CS-3 §5 legal deferral); any ES2015/Generators production work.
- **Prerequisites:** CS-3 merged with its envelope `ready`; gate-tax 2
  merged (walk adoption — the packet's re-mints ride it); the
  ten-family witness artifact frozen.
- **Trusted base:** `54bbbc0398872131558b4618939637acc7210898` (main
  after the gate-tax 2 merge; re-pinned 2026-08-20 at the train's
  design-gate pass — DRAFT authored at `acf41f78` on the CS-3 branch).
- **Activation state:** before — the statement family claims both
  sides flaglessly from one range; `declaration_list_container_end`
  exists in the scope triple but has no production writer (the
  `retains_end` reader is landed and inert on that side); ~83 emission
  call sites enter through the four contextless transitional entries
  (`emit_required_node` 22, `emit_node_id` 42,
  `emit_identifier_name` 10, `emit_child_after_token` 9). After — the
  statement family claims per side under the flag/JsxText predicate,
  `VariableDeclarationList` writes the declaration-list end exactly
  where tsc does, and the routes named above call the `_with_context`
  entries with the threaded scope; the four transitional entries
  remain only for callers CS-5 deletes with the entries themselves.
  `E-COMMENT-SCOPE-H`, `E-PRINTER-BASE`, `E-PRINTER-G`,
  `E-COMMENTS-G` remain `active-unqualified` (requalify at CS-6).
- **Next owner:** CS-5.
- **Authority artifacts:** re-pin at train start; the witness
  artifact, gap matrix, dispositions, handoff, and pinned
  TypeScript 6.0.3 exactly as CS-3 §1 lists them.

## 2. Required-reference table

| Row | Lifecycle before → after | Current Rust symbols | Role here |
|---|---|---|---|
| `E-COMMENT-SCOPE-H` | `active-unqualified` (unchanged; CS-4 lands the route fourth) | `CommentEmissionScope`, `EmitContext` | the row under implementation |
| `E-PRINTER-BASE` / `E-PRINTER-G` | `active-unqualified` (unchanged) | `EmitContext` and pipeline | invariants preserved premises |
| `E-COMMENTS-G` | `active-unqualified` (unchanged; already `modified-requalify` since CS-3) | cursor/resume machinery, list workers | statement-family projections re-expressed; cursor/resume semantics untouched |
| gap-matrix `comment-scope-threading` | `partial` (re-anchored, §8) | anchors incl. `statement_paired_container_claim` | that anchor is DELETED with its function; per-side anchors extend to the statement family |
| gap-matrix `claim_declaration_list_container` | **`missing` → `exists`** (§8) | none today (asserted absence) | this packet lands the producer |

## 3. Pinned upstream map

Same authority as CS-2/CS-3 §3: the witness artifact's `scope_graph`
spans. The two spans this packet ports beyond CS-3's:

- `emit-leading-comments-of-node` (`_tsc.js:121007-121032`, set-site).
  The complete per-side write, verbatim semantics:
  - outer gate: `(pos > 0 || end > 0) && pos !== end`;
  - `skipLeading = pos < 0 || NoLeadingComments || JsxText`,
    `skipTrailing = end < 0 || NoTrailingComments || JsxText`;
  - leading emission only when `!skipLeading`, with
    `isEmittedNode = kind !== NotEmittedStatement`;
  - `containerPos = pos` when `!skipLeading` OR
    (`pos >= 0` AND `NoLeadingComments`) — a suppression flag claims
    while suppressing;
  - `containerEnd = end` when `!skipTrailing` OR
    (`end >= 0` AND `NoTrailingComments`);
  - **inside the `containerEnd` claim only**:
    `if (node.kind === VariableDeclarationList)
    declarationListContainerEnd = end;` — the single producer this
    packet lands;
  - synthetic leading comments emit after the gate, unconditionally.
- `emit-trailing-comments-of-node` (`_tsc.js:121033-121052`,
  restore-site): synthetic trailing comments first (outside the gate);
  inside the same outer gate the SAVED triple is restored (all three
  fields); trailing emission only when `!skipTrailing` AND
  `kind !== NotEmittedStatement`.

The readers (`forEachLeading/TrailingCommentToEmit`,
`_tsc.js:121219-121238`) are already landed: the leading shared-start
guard and `retains_end` (`end !== containerEnd && end !==
declarationListContainerEnd`, inverted). This packet only activates
the declaration-list side by giving it its producer.

## 4. Rust semantic map

| Upstream construct | Rust target | Notes |
|---|---|---|
| per-side claim at statement-family sites | `established_container_sides(owner)` at the three `NodeData::Variable{Statement,DeclarationList,Declaration}` arms (printer.rs ~2585/2627/2702) | replaces `statement_paired_container_claim`; the predicate function already exists (CS-3) and its `JsxText`/flag arms are unreachable-but-exact for these kinds |
| `declarationListContainerEnd = end` (kind 262 inside end-claim) | new `CommentEmissionScope::claim_declaration_list_sides(pos, end)` | identical to `claim_sides` except the claimed `end` (when `Some`) is also written to `declaration_list_container_end`; used ONLY by the `VariableDeclarationList` arm; the `contract_scope` test hook's "no production writer" comment is retired |
| non-list claims keep the list end alive | `claim_sides` (unchanged) | already preserves `declaration_list_container_end` |
| trailing dedupe at the list end | `retains_end` (unchanged, landed CS-2) | activates with the producer; the escape/list-end workers already consult it |
| remaining route threading | callers of the four `detached_transitional` entries flip to `_with_context` variants; at every final consumption the argument is `expression_context.for_child(ExpressionSyntaxContext::NORMAL)` — the comment scope threads through while the syntax half keeps the exact NORMAL obligations the detached entry provided | **grammar is NOT in scope**: threading the whole context leaks enclosing expression grammars into statement-route children (caught live: for-await head parenthesization under the `__awaiter` generator); the entries survive caller-less with the CS-5 deletion note |
| threading inside a claiming arm | the arm's CLAIMED context (`declaration_context`/`initializer_context`) is the argument for every child emitted under the claim — never the incoming statement context | review-caught latent miswires: the variable statement's modifier list and the declaration's name ran under the unclaimed scope; silent today because those routes do not yet consult the ambient claim, load-bearing once CS-6 tightens |
| substitution/notification re-entries | `before_emit_node`/`substitute_node` call sites re-enter emission with the THREADED context (root sites already `file_root`) | verify no re-entry constructs a detached context |
| `statement_paired_container_claim` | **deleted same-commit** once its three callers migrate | grep-zero in §9 |

## 5. Current local-gap matrix

| Surface | Today | State | Owner |
|---|---|---|---|
| statement-family claim conditions | flagless paired claim from one range | `partial-or-stale` — CS-3's explicitly transitional carry | this packet, step 2 |
| `declaration_list_container_end` producer | none (reader landed, side inert) | `missing` | this packet, step 3 |
| declaration/class/JSX/parameter/transformed-node route contexts | ~83 contextless entries | `partial-or-stale` | this packet, steps 4–5 |
| substitution/notification context fidelity | root-level threaded; nested re-entries unaudited | `partial-or-stale` | this packet, step 5 |
| contextless entry deletion | four named `detached_transitional` constructors | out of scope | CS-5 |
| mixed per-side source positions | unrepresentable by type | `missing` — legal deferral (CS-3 §5) | H2.5h-b metadata packet |

## 6. Implementation sequence

Fence: `crates/emitter/src/printer.rs`,
`crates/emitter/src/comment_cursor.rs`, their unit/focused test trees,
plus the §8 evidence set. Corpus bytes may not change at all (ratchet);
focused expected strings change only under §7's witness rule.

1. **Writer.** `claim_declaration_list_sides` on
   `CommentEmissionScope` + unit contracts (claim-both, claim-end-only,
   claim-none inherits, non-list `claim_sides` preserves the list end).
   Check: cursor unit suite green.
2. **Statement family.** The three arms move to
   `established_container_sides`; the `VariableDeclarationList` arm
   uses the new producer; `statement_paired_container_claim` is
   deleted in the same commit. Check: variable topology focused suite
   byte-stable; deleted-symbol grep zero.
3. **Dedupe activation.** The declaration-list trailing dedupe now
   flows through `retains_end`; the escape/list-end workers'
   behavior must stay byte-identical on the corpus (they already
   consulted the reader). Check: `declaration-list-trailing-dedupe`
   witness family comparisons; corpus ratchet.
4. **Route threading, family by family** (declaration → class →
   parameter → JSX → transformed-node), flipping contextless callers
   to `_with_context` with the enclosing context. Each family lands
   with its focused check before the next starts.
5. **Substitution/notification audit**: every re-entry passes the
   threaded context; add the focused contract that a hook-wrapped
   nested node inherits the enclosing scope.
6. **Chain walk** (CS-2 §8.5 verbatim; with gate-tax 2 merged the
   re-mints adopt in seconds), §8 amendments, envelope/bootstrap/
   index, packet checker, full local gate at the final candidate
   (static `--lane rust` prefix FIRST, per the gate-tax 2 walk
   discipline).

Error behavior: no new `PrinterError` variants, escapes, or panics.

## 7. Frozen witnesses and the output-change rule

Authority: the frozen ten-family comment-scope witness artifact.
CS-4 consumes `declaration-list-trailing-dedupe` (the dedupe
semantics), `emit-flag-suppression` (suppression-claims-while-
suppressing on the migrated statement sites), and
`zero-width-and-not-emitted` (gate/NotEmittedStatement arms);
`synthetic-wrapper-relocation`, `container-start-shared-prefix`,
`delimited-list-starts`, `binary-trampoline-parity`,
`synthetic-comments-alongside-source`, `detached-comment-reroute`,
`type-node-trailing-repetition` are adjacent controls that must stay
byte-stable.

**Output-change rule (fail-closed), verbatim CS-3 §7:** corpus bytes
cannot change at all; a focused expected value may change only when
derivable from a frozen witness case's stored `observation.writes`
bytes (cited by `case_id` in the test comment); an output change with
no covering witness aborts the packet for a witness amendment first.

## 8. Evidence, ratchet, and documentation amendments

1. **Gap matrix**: `claim_declaration_list_container` `missing` →
   `exists` with the `claim_declaration_list_sides` anchor;
   `comment-scope-threading` anchors drop
   `statement_paired_container_claim` and record the statement-family
   migration; note records CS-4 landed, CS-5..6 remaining.
2. **Dispositions**: no row moves expected (`E-COMMENTS-G` is already
   `modified-requalify`); the manifest note records the CS-4 landing.
3. **Architecture map**: `E-COMMENT-SCOPE-H` text records the
   statement-family/writer landing; row IDs unchanged.
4. **Handoff** `h2-5h-a.md`: one amendment sentence → envelope re-pin
   + doc-pinning witness re-mints (adoption makes these seconds).
5. **Chain walk**: CS-2 §8.5b–5d verbatim, printer.rs byte-final
   first; the extended const-sync table (3c/3d/5g-profile/schema +
   5f-qual rule, recorded in the gate-tax 2 packet) applies.
6. **Readiness**: new envelope `h2-5h-a-cs-4` (`ready`, predecessor
   `h2-5h-a-cs-3`), bootstrap `allowedPacketIds += h2-5h-a-cs-4`,
   index row.

## 9. Acceptance

- `cargo test -p tsc-rs-emitter` fully green; every changed expected
  value carries its witness `case_id` citation.
- Deleted-API audit:
  `grep -rn "statement_paired_container_claim" crates` returns
  nothing.
- Migrated-family audit: the per-family contextless-caller counts
  pinned at train start reach zero for the CS-4 families (the four
  entries' remaining callers are exactly CS-5's deletion list).
- Packet checker: the eight `h2-5h-a.md` commands +
  slice-readiness `--check` for cs-2, cs-3, and cs-4, run
  individually.
- Complete local gate green at the final head — conformance
  49024/49024 all bands, FP=0, no ratchet regression, invariants
  full-corpus, A2/A5, escapes/ledger, README freshness — with the
  static `--lane rust` prefix run before the walk.
- Fail-closed: any witness observation drift, corpus byte change,
  un-witnessed focused-output change, or stale artifact outside §8's
  list aborts for a packet amendment.
- Complete when green at one head, PR carries the gate summary, merge
  commit lands; CS-5 opens against this document.

## 10. Traceability

| Upstream invariant | Rust target | Test | Evidence |
|---|---|---|---|
| per-side claims at statement sites | `established_container_sides` at the three arms | variable topology suite | witness `emit-flag-suppression` |
| kind-262-only list-end producer | `claim_declaration_list_sides` | cursor unit contracts | gap-matrix anchor |
| list-end dedupe | `retains_end` (landed) + producer | dedupe comparisons | witness `declaration-list-trailing-dedupe` |
| non-list claims keep list end | `claim_sides` | cursor unit contracts | §3 set-site bytes |
| route context inheritance | `_with_context` threading | per-family focused checks | corpus ratchet |

## 11. Prohibitions

As CS-3 §11: no expected value without a witness `case_id`; no flag
semantics invented beyond §3; no ES2015/Generators production work;
additionally — the four `detached_transitional` entries and the dual
nested APIs are NOT deleted here (CS-5 owns the deletion and its
zero-caller proof), and no `declaration_list_container_end` producer
other than the `VariableDeclarationList` arm.

## 12. Unresolved items (DRAFT — close before the envelope flips ready)

1. ~~Per-family caller census~~ — measured at `acf41f78` (83 sites
   across the four entries; re-verify counts at the re-pinned base):
   statements ≈20 (`Block`, `Try`, `If`, `Switch` + clause helper,
   `For`/`ForIn`/`ForOf`, `With`/while clause, `Break`/`Continue`,
   `ExpressionStatement`, embedded-statement anchor helper);
   declarations ≈15 (`Import`/`Export` declarations + clauses,
   `FunctionDeclaration`/`Expression`, `BindingElement`,
   `emit_modifiers`, renamed specifiers, required-identifier helper);
   class ≈5 (`MethodDeclaration`, `ClassStaticBlockDeclaration`,
   `emit_class`); JSX ≈8 (`JsxElement`/`Fragment`/`SelfClosing`/
   `Opening`/`Expression`); templates + JSDoc type nodes ≈6; the
   remainder sit inside shared helpers whose signatures gain the
   context parameter. Helper-signature threading
   (`emit_case_clause_statements`, `emit_embedded_statement_with_anchor`,
   `emit_modifiers`, `emit_required_identifier_name`) is the §6
   step-4 backbone.
2. ~~Substitution/notification re-entry audit~~ — measured at
   `acf41f78`: seven hook sites total; six are the top-level
   statement/root iterations where the ambient scope is the file
   root's zero scope by construction, and the seventh is the nested
   hook-wrapped emission core, which already receives and threads the
   enclosing `EmitContext` through substitution (the pipelineEmit
   parenthesizer ordering is documented at the site). No hook site
   constructs a detached context; §6 step 5 reduces to adding the
   focused inherits-enclosing-scope contract.
3. ~~Byte-risk review for the statement-family flag migration~~ —
   resolved by predicate analysis: for non-`JsxText` kinds the
   per-side predicate claims exactly the paired claim's sides
   (the flag arms alter EMISSION, never the claim, and the JsxText
   arm is unreachable for `Variable*`), so step 2's only behavioral
   delta is the intended declaration-list dedupe activation, owned by
   the `declaration-list-trailing-dedupe` witness family.
4. ~~Trusted base + authority hashes~~ — re-pinned 2026-08-20 at the
   train's design-gate pass: trusted base
   `54bbbc0398872131558b4618939637acc7210898` (main after the
   gate-tax 2 merge; CS-3 merged @7cc97478 is contained); the §8
   artifact amendments and the chain walk re-mint the authority
   hashes at this base.

## 13. Readiness summary

Upstream rows: the two remaining scope-graph spans at predicate
precision (§3). Rust-map rows: 7 (§4). Gap rows: 6 (§5) — 4 owned
here, 1 CS-5, 1 legal deferral. Witness rows: 3 consumed + 7 adjacent
controls. Architecture impact: none beyond the recorded
`E-COMMENT-SCOPE-H` progress note. Undispositioned 0. Unresolved: 0
(all four §12 items closed; item 4 re-pinned at the train).
