# H2.5h-a — ES2015/Generators architecture and design packet

Readiness: **packet machine-checked ready (status: `ready`,
2026-08-18)** — envelope `ratchets/fci-readiness/h2-5h-a.v1.json` under
the frozen shared slice-readiness schema/checker; prerequisite-transition
steps 1-7 are all complete: the disposition manifest, the step-6
ES2015/Generators witness freeze (W-H2.5H), and the step-7 checker run
and envelope flip recorded below. Production work still requires the
per-packet design gates listed in the packet ladder: the comment-scope
packets CS-2..CS-6 come first, and no ES2015/Generators production
work may precede CS-6 green.

This is the concrete handoff for the first post-FCI emitter/H2 design-gated
work. It is not an implementation specification yet and authorizes no
production edit.
Its readiness and active-slice status are owned by the
[slice-packet index](README.md) and the
[post-H1 schedule](../post-h1-completion-slices.md), not by this draft in
isolation.

Under the recorded Option A roadmap review (2026-08-17), the Functional-CI
packet tail is paused and is not a dependency of this slice: H2.5h-a may
begin on its own design branch now. What carries over from the interlock is
the machinery, not the ordering — the frozen shared slice-readiness
schema/checker (already validated inside the CI oracle suite) is the vehicle
for this packet's readiness manifest, and the mandatory design gate below is
unchanged. The final H2.5g architecture lineage, complete owner/local-gap
inventories, Rust mapping, and frozen H2.5h witnesses still authorize
implementation; nothing here waives them.

For the future packet, vendored TypeScript 6.0.3 owns semantics. The current
code and tests plus freshly revalidated rows in
[the current emitter architecture](../emitter-architecture.md) own Rust facts.
Earlier H1, residual, inventory, and emitter-design documents provide rationale
or history only; they are not implementation instructions.

## Prerequisite transition

The packet-control bootstrap froze the shared packet checker/schema before
the Option A pause; that shared contract is the readiness vehicle this slice
reuses. The H2.5h-a design branch performs these steps in order:

1. verify that every qualified H2.5g row in
   [the current emitter architecture](../emitter-architecture.md) names the
   immutable implementation/evidence commit as its final validation ref, then
   record the actual merge ref separately as delivery lineage and prove that
   it contains that validation ref with every profile-bound runtime/evidence
   input byte-identical; never substitute the merge ref for the validated
   commit;
2. generate the complete pinned `transformES2015`, `transformGenerators`,
   resolver, syntax-fact, factory, helper, printer, comment, and module
   composition owner graph;
3. generate the current Rust local-gap matrix without editing production code;
4. decide owner SCC boundaries and amend H2.5h-a/H2.5h-b suffixes before
   assigning implementation files;
