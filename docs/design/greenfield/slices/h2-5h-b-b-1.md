# H2.5h-b / B-1 — shared substrate: helpers, resolver queries, name generation, transform flags, hook chaining (DRAFT)

Design-gate packet for the FIRST H2.5h-b implementation packet, under
the mandatory implementation-ready design gate
(../post-h1-completion-slices.md). **DRAFT front-run status:** authored
in the `h2/5h-b-b1` worktree on the CS-6 front-run head (`01e2a8bd`);
trusted base, authority hashes, and the §12 re-pin land at the B-1
train's design-gate pass, which runs only after CS-6 merges (the
standing prohibition "no ES2015/Generators production work may precede
CS-6 green" holds — this document is design, and it authorizes no
production edit until its own gate passes). Machine check (once the
envelope exists): `node .github/ci/slice-readiness.mjs --check
h2-5h-b-b-1`.

## 1. Identity, purpose, and boundary

- **Slice ID:** `h2-5h-b-b-1`. **Kind:** `foundation` — a corpus-inert
  substrate packet. It closes the shared prerequisite capability rows
  BOTH owners (`transformES2015`, `transformGenerators`) consume, so
  the transformer packets start against a complete substrate:
  1. **Helper texts**: the four absent owner helper factories
     (`extends`, `values`, `read` exists, `spreadArray`, `generator` —
     absences measured in the frozen gap matrix; `asyncValues` /
     `asyncGenerator` are different helpers and already exist).
  2. **Emit-resolver queries**: the six queries the owners call —
     ES2015: `getReferencedDeclarationWithCollidingName`,
     `hasNodeCheckFlag`, `isArgumentsLocalBinding`,
     `isBindingCapturedByNode`, `isDeclarationWithCollidingName`;
     Generators: `getReferencedValueDeclaration` (owner-graph
     `owners[].resolver_methods`, measured). The Rust trait
     (`crates/emitter/src/resolver.rs:210`) declares three today
     (`get_referenced_value_declaration` :294, `has_node_check_flag`
     :332, `is_arguments_local_binding` :344); the collision/capture
     trio is absent.
  3. **Name generation**: complete
     `createUniqueName`/`createTempVariable`/`createLoopVariable` and
     `getGeneratedNameForNode`/`getInternalName`/`getLocalName`
     semantics on the eager scoped-reservation model
     (`generated_bindings.rs`), carrying the E-NAMES-H
     deferred-vs-eager equivalence argument (§12.3).
  4. **Transform-flag recomputation**: the EA-GAP-FLAGS deliverable —
     a full postorder classifier usable for freshly synthesized
     ES2015/Generators output (`propagate_child_flags` exists;
     the shared full classifier for changed nodes is outstanding).
  5. **Hook chaining**: chained `onSubstituteNode` for both owners and
     chained `onEmitNode` for ES2015 (Generators registers
     substitution only), per the frozen `substitution-chain`
     composition edge.
