# Post-H1 slice packets

Document role: **canonical index for executable post-H1 slice packets**.
The schedule in [post-h1-completion-slices.md](../post-h1-completion-slices.md)
defines which packet may be active. A packet is a self-contained execution
index over current architecture rows, pinned tsc owners, current local gaps,
frozen witnesses, implementation steps, and versioned evidence; it does not
replace any of those authorities.

Except for the bounded H2.5g legacy closure below, only a packet whose
machine-checked readiness state is `ready` authorizes production edits.
`blocked`, `research`, and `design` packets authorize only the files and
read-only commands they explicitly list. For every slice whose implementation
begins after H2.5g, a missing packet, readiness manifest, schema, checker,
expected count/hash, or current symbol is a fail-closed result, never an
invitation to fill in the answer during implementation.

| Packet | Status | Authorized work |
| --- | --- | --- |
| [H2.5g legacy closure](#h25g-legacy-closure-route) | **Closed and qualified** (final validation ref `0653e10d`; delivery merge `507a96ac`; recorded in the post-H1 schedule §1.2) | No further edits; the closure route below is retained as the immutable record of its commands and required results. |
| [Packet-control bootstrap](packet-control-bootstrap.md) | In progress; one-time bootstrap | Add and verify the versioned readiness schema/checker and bootstrap ratchet. It may authorize only the explicitly listed pre-closure FCI shadow packets; it cannot authorize H2.5g authority, FCI-6+, workflow, or provider changes. |
| [FCI-0a framework boundary record](../functional-ci-evidence.md#14-migration-stages-and-packets) | Documentation record only; never a runtime-ready packet | Maintain the charter, package/trust boundary, v1 non-goals, qualification ladder, and navigation without changing H2.5g commands, counts, scope, or authority. |
| [FCI-0b extension API-manifest record](../functional-ci-evidence.md#31-workspace-public-api-manifest) | Documentation record only; never a runtime-ready packet | Maintain public/sealed ownership, blocking/cancellation/panic/error contracts, and later packet ownership without declaring a Rust item or authorizing production. |
| [FCI-1a core identifiers](fci-1a-core-identifiers.md) | Ready; shadow implementation in progress | Add only the generic `ci-core` identifier/digest/input seam. No graph, codec, adapter, outcome, cache, or effect behavior. |
| [FCI-1b inert adapter descriptors](fci-1b-adapter-descriptors.md) | Ready; shadow implementation in progress | Add only typed adapter descriptors and strict ordering checks. No codec, registry, dispatch, graph, outcome, cache, or effect behavior. |
| [FCI-1c graph/profile records](fci-1c-graph-profile-records.md) | Ready; shadow implementation in progress | Generic graph/profile/pending records remain separate; executable codec/registration waits for FCI-4a.3. |
| [FCI-2a blocking runner/error boundary](fci-2a-runner-errors.md) | Ready; shadow implementation in progress | Add only the closed infrastructure-error and explicit cancellation vocabulary. FCI-2b still owns bounded effect-result seams; no live runner exists. |
| [FCI-2b bounded effect-result seam](fci-2b-bounded-effects.md) | Ready; shadow implementation in progress | Add only bounded chunk/result values, a synchronous source trait, and private staging-abandon behavior. No scheduler, snapshot, sandbox, publication, or cache. |
| [FCI-3a canonical bytes and hashes](fci-3a-canonical-hash.md) | Ready; shadow implementation in progress | Rust-owned bounded canonical encoding/strict decode and domain-separated digest types. FCI-3b/3c remain separate. |
| [FCI-3b execution/tool/reuse identities](fci-3b-execution-identities.md) | Ready; shadow implementation in progress | Generic execution/platform/toolchain, secret-free environment, reuse/disclosure, and sandbox observation values only; no effect trait. |
| [FCI-3c source/sandbox/resource primitives](fci-3c-source-sandbox-resource.md) | Closed; proof green 2026-08-16 | No further edits; snapshot/sandbox traits, bounded no-follow reads, no-replace staging, resource policy, and bounded queue are landed history. |
| [FCI-4a.1 graph schema/rendering](fci-4a.1-graph-schema.md) | Ready; shadow implementation in progress | Generic ordered graph/profile records and canonical rendering only. No closure, registry, membership, or adapter dispatch. |
| [FCI-4a.2 graph/model structural validation](fci-4a.2-graph-validation.md) | Ready; shadow implementation in progress | Generic edge/cycle/closure/global-id validation and stable plan only; no adapter dispatch or complete membership. |
| [FCI-4a.3 sealed registry/membership/testkit](fci-4a.3-sealed-registry-membership.md) | Ready; shadow implementation in progress | Consuming exact registry seal, private monomorphized decode/re-encode, typed verdicts, pending-to-complete membership, and dev-only testkit. No outcome/CAS/live runner. |
| [FCI-4b inventory](fci-4b-inventory.md) | Ready; shadow implementation in progress | Pure normalized-path, disposition, negative-lookup, generated/build ownership, collision, and unknown-policy values. No snapshot provider or impact calculation. |
| [FCI-4c paired impact and protected transition](fci-4c-impact-transition.md) | Ready; shadow implementation in progress | Pure prior/current graph comparison, reverse closures, trust root, and fail-closed transition decision. No source lookup, cache, or authority capability. |
| [FCI-4d pure explanations and planning budgets](fci-4d-explanations-budget.md) | Ready; shadow implementation in progress | Deterministic affected/reason-path/why-miss values and hard planning ceilings. No live cache, snapshot command, or semantic subprocess. |
| [FCI-5a protocol/control/plan](fci-5a-protocol-control-plan.md) | Ready; shadow implementation in progress | Typed invocation/observation/root evidence, protected fixed-plan validation, and the exact H2 source binding. No action registration or harness. |
| [FCI-5b miss-only candidate harness](fci-5b-candidate-harness.md) | Closed; proof green 2026-08-16 | No further edits; the miss-only candidate process boundary is landed history with no H2 action registration, cache, root, or authority. |
| [FCI-5c.1 H2.5g membership shadow](fci-5c.1-h2-5g-membership.md) | Ready; non-authoritative | Pure exact 9,027-case membership/disposition/shard report. No source read, candidate execution, observation, cache, or authority. |
| [FCI-5c.1b H2.5g observation shadow](fci-5c.1b-h2-5g-observation.md) | Design; **paused** by the 2026-08-17 roadmap review (Option A, emitter-first) | No production edits; the authored packet body is retained for the post-H2.9 framework review. |
| FCI-5c.2 complete H2 shadow | **Paused** with the Functional-CI tail (2026-08-17 roadmap review) | No production edits until a post-H2.9 framework review re-derives the packet chain. |
| [FCI-6a-e CAS/outcomes/capabilities/rollover/GC](../functional-ci-evidence.md#14-migration-stages-and-packets) | Blocked on FCI-5c; each letter requires its own ready packet | No production edits. |
| [FCI-7a-b, 7c.1-c.2 demand-driven local/composite shadow and framework qualification](../functional-ci-evidence.md#14-migration-stages-and-packets) | Blocked on FCI-6e; every lettered/numeric boundary requires its own ready packet | No production edits; the second real adapter precedes the API/conformance freeze. |
| [FCI-8a-f local shadow and hosted research/bootstrap/backend/shadow](../functional-ci-evidence.md#14-migration-stages-and-packets) | Blocked on FCI-7c.2; FCI-8b/8c are separate read-only protected-host/provider research and every letter requires its own ready packet | No production, bootstrap, workflow, or provider-backend edits; FCI-8a/8e append separately owned host/provider API partitions and FCI-8f freezes their exact union without reopening FCI-7c.2. |
| [FCI-9a-b activation](../functional-ci-evidence.md#14-migration-stages-and-packets) | Blocked on all FCI-8 proofs and separate activation approvals | No activation or workflow edits. |
| [FCI-10 cleanup](../functional-ci-evidence.md#14-migration-stages-and-packets) | Blocked on both FCI-9 activations and a missing ready packet | No cleanup edits. |
| [H2.5h-a](h2-5h-a.md) | **Active slice — packet machine-checked ready (2026-08-18)** (prerequisite steps 1-7 complete; the paused Functional-CI tail was not a dependency) | Production follows the packet ladder under per-packet design gates: comment-scope packets CS-2..CS-6 first, then the H2.5h-b implementation packets against the frozen graph/matrix/witnesses. |
| [H2.5h-a / CS-2 comment-scope root and core pipeline](h2-5h-a-cs-2.md) | **Merged 2026-08-18 (PR #456 @9e6235bc)**; envelope `h2-5h-a-cs-2` | Introduced the immutable `CommentEmissionScope`/`EmitContext` triple at the printer root and core pipeline, byte-identical (T0 100.0000%, FP=0); the `declaration_list_container_end` writer and route migrations stay with CS-3..CS-5. |
| [H2.5h-a / CS-3 comment-scope expression and list routes](h2-5h-a-cs-3.md) | **Merged 2026-08-19 (PR #457 @7cc97478)**; envelope `h2-5h-a-cs-3` | Per-side `containerPos`/`containerEnd` storage and tsc's exact flag-aware claim predicates on the expression/list routes; inert-container states retired for inheritance semantics under the frozen witness families; statement/declaration flag-aware migration and the declaration-list writer stay with CS-4. |
| [H2.5h-a / CS-4 comment-scope statement-family routes and the declaration-list writer](h2-5h-a-cs-4.md) | Active production packet (train 2026-08-20); envelope `h2-5h-a-cs-4` | Statement, declaration, class, JSX, parameter, transformed-node, substitution, and notification routes on the threaded scope with tsc's flag-aware per-side claims; the declaration-list writer (`claim_declaration_list_sides`) activates the trailing dedupe under its witness family; contextless deletion stays with CS-5, the fixture/audit gate with CS-6. |
| [H2.5h-a / CS-5 contextless and dual API deletion](h2-5h-a-cs-5.md) | Active production packet (train 2026-08-20); envelope `h2-5h-a-cs-5` | Purely subtractive: the five caller-less contextless shims and `EmitContext::detached_transitional` deleted, leaving `EmitContext::file_root` as the printer's single zero-scope entry; `_with_context` names kept by decision; zero behavior change (corpus ratchet + ten adjacent-control witness families); the fixture/audit gate stays with CS-6. |
| [H2.5h-a / CS-6 witness-driven fixture gate, permanent audit, requalification](h2-5h-a-cs-6.md) | Active production packet (train 2026-08-21); envelope `h2-5h-a-cs-6` | 30-case full-pipeline fixture gate byte-equal to the frozen witness artifact (both removeComments polarities, six transforms); permanent emitter-scoped zero-contextless audit (two-polarity canary); the four printer rows requalify active-qualified at `6acd5d43`; closes E-COMMENT-SCOPE-H and unblocks H2.5h-b (B-1+). |
| [H2.5h-b / B-1 shared substrate: helpers, resolver queries, name generation, transform flags, hook chaining](h2-5h-b-b-1.md) | Active production packet (train 2026-08-21); envelope `h2-5h-b-b-1` | Corpus-inert foundation packet of the H2.5h-b joint runtime slice: the four absent helper texts byte-pinned to vendored slices, the resolver collision/capture trio (production bridge in `crates/checker/src/emit.rs`), eager name-generation completion carrying the E-NAMES-H equivalence argument, the nine-facet transform-flags classifier (EA-GAP-FLAGS), and chained substitution/notification hooks; ratifies the B-1..B-5 ladder; the joint pass stays dormant and the corpus ratchet byte-identical. |
| [H2.5h-b / B-2 destructuring flattener: the 18-function shared family at FlattenLevel All](h2-5h-b-b-2.md) | Active production packet (train 2026-08-22); envelope `h2-5h-b-b-2` | Corpus-inert foundation packet: the `destructuring-flattener` shared module ported function-per-function from `_tsc.js:93251-93697` with both `FlattenLevel` arms behind the `FlattenHost` consumer seam, plus the rest/read helper-call constructors and the binding/assignment node converters; qualified by 26 byte-equal focused oracle projections + typed fault contracts; the module stays dormant and the corpus ratchet byte-identical. |
| [H2.5h-b / B-3 Generators state machine: transformGenerators as a dormant foundation module](h2-5h-b-b-3.md) | Active production packet (train 2026-08-22); envelope `h2-5h-b-b-3` | Corpus-inert foundation packet: the complete `transformGenerators` owner (129 pinned local functions, `_tsc.js:108119-110087`) ported as the dormant `GeneratorsTransformer` — labels, try/catch protocol, instruction encoding via `createGeneratorHelper`, catch-rename substitution — consumer-first per the pinned `yield-star-synthesis` edge; qualified by 72 byte-equal focused oracle projections + typed fault contracts; the module stays unregistered and the corpus ratchet byte-identical. |
| [H2.5h-b / B-4 ES2015 visitors: transformES2015 as a dormant foundation module](h2-5h-b-b-4.md) | Active production packet (train 2026-08-22); envelope `h2-5h-b-b-4` | Corpus-inert foundation packet: the complete `transformES2015` owner (171 pinned local functions, `_tsc.js:104740-108100`) ported as the dormant `Es2015Transformer` — class lowering lanes, captured this/arguments/new.target, parameters, block-scoped bindings, loop conversion WITH the two pinned `yield*` synthesis sites feeding B-3's machine, spread, templates, object-literal chunking, for-of in both modes — the first production `FlattenHost`; qualified by 123 byte-equal focused oracle projections through the real `[transformES2015, transformGenerators]` chain + typed fault contracts; tagged-template lowering stays the B-5 flip and the corpus ratchet byte-identical. |
| [H2.5h-b / B-5 runtime flip: tagged-template module, joint registration, the 32-case witness gate](h2-5h-b-b-5.md) | Active production packet (train 2026-08-23); envelope `h2-5h-b-b-5` | The ladder's runtime packet: the `tagged-template` shared module (`processTaggedTemplateExpression`/`createTemplateCooked`/`getRawLiteral`, `_tsc.js:93972-94033`) with the `__makeTemplateObject` helper text replaces B-4's typed seam; the joint `[transformES2015, transformGenerators]` registration goes live at `languageVersion < ES2015` (admission floor ES5, `h2_5h_profile`); the 32-case witness fixture gate (CS-6 analog) drives all nine families end-to-end through the production checker resolver with the frozen oracle bytes as the entire expectation; gap row 12 flips `exists` (13/0/0) and outputs for targets ≥ ES2015 stay byte-identical. |
| [H2.5h / CA-1 corpus-adoption evidence: the `h2-5h-qualification` ES5-band observation sweep](h2-5h-ca-1.md) | Active production packet (train 2026-08-23); envelope `h2-5h-ca-1` | First corpus-adoption packet (evidence, mjs-only): the 932-row dependency-closed H2.5h band frozen as a contract-registered TypeScript oracle artifact — 850 compiler/conformance rows observed ×2 (exact writes+diagnostics, hermetic VFS, check receipt + shards ported from 5g; 806 admitted / 44 deferred to H2.9), 82 project rows typed-deferred to CA-3; ratifies the CA-1..CA-4 corpus-adoption ladder; zero crate Rust bytes, corpus ratchet byte-identical. |
| [H2.5h / CA-2b corpus-adoption seam closures: module keywords, the `__assign` fork, checker/harness report parity](h2-5h-ca-2b.md) | Active production packet (train 2026-08-23); envelope `h2-5h-ca-2b` | Cross-cutting cluster of the census-driven CA-2 split (≈38 of 212 failing rows): the five module-lowering declaration-keyword sites downlevel to `var` below ES2015; the `__assign` helper registered (B-1 protocol) with the es2018 AND JSX spread forks; TS2396 reported on the parameter; the harness `noEmitOnError` silent drop mapped; the programmatic `ignoreDeprecations` invalid-value row ({"5.0","6.0"} accepted). Independent review NOT-READY→amended (the draft's blocked-emit claim falsified — production lane proven upstream-exact); es2015-cluster stays CA-2a. |
| [H2.5h / CA-2a corpus-adoption seam closures: the ES2015/wrapper cluster](h2-5h-ca-2a.md) | Active production packet (train 2026-08-23); envelope `h2-5h-ca-2a` | The CA-2 split's second half (185 census rows = 63 promote-lane typed seams + 122 write-diffs): the promoteToIIFE exported/default/namespace/decorated lanes open per `moveModifiers` (elided modifiers + trailing export statement); comment-ownership threading (NodeArray ranges + the wrapper-chain containerPos claim propagation + the source-file detached-comments wrap); the rest-loop `_i` per-printer-scope finalize assignment (owning the hoist-numbering order); the assigned-name harvest (four upstream arms); the super-fold identity match under eager naming; the synthesized-let void-0 predicate. Independent review NOT-READY→amended (two draft diagnoses falsified and re-pinned: the ctor-body Block stamping is already upstream-exact, the void-0 lane is landed — the real loci are the printer projection and the parse-tree early-return). |
| [H2.5h / CA-3 project-suite observation harness](h2-5h-ca-3.md) | Active evidence packet (train 2026-08-24); envelope `h2-5h-ca-3` | The corpus-adoption ladder's fourth rung: the 82 project rows (41 descriptors × amd/commonjs) observe under the hermetic double-observation discipline (mjs-only — no crate bytes, no h1 ladder); the CA-1 `project_deferral` retires; OBSERVED 850→932; dispositions via the same `analyzeCase` classification; production project execution stays with CA-4's `run_h2_5h`. The mjs lane mirrors the T0-gated `build_project_fixture` rules (current dir `/.src/<projectRoot>`, explicit-inputFiles vs project-config root arms, per-variant module override). |
| [H2.5h / CA-4 acceptance wiring](h2-5h-ca-4.md) | Active runtime packet (train 2026-08-24); envelope `h2-5h-ca-4` | The corpus-adoption ladder's final rung: `run_h2_5h` (932 rows: 5g-style qualified-VFS for compiler/conformance, `load_project_emit` for projects, the CA-2b blocked-row contract) gated by the frozen divergence ratchet (`h2-5h-known-divergences` — every diverging admitted row joined to its named r1–r5/project residual owner; new divergence fails, stale entry fails, the manifest only shrinks); the local `h2-5h-oracle` phase; the `fn acceptance` append with the same-commit policy pin refresh; the h2-transition/profile-transition flips; the handoff close. |

The packet-control bootstrap added the shared versioned packet schema/checker
and completed its pre-closure purpose: FCI-1a through FCI-5b and the FCI-5c.1
membership shadow are landed non-authoritative assets (FCI-3c and FCI-5b are
`closed` with green proofs), and the checker is wired into
`node .github/ci/qualification.mjs check`. **The 2026-08-17 post-merge
roadmap review (Option A, recorded in the post-H1 schedule §1.2) pauses the
remaining Functional-CI tail** — FCI-5c.1b, FCI-5c.2, FCI-6 through FCI-8,
the FCI-9a/9b activations, and FCI-10 — in favor of completing the emitter;
their packets stay `design` with no production authorization and are
resumable only by an explicit post-H2.9 framework review that re-derives the
packet chain. Completed packet prose remains as history, while status and
immutable evidence live in the owning profile/ratchet. H2.5h-a is the next
active slice and requires its own machine-checked ready packet. Read-only
work may overlap only under an indexed packet; no stage-table row authorizes
production code.

The [functional CI framework and evidence architecture](../functional-ci-evidence.md)
owns a pre-closure shadow and post-closure activation migration to a
demand-driven typed impact graph,
adapter-owned deterministic plans and optional bundle interiors (H2 uses fixed
shards), content-addressed verified roots, complete local-full projection, and
exact-key hosted cache consumption. It preserves the hosted ts-tests-only scope
and owner-control exclusion. It does not change any command, count, or
acceptance requirement in the H2.5g legacy closure route below.

The first non-authoritative impact/restart shadow is available through
cargo xtask acceptance-plan and cargo xtask acceptance-slice. It is
conservative and fail-closed: shared or unknown inputs select all slices,
disconnected documentation/framework inputs select none, and every slice
failure records the environment/semantic restart class. These commands are
local evidence only; the hosted workflow remains the fixed unsplit command
until FCI-9b proves and activates the complete graph.

The H2.5g exception is limited to the legacy closing route in this document.
The packet-control bootstrap adds only the explicitly bounded FCI shadow
exception; it never changes the H2.5g authority and expires as a pre-closure
authorization at H2.5g close.

## H2.5g legacy closure route

H2.5g began before the implementation-ready packet gate was adopted. It is the
sole non-retroactive exception: it may complete without a newly manufactured
packet, but only under the existing versioned qualification, owner-control,
and profile contracts below, together with revalidation against the
[current emitter architecture](../emitter-architecture.md). This exception
cannot authorize H2.5h-a or any later production work.

This is a dependency-closure boundary, not an ES2016-file boundary. A frozen
H2.5g witness may directly expose an already-reachable composition defect in
an earlier transform, and that existing defect may be repaired as part of the
same closure. It does not authorize a new language feature, an unobserved
cleanup, or any H2.5h implementation.

For emitter facts, TypeScript 6.0.3 owns semantics; the current code and tests,
revalidated through the current architecture map, own the Rust structure.
Earlier H1, residual, inventory, and emitter-design prose is rationale or
history only and cannot override either authority.

Follow the closure records in this order:

| Role | Candidate closure record | Generator/checker | Schema |
| --- | --- | --- | --- |
| Dependency-closed corpus disposition and exact execution | [H2.5g qualification](../../../../ratchets/h2-5g-qualification.v1.json) | [qualification generator](../../../../crates/oracle/h2-5g-qualification.mjs) | [qualification schema](../../../../.github/ci/contracts/h2-5g-qualification.schema.json) |
| Direct transform-owner witnesses and controls | [H2.5g owner controls](../../../../ratchets/h2-5g-owner-controls.v1.json) | [owner-control generator](../../../../crates/oracle/h2-5g-owner-controls.mjs) | [owner-control schema](../../../../.github/ci/contracts/h2-5g-owner-controls.schema.json) |
| Runtime/profile transition after both records agree | [H2.5g profile](../../../../ratchets/h2-5g-profile.v1.json) | [profile generator](../../../../crates/oracle/h2-5g-profile.mjs) | [profile schema](../../../../.github/ci/contracts/h2-5g-profile.schema.json) |

The Node commands below are artifact/oracle freshness and schema checks. They
do not execute the Rust compiler candidate and cannot qualify H2.5g by
themselves:

```text
node crates/oracle/h2-5g-qualification.mjs --check
node crates/oracle/h2-5g-owner-controls.mjs --check
node --test crates/oracle/vfs-directory-overlay.test.mjs
node crates/oracle/h2-5g-profile.mjs --check
node --test .github/ci/qualification.test.mjs
node .github/ci/qualification.mjs check
```

The qualification unit test protects the fail-closed JSON Schema evaluator;
the final command uses it to validate each H2.5g artifact against its complete
checked-in schema in addition to checking the repository-wide CI policy. The
VFS overlay test is a separate three-case behavior check and is profile-bound;
a fresh artifact alone cannot substitute for it.

The CI policy also content-addresses the raw bytes of `crates/xtask/src/main.rs`
and all 14 modules reachable from the hosted `acceptance` entry, while pinning
that entry's complete statement shape and canonical module declarations. A
legitimate future ts-tests expansion therefore changes the source and its
policy pins together as an explicit reviewed transition; the pins are not
silently regenerated. Owner-control runners remain outside that hosted entry
and in the complete local gate.

The separate Rust runtime closure is mandatory. Run these commands on the
clean implementation/evidence commit that is proposed as the final validation
ref; a change to any runtime or evidence input creates a new candidate commit
and requires the applicable commands again.

| Purpose | Exact command | Required result |
| --- | --- | --- |
| Exhaustive zero-difference inventory | `cargo xtask h2-5g-inventory --start 0 --end 9027` | Final summary is exactly `H2.5g inventory complete: range=0..9027 cases=9027 admitted=8511 h2_8a_deferred=6 h2_9_deferred=510 failing_cases=0`. Exit success alone is insufficient because the diagnostic inventory deliberately continues after a mismatch. |
| Slice acceptance, including its owner-control tail | `cargo xtask h2-5g-acceptance` | The first line is exactly `H2.5g emit acceptance: candidates=9027 exact=8511 h2_8a_deferred=6 h2_9_deferred=510 exact_diagnostics=26815 exact_writes=9466 repetitions=2`, followed by exactly `H2.5g owner controls: controls=22 exact_writes=21 reported_diagnostics=2 repetitions=2`; the candidate owner artifact also retains one exact emitted diagnostic and one `noEmitOnError` control. |
| Independently reproducible owner controls | `cargo xtask h2-5g-owner-controls` | Exactly `H2.5g owner controls: controls=22 exact_writes=21 reported_diagnostics=2 repetitions=2`. |
| Fixed hosted boundary | `cargo xtask acceptance` | The unsplit `ts-tests` acceptance sequence exits successfully and reaches the same H2.5g corpus summary. Owner controls remain a separate local command; hosted CI is not expanded with them. |
| Complete resumable local CI | `cargo xtask ci --baseline 11f5d0abb93fed4b109bdb1dc552721ceb05e707` | Every selected local phase succeeds and the resumable gate reports completion. A reused phase is acceptable only when the command reports its exact inputs and outputs reusable. |

Only after all Node checks and Rust runtime rows pass on the same immutable
commit may that implementation/evidence commit be designated the **final
validation ref**. A following documentation-only commit records that ref,
binds the frozen profile to it, and promotes only its validated architecture
rows to `active-qualified`. The actual merge ref does not exist yet and must
not be predicted or self-referenced; the post-merge roadmap review records it
as delivery lineage, verifies that it contains the final validation ref, and
checks that every profile-bound runtime/evidence input remains byte-identical.

**Recorded outcome (2026-08-17):** every command and required result above is
green at the final validation ref `0653e10d84351c33ebd34d9442198ffff754722b`
(the `cargo xtask ci` row against trusted base `2df0b5be…`); the delivery
merge `507a96ac` is an ancestor of that ref, and the
[post-H1 schedule §1.2](../post-h1-completion-slices.md) records the
qualification, the process deviation, the twenty-nine gate repairs, and the
reviewed 180-second timed-conformance ceiling with its mandated performance
follow-up. The H2.5g profile is frozen at the validation ref. This closes the
sole legacy exception; all following production work requires a
machine-checked ready packet, and H2.5h-a is the next active slice.
