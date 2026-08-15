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
| [H2.5g legacy closure](#h25g-legacy-closure-route) | In progress; not yet qualified | Close the H2.5g dependency closure. Outside `transformES2016`, only repair a pre-existing composition difference directly exposed by a frozen witness; no new feature or H2.5h work. |
| [Packet-control bootstrap](packet-control-bootstrap.md) | In progress; one-time bootstrap | Add and verify the versioned readiness schema/checker and bootstrap ratchet. It may authorize only the explicitly listed pre-closure FCI shadow packets; it cannot authorize H2.5g authority, FCI-6+, workflow, or provider changes. |
| [FCI-0a framework boundary record](../functional-ci-evidence.md#14-migration-stages-and-packets) | Documentation record only; never a runtime-ready packet | Maintain the charter, package/trust boundary, v1 non-goals, qualification ladder, and navigation without changing H2.5g commands, counts, scope, or authority. |
| [FCI-0b extension API-manifest record](../functional-ci-evidence.md#31-workspace-public-api-manifest) | Documentation record only; never a runtime-ready packet | Maintain public/sealed ownership, blocking/cancellation/panic/error contracts, and later packet ownership without declaring a Rust item or authorizing production. |
| [FCI-1a core identifiers](fci-1a-core-identifiers.md) | Ready; shadow implementation in progress | Add only the generic `ci-core` identifier/digest/input seam. No graph, codec, adapter, outcome, cache, or effect behavior. |
| [FCI-1b inert adapter descriptors](fci-1b-adapter-descriptors.md) | Ready; shadow implementation in progress | Add only typed adapter descriptors and strict ordering checks. No codec, registry, dispatch, graph, outcome, cache, or effect behavior. |
| FCI-1c graph/profile/typestate seam | Blocked on FCI-1b and its own ready packet | Generic graph/profile/pending records remain separate; executable codec/registration waits for FCI-4a.3. |
| [FCI-2a-b runner seams](../functional-ci-evidence.md#14-migration-stages-and-packets) | Blocked on FCI-1c and two missing ready packets | No production edits; blocking error/cancellation ownership precedes bounded effect-result seams. |
| [FCI-3a-c canonical/execution/runner primitives](../functional-ci-evidence.md#14-migration-stages-and-packets) | Blocked on FCI-2b; each letter requires its own ready packet | No production edits. |
| [FCI-4a.1-a.3, 4b-d graph/registry/membership/inventory/impact/explanations](../functional-ci-evidence.md#14-migration-stages-and-packets) | Blocked on FCI-3c; every numeric/lettered boundary requires its own ready packet | No production edits; graph schema/rendering, structural validation, and sealed adapter preparation/membership/testkit remain separate before FCI-4b. |
| [FCI-5a-b tsc-rs protocol/control/harness](../functional-ci-evidence.md#14-migration-stages-and-packets) | Blocked on FCI-4d and their own ready packets | Protocol/control extraction and the miss-only harness remain separate; framework projections wait for FCI-6c. |
| FCI-5c.1 H2.5g inventory profile shadow | Blocked on FCI-5b and the final profile packet | Shadow only: exact 9,027-case membership and two-repetition observations; legacy H2.5g remains authoritative. |
| FCI-5c.2 complete H2 shadow | Blocked on H2.5g final validation, close/merge lineage, and packet rebind | Complete the remaining H2 adapter before FCI-6; no authority may be minted by the shadow. |
| [FCI-6a-e CAS/outcomes/capabilities/rollover/GC](../functional-ci-evidence.md#14-migration-stages-and-packets) | Blocked on FCI-5c; each letter requires its own ready packet | No production edits. |
| [FCI-7a-b, 7c.1-c.2 demand-driven local/composite shadow and framework qualification](../functional-ci-evidence.md#14-migration-stages-and-packets) | Blocked on FCI-6e; every lettered/numeric boundary requires its own ready packet | No production edits; the second real adapter precedes the API/conformance freeze. |
| [FCI-8a-f local shadow and hosted research/bootstrap/backend/shadow](../functional-ci-evidence.md#14-migration-stages-and-packets) | Blocked on FCI-7c.2; FCI-8b/8c are separate read-only protected-host/provider research and every letter requires its own ready packet | No production, bootstrap, workflow, or provider-backend edits; FCI-8a/8e append separately owned host/provider API partitions and FCI-8f freezes their exact union without reopening FCI-7c.2. |
| [FCI-9a-b activation](../functional-ci-evidence.md#14-migration-stages-and-packets) | Blocked on all FCI-8 proofs and separate activation approvals | No activation or workflow edits. |
| [FCI-10 cleanup](../functional-ci-evidence.md#14-migration-stages-and-packets) | Blocked on both FCI-9 activations and a missing ready packet | No cleanup edits. |
| [H2.5h-a](h2-5h-a.md) | Blocked through FCI-10, including H2.5g freeze/merge lineage and every Functional-CI shadow/activation gate | No production edits; preserve this handoff only. |

The packet-control bootstrap adds the shared versioned packet schema/checker
before changing FCI-1a or any later pre-closure shadow packet to `ready`. Each
successor still requires its own exact packet and predecessor receipt; the
post-H2.5g roadmap review rebinds FCI-5c.2 and later packets to the final
validation/merge lineage before they can become ready.
Completed packet prose remains as history, while its status and immutable
evidence move to the owning profile/ratchet. The hard order is bootstrap,
FCI-1a through FCI-5b, FCI-5c.1 shadow, H2.5g final validation/close/merge,
FCI-5c.2 through FCI-8 complete shadow, FCI-9a local-full activation, FCI-9b
hosted ts-tests-only activation, FCI-10 cleanup, then H2.5h-a. Read-only work
may overlap only under an indexed packet; no stage-table row authorizes
production code.

The [functional CI framework and evidence architecture](../functional-ci-evidence.md)
owns a pre-closure shadow and post-closure activation migration to a
demand-driven typed impact graph,
adapter-owned deterministic plans and optional bundle interiors (H2 uses fixed
shards), content-addressed verified roots, complete local-full projection, and
exact-key hosted cache consumption. It preserves the hosted ts-tests-only scope
and owner-control exclusion. It does not change any command, count, or
acceptance requirement in the H2.5g legacy closure route below.

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

These checked-in files are candidate closure records while H2.5g remains in
progress. Their presence, internal fields, and current counts do not declare
the slice complete: the final checks must pass and the
[post-H1 schedule](../post-h1-completion-slices.md) must record qualification
before the profile becomes frozen evidence. No H2.5h-a foundation artifact is
part of this closure route or an authority for H2.5g; the linked H2.5h-a file
remains only a blocked post-merge handoff. This is the end of the sole legacy
exception; all following production work requires a machine-checked ready
packet.