- **Non-goals:** transformer registration or activation (the dormant
  seam stays a typed fail-closed rejection —
  `crates/emitter/src/builtins.rs:145-148` "older targets belong to
  later target-ladder slices" is preserved verbatim); loop conversion;
  the Generators state machine; the FlattenLevel-All destructuring
  family; tagged-template lowering; witness-set extension; any corpus
  output-byte change.
- **Prerequisites:** CS-6 merged with its envelope `ready`
  (E-COMMENT-SCOPE-H closed, the four requalified rows
  `active-qualified`); gate-tax 2 merged (walk re-mints ride
  adoption) — merged 2026-08-20 @54bbbc03.
- **Trusted base:** re-pin at train start (DRAFT: authored at
  `01e2a8bd` on the CS-6 branch).
- **Activation state:** before — five gap rows `partial`
  (`helper-emission`, `name-generation-deferred`,
  `resolver-collision-capture-queries`, `transform-flag-recomputation`,
  `substitution-notification-hooks`), the joint pass dormant. After —
  those five rows `exists` with pinned anchors, the pass STILL dormant,
  the corpus ratchet byte-identical (T0=100.0000% 49024/49024 FP=0
  unchanged).
- **Next owner:** B-2 (§2 ladder).

## 2. The B-ladder (decomposition this packet's design pass ratifies)

Ordered reviewable packets inside the single joint runtime slice
H2.5h-b (the step-4 SCC decision: activation split is not revisited —
every packet before the last is corpus-inert):

| Packet | Kind | Scope | Qualifying witnesses |
|---|---|---|---|
| **B-1** (this) | foundation | the five shared-substrate capability rows above | focused contracts + foundation direct controls (families qualify at B-5) |
| **B-2** | foundation | destructuring flattener at FlattenLevel All (the 18-function shared family; ObjectRestSpread level already lives in `es2018.rs`) | `destructuring-flattener` family focused projections |
| **B-3** | foundation | the Generators state machine (labels, try/catch protocol, instruction encoding via `createGeneratorHelper`) — dormant, driven by focused fixtures only | `loop-conversion-capture` yield-star consumers deferred to B-4; state-machine focused contracts |
| **B-4** | foundation | ES2015 visitors: class lowering lanes, captured this/arguments/new.target, parameters, loop conversion WITH the two pinned `yield*` synthesis sites feeding B-3's machine | `class-lowering-lanes`, `loop-conversion-capture` focused projections |
| **B-5** | runtime | tagged-template lowering + the joint registration flip (`languageVersion < ES2015` → `[transformES2015, transformGenerators]`) + the 32-case witness fixture gate (the CS-6 analog: frozen bytes are the entire expectation) + requalification | ALL nine families end-to-end, 32/32 byte-equal |

Ordering rationale, pinned: the `yield-star-synthesis` composition
edge makes ES2015 loop conversion semantically incomplete without its
Generators consumer — the consumer (B-3) lands before the producer
(B-4). Registration order `[transformES2015, transformGenerators]`
and the activation predicate are frozen in the owner graph
(`upstream_registration`: `vendor/typescript-6.0.3/lib/_tsc.js`
:115942-115945, registration sha `f13bde7bd85c8fdc…`). Granularity
ratification is §12.2.

## 3. Required-reference table

| Row | Lifecycle before → after | Role here |
|---|---|---|
| `E-HELPERS-BASE` / `E-HELPERS-H` | per dispositions manifest → B-1 adds the four texts | helper factory carrier; `typescript:read` precedent (`helpers.rs:112`) |
| `E-NAMES-BASE` / `E-NAMES-H` | `active-*` → E-NAMES-H carries the equivalence argument | eager reservation model owner (`GeneratedBindingScopes`, `allocate_temp`) |
| `E-RESOLVER-CAPTURE-H` / `E-CHECKER-FACTS-H` | per manifest → query trio landed | six-query surface; checker-facts production site is §12.5 |
| `EA-GAP-FLAGS` / `E-METADATA-BASE` | open gap → classifier landed | full postorder classification for synthesized subtrees |
| `E-ORDER-H` | `active-*` (hooks half) | chained substitution/notification parity |
| `EA-GAP-COMPOSITION` | untouched | registration boundary stays dormant (non-goal) |
| `E-ARENA`, `E-CONTEXT` | `active-qualified` premises | generic construction + lexical environment (gap rows `exists`) |
| gap rows | 5× `partial` → `exists` | §6 matrix |

Exact lifecycle values are transcribed from the dispositions manifest
at the train's re-pin (§12.1); the manifest amendment itself is §8.

## 4. Pinned upstream map

The upstream IS the frozen artifact chain:

- **Owner graph** (`ratchets/h2-5h-a-owner-graph.v1.json`): the six
  `resolver_methods` per owner; the five helper factories; the
  `substitution-chain` composition edge (hook parity input); the
  `yield-star-synthesis` edge (B-3/B-4 boundary input); the
  `upstream_registration` record pinning the activation predicate,
  transformer order, and source range/sha; 132 external utilities and
  242 enum value/name pairs backing §5's flag/guard rows.
- **Gap matrix** (`ratchets/h2-5h-a-gap-matrix.v1.json`): the five B-1
  capability rows with pinned anchors AND asserted absences — landing
  an implementation breaks the matrix and forces the reviewed
  re-disposition this packet's §8 performs.
- **Witness artifact**
  (`ratchets/h2-5h-a-es2015-generators-witnesses.v1.json`): closure
  authority (B-5); B-1 cites the `helper-graph` (3 cases),
  `name-generation` (3), `resolver-foundation-controls` (4, byte-equal
  to foundation direct controls), `hook-chains` (3), and
  `enum-pair-guards` (2) families as the frozen end-state its
  substrate must make reachable.
- **Vendored tsc**: helper text declarations and
  `computeTransformFlags`/`propagateChildFlags` tables in
  `vendor/typescript-6.0.3/lib/_tsc.js`; exact spans + `tsc-hash`
  slices are §12.6/§12.4 outputs (mechanical extraction commands
  given there).

## 5. Rust semantic map

| Item | Target |
|---|---|
| helper texts | `crates/emitter/src/builtins/helpers.rs`: four `const *_HELPER_TEXT: &str` raw strings + `EmitHelper::with_text("typescript:<name>", …)` registrations, matching the `READ_HELPER_TEXT` precedent (:31, :112); each const carries the ledger `tsc-hash` header (d2 discipline) |
| resolver queries | `crates/emitter/src/resolver.rs` trait `EmitResolver` (:210): add `get_referenced_declaration_with_colliding_name`, `is_declaration_with_colliding_name`, `is_binding_captured_by_node` as typed fail-closed defaults beside the three declared (:294/:332/:344); production checker-facts implementation site resolves at §12.5 |
| name generation | `crates/emitter/src/builtins/generated_bindings.rs`: extend `GeneratedBindingScopes`/`allocate_temp` with loop-variable and unique-name flag semantics plus `getGeneratedNameForNode`-class node-keyed lookups, all under eager reservation; the equivalence argument (§12.3) is a design deliverable of this packet, not an implicit judgment call |
| transform flags | `crates/emitter/src/factory.rs`: full postorder classifier beside `propagate_child_flags`, driven by the pinned upstream tables; EA-GAP-FLAGS bans inheriting stale ES2015/Generators facets on synthesized output |
| hooks | `crates/emitter/src/transform.rs`: `substitution_factory` grows chained-hook composition (both-owner substitution, ES2015-only notification) with a pinned chaining order |

All five targets exist today as files/symbols (gap-matrix anchors,
measured); no new module is created.

## 6. Current local-gap matrix (B-1 rows, from the frozen artifact)

| Capability | State | Anchor evidence | Absence evidence |
|---|---|---|---|
| `helper-emission` | `partial` | `typescript:read` (`helpers.rs`) | 4 absences asserted in `helpers.rs` |
| `resolver-collision-capture-queries` | `partial` | 3 trait methods (:294/:332/:344) | 2 absences asserted in `resolver.rs` |
| `name-generation-deferred` | `partial` | `GeneratedBindingScopes`, `allocate_temp` | eager model vs deferred semantics (note) |
| `transform-flag-recomputation` | `partial` | `propagate_child_flags` | full classifier outstanding (note) |
| `substitution-notification-hooks` | `partial` | `substitution_factory` | ES2015-grade chaining parity (note) |

## 7. Implementation sequence (dependency order; every step corpus-inert)

Fence: `crates/emitter/src/builtins/helpers.rs`,
`crates/emitter/src/resolver.rs`,
`crates/emitter/src/builtins/generated_bindings.rs`,
`crates/emitter/src/factory.rs`, `crates/emitter/src/transform.rs`,
the checker-facts site named by §12.5 once resolved, their unit/focused
test trees, and the §8 evidence set. `builtins.rs`'s registration
rejection is read-only.

1. **Helper texts.** Four consts + registrations + per-helper
   byte-equality tests against the pinned vendored slices (§12.6
   spans). Check: focused helper suite green; corpus ratchet
   byte-identical.
2. **Resolver trio.** Trait declarations (fail-closed defaults) +
   production implementations at the §12.5 site + contracts replaying
   the foundation direct-control expectations. Check: resolver suite
   green; `resolver-foundation-controls` family inputs compile
   against the trait without defaults firing.
3. **Name generation.** Eager-model completion + oracle-probe
   contracts (probe.sh pattern) for uniqueness/flags/node-keyed
   lookups. Check: probes byte-parity on generated-name spellings.
4. **Flags classifier.** Postorder classifier + table contracts
   against the pinned upstream rows. Check: classifier unit suite;
   synthesized-subtree flag assertions.
5. **Hook chaining.** Chained substitution/notification with a pinned
   order + order contracts. Check: hook suite green.
6. Train items: §8 amendments, chain walk (adoption,
   qualification-before-profile), envelope `h2-5h-b-b-1` (`ready`,
   predecessor `h2-5h-a-cs-6`), bootstrap + index, static
   `--lane rust` BEFORE the walk, full local gate at the final head
   from the canonical repository path.

## 8. Evidence, ratchet, and documentation amendments

1. **Gap matrix generator**: the five rows `partial` → `exists`
   (counts move from the post-CS-6 4 exists / 6 partial / 3 missing to
   9 / 1 / 3), anchors gain the landed symbols, absences retire.
2. **Dispositions manifest**: the affected rows' amendment through
   this packet's own gate (per-packet amendment rule).
3. **Architecture map**: E-HELPERS-H / E-NAMES-H /
   E-RESOLVER-CAPTURE-H / EA-GAP-FLAGS / E-ORDER-H rows updated at the
   B-1 final validation ref.
4. **Handoff** `h2-5h-a.md`: ladder item 3 gains the ratified B-ladder
   table (§2) ⇒ envelope re-pin + doc-pinning witness re-mints
   (adoption: seconds).
5. **Chain walk**: CS-2 §8.5 verbatim with the extended const-sync
   table; pin-sweep audit before the gate after any multi-attempt walk.
6. **Readiness**: envelope `h2-5h-b-b-1`, bootstrap
   `allowedPacketIds += h2-5h-b-b-1`, index row.

## 9. Acceptance

- Four helper texts byte-equal to their pinned vendored slices; ledger
  d2 test green.
- The six resolver queries declared; the trio implemented at the
  production site; foundation direct-control contracts green.
- Name-generation probe contracts byte-parity; the E-NAMES-H
  equivalence argument recorded and reviewed.
- Flags classifier table contracts green; no stale-facet inheritance
  path remains (EA-GAP-FLAGS wording).
- Hook chaining order contracts green.
- Corpus ratchet: T0=100.0000% 49024/49024 FP=0, all bands, tiers —
  byte-identical (foundation packet; zero output change).
- `cargo test -p tsc-rs-emitter` fully green; zero expected-string
  changes outside the new focused suites.
- Packet checker + `slice-readiness --check h2-5h-b-b-1`; complete
  local gate green at the final head.

## 10. Traceability

| Invariant | Target | Test | Evidence |
|---|---|---|---|
| helper text fidelity | helpers.rs consts | per-helper byte tests | vendored slices + ledger tsc-hash |
| six-query surface | EmitResolver trait + impl | foundation-control contracts | owner graph `resolver_methods` + foundation artifact |
| eager/deferred name equivalence | generated_bindings.rs | oracle probes | E-NAMES-H argument + name-generation family (B-5) |
| synthesized-flag correctness | factory.rs classifier | table contracts | pinned upstream tables |
| hook order parity | transform.rs chaining | order contracts | substitution-chain edge |
| dormancy | builtins.rs rejection | untouched-file assert + ratchet | corpus byte-identity |

## 11. Prohibitions

No transformer registration or activation change; no corpus
output-byte change (the ratchet is the enforcement); no witness
amendment; no ES2015/Generators visitor or state-machine code (B-3/B-4
scope); no new `EmitContext` constructors; no generic fallback that
converts an unknown branch into success; the CS-3/4/5/6 prohibitions
remain. This document authorizes no production edit until its own
design-gate pass and envelope exist.

## 12. Unresolved items (DRAFT — close before the envelope flips ready)

1. Trusted base + authority hashes: re-pin after CS-6 merges.
2. **B-ladder granularity ratification** (§2): confirm B-1 carries all
   five substrate rows in one packet vs splitting emit-side
   (helpers/names/flags) from resolver-side. Owner: the design-review
   pass. Command: closure counts per surface from
   `ratchets/h2-5h-a-owner-graph.v1.json` (`owners[].local_functions`
   grouped by §2's packet assignments).
3. **E-NAMES-H equivalence argument**: prove the eager
   scoped-reservation model reproduces tsc's deferred-resolution
   observable spellings for every generated-name class the owners use.
   Owner: this packet's design review. Command: oracle probes over the
   name-generation family inputs + targeted probe fixtures
   (`cargo xtask expand … + node crates/oracle/driver.mjs`).
4. **EA-GAP-FLAGS classifier scope**: full `computeTransformFlags`
   port vs the owner-called subset. Owner: design review. Command:
   pin the upstream table spans and intersect with the owner graph's
   external-utility rows citing flag facets.
5. **Production checker-facts site** for the collision/capture trio:
   locate the production `EmitResolver` implementation (only test
   impls exist under `crates/emitter/tests/`, measured) and the
   `NodeCheckFlags` source feeding it. Owner: design review. Command:
   `grep -rn "impl EmitResolver" crates/ --include='*.rs'` extended to
   the checker/compiler crates + the foundation direct-control replay.
6. **Helper text spans**: extract the four helper declarations'
   exact `_tsc.js` line spans + `tsc-hash` slices. Mechanical.
   Command: locate `var __extends|__values|__spreadArray|__generator`
   in `vendor/typescript-6.0.3/lib/_tsc.js`, pin span + sha256.
7. **Hook chaining order**: transcribe the `substitution-chain` edge's
   pinned spans into the §5 chaining contract. Mechanical.

## 13. Readiness summary (draft)

Upstream: the frozen owner-graph/gap-matrix/witness chain (§4).
Rust-map rows: 5 (§5), all target files/symbols measured present.
Gap rows: 5 (§6). Witness families cited: 5 of 9 (closure at B-5).
Architecture impact: five-row substrate closure, dormancy preserved,
ladder ratification recorded in the handoff at the train.
Undispositioned: pending §12.1 re-pin. Unresolved: §12 (7 items — 2
mechanical, 5 review-owned).