5. use the already frozen shared slice-readiness schema/checker to add the
   H2.5h-a manifest and replace this blocked handoff with the complete exact
   packet required by
   [the mandatory design gate](../post-h1-completion-slices.md#11-mandatory-implementation-ready-design-gate);
   if that shared contract itself must change, stop and close a separate
   amendment packet before creating the H2.5h-a packet;
6. freeze oracle-produced positive, adjacent-negative, composition, and fault
   witnesses; and
7. run the packet checker. Production work begins only when the checker
   reports fresh hashes, zero undispositioned rows, zero unresolved rows,
   full architecture/upstream/local-gap/step/test coverage, and legal
   lifecycle transitions (the paused Functional-CI tail is not a
   prerequisite under the recorded Option A review).

Prerequisite-transition progress:

- Step 1 **verified (2026-08-17)**: every qualified H2.5g row in the
  architecture map names the immutable final validation ref `0653e10d`
  with the merge ref `507a96ac` recorded as delivery lineage only
  (architecture map §1), and the ancestry proofs
  (`507a96ac` ⊂ `0653e10d` ⊂ current main) re-ran green.
- Step 2 **complete (2026-08-18)**: the complete pinned owner graph is
  frozen at `ratchets/h2-5h-a-owner-graph.v1.json` (generator
  `crates/oracle/h2-5h-a-owner-graph.mjs` `--write|--check`, contract
  `.github/ci/contracts/h2-5h-a-owner-graph.schema.json`, registered in
  the artifact-contract table): both owner declarations re-validated
  from the frozen owner inventory, parser-exact classified reference
  closures (300 pinned local functions, 98 factory methods, the six
  resolver methods matching the foundation coverage, five helper
  factories, 132 external utilities, 242 frozen enum value/name pairs),
  the destructuring-flattener (18 functions) and tagged-template (2)
  shared-module closures, five composition edges (including the two
  pinned `yield*` synthesis sites through which ES2015 loop conversion
  feeds the Generators state machine), and seventeen
  census-surface-to-architecture-row assignments verified against both
  the census and the architecture map. Assignment is not disposition:
  the applicability manifest remains step 5's output.
- Step 3 **complete (2026-08-18)**: the current-Rust local-gap matrix is
  frozen at `ratchets/h2-5h-a-gap-matrix.v1.json` (generator
  `crates/oracle/h2-5h-a-gap-matrix.mjs` `--write|--check`, registered):
  thirteen capability rows (3 exists / 7 partial / 3 missing) verified
  mechanically on both sides — every requirement surface must exist in
  the frozen owner graph, every positive Rust anchor is pinned
  (path+symbol+file hash), and every recorded absence is asserted, so a
  landed implementation breaks the matrix and forces a reviewed
  re-disposition. The mint itself corrected two draft assumptions
  fail-closed: the `EmitResolver` trait already declares three of the
  six owner queries (typed fail-closed defaults), and the
  ObjectRestSpread flatten level already lives inside the ES2018
  lowering. No production code was edited.
- Step 4 **decided (2026-08-18)**: the owner graph proves ES2015 and
  Generators form ONE implementation unit, so H2.5h receives **no
  further owner-SCC suffix split**: the runtime activation slice is
  **H2.5h-b** (single, joint), resolving the deferred
  `H2.5h-b+`/`determined-by-H2.5h-a-owner-graph` split. Evidence, all
  pinned in the owner-graph artifact: (1) the `yield-star-synthesis`
  composition edge — ES2015 loop conversion re-emits generator-crossing
  loop calls as synthesized `yield*` at two pinned sites, which only the
  Generators state machine can lower, so ES2015's loop conversion is
  semantically incomplete without its consumer; (2) the joint upstream
  registration (one guard pushes both transformers); (3) the shared
  surface (35 factory methods, the values helper, the chained
  substitution hook). Implementation may still be delivered as ordered
  reviewable packets inside H2.5h-b, and the E-COMMENT-SCOPE-H
  implementation packets (steps 2-6 of the comment-scope plan) precede
  any ES2015/Generators production work as already mandated. The
  machine encoding of this decision (profile transition update and the
  readiness manifest's slice assignment) is step 5's output; this
  record authorizes no production edit by itself.
- Step 5 manifest half **complete (2026-08-18)**: the architecture-row
  disposition manifest is frozen at
  `ratchets/h2-5h-a-dispositions.v1.json` (generator
  `crates/oracle/h2-5h-a-dispositions.mjs` `--write|--check`,
  registered): all 45 rows dispositioned exactly once (16
  premise-unchanged / 15 modified-requalify / 10 activate / 4
  future-owned-fail-closed / 0 proven-unreachable, undispositioned = 0
  by construction — the row inventory is derived from the architecture
  map and compared against the reviewed table, so a map change makes
  the manifest stale), every citation verified against the frozen owner
  graph and local-gap matrix, and premise-unchanged rows forbidden from
  citing non-`exists` capabilities. The readiness envelope
  (`ratchets/fci-readiness/h2-5h-a.v1.json`, status `design`) and the
  bootstrap authorization (`allowedPacketIds += h2-5h-a`) are live.
  Remaining for `ready`: step 6 and step 7 below.
  **Amended 2026-08-18 by the CS-2 packet**
  ([CS-2](h2-5h-a-cs-2.md) §8): landing the threaded comment scope
  reshapes the private printer context carrier the `E-PRINTER-BASE`
  and `E-PRINTER-G` rows name, so both moved `premise-unchanged` ->
  `modified-requalify` (counts then 14 premise-unchanged / 17
  modified-requalify / 10 activate / 4 future-owned-fail-closed / 0
  proven-unreachable, undispositioned still 0 by construction).
  **Amended 2026-08-19 by the CS-3 packet** ([CS-3](h2-5h-a-cs-3.md)
  §8): the per-side route migration re-expresses `E-COMMENTS-G`'s
  qualified expression/list projections, so that row moved
  `premise-unchanged` -> `modified-requalify` (counts now 13 / 18 / 10
  / 4 / 0, undispositioned 0).
  **Amended 2026-08-20 by the CS-4 packet** ([CS-4](h2-5h-a-cs-4.md)
  §8): the statement-family routes and the declaration-list writer
  landed with no disposition move (counts unchanged 13 / 18 / 10 / 4 /
  0); the gap-matrix `comment-scope-threading` absences retired to the
  landed `claim_declaration_list_sides` anchor.
  **Amended 2026-08-20 by the CS-5 packet** ([CS-5](h2-5h-a-cs-5.md)
  §8): the five contextless shims and the `detached_transitional`
  constructor deleted with no disposition move (counts unchanged 13 /
  18 / 10 / 4 / 0); no anchor change — the matrix anchors never
  referenced the deleted names (measured).
  **Amended 2026-08-21 by the CS-6 packet** ([CS-6](h2-5h-a-cs-6.md)
  §8): the witness-driven fixture gate and the permanent
  zero-contextless audit landed; the four printer rows requalified
  `active-qualified` at `6acd5d43`; the E-COMMENT-SCOPE-H mandatory
  sub-packet is CLOSED and H2.5h-b (B-1+) is unblocked. The manifest remains
  this slice's; each production packet owns its own amendment through
  its own design gate.
- Step 6 **complete (2026-08-18)**: the W-H2.5H ES2015/Generators
  witness set is frozen at
  `ratchets/h2-5h-a-es2015-generators-witnesses.v1.json` (generator
  `crates/oracle/h2-5h-a-es2015-generators-witnesses.mjs`
  `--write|--check`, contract
  `.github/ci/contracts/h2-5h-a-es2015-generators-witnesses.schema.json`,
  registered as artifact-contract row 9): nine witness families over
  the owner-graph census surfaces, thirty-two oracle-captured cases in
  the four mandated roles (10 positive / 9 adjacent-negative controls
  / 7 composition / 6 fault), each observed twice in fresh
  pinned-TypeScript processes (64 oracle runs). Machine-checked
  invariants: all five owner-graph composition edges cited and
  covered, with both pinned yield* synthesis sites re-derived from the
  implementation bytes (enclosing functions
  `generateCallToConvertedLoopInitializer` /
  `generateCallToConvertedLoop`) and each named by its covering
  composition case; an exact 14+3 partition of the seventeen census
  surfaces, the three excluded surfaces recorded with their owning
  authority (comment-apis under the frozen comment-scope witnesses,
  source-map-apis under EA-GAP-MAPS-DECLS, outer-expression-wrappers
  under the comment-scope/E-POSITIONS lineage); the three
  resolver-family cases byte-identical to the foundation
  direct-control inputs, options, and stored writes; the six fault
  cases pinning their exact diagnostic codes (2802, 2304, 2548, 2349,
  2354, 1100+2496) together with the emit-under-fault bytes; and
  per-family pairwise-distinct outputs. The initializer-call
  composition witness freezes the upstream-faithful unassigned
  `out_index_1` read exactly as the pinned oracle emits it — oracle
  bytes are the authority, never a hand-derived correction.
- Step 7 **complete (2026-08-18)**: the envelope flipped `design` ->
  `ready` on the witness-machine train — packet-document digest
  re-pinned after this record, the three witness-machine paths added
  to `allowedPaths`, the witness `--check` and the readiness check
  appended to the proof commands, and the trusted base advanced to the
  branch base `a2343fcf` — and the complete packet checker below ran
  green at one head. The paused Functional-CI tail was not a
  prerequisite (recorded Option A review). Production authorization
  remains per-packet: CS-2..CS-6 precede any ES2015/Generators
  production work, and the H2.5h-b implementation packets are authored
  after this freeze against the frozen graph/matrix/witnesses.

## Packet ladder and checker

The campaign delivers through machine-tracked packets in this order.
Every packet that edits a production file passes the
[mandatory design gate](../post-h1-completion-slices.md#11-mandatory-implementation-ready-design-gate)
in its own packet document before that edit; this manifest never
substitutes for a per-packet gate.

1. **CS-2 .. CS-6 — comment-scope implementation packets** (printer
   production edits; the six-step plan's steps 2-6 in
   [the comment-scope section](#first-mandatory-design-packet-global-comment-scope)):
   CS-2 introduces the immutable `CommentEmissionScope`/`EmitContext`
   triple at the root and core pipeline; CS-3 migrates expression and
   list routes; CS-4 statements, declarations, classes, JSX,
   parameters, transformed nodes, substitution, and notification; CS-5
   token/comment scanners and deletes every contextless or dual nested
   API; CS-6 runs the focused fixtures, complete emitter suite, owner
   controls, inventory, and the zero-contextless-use audit. Inputs
   already frozen: the scope-graph study and the ten-family witness
   artifact. No ES2015/Generators production work may precede CS-6
   green. **ALL SIX LANDED; closed at `6acd5d43` (2026-08-21,
   [CS-6](h2-5h-a-cs-6.md)).**
2. **W-H2.5H — step-6 witness machine, complete (2026-08-18)**:
   oracle-produced positive, adjacent-negative, composition, and fault
   witnesses for the ES2015/Generators surface, extending the
   comment-scope witness mechanism; witness families are enumerated
   per owner-graph surface (loop conversion incl. the two pinned
   `yield*` sites, class lowering lanes, destructuring flattener
   family, tagged templates, helper graph, name generation, resolver
   queries against the foundation's direct controls, hook chains,
   enum-pair guards). Frozen ahead of CS-2 as the step-6 readiness
   prerequisite (step-6 record above); the ladder position here
   governs production delivery order, not the freeze order.
3. **B-1 .. B-5 — H2.5h-b implementation packets** (single joint
   runtime slice per the step-4 SCC decision; the activation split is
   not revisited). The B-1 design pass ratified the ladder
   ([B-1 packet](h2-5h-b-b-1.md) §2); every packet before the last is
   corpus-inert:
   - **B-1 — shared substrate (foundation)**: the five capability rows
     both owners consume — helper texts byte-pinned to the vendored
     declarations, the resolver collision/capture trio at the checker
     bridge, eager name-generation completion carrying the reviewed
     E-NAMES-H equivalence argument, the EA-GAP-FLAGS postorder
     classifier over the nine-facet qualification surface, and the
     pinned hook chain. **LANDED at `ad62e4a5` (2026-08-21).**
   - **B-2 — destructuring flattener (foundation)**: the 18-function
     shared family at FlattenLevel All (the ObjectRestSpread level
     already lives in `es2018.rs`). **LANDED at `f6c18ff4`
     (2026-08-22).**
   - **B-3 — Generators state machine (foundation, dormant)**: labels,
     try/catch protocol, instruction encoding via
     `createGeneratorHelper`; consumer-first per the pinned
     `yield-star-synthesis` edge.
   - **B-4 — ES2015 visitors (foundation)**: class lowering lanes,
     captured `this`/`arguments`/`new.target`, parameters, and loop
     conversion WITH the two pinned `yield*` synthesis sites feeding
     B-3's machine.
   - **B-5 — runtime flip**: tagged-template lowering, the joint
     `[transformES2015, transformGenerators]` registration
     (`languageVersion < ES2015`), the 32-case witness fixture gate
     (the CS-6 analog: frozen bytes are the entire expectation), and
     requalification.

**Packet checker** (step 7; also the envelope's proof commands): all of

```text
node crates/oracle/h2-5h-a-foundation.mjs --check
node crates/oracle/h2-5h-a-comment-scope-witnesses.mjs --check
node crates/oracle/h2-5h-a-owner-graph.mjs --check
node crates/oracle/h2-5h-a-gap-matrix.mjs --check
node crates/oracle/h2-5h-a-dispositions.mjs --check
node crates/oracle/h2-5h-a-es2015-generators-witnesses.mjs --check
node .github/ci/qualification.mjs check
node .github/ci/slice-readiness.mjs --check h2-5h-a
```

green at one head, first satisfied on the witness-machine train
(step 7 above). The six artifact `--check`s are the packet checker's
own commands: the full local gate does NOT run them (its oracle phase
validates these artifacts only through the qualification registry's
contract table — schema subset plus fingerprint, no re-observation),
so the once-per-slice packet-checker run above is their full
re-observation backstop. The registry does run inside every full local
gate and revalidates every `ready` envelope including this packet's
document digest — any later edit to this document therefore requires
an envelope re-pin in the same change.


If the ready packet adds an H2.5h ts-tests runner to hosted acceptance, it must
also preserve the single unsplit `cargo xtask acceptance` command and update
the complete hosted action union, protected engine adapter/profile registry,
fallback plan, and raw source SHA-256 pins in
`.github/ci/qualification-policy.v2.json`. Engine/verifier/profile changes use
the Functional-CI N+1 promotion route; candidate code cannot approve itself.
Before activation, the changed complete union must pass the disabled hosted
shadow with exact fresh/mixed/rejected equivalence and every protected
bootstrap/transition proof. It must prove that no owner-control or
`NonReusable` action became reachable there; owner controls stay in the
complete local gate. Updating a pin without the corresponding reviewed
source/entry, protected promotion, complete-membership, and shadow transition
is not evidence.

The readiness checker command and expected counts/hashes are owned by the
versioned packet-control bootstrap and the active packet, not by this handoff.
The packet is machine-checked ready as of 2026-08-18; every production
packet in the ladder still passes the mandatory design gate in its own
packet document before its first production edit. An agent that encounters
this page may extend the frozen inventory read-only, and never creates
ES2015 or Generator runtime code before CS-6 is green and the H2.5h-b
packet documents exist under the design gate.

## Mandatory architecture inputs

The ready manifest must enumerate every row in the current architecture map
exactly once, including entry/protocol/plan/output and dormant future-product
rows. Each row receives one applicability disposition:
`premise-unchanged`, `modified-requalify`, `activate`,
`future-owned-fail-closed`, or `proven-unreachable`. A wildcard in prose does
not satisfy this requirement, and an omitted supposedly unrelated row is an
undispositioned row.

At minimum, the owner graph must treat the following current and planned
sub-rows as reachable inputs unless the pinned research supplies a
`proven-unreachable` proof:

- `E-ARENA`, `E-RESOLVER-IDENTITY-G`, `E-CONTEXT`, and `E-SYNTAX-FACTS`;
- `E-METADATA-BASE`, `E-METADATA-G`, `E-METADATA-G-CLASS`, and
  `E-JSX-FACTORY-G`;
- `E-CAPTURE-BASE`, `E-CAPTURE-CLASS-G`, `E-CLASS-PENDING-G`,
  `E-DECORATOR-INITIALIZERS-G`, `E-DECORATOR-CLASS-INITIALIZERS-G`, and
  `E-DECORATOR-PARAMETER-PROPERTY-G`; the packet must state how every
  ES2015/Generators class or wrapper consumer preserves all three independent
  ordered lanes (class-definition pending effects, member `addInitializer`
  queues, and the exactly-once class-decorator finalizer), their
  consumer-specific placement and receiver selection, the synthesized
  `PropertyDeclaration -> Parameter` provenance chain, and constructor-local
  parameter-property materialization in every target/field-mode route;
- `E-ORDER-G` and `E-ORDER-H`;
- `E-RESOLVER-BASE`, `E-RESOLVER-CAPTURE-BASE`, and
  `E-RESOLVER-CAPTURE-H`;
- `E-CHECKER-FACTS-BASE` and `E-CHECKER-FACTS-H`;
- `E-NAMES-BASE`, `E-NAMES-CLASS-G`, and `E-NAMES-H`;
- `E-HELPERS-BASE`, `E-HELPERS-PROVENANCE-G`, and `E-HELPERS-H`;
- `E-PRINTER-BASE` and `E-PRINTER-G`;
- `E-COMMENTS-G`, `E-COMMENT-SCOPE-H`, and `E-COMMENTS-H`; and
- `E-POSITIONS` and `E-STRINGS`.

The complete packet must also disposition:

- `EA-GAP-FLAGS`, `EA-GAP-CAPTURE`, `EA-GAP-COMPOSITION`, and
  `EA-GAP-MAPS-DECLS`; the last remains `future-owned-fail-closed` under
  H2.6/H2.7 unless the pinned graph proves that H2.5h must requalify a shared
  provenance fact;
- the exact ES2015 -> Generators -> module pass order and every hook/finalizer
  composition edge; and
- every inherited deferral, revalidated with its guard, earliest owner, typed
  failure boundary, and adjacent-negative control.

No `TBD`, implied repository search, remembered tsc behavior, data-model
choice, unnamed file edit, or hand-authored expected output is permitted in
the ready packet.

## First mandatory design packet: global comment scope

**Closed 2026-08-21 by the CS-6 packet** ([CS-6](h2-5h-a-cs-6.md) §8):
all six comment-scope packets landed; the four printer rows
requalified `active-qualified` at `6acd5d43`; H2.5h-b (B-1+) is
unblocked.

Step-1 progress: **complete.** The pinned scope graph and the
current-Rust delta are frozen in
[the comment-scope study](h2-5h-a-comment-scope.md), and the ten-family
witness set is frozen at `ratchets/h2-5h-a-comment-scope-witnesses.v1.json`
(generator `crates/oracle/h2-5h-a-comment-scope-witnesses.mjs`
`--write|--check`, contract
`.github/ci/contracts/h2-5h-a-comment-scope-witnesses.schema.json`,
subset-validated by the registered artifact-contract table): thirty
oracle-captured cases — one positive, one remove-comments control, and
one adjacent-negative control per family — observed twice each in fresh
pinned-TypeScript processes, plus the machine-checked scope-graph pins
(the complete 23-line occurrence set and seven anchored span hashes).
The implementation packets (steps 2-6) remain open and are still
design-gated.

Before the ES2015 or Generators owner graph may authorize production work, the
slice must close `E-COMMENT-SCOPE-H`. This is an architecture prerequisite,
not an opportunistic printer fix. The pinned tsc study must trace the set,
save/restore, and read sites for all three independent scoped values:
`containerPos`, `containerEnd`, and `declarationListContainerEnd`. It must then
map their meaning to Rust without copying tsc's mutable closure structure.

The preferred Rust candidate for the packet to prove is immutable value
threading:

```rust
#[must_use]
struct CommentEmissionScope {
    container_pos: Option<CommentCursor>,
    container_end: Option<CommentCursor>,
    declaration_list_container_end: Option<CommentCursor>,
}

#[must_use]
struct EmitContext {
    comments: CommentEmissionScope,
    syntax: ExpressionSyntaxContext,
}
```

The names are not pre-approved API, but the semantics are mandatory. A child
operation preserves the complete comment scope while replacing only its
edge-specific syntax context. There is no `Default` nested scope. A private
root constructor creates the initial scope, while every nested node, list,
token, comment, transformed-node, and notification/substitution route accepts
an explicit context. The packet must name the root boundary and every old
contextless API that will be deleted or made root-only; leaving two callable
nested pipelines is not an allowed intermediate completion state.

The frozen oracle set starts with this known counterexample and its
remove-comments and adjacent-negative controls (bytes below are the
oracle-captured output from the frozen witness artifact; an earlier
draft of this page transcribed `() => { x; }; /*TAIL*/` from memory and
the pinned oracle falsified it — the trailing comment relocates with
the original statement into the wrapper and is neither duplicated at
the end of the file nor dropped):

```text
source: x /*TAIL*/\n  (after `declare const x: number;`)
transform: replace the outer statement with synthetic `() => { originalStmt }`
tsc output: () => { x; /*TAIL*/ };\n
```

It then covers direct children, ordinary and declaration lists, synthetic
wrappers, statements, declarations, classes, JSX, parameters, token/comment
scanners, source changes, zero-width ranges, and errors during nested emit.
Expected bytes are captured from the pinned oracle; they are never transcribed
from this example into tests. The falsified draft output above is the
standing argument for that rule.

The implementation plan must be split into independently reviewable packets:

1. freeze the direct/list/wrapper/declaration-list witnesses and the complete
   tsc scope graph;
2. introduce the triple scope at the root and core pipeline;
3. migrate expression and list routes, including Call, New, Array, Object, and
   Spread;
4. migrate statements, declarations, classes, JSX, parameters, transformed
   nodes, substitution, and notification;
5. migrate token/comment scanners and delete every contextless or dual nested
   API; and
6. run the focused fixtures, complete emitter suite, owner controls, inventory,
   and a zero-contextless-use audit in that order.

No ES2015/Generators implementation packet may assume comment relocation is
safe until this sequence is approved and its final gate is green.
