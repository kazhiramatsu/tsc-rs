# Functional CI framework and evidence architecture

Status: **normative architecture for the reusable functional-CI framework and
its pre-closure shadow migration**. It does not replace or amend the
authoritative H2.5g closure contract.

This document owns the future reusable framework protocol and runner, their
extension API, adapter-owned deterministic plans and optional bundle interiors
(the tsc-rs reference adapter uses fixed H2 shards), content-addressed
evidence, verified-root capabilities, and exact hosted-cache consumption.
tsc-rs is the first reference application of this framework, not its semantic
or permanent package boundary. The
[evidence and steady-state contract](evidence-and-steady-state.md) continues to
own the existing B1-B4 and M9 workflows. The
[post-H1 schedule](post-h1-completion-slices.md) owns slice order, and the
[slice-packet index](slices/README.md) owns which packet may authorize work.

The current H2.5g qualification, inventory, owner-control, acceptance, hosted,
and local-CI commands remain exactly as recorded in the slice-packet index.
Nothing in this document changes an H2.5g count, permits a replacement command,
turns a cache into current H2.5g evidence, or declares H2.5g qualified. FCI-0a
and FCI-0b record this architecture only. A one-time packet-control bootstrap
may authorize the dependency-ordered FCI-1a through FCI-5b packets before
H2.5g closes, followed by the explicitly narrow FCI-5c.1 H2.5g inventory
profile shadow. These packets may produce only non-authoritative shadow
evidence; they cannot mint an H2.5g qualification, acceptance result, root,
capability, or hosted cache authority. The final H2.5g validation reference,
closure, and merge lineage remain a barrier before FCI-5c.2 and every later
FCI stage.

The hard gate is:

```text
packet-control bootstrap + versioned packet freeze
  -> FCI-1a through FCI-5b
  -> FCI-5c.1 H2.5g inventory complete-profile shadow
  -> H2.5g final validation reference + close/merge lineage
  -> FCI-5c.2 complete-H2 shadow
  -> FCI-6 through FCI-8 complete shadow
  -> FCI-9a local-full activation
  -> FCI-9b hosted ts-tests-only activation
  -> FCI-10 cleanup
  -> H2.5h-a
```

Read-only design, inventory, and provider-capability research may overlap when
an indexed research packet permits it. Pre-closure shadow production code may
start only after the bootstrap checker has accepted the specific packet and
marked it `ready`; a stage heading or this architecture document alone is not
authorization. Authoritative H2.5g commands and their evidence remain the only
closure authority until the later activation packets explicitly say otherwise.

## 1. Required result

For a versioned functional-CI profile, the semantic engine implements one
function:

```text
f(canonical inputs) = canonical semantic outcome
```

The same canonical inputs must produce byte-identical canonical outcome bytes,
independent of worker count, completion order, cache presence, filesystem
iteration order, wall clock, process id, temporary path, or runner-local
absolute paths.

The function has two semantic outcomes. Both are represented by one canonical
`OutcomeManifestV1` envelope:

```text
OutcomeManifestV1 = Passed(VerifiedTree) | Rejected(FailureTree)
```

`Passed` contains every raw observation required to recompute acceptance.
`Rejected` contains deterministic semantic differences or typed compiler
failures, references the complete sorted set of required action observations,
and is as reproducible and content-addressable as `Passed`. A rejected
manifest may be stored and exactly reused in a non-acceptance namespace, but it
can never mint an acceptance capability. A deterministic rejection does not
cancel sibling semantic actions: the evaluator completes the required action
set and constructs `FailureTree` in stable node-id order. Completion-order
fail-fast is forbidden because it would make failure bytes scheduler-dependent.

Infrastructure failure is not a third semantic outcome: an I/O
failure, worker panic, spawn failure, timeout, cancellation, out-of-memory
termination, required-input transport failure, or source-snapshot/guard
mutation before semantic completion means that `f` was not evaluated. A failure during the
generation-guarded publication step means canonical bytes may have been
computed but no authoritative result was committed. Infrastructure failure
publishes no authoritative outcome receipt, capability, or reusable
action-index generation and is never reusable as a semantic result; unreachable
staged/CAS bytes may remain. A sandbox child exit that the adapter's versioned
classifier deliberately models is a semantic observation; a runner panic,
signal/forced termination, timeout, or missing observation is infrastructure
failure.

`NondeterminismDetected` is likewise an invariant violation, not a third
semantic outcome. Repetition inequality or multiple authority-valid canonical
objects for one action key inside the sealed evidence snapshot, including a
different fresh result produced by that invocation, means the repository has
not implemented the stated function. The runner publishes no semantic outcome,
candidate authority, capability, or action-index winner. It must instead
linearize a nonsemantic, monotonic `ConflictRegistryV1` update in the authority
control plane before returning `NondeterminismDetected`. The update removes the
conflicted key's discoverable candidate set and pins the independently verified
witnesses, tombstone, conflict-authority receipt, and sealed-snapshot reference.
Its distinct `IndexCommitGuard<LocalConflictCommit>` cannot mint any root.
Staged or immutable semantic CAS bytes written before the conflict was known
may remain unreachable; their existence grants no authority and GC may later
collect them. Authority-control conflict evidence remains pinned while that
action key exists.

A capability is minted only after its local commit linearizes against the
local index-head generation bound by that invocation's sealed snapshot and
final commit check. Remote candidate bytes and authority proofs are immutable
inputs frozen when that snapshot is sealed; no distributed transaction with a
remote index is claimed or required. A distinct valid candidate first observed
after a completed outcome commit must first pass the same monotonic conflict
commit and then invalidates future reuse of that action key on that authority
store, but cannot
retroactively change that linearization point, revoke or rewrite an already
returned non-serializable in-process capability, or turn it into stored
authority. Historical receipts remain immutable evidence of the snapshot
against which they were minted; they are not proof that no later conflict was
published. A missing, corrupt, rolled-back, or over-limit conflict registry is
infrastructure failure, never a cache miss or permission to execute under the
same key. A valid tombstoned action recovers only through a corrected
semantic/schema input and therefore a new action key; v1 exposes no candidate-controlled
tombstone deletion API. An authority-store candidate/index failure may require
protected exact repair/restore or a protected authority-store/locator epoch
rollover that makes every old candidate and receipt ineligible for future
lookup, publication, or capability minting and requalifies every eligible,
nonconflicted action fresh under a new storage namespace. The mutable
store/locator epoch is not a semantic action-key input. The replacement head must
carry the exact authenticated predecessor conflict set under the unchanged
action keys as well as the predecessor disclosure union. If the conflict set
is missing, corrupt, rolled back, or otherwise unverifiable, rollover is
forbidden and only an exact authenticated restore may recover the store.
Capacity exhaustion is handled only by membership-preserving authenticated
compaction, never by rollover. Before an allowed rollover may
commit, a store-wide exclusive epoch barrier stops new old-epoch work and
drains every old-epoch lease and invocation. It does not retroactively revoke,
rewrite, or relabel a non-serializable in-process capability already returned
by a completed invocation.
Capacity repair may compact the exact conflict set into an authenticated Merkle
representation but cannot forget membership. A rollover is recorded outside
candidate state, rejects the prior storage epoch for all future authority,
retains both old conflict and disclosure-history roots as predecessor
commitments, and is never represented as proof that previously conflicted
semantics became deterministic or previously disclosed bytes became secret.

Remote candidate-index/object transport is optional and outside semantic
execution. A normal absence, invalid entry, or transport failure classified by
the checked-in profile as `cache-unavailable-as-miss` is frozen as an explicit
miss before scheduling and executes fresh; it cannot change canonical bytes.
For a profile that admits shared evidence, the small authenticated monotonic
`RemoteAuthorityHeadV1`, containing the monotonic conflict root and
disclosure-history root plus the predecessor and append-only publication-event roots, is
separate protected authority-control transport and is
mandatory even when candidate objects are optional. It is pinned by the
protected run capsule and checked before acquisition; absence, rollback,
corruption, or inability to prove freshness is infrastructure failure, not a
fresh fallback. A profile that disables all shared read/write authority may run
locally fresh without that remote channel and cannot publish a shared result.
Failure to upload after a locally
complete outcome never changes or erases that outcome; it changes only the
publication receipt and shared-cache availability.

Timestamps, nonces, runner ids, retry counts, logs, and authentication belong
to a separate execution receipt. They must not enter canonical semantic bytes
or their content address.

## 2. Authority and language boundary

Responsibilities are fixed:

| Owner | Responsibility | Forbidden responsibility |
| --- | --- | --- |
| Rust `ci-core` | Pure canonical values, action/impact graph, fingerprints, outcomes, Merkle verification, policy-state vocabulary, and projections | Filesystem, process, network, clock, cache transport, or compiler-specific dependencies |
| Rust `ci-runner` | CAS and cache effects, atomic publication, bounded scheduling, sandbox invocation, authenticated receipt handling, and explanations | Compiler-specific semantics or changing a pure result based on cache/scheduler behavior |
| Repository adapter (`ci-adapter-tsc-rs-control` for the reference application) | Application namespace, typed graph/node/action schemas, semantic execution hooks, verification, aggregation, projections, and profile membership | Adding repository/compiler branches to generic crates, linking candidate production/compiler code into the protected control plane, or bypassing framework verification/publication contracts |
| Candidate action harness (`ci-harness-tsc-rs` for the reference application) | Execute one adapter invocation against the mounted candidate snapshot and return one bounded canonical observation through the adapter protocol | Decide cache authority, aggregate a root, publish evidence, load into the protected control process, or provide a verifier/profile implementation |
| Framework testkit | Reusable fake adapters/backends, compile-fail fixtures, golden protocol fixtures, and adversarial conformance helpers | Production authority, repository semantics, provider credentials, or a dependency from a production runtime path |
| Remote provider adapter (runner-side, selected after FCI-8c) | Map one frozen storage/attestation provider to bounded reads, immutable objects, exact candidate indices, one selected atomic publication capability, authentication, and typed transport failures | Interpret repository nodes or semantic outcomes, invent keys, select trust from candidate content, provide an inexact fallback, or mint a local root capability |
| Node | Generate and check pinned TypeScript oracle observations and compatibility fixtures | Mint a verified Rust root or decide that cached Rust execution is acceptable |
| Nix, if adopted | Pin tools and provide an optional outer sandbox/derivation for Rust and Node commands | Define shard membership, canonical bytes, invalidation, summaries, or verified-root policy |
| GitHub Actions | Invoke the one hosted command and transport exact-key cache bytes and authenticated producer receipts | Recreate shard logic, inspect summaries to decide success, use prefix restore, or add owner-control scope |

Rust is the only semantic CI language. Nix is optional and thin: removing Nix
must not change a semantic schema, action/content digest, shard, root,
projection, or acceptance result. It may change a nonsemantic build-artifact or
execution-receipt identity.
Bazel/Starlark, Haskell, OCaml, and TypeScript are not alternate semantic
engines for this repository. Introducing one would create a second build graph
or a second canonicalization and policy authority without removing the Rust
execution boundary.

### 2.1 Framework charter and qualification

This is an incubating reusable **functional-CI framework**. tsc-rs is its first
application and reference adapter, not its permanent domain or package
boundary. The framework is intended to make a repository's CI a deterministic function over
typed inputs, preserve its complete logical gate membership, and reuse only
exact verified evidence. A successful H2 run by itself proves only the tsc-rs
adapter; it does not qualify the framework abstraction.

The four extension rings and their dependency direction are fixed:

```text
repository adapter ----> ci-runner ----> ci-core
        |                    ^
        +------> ci-core     |
provider adapter -----------+
```

- `ci-core` is the protocol kernel. It owns canonical bytes, domains/digests,
  typed graph relationships, outcomes, verification vocabulary, and pure
  composition. It has no effect or application/provider dependency.
- `ci-runner` is the capability-safe effect engine. It owns bounded execution,
  staging, local authority publication, cache transport orchestration,
  scheduling, receipts, resource enforcement, and explanations. It depends on
  `ci-core`, never on a repository or concrete provider.
- a **repository adapter** owns every domain noun and decision: application
  namespace, typed node/action/observation/root schemas, inventory and owner
  mapping, execution hook, verifier, aggregate, projection, and profile
  membership. It may depend on both framework crates.
- a **provider adapter** implements the runner's bounded transport and one
  reviewed publication strategy for a concrete remote service. It owns
  provider locators, API calls, authentication/attestation decoding, quotas,
  retry classification, and recovery. It does not depend on a repository
  adapter and cannot interpret or alter semantic payloads.

The workspace-public extension surface is deliberately small. Repository
integration uses `ActionModel`, `AdapterCodec`, `Projection`, typed
`AdapterRegistration`, versioned `ActionInvocationV1`,
`CompositeProfileV1`/`CompletePhaseRegistryV1`/`AdapterInstanceRefV1` values,
and strongly typed adapter ids/specs. Effect integration uses
`SourceSnapshotProvider`, `Sandbox`, `CasBackend`, `ExactCacheBackend`, and one
provider-neutral `AtomicSnapshotPublisher` capability whose concrete backend
is selected by the frozen protected profile. Protected host composition alone
uses `AdapterRegistryBuilder::seal` and the eventual blocking
`Runner::evaluate` entry; adapters receive neither constructor authority nor a
second evaluation callback. These are public so another workspace adapter can
implement them; they are not a crates.io stability promise. Canonical encoding
and hash framing, graph closure rules,
outcome/Merkle validation, same-key conflict handling, authority commit order,
and constructors for `VerifiedPolicySpec`, `IndexCommitGuard`, and
`AuthorizedRoot` are framework invariants, not extension points. No adapter may
override them with a callback, policy trait, string kind, or unchecked
constructor.

Framework qualification requires the FCI-7c.1 second adapter followed by the
FCI-7c.2 API/conformance freeze. The required `workspace-audit` adapter has no
TypeScript/compiler/oracle observation, case
corpus, repetition policy, shard/bundle interior, or H2 projection. It uses a
flat leaf set and its own audit observations, while using the same canonical
input/key, typed graph, impact, outcome, runner, local CAS, sealed snapshot,
explanation, verified-root, and composite-profile paths. Qualification passes
only when both H2 and workspace-audit use one frozen generic API with:

1. no adapter-id/kind match, downcast, opaque domain string, or repository noun
   in generic implementation code;
2. no dependency from a framework crate to either adapter or production code;
3. byte-identical results on replay and exact complete suboutcomes for both
   shapes; and
4. the generic adversarial suite plus each adapter's contract suite passing.

If implementing the second adapter requires a generic API change, that change
is a new reviewed framework packet: its signature, errors, invariants, and
two-adapter fixtures are frozen first, then both adapters are rerun. The framework
is not called qualified until the changed API satisfies the four checks and
FCI-7c.2 records the frozen API manifest. FCI-7c.2 qualifies reuse inside this
workspace for the repository/core/local-runner surface. FCI-8e later freezes
the independently owned provider-publication SPI and may not reopen that
adapter surface. External publication/extraction remains a separate future decision.
Complete tsc-rs framework migration still means FCI-10, after the distinct
FCI-9a local and FCI-9b hosted activations.

Qualification is deliberately graduated; an earlier level cannot claim a
later one:

| Level | Closing packet or gate | Claim permitted |
| --- | --- | --- |
| Architecture recorded | FCI-0a boundary record plus FCI-0b API-manifest record | The intended framework/application/provider boundaries and the owner of every future public seam are normative. This is documentation only and grants no implementation authority. |
| Generic seam proved | FCI-4a.3 | Two structurally different in-memory fake adapters pass the graph, sealed registry, preparation, membership, and negative-dependency contracts. No real repository reuse claim is permitted. |
| Workspace framework qualified | FCI-7c.2 | The real H2 and workspace-audit adapters use one frozen API and shared conformance kit without a generic branch or downcast. |
| Complete shadows and final extension manifest proved | FCI-8a and FCI-8f | The complete local and hosted logical denominators agree with their existing authoritative commands while reuse remains disabled; the appended host/provider API partitions are frozen without reopening the FCI-7c.2 adapter/local surface. |
| Activated | FCI-9a and FCI-9b separately | The applicable local-full or unchanged hosted ts-tests-only boundary may consume verified reuse after its separate approval. |
| Reference migration complete | FCI-10 | tsc-rs duplicate qualifying traversals are retired while the framework protocol and historical readers remain. |

External packaging, crates.io publication, public SemVer, or support for an
unrelated workspace is not implied by any level above. A later distribution
design must add a separate compatibility/support matrix and external-workspace
conformance proof without reopening the tsc-rs activation gate.

#### 2.1.1 Functional-CI v1 non-goals

Functional-CI v1 is a deterministic verification-and-evidence framework, not
a second general-purpose workflow language. Its boundaries are fixed:

- it stores and revalidates semantic evidence; it does not distribute or
  execute restored build artifacts, and no remote-restored file becomes an
  executable, library, source input, or `PATH` entry;
- it runs bounded actions on one runner and does not define distributed remote
  execution, speculative execution, or a cross-run scheduler protocol;
- repository and provider registrations are compiled into a protected closed
  registry; v1 has no runtime-loaded plugin, candidate-selected callback, or
  opaque string dispatch;
- it does not replace deployment/release automation or general secret-bearing
  jobs. A typed `NonReusable` effect gate may represent such a mandatory local
  phase, but it publishes no reusable semantic evidence and cannot enter a
  hosted `ReuseAllowed` root;
- it does not define a second build graph, compiler package manager, or
  provider-specific workflow DSL. Existing build tools may produce a
  miss-only harness, but their opaque cache is not semantic authority;
- v1 requires exact protocol/schema matches and exposes no payload migration,
  version negotiation, prefix restore, or best-effort compatibility hook; and
- the workspace-private Rust API is reusable by checked-in adapters but is not
  a crates.io stability promise during this migration.

Version and portability boundaries are independent:

| Boundary | Version authority | Required compatibility behavior |
| --- | --- | --- |
| Framework protocol | `ProtocolDomainV1`, canonical/schema ids, and store wire/layout version | A byte/hash interpretation change uses a new domain or schema and golden fixtures; old authority is never silently reinterpreted. |
| Application adapter | `ApplicationNamespaceV1`, adapter id/schema, graph/action/root specs, and semantic implementation ids | A semantic change invalidates exactly its declared closures. A changed payload uses a new exact schema/domain/namespace; any separately authorized total offline converter writes new objects and is not a v1 reader migration/negotiation hook. |
| Provider adapter | FCI-8c capability record, provider/API version, selected publication strategy, namespace epoch, and trust-root binding | A provider change may change availability/receipts, never canonical semantic bytes; an unsupported atomicity, durability, auth, or quota contract fails closed. |
| Execution platform | `ExecutionPlatformV1`, `ToolchainSetV1`, sandbox ABI, and runner capability probes | Platform-bound actions get distinct keys; a reviewed platform-independent action must pass partition-invariance fixtures on every admitted class. |

`ci-core` must produce the same golden canonical bytes and digests on every
supported host. `ci-runner` is portable by explicit backend capability, not by
assuming POSIX behavior: a filesystem, sandbox, process-quota mechanism, or
provider that cannot prove the required primitive is unsupported for
authoritative execution and fails closed. Replacing a repository adapter or a
provider adapter must not require editing the other or adding a branch to the
framework kernel.

### 2.2 Protected consumer engine and candidate-as-data boundary

A hosted candidate is never the program that decides whether that candidate
passed. The candidate checkout is an untrusted `CandidateTreeSnapshotV1`
consumed as data by a separately acquired protected consumer engine. This is
the security boundary that makes the capability types below meaningful against
an adversarial pull request rather than only against accidental misuse in
trusted Rust code:

```text
protected workflow/bootstrap
  -> protected run envelope
  -> signed engine channel
  -> attested consumer-engine binary
  -> protected base/profile/trust capsule
  -> candidate tree as read-only semantic input
```

The protected run envelope binds repository and provider-event identities,
target base, candidate head and exact tested-tree digests, protected workflow
identity, engine-channel generation, engine release/attestation digests,
authority-capsule and trust-root digests, hosted resource ceilings, and the
read-only cache-access class plus the required remote authority-head
channel/generation for a shared profile. The release manifest binds the exact source tree,
platform, binary length/digest, build attestation, bootstrap ABI, supported
protocol/graph schemas, semantic implementation registry, verifier bundle, and
protected fallback plan. The bootstrap selects every one of these values from
protected state. A candidate URL, version, digest, channel, trust root,
workflow, `.cargo/config.toml`, Cargo alias, `PATH`, toolchain file, profile,
graph, verifier, or exit-code implementation is only candidate data or a
transition proposal and cannot become authority by conversion.

`ConsumerEngineIdentityV1` is a serializable receipt value and grants no
authority. Only the bootstrap may construct the private, noncloneable,
nonserializable `VerifiedConsumerEngine` that holds the race-free executable
image capability plus that identity. Hosted verification and root
authorization consume this effect-bound value; decoding an identity record or
rehashing a candidate binary cannot recreate it.

The bootstrap verifies an exact signed channel generation and release manifest,
issuer/subject/audience/workflow/base bindings, build attestation, bounded
download, and binary digest, then executes an immutable opened image or an
equivalent race-free platform primitive. `latest`, a branch-name or prefix
lookup, candidate-provided fallback, and building the control engine from the
candidate checkout are forbidden. Acquisition or attestation failure happens
before candidate execution and produces no outcome, receipt, or capability.

The visible hosted semantic command remains exactly one unsplit, argument-free
command:

```text
cargo xtask acceptance
```

After FCI-9b it runs from a protected control directory with an attested
external `cargo-xtask` on a bootstrap-fixed `PATH`/`CARGO_HOME`; it does not run
from the candidate tree or one of its ancestors. The candidate snapshot is
mounted read-only/no-exec at a separate logical input root, and
invocation-private output is separate again. Provider/bootstrap preparation may establish
that envelope but cannot itself manufacture a semantic pass. FCI-8b must prove
that the selected required-workflow/provider mechanism preserves this exact
one-command surface; otherwise hosted activation is unavailable.

The protected engine has two wire-separated halves. Its lightweight control
plane contains the framework, registered adapter schemas, graph/inventory
rules, invocation builders, strict decoders, verifiers, aggregation, and
projections, and must not link candidate production/compiler crates. A
candidate action harness or compiler is a declared executable artifact built
and spawned in the sandbox only for an acquisition miss. It receives one
`ActionInvocationV1` and returns bounded canonical observation bytes; it never
runs as a library callback in the trusted process. Consequently a fully warm
no-impact run does not build or spawn a candidate compiler, oracle, test, build
script, or action harness.

Consumer-engine changes use N+1 promotion. Signed engine `E_n` evaluates an
engine/verifier/profile proposal with reuse and remote publication disabled,
using the complete protected fresh fallback when it can represent the safe
prior/current union. If it cannot, the result is
`RequiresStagedEnginePromotion`, never execution of the candidate engine as
authority. A secret-free protected build creates `E_(n+1)`; `E_n` and the
protected harness verify its canonical, adversarial, and complete-fresh
fixtures; the new engine shadows; and a protected signer atomically
advances the channel. Only a later invocation may use `E_(n+1)` as authority.
An engine cannot approve its own release manifest or channel advancement.

Producer and consumer receipts, `PolicyProof`, and `HostedVerifiedRoot` bind
the exact consumer/producer engine binary and release-manifest digests, channel
generation, protected workflow identity, authority capsule, candidate-tree
digest, evaluation mode, evidence snapshot, root action key, and outcome
digest. Engine identity is authority/provenance and does not enter semantic
outcome bytes or unrelated action keys; the relevant semantic implementation
digest remains in the reviewed action closure.

Every pull request, including a same-repository PR, is untrusted. It receives
no remote publication, repair, rotation, signing, or accepted-root capability.
Authenticated remote reads, when the provider can issue a proven read-only
audience-scoped credential, finish before candidate-controlled execution; the
credential and descriptors are then revoked and closed. Candidate processes
run without network, parent environment, control directory, provider sockets,
GitHub environment-command files, or readable host `/proc`, and their output is
escaped log data rather than workflow commands. All candidate process groups
are dead and the sandbox is destroyed before a trusted post-run publisher may
obtain write authority. If those separations cannot be proved, remote evidence
is an exact miss and the complete required set executes fresh; a broader
credential is never substituted.

## 3. Crate and dependency placement

Add the workspace-private framework, testkit, and reference-application
packages only through their separate ready packets. The tree below is the final
provider-neutral package map after the migration, not the file set created by
FCI-1 or FCI-2; FCI-8c/FCI-8e append the one separately researched provider
package as described below.
FCI-1a creates only `ci-core` identifiers and dependency guards; FCI-1b and
FCI-1c add only adapter-descriptor and graph/profile/typestate record seams;
the executable codec/registration API waits until FCI-4a.3 after strict decode
and all types in its signature exist. FCI-2a creates the `ci-runner` crate and blocking
error/cancellation taxonomy; FCI-2b adds only bounded effect seams and fakes.
The complete `RunContext` and `Runner::evaluate` appear only at FCI-7b after
all of their argument types exist. The shared
`ci-testkit` package is extracted at FCI-4a.3 after both framework crates and
both fake adapter shapes exist. The tsc-rs protocol/control adapter packages
start only at FCI-5a, its candidate harness at FCI-5b, and the independent
workspace-audit adapter at FCI-7c.1. FCI-3b owns
invocation and sandbox-identity value types. FCI-3c owns source snapshots,
mounted-source/path types, the `SourceSnapshotProvider` and `Sandbox` traits,
staging, scheduling, and resource types as one invariant set. Because
`Sandbox::execute` consumes `MountedSourceSnapshot`, the trait is not declared
in FCI-3b. `CasBackend`, `ExactCacheBackend`, the local conflict/outcome
publishers, and `AtomicSnapshotPublisher` are not forward-declared:
FCI-6a owns `CasBackend`; FCI-6b owns `ExactCacheBackend` and
`LocalConflictPublisher` plus the local/remote-neutral `PublicationEventV1`
schema; FCI-6c owns `LocalOutcomePublisher`; and FCI-8e owns
`AtomicSnapshotPublisher` and applies that event schema to remote authority.
Each appears only after every type in its signature exists. Later rows in
section 14 add the remaining modules when their
invariants and consumers exist. Empty placeholder modules must not be added
merely to make the final tree appear early.

```text
crates/ci-core/
  Cargo.toml
  src/lib.rs
  src/canonical.rs
  src/digest.rs
  src/input.rs
  src/profile.rs
  src/graph.rs
  src/impact.rs
  src/outcome.rs
  src/merkle.rs
  src/policy.rs
  src/projection.rs
  tests/unit/

crates/ci-runner/
  Cargo.toml
  src/lib.rs
  src/path.rs
  src/cas.rs
  src/publication.rs
  src/scheduler.rs
  src/sandbox.rs
  src/cache.rs
  src/receipt.rs
  src/explain.rs
  src/resource.rs
  tests/unit/

crates/ci-testkit/
  Cargo.toml
  src/lib.rs
  src/adapter.rs
  src/backend.rs
  src/conformance.rs
  src/golden.rs

crates/ci-adapter-tsc-rs-protocol/
  Cargo.toml
  src/lib.rs
  src/invocation.rs
  src/observation.rs
  src/root.rs

crates/ci-adapter-tsc-rs-control/
  Cargo.toml
  src/lib.rs
  src/registry.rs
  src/plan.rs
  src/invocation.rs
  src/verify.rs
  src/aggregate.rs
  src/projection.rs

crates/ci-harness-tsc-rs/
  Cargo.toml
  src/main.rs

crates/ci-adapter-workspace-audit/
  Cargo.toml
  src/lib.rs

crates/xtask/
  # CLI parsing, protected composition, and command projection only;
  # no repository semantic implementation remains here after FCI-10.
```

Package identities and roles are fixed:

| Directory | Cargo package / Rust target | Role and dependency boundary |
| --- | --- | --- |
| `crates/ci-core` | `tsc-rs-ci-core` / `tsc_ci_core` | Pure framework protocol; no I/O or other workspace dependency. |
| `crates/ci-runner` | `tsc-rs-ci-runner` / `tsc_ci_runner` | Generic effect engine; depends on `ci-core`, never an adapter or production crate. |
| `crates/ci-testkit` | `tsc-rs-ci-testkit` / `tsc_ci_testkit` | Development/test-only conformance helpers; may depend on both framework crates and cannot enter an authoritative binary's normal dependency closure. |
| `crates/ci-adapter-tsc-rs-protocol` | `tsc-rs-ci-adapter-protocol` / `tsc_ci_adapter_protocol` | Pure tsc-rs invocation/observation/root wire schema shared across the process boundary; depends on `ci-core`, never production or `ci-runner`. |
| `crates/ci-adapter-tsc-rs-control` | `tsc-rs-ci-adapter-control` / `tsc_ci_adapter_control` | Protected reference adapter; depends on the protocol, `ci-core`, and only the public `ci-runner` SPI, never candidate production/compiler crates. |
| `crates/ci-harness-tsc-rs` | `tsc-rs-ci-harness` / `tsc-rs-ci-harness` binary | Candidate-side miss-only action executable; may depend on the protocol and production/compiler crates, never on control, cache, publication, or authority code. |
| `crates/ci-adapter-workspace-audit` | `tsc-rs-ci-adapter-workspace-audit` / `tsc_ci_adapter_workspace_audit` | Independent shard/repetition/compiler-free qualification adapter; no production/compiler dependency. |
| `crates/xtask` | existing package/binary | Protected/local host composition, CLI, profile selection, and final projection only; no adapter wire, graph, verifier, aggregate, or harness implementation after FCI-10. |

The root dependency aliases for the three generic libraries are
`tsc-ci-core`, `tsc-ci-runner`, and `tsc-ci-testkit`. Every listed new manifest
sets `publish = false`. They are repository support packages, not a promised
external API and not candidates for publication until a separate future design
approves that change. The selected provider adapter is intentionally absent
from this pre-research file map: FCI-8c freezes one exact provider package name,
dependency set, and capability contract, and only FCI-8e may add it. It may
depend on `ci-runner`, never on either repository adapter.

Workspace-private is a distribution and stability boundary, not permission to
specialize the framework for H2. The framework API must remain reusable across
independently typed adapters, with H2 and the structurally different
workspace-audit adapter as the first two proofs. Whether the crates are later
extracted or published is a separate decision and cannot weaken this
in-workspace genericity requirement.

This is an independent, reusable functional-CI framework with repository
adapters, not a tsc-rs-specific cache optimization. `ci-core` owns only the
pure function/graph/evidence protocol; `ci-runner` owns only bounded effects and
publication; an adapter supplies application namespace, graph/node schemas,
execution semantics, verification, aggregation, and projection. The internal
Cargo package names may retain `tsc-rs` while the framework is incubated, but
those names have no semantic role. Moving the generic crates to another
workspace must require only dependency/package wiring and new adapters, never
removal of an H2, Cargo, branch, shard, or repository special case from generic
code.

`ci-core` may depend on `serde`, `serde_json`, and `sha2`; it performs no I/O and
depends on no other workspace crate. `ci-runner` depends on `ci-core` and only
generic effect-support dependencies. Neither crate may depend on a `tsc-*`
production, oracle, harness, conformance, or xtask crate. `ci-core` must not
depend on `ci-runner`.

tsc-rs-specific semantics live in its protocol/control/harness packages, not
in `xtask` and never in a generic framework crate. During shadow migration,
`xtask` may retain an explicitly indexed legacy forwarding module only until
the owning FCI-5a through FCI-5c packets move that implementation; it cannot be
a second semantic owner, and FCI-10 removes the forwarding path. Only the
protected control-engine/xtask composition depends on both framework crates
and the control adapter. The control adapter consumes production source and artifact
identities as data but must not link a candidate production/compiler crate.
Miss-only candidate action harnesses are separate executables described by
`ActionInvocationV1`; they communicate only through bounded canonical wire
bytes and never load into the trusted process. The production roles
`syntax`, `types`, `diagnostics`, `binder`, `host`, `program`, `emitter`,
`checker`, and `compiler` must not depend on role `ci-core` or `ci-runner` in
normal, development, build, or target-specific dependencies. The workspace
audit must enforce those negative edges. CI types do not appear in production
APIs. The protected adapter strictly decodes harness observations into
canonical CI values after sandbox execution. Local development may build both
assemblies from one workspace command, but their Cargo dependency closures and
process boundary remain independently auditable; a warm control-plane lookup
cannot trigger a candidate compiler build.

The generic crates own framework mechanics only. An H2 case id, H2
disposition, owner-control type, TypeScript observation, compiler option, or
`EmitOutcome` never appears in either crate. Those belong in the repository
adapter's versioned wire schema or in its isolated candidate harness, not in a
generic crate or a trusted in-process dependency on candidate production code.
H2 is the first adapter, not the framework's type model.

### 3.1 Workspace-public API manifest

FCI-0b owns the normative API-manifest record below. It freezes which packet
must introduce each public seam, the owning crate, its error family, and the
constructor that remains sealed. It does not create a Rust item, reserve an
empty module, mark an implementation packet `ready`, or permit an implementer
to invent a missing signature. Before each owning packet becomes `ready`, that
packet must replace the applicable conceptual signature below with its exact
Rust signature, full fields/bounds/visibility, errors, and compile-fail
fixtures. The packet may make a surface smaller; it may not add an adapter
escape or move a responsibility across this table without first amending
FCI-0b.

| Surface | Owning packet | Workspace-public extension | Sealed/private invariant |
| --- | --- | --- | --- |
| Canonical identifiers and adapter descriptors | FCI-1a/FCI-1b | Typed ids plus inert descriptor/schema records | Codec bounds, monomorphized registration, descriptor-set validation, and any executable registry remain unavailable. |
| Graph/profile/root and pending/complete record seams | FCI-1c | Generic records and checked proposal builders that name no future codec/outcome type | Complete membership, adapter traits, and prepared-plan constructors remain unavailable. |
| Blocking effect/error vocabulary | FCI-2a/FCI-2b | `InfraError`, `RunCancellation`, bounded chunk/result seams | No `RunContext`, worker, snapshot, sandbox, cache, or publication interface exists yet. |
| Canonical codec and execution values | FCI-3a/FCI-3b | Strict bounded codec plus typed invocation/repetition/reuse/tool/platform values | Domain registration, canonical framing, and `PreparedExecutionV1` construction are framework-owned. |
| Snapshot/sandbox/resource SPI | FCI-3c | `SourceSnapshotProvider`, `Sandbox`, resource claims and bounded readers | Snapshot/sandbox guards and authority-bearing constructors remain private. |
| Model/codec, registry seal, prepared executions, and composite membership | FCI-4a.3 | Checked adapter model/codec registrations and registry-builder input | Only `AdapterRegistryBuilder::seal` yields `VerifiedAdapterRegistry`; only core preparation yields `PreparedExecutionV1` or a complete typed input. |
| CAS/cache/outcome/projection/local commit SPI | FCI-6a through FCI-6c | Bounded backend/local publisher traits and typed projection registration | Verified objects, commit guards, verified outcome views, complete outcomes, and authority constructors remain sealed. |
| Live planning and evaluation entry | FCI-7a/FCI-7b | Explicit immutable source/evidence snapshots and `Runner::evaluate` | The runner, not an adapter/backend, owns scheduling, retry, conflict handling, final commit order, and `RunError`. |
| Remote atomic publisher | FCI-8e | One provider-neutral `AtomicSnapshotPublisher` capability | Concrete provider strategy, credentials, remote guards, and trust selection remain provider/protected-host private. |

This manifest freezes in dependency-ordered partitions rather than pretending
that a later type already exists. FCI-7c.2 freezes the repository/core/local
runner partition. FCI-8a appends and freezes the host dispatcher and
`CompletePhaseRegistryV1` partition. FCI-8e appends and freezes the
provider-publication partition after FCI-8c research. FCI-8f records the exact
union as the final workspace-public manifest and reruns the earlier conformance
suite. A later partition may depend on an earlier one; it cannot change its
signature, errors, visibility, semantics, or qualification evidence without a
new FCI-0b amendment and all affected prior proofs.

The minimum final cross-crate API shape is generic over adapter-owned
semantics. This is the post-FCI-8f shape, not the API that FCI-1a through
FCI-1c must create in one commit. Those subpackets introduce only canonical
identifiers, inert descriptors, `NodeClass`, graph/profile/root records,
pending/complete record shapes, and composite-profile references needed by the
two fake data shapes. `ActionModel`, `AdapterCodec`,
`AdapterRegistration::of`, strict runtime preparation, verification,
aggregation, projection, outcome, CAS, cache, and capability methods shown
below are added
only by the migration packet that introduces their owning types and invariants:

```rust
// ci-core: pure, deterministic, no I/O.
pub trait CanonicalSink {
    fn write(&mut self, bytes: &[u8]) -> Result<(), CanonicalError>;
    fn remaining(&self) -> u64;
}

pub trait CanonicalEncode {
    fn encode_canonical<S: CanonicalSink>(
        &self,
        out: &mut S,
    ) -> Result<(), CanonicalError>;
}

pub trait StrictCanonicalDecode: Sized + CanonicalEncode {
    const SCHEMA: SchemaId;
    const MAX_BYTES: u64;
    type Decoder: CanonicalDecoder<Output = Self>;

    fn decoder() -> Self::Decoder;
}

pub trait CanonicalDecoder {
    type Output;

    fn push(&mut self, chunk: &[u8]) -> Result<(), DecodeError>;
    fn finish(self) -> Result<Self::Output, DecodeError>;
}

pub enum NodeClass {
    Input,
    Executable,
    Derived,
    Aggregate,
}

pub trait ActionModel: Sized + 'static {
    type NodeId: Ord + CanonicalEncode;
    type NodeKind: Ord + CanonicalEncode;
    type NodeSpec: CanonicalEncode;
    type ActionSpec: CanonicalEncode;
    type RawObservation: StrictCanonicalDecode;
    type VerifiedObservation: CanonicalEncode;
    type DerivedSpec: CanonicalEncode;
    type DerivedValue: CanonicalEncode;
    type RootSpec: CanonicalEncode;
    type AggregatePayload: CanonicalEncode;

    fn graph(
        &self,
    ) -> &ActionGraph<Self::NodeId, Self::NodeKind, Self::NodeSpec>;
    fn root_spec(&self) -> &Self::RootSpec;
    fn action_spec(&self, id: &Self::NodeId) -> Result<Self::ActionSpec, ModelError>;
    fn execution_spec(
        &self,
        id: &Self::NodeId,
    ) -> Result<AdapterExecutionSpec<Self::ActionSpec>, ModelError>;
    fn verify_observation(
        &self,
        spec: &Self::ActionSpec,
        raw: Self::RawObservation,
    ) -> Result<LeafVerdict<Self::VerifiedObservation>, CandidateVerificationError>;
    fn derived_spec(&self, id: &Self::NodeId)
        -> Result<Self::DerivedSpec, ModelError>;
    fn evaluate_derived(
        &self,
        spec: &Self::DerivedSpec,
        dependencies: CompleteDependencyView<'_, Self>,
    ) -> Result<DerivedVerdict<Self::DerivedValue>, AdapterInvariantError>;
    fn aggregate(
        &self,
        complete: CompleteAdapterInput<'_, Self>,
    ) -> Result<AdapterVerdict<Self::AggregatePayload>, AdapterInvariantError>;
}

// Fields and constructors are private. Runtime membership is proven before an
// adapter can aggregate; raw Vec values are never a verified input.
pub struct VerifiedAdapterPlan<M: ActionModel> { /* root + ordered denominator */ }
pub struct PreparedExecutionV1 { /* checked key + invocation + execution policy */ }
pub struct ObservationCollector<'p, M: ActionModel> { /* pending slots */ }
pub struct CompleteObservationSet<'p, M: ActionModel> { /* all slots exactly once */ }
pub struct CompleteDependencyView<'p, M: ActionModel> { /* exact declared deps */ }
pub struct CompleteAdapterInput<'p, M: ActionModel> { /* leaves + all derived */ }

impl<'p, M: ActionModel> ObservationCollector<'p, M> {
    pub fn insert(
        &mut self,
        observation: VerifiedLeaf<M>,
    ) -> Result<(), MembershipError<M::NodeId>>;

    pub fn finish(
        self,
    ) -> Result<CompleteObservationSet<'p, M>, IncompleteMembership<M::NodeId>>;
}

pub trait AdapterCodec: Sized + 'static {
    type Model: ActionModel;

    fn descriptor() -> &'static AdapterDescriptorV1;
    fn decode_instance(
        bytes: StrictCanonicalInstanceBytes<'_>,
    ) -> Result<DecodedAdapterInstance<Self::Model>, AdapterDecodeError>;
}

pub struct AdapterRegistration { /* private monomorphized prepare fn */ }
pub struct AdapterRegistryBuilder { /* protected registrations under construction */ }
pub struct VerifiedAdapterRegistry { /* sealed unique (adapter id, schema) set */ }

impl AdapterRegistration {
    pub fn of<C: AdapterCodec>() -> Self;
}

impl AdapterRegistryBuilder {
    pub fn register(
        &mut self,
        registration: AdapterRegistration,
    ) -> Result<(), RegistryError>;

    pub fn seal(
        self,
        expected: &CompleteAdapterRegistryV1,
    ) -> Result<VerifiedAdapterRegistry, RegistryError>;
}

pub trait Projection<M: ActionModel> {
    type Output: CanonicalEncode;

    fn project(
        &self,
        outcome: VerifiedOutcomeView<'_, M>,
    ) -> Result<Self::Output, ProjectionError>;
}

// ci-runner: effect shell implemented by local and hosted backends.
pub trait SourceSnapshotProvider: Send + Sync {
    fn seal(
        &self,
        request: &SourceSnapshotRequestV1,
        limits: SourceSnapshotLimits,
    ) -> Result<VerifiedSourceSnapshot, InfraError>;
}

pub trait CasBackend: Send + Sync {
    type Reader: BoundedRead + Send;
    fn open_bounded(
        &self,
        digest: &ObjectDigest,
        limits: ObjectLimits,
    ) -> Result<Option<Self::Reader>, InfraError>;
    fn publish_no_replace(&self, object: &VerifiedObject) -> Result<(), InfraError>;
}

pub trait ExactCacheBackend: Send + Sync {
    fn open_candidate_index(
        &self,
        locator: &ExactActionLocator,
        limits: CandidateLimits,
    ) -> Result<Option<Box<dyn BoundedRead + Send>>, InfraError>;
    fn open_candidate(
        &self,
        candidate: &CandidateRef,
        limits: ObjectLimits,
    ) -> Result<Box<dyn BoundedRead + Send>, InfraError>;
    fn publish_immutable(
        &self,
        locator: &ExactActionLocator,
        candidate: &VerifiedCandidate,
    ) -> Result<(), InfraError>;
}

pub enum LocalAuthorityCommit {}
pub enum LocalConflictCommit {}
pub enum RemotePublicationCommit {}
pub enum RemoteConflictCommit {}

pub trait LocalOutcomePublisher: Send + Sync {
    fn commit_outcome(
        &self,
        expected: &SealedLocalGeneration,
        delta: VerifiedOutcomeDelta,
    ) -> Result<IndexCommitGuard<LocalAuthorityCommit>, CommitError>;
}

pub trait LocalConflictPublisher: Send + Sync {
    fn commit_conflict(
        &self,
        expected: &SealedLocalGeneration,
        conflicts: NonEmpty<VerifiedConflict>,
    ) -> Result<IndexCommitGuard<LocalConflictCommit>, CommitError>;
}

// One provider-neutral atomic snapshot-root contract. The concrete CAS or
// epoch mechanism selected by FCI-8c stays private to its provider adapter.
pub trait AtomicSnapshotPublisher: Send + Sync {
    type CommitScope: sealed::CommitScope;

    fn compare_and_publish(
        &self,
        expected: &OpaquePublicationGeneration,
        replacement: &VerifiedPublicationRoot,
    ) -> Result<IndexCommitGuard<Self::CommitScope>, InfraError>;
}

pub trait Sandbox: Send + Sync {
    fn execute(
        &self,
        invocation: &ActionInvocationV1,
        source: &MountedSourceSnapshot,
        guard: SandboxExecutionGuard,
    )
        -> Result<GuardedProcessObservation, InfraError>;
}

pub struct RunContext<'a> { /* explicit cancellation + frozen resource/engine guards */ }
pub struct Runner { /* bounded scheduler plus configured generic backends */ }

impl Runner {
    pub fn evaluate(
        &self,
        profile: &PreparedCompositeProfile,
        plan: &SealedEvaluationPlan,
        context: RunContext<'_>,
    ) -> Result<CommittedEvaluation, RunError>;
}
```

### 3.2 Blocking runner, cancellation, panic, and error ownership

The v1 runner API is synchronous and blocking. It has no public async trait,
runtime handle, hidden global executor, or backend-selected scheduling model.
`Runner` owns one bounded worker pool and bounded result channels under the
frozen `ResourcePolicyV1`. Effect SPIs that may be called by workers are
`Send + Sync`; a reader moved to a worker is `Send`. Adapter decoding,
verification, derived evaluation, aggregation, projection, and the final
coordinator commit run on the single coordinator in stable plan order. A ready
packet may parallelize a pure adapter operation only after adding a
partition-invariance fixture and without changing this public SPI.

`RunContext` carries a borrowed explicit cancellation capability, the frozen
resource policy, source/evidence snapshot guards, and the applicable local or
protected consumer-engine guard. The host translates Ctrl-C, provider
cancellation, and deadline expiry into that capability; backends do not read
ambient global cancellation. The runner checks it before and after each
blocking effect and while joining workers. Cancellation before semantic
completion or before the authority commit is `InfraError::Cancelled`, abandons
staging, and yields no outcome/capability/index generation. Cancellation after
an already linearized commit cannot erase or rewrite that commit and does not
turn the committed evaluation into `RunError`; it is recorded only as later
nonsemantic execution-receipt state.

A worker unwind is joined and classified as infrastructure failure. An unwind
from framework or adapter code is caught only at the protected/local host
boundary when the build supports unwinding; a `panic=abort` process exit is the
same infrastructure failure to its caller. Panic text is log data, never a
semantic observation. No panic path may synthesize a rejection, cache miss,
successful slot, commit guard, or capability. Immutable unreachable staged
bytes may remain exactly as for another infrastructure failure.

Error ownership and terminal behavior are fixed:

| Owner/type family | Meaning | Retry, outcome, and publication behavior |
| --- | --- | --- |
| `ci-core`: `CanonicalError`, `DecodeError`, `ModelError`, `AdapterDecodeError`, `MembershipError`, `ProjectionError` | Invalid current protocol/model/profile or incomplete/incorrect framework relationship | Fail closed; never a semantic rejection or cache miss. A new invocation is allowed only after its authoritative input changes or is repaired. |
| `ci-core`: `CandidateVerificationError` | Bounded candidate observation cannot satisfy the current adapter verifier | Reject that acquisition source; a protected policy may continue with another exact source or fresh execution, but cannot reuse the invalid bytes. A fresh harness protocol violation is a failed evaluation, not a semantic rejection. |
| `ci-core`: `AdapterInvariantError` | Trusted adapter violated the complete typed contract after preparation | Engine defect; terminate without outcome/publication/capability and do not retry as a miss. |
| `ci-core`: `AdapterVerdict::Rejected` / `DerivedVerdict` rejection | Deterministic semantic result from a complete required input | Canonical rejected outcome; complete siblings, never convert to `Err`, and never mint acceptance authority. |
| `ci-runner`: `InfraError` and `CommitError` | I/O, transport, spawn, signal, timeout, cancellation, OOM, panic, quota, guard, race, or durability failure before commit | No semantic outcome or capability. Retry only the explicitly typed bounded operation under the frozen policy; no retry may change membership or semantic bytes. |
| `ci-runner`: `NondeterminismDetected` terminal | Two distinct authority-valid canonical objects for one exact action key | First commit the monotonic conflict control when possible, choose no winner, publish no semantic outcome, and return the typed terminal. |
| `ci-runner`: `RunError` | Closed top-level sum preserving the exact family above | Adds context and stable explanation ids only; it has no catch-all success, rejection, or miss conversion. |

`AdapterExecutionSpec<A>` is an adapter proposal, not executable authority.
During FCI-4a.3, core validates its node/class/graph membership, root and action
specs, schema-tagged `ActionInvocationV1`, repetition policy, resource claim,
effective reuse scope, observation schema, and exact semantic closure; computes
the action key; and constructs private-field `PreparedExecutionV1`. Runner
workers receive only that prepared value. A backend, candidate profile, raw
invocation, or decoded identity cannot construct or mutate it.

Likewise, `AdapterRegistryBuilder` accepts only statically linked protected
registrations. Its `seal` operation checks the expected complete descriptor
set, uniqueness, schemas, implementation identities, and registry digest and
then consumes the builder. Planning and `Runner::evaluate` accept only
`VerifiedAdapterRegistry`-bound prepared profiles; there is no unseal, late
registration, candidate registration, or id/kind callback path.

`Runner::evaluate` is introduced only by FCI-7b, after every argument and error
in its signature exists. It is the sole generic live evaluation entry: it
consumes the FCI-7a sealed plan/context, performs exact acquisition and bounded
execution, invokes adapter verification through the sealed registry, orders
complete results, handles conflicts, and coordinates the final local commit.
An adapter or backend may implement its own typed operation but cannot expose a
second authoritative evaluation loop.

Concrete types may refine these signatures, but they must preserve the split:
`ci-core` validates values and relationships; `ci-runner` performs effects;
the protected control engine registers repository adapters and constructs
`ActionInvocationV1`; a candidate harness is only a sandbox child. An effect
error cannot be smuggled into `OutcomeManifestV1`. Candidate rejection,
candidate-byte corruption/untrusted authority, invalid current model/profile,
nondeterminism, adapter invariant failure, and infrastructure failure are
distinct enums; a deterministic semantic rejection is an `AdapterVerdict`, not
`Result::Err`, and cannot be converted to a cache miss.

This listing is the final post-FCI-8e surface, not permission to introduce
forward declarations. FCI-4a.3 owns the registry seal and
`PreparedExecutionV1` construction only after its FCI-3 value dependencies
exist; FCI-7b owns the complete `RunContext`, `Runner`, and live entry. FCI-3c
owns `SourceSnapshotProvider` and `Sandbox` only;
FCI-6a owns `CasBackend`; FCI-6b owns `ExactCacheBackend`,
`PublicationEventV1`, `LocalConflictPublisher`, and its conflict guard; FCI-6c owns
`LocalOutcomePublisher` and its outcome guard; FCI-8e owns
`AtomicSnapshotPublisher` and both remote guards. Each packet compiles without
an opaque placeholder for a type owned by a later packet.

`IndexCommitGuard<S>` is private, non-cloneable, and non-serializable. A local
authority backend returns only `IndexCommitGuard<LocalAuthorityCommit>`; a
conflict-control commit returns only
`IndexCommitGuard<LocalConflictCommit>`; a
shared-cache writer returns only
`IndexCommitGuard<RemotePublicationCommit>`; and a remote conflict commit
returns only `IndexCommitGuard<RemoteConflictCommit>`. No scope converts to
another, and only `LocalAuthorityCommit` is accepted by an `AuthorizedRoot<P>`
constructor.

`NodeClass` describes only the lifecycle behavior required by the generic
algorithms. `NodeKind`, `NodeSpec`, action/observation/root schemas, repetition
policy, and aggregation topology are strongly typed associated adapter values.
They are not opaque strings interpreted by `ci-core`. `Oracle`, `Candidate`, `Cargo`,
`CaseObservation`, `Shard`, and a two-repetition rule are H2-adapter concepts
and must not become `ci-core` variants. A profile may have no bundle/shard
interior at all; the `workspace-audit` adapter is required to exercise that
shape.

A hosted/local command may compose more than one adapter. Generic
`CompositeProfileV1` contains an ordered, duplicate-free list of
`AdapterInstanceRefV1` values binding a stable instance id, adapter id/schema,
graph digest, adapter root action key, scope, and exact required leaf set.
Adapter-local node ids are strongly decoded by that adapter and then wrapped in
a generic `GlobalNodeIdV1(instance id, adapter id, canonical local-id bytes)`
for ordering and Merkle
composition; `ci-core` never interprets the local bytes as a kind. Each adapter
is selected only through a closed, protected `VerifiedAdapterRegistry`:
candidate profile bytes cannot register code. `AdapterRegistration::of::<C>()` installs a
monomorphized strict decoder keyed by unique `(adapter id, schema)`. It strongly
decodes every local id/spec/graph/root to `DecodedAdapterInstance<M>`; framework
code then canonical-re-encodes it, checks the recorded digests, constructs the
stable executable/derived/aggregate evaluation plan, and rederives required
membership before erasing the validated instance behind sealed
`PreparedAdapterDyn`. Generic code uses no `Any`,
downcast, `unsafe`, adapter-id match, or opaque semantic string.

Each adapter collects exactly one verified leaf for every slot in its private
`VerifiedAdapterPlan`; unknown, duplicate, wrong-key, wrong-closure, and missing
observations cannot produce `CompleteObservationSet`. Core then evaluates every
derived node in stable topological order. It supplies only a private
`CompleteDependencyView` containing that node's exact declared dependencies and
produces `CompleteAdapterInput`; a rejection is a completed typed value, while
an adapter invariant error cannot become a verdict. The adapter returns only
its aggregate policy payload/verdict. `ci-core`, not adapter code, seals the
complete leaf references into the suboutcome. A corresponding
`CompositeCollector -> CompleteCompositeInput` typestate rejects unknown,
duplicate, or missing adapter instances and global leaf collisions before the
pure composer builds one `OutcomeManifestV1<GlobalNodeIdV1>`. Any adapter
rejection occupies a completed slot and contributes to the composite
`Rejected` outcome; it cannot omit or cancel a sibling adapter. Projections
consume a lease-bound `VerifiedOutcomeView`, not an unverified manifest whose
CAS children could disappear concurrently.

For `hosted-ts-tests`, the composite profile is the mechanism that covers the
current conformance, H1, and historical/current H2 subcalls while preserving an
H2-specific case/shard adapter. An H2 subroot alone is structurally incapable
of satisfying the composite root spec.

## 4. Canonical inputs

Planning and execution read one immutable, VCS-neutral `SourceSnapshotV1`, not
a sequence of live-worktree reads. It binds the logical repository identity and
revision reference, ordered entry Merkle root, entry count/byte limits,
source-provider implementation/capability digest, acquisition receipt, and the exact
read-only mount later given to sandboxes. Graph/profile/policy proposals,
inventory, direct files, directory listings, absence/negative lookups,
generated-input validation, and build inputs all resolve through that same
snapshot. A `SourceSnapshotGuard` rechecks the mounted identity at each
effect boundary; mutation or inability to provide immutable lookup and mount
semantics is infrastructure failure before authority publication. A post-hoc
"repository changed" check cannot bless a graph/input mixture that never
existed.

`SourceSnapshotProvider` is a runner SPI and knows no Git or SHA. The tsc-rs
repository adapter may instantiate it from an exact Git tree plus a canonical
dirty overlay, but must freeze that overlay, submodule state,
symlink/special-file dispositions, case/Unicode collisions, negative entries, and directory
enumerations before planning. Another adapter may use an archive, database
snapshot, or Merkle filesystem without a core branch. Git base/head values are
typed adapter revision inputs and never framework-global identity.

`CanonicalInputsV1` is a Rust type with `#[serde(deny_unknown_fields)]`. One
instance describes one action node's semantic input closure, not the bytes of
the entire repository, executable, or impact graph. It contains, in a fixed
order:

1. canonical-input schema and this action kind's semantic schema;
2. impact-graph schema, typed action-node id, exact reviewed node-spec digest,
   and that node's recomputed closure digest;
3. the exact adapter-owned action-spec digest and, for an executable action,
   its mandatory `ExecutionSpecV1` digest; aggregation/profile digests belong
   only to actions that consume them;
4. every typed input, generated, negative-lookup, toolchain, and policy node in
   that action's exact reviewed semantic dependency closure;
5. the stable adapter action id and its exact direct-input digest where
   applicable;
6. every direct file input in the closure as a validated workspace-relative
   path, byte length, and SHA-256;
7. relevant generated-output, directory-listing, absence assertion, command,
   feature, target, platform, toolchain, and allowlisted non-secret environment
   digests; and
8. the schema/encoder/capture, verifier, or projection version only when that
   component is owned by this action node.

Entries are sorted by normalized workspace-relative path. Paths must be UTF-8,
use `/`, contain only normal components, remain under the canonical workspace,
and traverse no symlink. A directory input expands to its sorted regular-file
leaves. Absolute paths and filesystem metadata such as mtime, uid, or inode are
not semantic inputs.

Every executable action owns one canonical `ExecutionSpecV1`. At minimum it
contains the adapter id and schema, typed action-kind id, semantic
implementation digest/version, normalized argv, logical workspace mount and
working directory, allowlisted non-secret environment, feature/target set,
`ExecutionPlatformV1` or an explicit platform-independence capability,
`ToolchainSetV1` digest, sandbox ABI, invocation-builder version,
observation-capture/encoder version, and process-result/failure-classifier
version. A derived action similarly owns its derivation/verifier/projection
implementation identity. Changing semantic implementation code must therefore
change a declared owner closure or an owning schema/version; changing only the
whole executable digest is never used as a substitute for this requirement.

`ExecutionPlatformV1` has a canonical schema for OS, architecture, target,
runtime ABI/libc where relevant, filesystem case/Unicode behavior, path and
line-ending behavior, semantic sandbox/runtime component digests, and any
admitted kernel/CPU capability. It does not hash an opaque runner-image, Nix,
or orchestrator label when the exposed semantic runtime is identical.

The core `ToolchainSetV1` is an ordered, duplicate-free set of generic
`ToolRefV1` values. A tool reference contains an adapter-owned canonical tool
id and role, a content or installation-manifest digest, and the exact generic
platform/capability fields needed to interpret that digest. `ci-core` neither
defines nor branches on a Rust, Cargo, Node, TypeScript, linker, compiler, or
sysroot variant. The H2 adapter instantiates tool references for Rust/Cargo,
Node/TypeScript, the linker, sysroot/runtime, and every other executable that
can affect an H2 action. Version strings alone are insufficient.
Cross-platform reuse is forbidden unless the action declares a reviewed
platform-independent scope and the mandatory partition-invariance tests cover
every admitted platform class.

Cacheable semantic sandboxes are secret-free. Cache credentials, OIDC tokens,
signing material, and other secrets remain in the runner's transport/authority
boundary and are never passed to an action, hashed into a shared key, placed in
CAS, or printed in logs/explanations. An adapter action that genuinely requires
a secret is typed `NonReusable`, always executes in a separate effect gate, and
cannot contribute to a `ReuseAllowed`/hosted outcome or shared semantic
evidence.

Reuse and disclosure are separate protected-policy decisions:

```rust
pub enum ReuseScopeV1 {
    NonReusable,
    LocalReusable,
    SharedReusable { audience: EvidenceAudienceV1 },
}
```

Every direct input, derived object, action, and profile receives an effective
scope computed as the most restrictive scope in its complete closure. Current
reuse eligibility and historical disclosure are distinct. The protected base
may narrow future lookup/publication eligibility; candidate content cannot
widen it, declare sensitive evidence public, or select its own audience. A
read-only credential protects integrity but not confidentiality, so an
untrusted PR sees only objects currently authorized for its audience. An
unavailable or insufficiently narrow credential is an explicit exact miss,
never a reason to expose `LocalReusable` or another audience's bytes.

`DisclosureHistoryV1` is monotonic authority state. For every published object
digest it records the union of every audience that has ever been authorized to
read it plus the first `PublicationEventV1` reference for each audience. A
publication event binds its authority epoch, the **observed prior** authority
generation, action/object and producer-receipt digests, and the audience delta;
it never contains the digest of the replacement history or replacement head.
The producer receipt likewise binds the observed prior generation and proposed
audience, not a history/head being created by the same transaction. The
replacement generation/head then commits the ordered event reference and the
monotonically expanded disclosure root. Content references therefore form the
one-way DAG `prior generation -> receipt -> publication event -> replacement
head`; no receipt, event, history, generation, or head digest depends on itself.

A later scope narrowing
may remove indices and prevent future framework-served reads/writes, but it
cannot shrink that union, recall immutable bytes, or claim retroactive secrecy.
The same already-public payload cannot be reclassified as confidential. Truly
sensitive replacement data requires a new semantic/schema input and action key,
a protected namespace/epoch that never exposes it to the old audience, and an
explicit record that the old bytes remain historically disclosed. Scope,
sensitivity, and disclosure history remain authority metadata/receipts rather
than semantic payload; changing them cannot change canonical results but may
remove every eligible acquisition source. Candidate state cannot erase or
rewrite disclosure history.

Hash domains and application identity are separate. `ProtocolDomainV1` is a
closed, versioned registry with one unique tag for every hashed wire object,
including canonical input, action/build/root keys, graph, node spec, source
snapshot, adapter descriptor/complete registry, object, outcome, interior,
candidate/conflict manifest, authority receipt, publication event,
generation/head, evidence/publication snapshot, trust/transition, policy proof,
lease, and GC plan. FCI-3a freezes the
complete v1 registry and golden framing; an unregistered or reused tag is a
schema error. Purpose-specific newtypes such as `ActionKeyV1`, `ObjectDigest`,
`OutcomeDigest`, `AdapterRegistryDigest`, `ConflictRegistryDigest`,
`AuthorityReceiptDigest`, and `PublicationEventDigest` prevent cross-domain
digest substitution; generic
untyped `Digest` is not accepted at authority APIs. The domain tag and digest
newtype do not forward-declare the later FCI-6b event schema.
`ApplicationNamespaceV1` is a canonical value
binding the repository/application identity and adapter namespace; the H2
adapter supplies the `tsc-rs` application identity. It also fixes stable
repository-id, fork/rename, adapter-namespace, and authority-epoch policy so a
display-name change neither aliases another application nor silently abandons
authority. The digest contains the stable epoch **policy**, never the mutable
current authority-store/locator epoch. Consequently a recovery rollover changes
physical authority routing but not `ApplicationNamespaceV1`, semantic closure,
or `ActionKeyV1`; an existing conflict tombstone remains keyed to the same
semantics after rollover.
Generic implementation and
schema sources contain no `tsc-rs`, branch, workflow, compiler, or suite
literal except in explicit test fixtures. Every action, build, and root key
below binds the canonical
application-namespace digest, so equal payloads from two applications cannot
alias even though they share the framework protocol domain.
The Cargo package/library names frozen in section 3 are the sole
incubating-workspace metadata exception; they are never hashed or inspected by framework
logic.

The semantic action key is known before execution:

```text
ActionKeyV1 = SHA256(
  domain("functional-ci.action.v1") ||
  length(application_namespace_digest) || application_namespace_digest ||
  length(canonical_input_bytes) || canonical_input_bytes
)
```

Every listed field participates. A change to one listed byte changes the
action key. The full graph digest does not participate in every case-leaf key:
an unrelated reviewed graph edit therefore cannot invalidate a raw case
observation.

The complete graph digest is bound by planning, root keys, root manifests, and
verified-root capabilities. A path deliberately outside the profile does not
affect this profile. A docs-only edit is reusable only when no changed
documentation path is a declared dependency of the relevant action.

Build artifact identity is separate. The generic core hashes an ordered,
duplicate-free list of `BuildComponentV1` values; each contains an
adapter-owned canonical component id, a component-kind/schema id, and its
canonical digest. The core validates ordering, uniqueness, framing, and
digests but never interprets a Cargo package, target, build script, or other
tool-specific component:

```text
BuildArtifactIdV1 = SHA256(
  domain("functional-ci.build.v1") ||
  length(application_namespace_digest) || application_namespace_digest ||
  ordered-tool-refs || ordered-build-components || executable-digest
)
```

For H2, adapter-owned build components include the Cargo resolution, selected
targets, build environment, build-script inputs, generated-output digests, and
compile profile/flags. A different adapter may use none of those components
and must not encounter a core branch for their absence.

`BuildArtifactIdV1` identifies the exact executable bytes named by producer and
execution receipts; an authenticated producer attestation proves which bytes
ran. The digest alone is not provenance. `ActionKeyV1` identifies the semantics
that can affect one adapter action. An unrelated xtask module may change the
executable and build identity without changing an H2 case's semantic key.
Reuse is then legal only when the reviewed impact graph proves the changed
build nodes are outside that case's semantic closure. A relevant generated
output, build-script input, compile profile, Rust flag, toolchain/runtime, or
execution implementation is a semantic node and still changes the affected
action keys.

The root action key is also known before execution. It hashes the root domain,
full impact-graph digest, functional profile digest, adapter root-spec and
aggregation-plan digests, verifier/projection action keys, and the ordered list
of all expected executable/derived leaf action keys. A composite profile also
binds the ordered adapter instance/subroot action keys and exact
global-id-qualified membership of their required leaves. The H2 adapter's subroot spec
additionally binds its fixed shard plan and union-membership digest:

```text
RootActionKeyV1 = SHA256(
  domain("functional-ci.root-action.v1") ||
  length(application_namespace_digest) || application_namespace_digest ||
  length(canonical_root_inputs) || canonical_root_inputs
)
```

`RootActionKeyV1` selects candidate passed/rejected outcome objects from local
or remote storage. The separate outcome content digest proves the bytes
returned for that action; only a verified `Passed` outcome exposes a root
capability.

The `functional profile digest` above is the digest of
`SemanticProfileV1`: membership, semantic policies, adapter/root specs, and
projection contracts. It excludes `ResourcePolicyV1`, cache availability,
producer authority, retry/worker settings, and origin receipts. Those values
may determine whether evaluation completes and which nonserializable capability
is minted, but changing them alone cannot change an action key or canonical
outcome.

## 5. Demand-driven impact graph

The engine never treats a repository-wide path hash or a workflow path filter
as the semantic dependency graph. Every profile manifest names one checked-in
typed graph resource for each adapter instance. For the tsc-rs adapter these
configured resources live at
`.github/ci/impact/<profile>.v1.json`. Every graph is validated by
`.github/ci/contracts/functional-ci-impact-graph.schema.json` and `ci-core`.
Generic crates receive typed resource locators and canonical bytes; neither
`.github`, a workspace root, nor this filename is a protocol convention.

The core representation is equivalent to the following generic shape:

```rust
pub struct NodeRecord<I, K, S> {
    pub id: I,
    pub class: NodeClass,
    pub kind: K,
    pub kind_spec: S,
    pub kind_spec_digest: NodeSpecDigest,
    pub direct_inputs: Vec<SemanticInput>,
    pub dependencies: Vec<I>,
    pub closure_digest: ClosureDigest,
}

pub struct ActionGraph<I, K, S> {
    pub schema: u32,
    pub profile: ProfileId,
    pub adapter_id: AdapterId,
    pub adapter_schema: u32,
    pub nodes: BTreeMap<I, NodeRecord<I, K, S>>,
    pub graph_digest: ActionGraphDigest,
}
```

The stored graph records the canonical adapter id/schema, typed `K`, and the
complete typed `S` kind specification as `kind_spec`; it never stores only a
digest in place of the data needed for validation. The selected adapter must
strongly decode the stored specification to `ActionModel::NodeSpec`, reject an
unknown kind or field, canonical re-encode it, and verify both byte identity
and `kind_spec_digest` before planning. `ci-core` branches only on `NodeClass`
and generic input/action relationships. It never branches on an H2 case,
TypeScript oracle, Cargo target, shard, or workspace role.

An edge `consumer -> dependency` means that changing the dependency can change
the consumer. Dependencies and direct inputs are sorted and duplicate-free.
`closure_digest(node)` hashes the node's typed identity, direct inputs, and the
ordered `(dependency id, dependency closure digest)` pairs. Cycles, missing
nodes, ambiguous ids, or a stored closure digest different from recomputation
reject the graph.

The first H2 adapter projects its typed kinds into four semantic regions:

- the **oracle graph** maps vendored TypeScript, generators, schemas, configs,
  resolution lookups, and fixtures to exact oracle case records;
- the **candidate graph** maps Rust workspace modules, Cargo targets, generated
  Rust, build scripts, features, and runtime configuration to semantic owners;
- the **test graph** maps harness/test targets, fixture inputs, case selection,
  plans, raw case observations, shard-manifest interiors, canonical payload
  ownership, verifiers, projections, profile summaries, and roots; and
- the **documentation graph** classifies each relevant document as normative
  policy/schema input or narrative/no-semantic-impact input. Narrative nodes
  have no edge to semantic actions, while a policy document has reviewed edges
  to the verifier, profile, or plan it governs.

Every profile names a `WorkspaceInventorySpecV1` that fixes the complete
discovery roots, repository/worktree source, ignore dispositions, generated
and transient roots, submodule policy, untracked-file policy, symlink policy,
path normalization/case-collision policy, and directory/negative-lookup
algorithms. Every discovered path has exactly one disposition: direct input of
a node, explicitly ignored generated/transient path, known input owned only by
another profile, or unknown. A new or changed unknown path fails closed as
described below. Merely matching `docs/**`, `crates/**`, or another path
expression is not a semantic disposition, and a profile cannot define its
inventory universe by listing only the paths it already knows.
`known input owned only by another profile` is valid only when the protected
global disposition registry and current Cargo/generated/action graphs prove no
selected target or action consumes it; candidate content cannot self-classify
a new production input out of the current profile.

Generic execution and interpretation layers are separate `Executable`,
`Derived`, and `Aggregate` nodes. An executable leaf owns exactly one adapter
action; a derived node owns its canonical payload/verifier/projection contract;
an optional bundle node is only a deterministic Merkle/scheduling interior;
and the root owns aggregate policy. Edges may flow from later layers to earlier
ones, never the reverse. A bundle, verifier, projection, summary, or plan
change therefore cannot silently become an input of raw execution.

In the H2 adapter, those generic roles are strongly typed as
`CaseObservation`, `CanonicalPayload`, `Verifier`, `Projection`, `Shard`,
`ProfileSummary`, and `Root`. In particular, `Shard -> CaseObservation`; there
is no reverse edge. An H2 case observation depends on its exact
`CaseObservationSpecV1`, not on a shard id, range, job assignment, complete
plan, full graph digest, verifier, projection, profile-summary, or root node.

### 5.1 Reviewed semantic owners

Each case-plan node lists the oracle, candidate, harness, fixture, and policy
owner nodes that can affect its observation. Initial candidate ownership is at
whole-file or whole-Rust-module granularity. The migration must not guess a
function-level dependency from symbol text, an incomplete call graph, changed
lines, coverage, or the absence of a direct call.

If one module contains shared and slice-specific behavior, the whole module is
a shared owner until a later reviewed graph change extracts an independently
typed module and proves its boundary. Narrowing an owner is a graph/schema
change, changes the reviewed closures and action keys of its consumers, and
requires adversarial tests. Acquisition may still find an independently
produced exact new key. A generated mapping may propose edges, but checked-in
review, protected transition approval where required, and the graph verifier
together make them authoritative.

Structural graph validity does not prove that a removed dependency was
semantically irrelevant. Each run therefore starts with a protected
`TrustRootV1` and the selected base graph from outside the candidate checkout.
`TrustRootV1` fixes the repository, protected workflow identity/digest,
producer issuers/subjects/audiences, global disposition-registry digest, remote
namespace-kind epochs, hosted resource ceilings, graph-transition authority,
and engine-promotion authority.
A `GraphTransitionV1`
binds the exact prior/current
profile and ordered adapter-graph-set digests and classifies added/removed
adapter instances/nodes, dependency-edge changes, owner
narrowing, inventory-root/ignore changes, and trust-policy changes. Removing or
narrowing an owner, expanding an ignore, or weakening a conservative mapping
requires an authenticated transition approval issued by the protected base
authority. Candidate/PR content cannot approve its own transition or widen its
producer/cache-write authority. Without that approval, the runner retains a
validated safe superset of prior/current ownership (which may be the typed
all-raw mapping); if no safe superset can be represented, it stops before
acquisition or execution. Merely executing every action with the candidate
graph does not make an underdeclared graph authoritative for future reuse.
The first graph for a profile requires an authenticated genesis transition;
absence of a base graph is not implicit approval.

Any candidate owner marked `shared` and any global raw case-observation
schema/capture/encoder owner are dependencies of every raw case leaf they can
affect. A change to one of those nodes invalidates its full declared reverse
closure. The graph validator, evidence verifier, projection, and profile
summary are dependencies of validation and root nodes, not automatically of
raw case observations. Changing any of them forces current raw-case
revalidation and root reconstruction. A verifier change also rebuilds derived
case records and their shard manifests; a projection/profile-summary change
reprojects without repacking a shard whose exact raw/verified case references
and bundle spec remain unchanged. Raw case-observation keys whose exact
reviewed closures did not change remain reusable.

An unknown graph version, invalid graph transition, unmapped production
source, or unverifiable shared-owner boundary fails closed. If a validated
conservative mapping proves a safe superset, that superset may be all raw
case-observation nodes. The graph contains a typed `conservative-all-raw`
sentinel; the normalized unknown identity and digest enter that sentinel, so
every raw case consumer receives a new exact closure and action key. If no such
mapping can be established, the run stops as a policy/configuration failure;
executing against an untrusted graph cannot make the result authoritative.

### 5.2 Change and invalidation algorithm

The runner independently validates the protected-base and current graphs from
their separately sealed `SourceSnapshotV1` values, computes current
direct-input digests for every node from the current immutable snapshot, and compares
the union of their stable node ids. A repository adapter may supply candidate
changed paths as a hint; neither Git nor another VCS decides impact. Directory,
generated, negative, build-system, inventory, and policy nodes are evaluated
from the same snapshot even when a path hint is empty. The selected prior
source/evidence snapshots and receipts are authenticated inputs to comparison,
never authority for the current graph. No planning lookup falls back to the
ambient live worktree.

Impact and execution are different calculations. The impact calculation is
exact:

```text
delta_ids = union(prior.node_ids, current.node_ids) where any of these differ:
  presence, node class, typed kind/spec, direct-input set/digest, dependency set
changed_prior = delta_ids present in prior
changed_current = delta_ids present in current
unknown = inputs discovered by current WorkspaceInventorySpecV1 with no disposition

if either required graph, inventory, or GraphTransitionV1 cannot validate:
    fail before acquisition or execution
if unknown can map only to the conservative all-raw owner:
    changed_current += conservative-all-raw-owner
if unknown cannot map to any validated safe owner:
    fail before acquisition or execution

prior_reach = reverse_dependency_closure(prior, changed_prior)
current_reach = reverse_dependency_closure(current, changed_current)
impacted = current nodes whose stable id is in prior_reach or current_reach,
           plus current nodes whose recomputed closure digest changed
```

Taking both closures is mandatory: a removed node or edge exists only in the
prior graph, while an added node or edge exists only in the current graph.
Changed-set records use a typed prior/current/both identity so explanations can
name deleted and renamed nodes without inventing a current node. The current
required action set, not the prior set, determines what may execute or appear
in the new outcome. Composite profiles run this algorithm over
`GlobalNodeIdV1`; adding/removing an adapter instance is a typed profile delta,
not an invisible change outside its local graphs.

The runner then derives every required current executable-leaf key from the
current node spec and exact closure. For a shared profile it first opens the
protected capsule's exact `RemoteAuthorityHeadV1`, verifies its authenticated
monotonic generation plus conflict/disclosure roots, and rejects every required
conflicted key. Only then does it fetch the separately optional bounded
candidate indices, validate their authority and bytes, and seal one canonical
`EvidenceAvailabilitySnapshotV1`. The snapshot binds the protected prior-root
receipt, immutable prior/current source-snapshot digests and guards, exact
local/remote logical locators, ordered candidate references and
authority digests, validation/rejection reasons, explicit misses, trust-root
digest, graph-transition digest, local/remote conflict-registry digests and
dispositions, local/remote disclosure-history digests,
namespace-kind/epoch-map digest, and the exact
candidate-index strategy and generation for every locator. Entries appearing
after the snapshot is sealed are ignored by semantic evaluation in this
invocation. The final local publication gate below proves only that the local
index-head generation has not changed. Remote candidate bytes and proofs are
already frozen by digest in the snapshot; a late remote generation is observed
only by a future invocation or by the separate post-outcome remote-publication
protocol. Candidate-index/object transport follows the profile's explicit
optional-miss rule; the protected remote authority-head transport for a shared
profile is always required as specified in section 1.

The generic acquisition calculation is:

```text
required = executable leaves named by the current validated adapter root spec
prior_candidates = leaves under the selected prior root with an exact current key
cache_candidates = exact-current-key local/remote entries not already acquired
carry_forward = authority-valid, rehashed and reverified prior_candidates
cache_reuse = authority-valid, rehashed and reverified cache_candidates
acquired = carry_forward union cache_reuse
execute = required - acquired
schedule = adapter scheduling projection over execute
revalidate = required
reproject = current profile projections and summaries
repack = optional bundle interiors whose ordered references or bundle spec changed
rebuild = current outcome root
```

For each action key, the runner considers all authority-valid candidates named
by the sealed snapshot and any fresh result produced for that key by this
invocation. If two independently valid candidates, or a fresh result and a
valid snapshot candidate, have distinct canonical object digests for the same
action key, the function invariant has been violated. The runner reports
`NondeterminismDetected` only after the monotonic conflict-control commit from
section 9 removes that key's candidate set and installs its durable tombstone.
It publishes no semantic outcome receipt, capability, or action-index choice,
and never resolves the conflict by source, arrival time, or first writer. A
crash before that commit means the invocation did not yet report the conflict;
a crash after it leaves the key durably ineligible for reuse. Invalidly authenticated
or corrupt candidates are ordinary cache rejections and do not participate in
this comparison. Multiple valid receipts for the same object digest are not a
semantic conflict; the snapshot collapses them by digest and uses the
lexicographically least `(receipt digest, source locator)` as the deterministic
origin proof.

Prior-root carry-forward and exact cache lookup are acquisition mechanisms,
not impact authority. No FCI-1 through FCI-10 profile configures payload
migration. A semantically impacted action can reuse an exact current-key result
produced elsewhere. An unchanged action executes when no acquisition source
supplies valid current evidence. The H2 scheduler groups only missing case actions by
its fixed shard plan and performs exactly two isolated repetitions; those are
adapter rules, not generic key or runner rules. All acquired raw observations
are reverified with the current verifier, changed optional bundle manifests are
rebuilt from ordered individual references, and a new outcome root is always
built for the current full graph and plan.

Workers write only invocation-private staged observations and return typed
results. They never update an action index, bundle, root, or accepted namespace.
After every required action is acquired or completes, one coordinator orders
results by stable node id, validates same-key uniqueness, constructs the complete
`Passed` or `Rejected` outcome, and may place immutable objects, interiors, and
outcome, receipt, and replacement-index bytes in CAS as unreachable staged
content. It then invokes the local backend's one atomic generation commit:
compare the local index-head generation captured by the sealed snapshot and,
only if that local generation is unchanged, install the replacement local
generation that references those already-staged bytes. Remote indices and
objects already decoded into the snapshot are immutable inputs to this
invocation; a remote change after sealing is deliberately invisible until a
future snapshot and is never part of the local compare-and-swap. The local
commit is the authority-publication
linearization point and returns a non-serializable
`IndexCommitGuard<LocalAuthorityCommit>`; only then may the coordinator mint a
capability. If the local generation changed, the coordinator chooses no winner:
it validates the new local generation and either seals a replacement snapshot
and repeats the bounded conflict/publication check or terminates as
infrastructure failure. It never retrofits ambient local or remote candidates
into the old snapshot. Remote publication is the later, independent protocol
in section 11 and cannot mint or revoke this local capability. A deterministic
rejection does not cancel siblings.
External cancellation or any infrastructure failure abandons staging and
publishes no authoritative receipt or reusable index generation; unreachable
immutable CAS bytes may remain for GC.

For every impacted action, `ci-runner` records the lexicographically least
shortest `changed-node -> ... -> action` explanation path. For every acquired
or executed action it records the exact current key, node closure, source, and
validation or miss reason. All changed, impacted, carry-forward, cache-reuse,
execute, revalidate, repack, and rebuild sets and the
adapter schedule mapping are sorted and deterministic for a fixed
evidence-availability snapshot. Explanations bind that snapshot, prior/current
graph, graph-transition, trust-root, and namespace-epoch-map digests.

A changed node invalidates only its reverse dependency closure. It does not
invalidate unrelated oracle cases, candidate slices, test targets, or narrative
documents. Conversely, a cache hit cannot remove a node from `impacted` and a
cache miss cannot add an unchanged node to semantic impact. Evidence
availability changes the `execute` set, not impact truth.

### 5.3 Negative and generated inputs

Resolution can depend on a file not existing. Each negative lookup is a typed
node containing the normalized requested path, lookup algorithm/version,
ordered search roots, relevant directory-listing digests, and the asserted
absence. Adding a previously absent matching file changes that node even if no
old tracked file changed. Globs and directory scans similarly depend on a
canonical listing node plus the selected children.

Every generated input has separate generator-action and generated-output
nodes. The generator node includes its source, tool, config, environment,
positive inputs, negative lookups, and command. Consumers depend on the output
node's canonical digest. A checked-in generated file binds both its bytes and
generator identity; current bytes alone do not prove freshness.

Cargo build scripts are explicit `BuildScript` nodes. At minimum they include
`build.rs`, the complete build-dependency closure, package inputs, target,
toolchain, allowlisted environment, emitted Cargo directives, and all generated
output digests. `cargo::rerun-if-*` lines are observations, not complete
authority. A build script that reads an unmodeled file, probes an unmodeled
system library, uses the network, or produces an unenumerated output is opaque;
all consuming Cargo targets are impacted, and hosted reuse is forbidden until
the input is modeled or the action runs in a sandbox that proves the closure.
Until then those consuming actions are typed `NonReusable` and publish no
shared evidence.

### 5.4 H2 Cargo crate and test-target selection

The adapter derives the Cargo package graph from pinned `cargo metadata`
output, then records a reviewed canonical projection. A `CargoTarget` node
contains package role/name, target name and kind, enabled features, profile,
target triple, library/dependency targets, build-script output, and relevant
manifest/lock/toolchain nodes.

Each fixed action names exact Cargo targets. A test target node contains its
source module tree, its package library target, dev-dependency closure,
fixtures, environment, and case plan. `cargo test -p <package>` is an invocation
convenience, not proof that every or only relevant target was selected. A new
target, changed feature, changed build script, or changed resolved dependency
alters the corresponding typed nodes before execution.

For the initial H2 adapter, case plans depend on reviewed candidate
file/module owners and exact harness/test targets. An unrelated test target has
no edge to the hosted H2 plan. A shared compiler/program/emitter owner has edges
to every case plan that executes it. The semantic action remains one case;
Cargo process reuse or batching may optimize execution only when isolation and
partition-invariance tests prove byte-identical `RawCaseObservationV1` objects.

### 5.5 H2 mixed fresh and reused root

Before consulting prior evidence or any cache, the engine computes the current
validated graph, expected case set, semantic action key for every case, and
impacted set. It first considers exact-current-key case children from the
selected prior root, then exact-current-key case cache entries. Every candidate
must pass the profile's producer authority, canonical decode, content rehash,
semantic-key/node-closure comparison, and raw-case validation. An entry under
an old or merely related key is a miss.

The final root may combine authenticated and reverified prior-root case leaves,
authenticated and reverified exact-cache case leaves, and freshly executed case
misses. This mix is
independent of whether a case was semantically impacted: an impacted case may
have an exact current-key result from another producer, and an unchanged case
may execute after a miss. Each current `ShardManifestV1` is reused only when
its shard spec and complete ordered case-object references are identical;
otherwise it is deterministically rebuilt from those individual case objects.
The root is always newly aggregated against the current full graph, plan,
verifier, projection, and exact expected case set. Case origin is receipt
metadata and does not change canonical semantic bytes. A prior root is only an
acquisition source; its bytes alone are insufficient to prove current impact
or acceptance.

### 5.6 Required H2 impact fixtures

FCI migration owns a checked-in `impact-cases.v1.json`. Each row names the
baseline graph, one synthetic change, the exact prior-root inventory, exact
local/remote cache inventory, and the exact sorted `changed`, `impacted`,
`carry_forward`, `cache_reuse`, `execute`, `revalidate`,
`repack`, and `rebuild` node ids plus the exact `schedule` mapping. It contains
at least these cases:

| Synthetic change | Declared evidence availability | Exact required result |
| --- | --- | --- |
| Narrative document explicitly classified no-impact | Prior root has every case | Only the documentation/root path is impacted; all cases carry forward, zero execute or repack, all cases revalidate, root rebuilds |
| Test-only module outside selected hosted targets | Prior root has every hosted case | Only that test node and its local consumers are impacted; all hosted cases carry forward, zero hosted execute or repack, hosted root rebuilds |
| One fixture used by exactly one case | Prior root has every unchanged case key; changed case key is absent | Exactly that case is impacted and executes twice; its shard manifest repacks from one fresh case plus the remaining exact carried cases; other shard manifests reuse |
| The same one-case fixture change | Prior root has unchanged case keys; cache has the exact new case key | Impact is identical; changed case is exact-cache reuse, zero cases execute, and its shard manifest still repacks from the new plus carried case references |
| One slice-owned candidate module | No current-key evidence for its case consumers; prior root has all other case keys | Only reviewed case consumers execute; exactly their containing shard manifests repack; other cases and shard manifests carry forward |
| Shared compiler/program/emitter owner | No current-key case evidence | Every raw case is impacted and executes twice; every shard manifest repacks |
| Newly added file satisfying a negative lookup | Exact current key exists for one impacted case only | The exact reverse closure is impacted; that case uses exact-cache evidence, remaining case misses execute, and only containing shard manifests repack |
| Unrelated valid graph-node edit | Prior root has every current raw-case key | No raw case key changes, zero cases execute or repack, all cases revalidate, root rebuilds |
| Verifier change | Prior root has every current raw-case key | Zero cases execute; all raw cases revalidate, all derived case records and shard manifests rebuild, root rebuilds |
| Projection/profile-summary change | Prior root has every current raw-case key | Zero cases execute; cases revalidate/reproject, raw and verified case objects remain usable, shard manifests remain when their references match, root rebuilds |
| Shard-boundary-only change | Prior root has every current raw-case key | Every case key and raw case digest is unchanged, zero cases execute, affected shard manifests repack under the new partition, root rebuilds |
| Approved removal of one dependency edge | Prior root has every old case key; no current key for the changed consumer | The removed dependency is a prior-graph delta, the consumer is reached through both-graph comparison, exactly its current reverse closure is impacted, and its missing cases execute |
| Candidate attempts an unapproved owner narrowing or ignore expansion | No current-key evidence for the protected prior/current ownership union | The candidate cannot authorize its own graph; the validated union remains the effective conservative owner and exactly its current consumers execute, or planning fails if that union cannot be represented |
| One action key has two authority-valid candidates with distinct object digests | Both candidates are in the sealed snapshot | The monotonic conflict tombstone commits and removes the candidate set, then `NondeterminismDetected` returns; neither candidate is chosen and no semantic outcome/capability publishes |
| Global raw schema/capture/encoder change, or unknown production owner conservatively mapped to all raw cases | No exact current-key evidence | Every raw case is impacted and executes twice, every shard manifest repacks, then all revalidate and root rebuilds |
| Invalid graph or unknown input with no validated safe owner | Any | Policy/configuration failure before acquisition or execution |

Tests assert equality with every declared set, not merely that an expected node
appears. `impacted` must neither omit a required action nor include an
unrelated action, and `execute` must equal current case-observation-key misses
for the declared evidence snapshot, rather than the impact set. `repack` is derived independently from the current
shard specs and ordered case-object references.

## 6. Canonical encoding

The Rust encoder is authoritative. The v1 transport encoding is UTF-8 JSON
with these restrictions:

- object keys are sorted by the raw UTF-8 bytes of their decoded Unicode
  scalar strings before escaping and are emitted in that order;
- array order is preserved and semantically significant;
- integers use the shortest base-10 spelling;
- floating-point values are forbidden;
- strings preserve their Unicode scalar sequence without normalization and use
  exactly these escapes: `\"` for U+0022, `\\` for U+005C, `\b`, `\t`, `\n`,
  `\f`, and `\r` for their corresponding controls, and lowercase `\u00xx` for
  every remaining U+0000 through U+001F scalar;
- U+002F `/`, all other non-ASCII scalars, U+2028, and U+2029 are emitted
  unescaped as UTF-8;
- absent optional fields are absent, not `null`;
- there is no insignificant whitespace and no final newline; and
- maps use `BTreeMap` or an equivalently ordered representation.

Encoding is bounded and streaming. `CanonicalEncode` writes to a
size-limited `CanonicalSink` that may feed an incremental domain-separated
hasher and/or a bounded file; it never requires materializing an unbounded
`Vec<u8>`. The sink fails before crossing its declared byte ceiling. Strict
decoders have a schema-specific maximum, stream through bounded readers,
reject unknown/duplicate/noncanonical data, and compare an incremental
canonical re-encoding before yielding a typed value. Every hashed wire type
uses its purpose-specific digest newtype and its entry in the frozen
`ProtocolDomainV1` registry; a generic digest cast is unavailable.
`ci-runner` owns the effectful `BoundedRead` and feeds chunks into the pure
`CanonicalDecoder`; transport/I/O failure remains `InfraError`, while malformed
canonical bytes remain `DecodeError`. Neither layer converts one into the
other.

Content digests include a domain tag and a length-delimited canonical byte
string. Human-readable pretty JSON is a projection and is never hashed in
place of canonical bytes. Existing H2 v1 Node canonicalization remains a
compatibility input during migration; golden tests must prove exact parity for
all existing values before Rust validates a v1 fingerprint.

The binary hash framing is itself versioned and golden-tested: an ASCII domain
is prefixed by its unsigned big-endian 32-bit byte length, and the canonical
payload by its unsigned big-endian 64-bit byte length. Integer schemas declare
their exact signed/unsigned width. Decoders combine a valid UTF-16 surrogate
pair into its Unicode scalar, reject an unpaired surrogate, reject duplicate
decoded object keys even when their source escape spellings differ, and reject
out-of-range integers, unknown fields, noncanonical path spellings/collisions,
or any input whose canonical re-encoding is not byte-identical. Thus a valid
but noncanonical optional escape is not accepted as canonical transport. No
implementation may infer hash framing from the informal `||` notation in this
document.

A raw case-observation schema, capture contract, or encoder version is part of
its `CanonicalPayload` closure and normally changes every consuming case key.
Reusing old payload bytes under the new key is forbidden. FCI-1 through FCI-10
configure no payload migrations and add no migration hook or placeholder; an
old-key/current-key mismatch executes fresh. Payload migration is outside the
v1 architecture and packet sequence. If concrete multi-adapter evidence later
justifies it, it requires a separately reviewed version with its own types,
authority model, packet ownership, and adversarial proof. No current type,
receipt, registry, branch, or fixture reserves that future design.

## 7. H2 fixed shard plan

The generic framework supports an ordered flat leaf set or adapter-owned
deterministic bundle interiors; it does not require shards. The workspace-audit
adapter deliberately uses no corpus shard. Every authoritative H2 profile names
one checked-in plan. The tsc-rs adapter stores its plan at
`.github/ci/plans/<profile>.v1.json`, validated by
`.github/ci/contracts/functional-ci-shard-plan.schema.json` and its Rust type.
The framework sees an adapter plan resource and has no `.github` or shard-plan
path convention.
The plan contains:

- schema, profile id, impact-graph digest, suite kind, and total denominator;
- the qualification/oracle digest from which membership was selected;
- an ordered non-empty list of stable shard ids;
- for each shard, an explicit half-open index range and the digest of its
  ordered case ids;
- the expected union-membership digest; and
- the policy ids that may consume the completed root.

The semantic leaf is one case, never one shard. For each case the adapter
derives a `CaseObservationSpecV1` containing the stable case id, graph schema,
exact reviewed node-spec/closure digests, direct test-input digest, compiler
target/features and allowlisted semantic environment, raw payload
schema/capture/encoder id, and the fixed repetition policy. It contains no
shard id, shard range, job id, worker count, complete plan/profile digest,
verifier, projection, or root identity.

For a case using the recorded-compiler-plan route, that direct input binds the
digest of exactly the resolved recorded-plan row for the case, including its
provenance and execution inputs. It does not bind the aggregate 7,276-case
recorded-plan index or every other row in that index. The aggregate index is a
validated lookup and planning input, and its parser/selection contract remains
reviewed, but changing an unrelated row must not change this case's action
key. A missing, duplicate, ambiguously selected, or digest-mismatched row
rejects planning rather than falling back to the aggregate index digest.

The repetition policy is exactly two fresh isolated executions per case when a
case action must run. `RawCaseObservationV1` retains both results in fixed
repetition order; a mismatch between repetitions is
`NondeterminismDetected`, not permission to choose one result or construct a
canonical semantic outcome from run-dependent bytes. The raw staged record
contains the two semantic observations but no claim about where or when they
were obtained. For a fresh
object, the current non-serializable guard and execution receipt prove both
executions occurred in this invocation. For a cached object, authenticated
producer authority proves its origin; reuse does not pretend they ran in the
consuming job.

The number and boundaries of shards are reviewed scheduling and transport
data. They are not computed from worker count, CPU count, runner type, or case
duration during a run. The planner derives one `ShardBundleSpecV1` containing
only the bundle schema, stable shard id, range, and ordered case ids; it excludes
the full graph/profile and interpretation policy. Changing a boundary changes
shard manifests, the plan, and the root, but it does not change any
`CaseObservationSpecV1`, case action key, or raw case bytes. This independence
is enabled only after isolation and partition-invariance tests prove that the
same case is byte-identical across job assignments, shard boundaries, worker
counts, and neighboring-case sets.

The plan verifier rejects an empty shard, duplicate id, duplicate case, gap,
overlap, reordering, range outside the denominator, changed membership digest,
or union different from the qualified case list. Authoritative execution
accepts `--shard <closed-id>` only. Arbitrary `--start`/`--end` traversal may
remain as explicitly non-authoritative diagnostic tooling.

The scheduler groups only missing case actions inside their selected fixed
shards. Each case receives a fresh sandbox for each of its two repetitions.
Completion order is discarded: case results are addressed by stable case id,
and shard manifests list them only in the checked-in plan order. Cross-case
process reuse is forbidden until the same isolation tests prove it cannot
change a case object. Authoritative `--shard <closed-id>` execution is thus a
resume/scheduling operation, not a different semantic action.

## 8. H2 leaf objects and generic deterministic outcomes

The generic `OutcomeManifestV1<I>` is a canonical Merkle envelope over ordered
stable adapter node ids and verified object references. `Passed` references the
complete verified tree required by the root spec. `Rejected` references the
same complete required action set plus a `FailureTree<I>` of adapter-produced,
canonically ordered deterministic rejection records. Adapter-specific rows
remain in strongly typed verified objects; the generic envelope owns their
keys, digests, membership, ordering, and passed/rejected state. A profile with
no shards uses a flat ordered interior.

One case action produces one `RawCaseObservationV1`, the semantic/cache leaf.
It contains:

- the exact case-observation action key, graph schema,
  `CaseObservationSpecV1` digest, reviewed node-spec/closure digests, stable
  case id, and direct test-input digest;
- exactly two ordered repetition-observation records, each containing the
  candidate and oracle observations required for later comparison; and
- for each repetition, exact output paths, order, and the one bytes-or-digest
  representation mandated by the payload schema, plus diagnostics, process
  result/status, failure boundary, and slice activity where applicable.

It contains no shard id, range, neighboring case, job/worker identity,
authoritative `passed` bit, profile summary, verifier/projection version, full
plan, or full graph digest. Those fields would couple a reusable case to its
current scheduling or interpretation policy.

The current `Verifier` consumes a raw case object and produces a separately
keyed `VerifiedCaseV1`. That derived record binds the raw case-object digest,
verifier action key, deterministic ordered comparison rows, a successful
repetition-equality check, recomputed case summary, and `passed` or
`semantic-rejected` status. Repetition inequality stops before publication of
this record. A verifier change discards or recomputes a derived record but
leaves a matching raw case reusable. A projection or profile-summary change
consumes verified cases without changing raw case bytes or their action keys.

`ShardManifestV1` is a deterministic Merkle interior node, not a semantic or
cache leaf. It contains the exact `ShardBundleSpecV1` digest and an ordered
plan-order list containing, for every member case, its case id, case action key,
raw object digest and byte length, verifier action key, verified-case digest,
and recomputed case summary.
It embeds no raw observation rows. If one case object changes, the runner
rebuilds that shard manifest from the one new case reference and every
remaining exact acquired case reference. A boundary-only change repartitions
the same case references into new manifests without rerunning a case.

Functional-CI v1 transports each digest-addressed object independently. It has
no pack/archive schema, locator, receipt, store path, decompressor, or optional
batching hook. A later measured version may add batching only through a new
owned packet and must still verify each object independently; no v1 interface
reserves that design.

A deterministic candidate-versus-oracle mismatch is an ordered verified-case
comparison, not a thrown orchestration error. Repeating verification of the
same raw case bytes with the same verifier key must produce the same canonical
failure payload. Inequality between repetitions is instead the no-outcome
nondeterminism violation from section 5.2; its differing digests remain in a
non-authoritative receipt. A rejected derived case may be stored for
diagnosis and may be referenced by an exact `Rejected` outcome. It cannot be a
child of a `Passed` outcome or mint an acceptance capability; a deterministic
interior referenced only by `Rejected` may contain it.
All deterministic failures are accumulated in stable case/node order after the
complete required semantic set finishes; a worker may not stop or cancel its
siblings after observing one.

Each adapter owns a total, versioned classifier for sandbox child observations.
Expected/nonzero compiler exits and typed compiler failures may be semantic
records when the classifier says so. Missing output, runner panic, signal or
forced termination, timeout, OOM, and cancellation are infrastructure failures
and cannot be converted to a deterministic rejection because they happened
repeatedly.

An infrastructure failure writes only a non-authoritative per-case execution
log or receipt. It writes no authoritative outcome receipt, capability, or
reusable action-index generation. Unreachable immutable raw/interior/outcome
bytes may remain in staging or CAS but are not discoverable evidence.
Orchestration may retry an infrastructure failure under an explicit bounded
retry policy. A retry reruns both isolated repetitions unless the failure
occurred only at the final generation commit, in which case the coordinator may
retain staged results while it seals a replacement snapshot and repeats the
bounded conflict/publication check. It must not retry a deterministic mismatch
in the hope of a different semantic result. Successfully staged sibling
observations remain non-authoritative when the invocation ends with
infrastructure failure.

## 9. CAS and Merkle root

The existing `target/local-ci-resume/v1/journal.json` mechanism remains a
coarse legacy failed-run journal while the functional runner is introduced.
Its broad phase fingerprints, executable/tool fingerprint, and output
receipts are neither semantic case objects nor action-index, CAS, shard, or
root authority. FCI must not import, rename, or promote one of those receipts
into the semantic store. Legacy phase reuse may coexist during shadowing, but
only objects independently produced and verified through the new core/runner
contracts may enter the functional CAS.

The generic runner receives a configured store root and a backend capability
that proves immutable no-replace objects, atomic snapshot-root compare/publish,
durability, bounded reads, leases, and GC exclusion. It does not prescribe a
path, POSIX filesystem, global generation rewrite, or Git layout; a SQLite or
Merkle-index backend may satisfy the same SPI. The following is the tsc-rs
`LocalFilesystemBackendV1` instantiation and reference crash-test backend, not a
framework-global location:

```text
target/functional-ci/v1/
  objects/sha256/<object-digest>
  outcomes/sha256/<outcome-digest>/outcome.json
  authority-receipts/sha256/<receipt-digest>.json
  publication-events/sha256/<event-digest>.json
  action-index/<action-key>/<candidate-manifest-digest>.json
  conflict-witnesses/sha256/<witness-digest>
  conflict-tombstones/sha256/<tombstone-digest>.json
  conflict-index/<action-key>/<conflict-manifest-digest>.json
  conflict-registries/sha256/<registry-digest>.json
  disclosure-histories/sha256/<history-digest>.json
  index-generations/<generation-digest>.json
  index-head/current.json
  issuer-state/current.json
  locks/index-head.lock
  leases/<invocation-id>.json
  gc-plans/<plan-digest>.json
  staging/<invocation-id>/...
  execution-receipts/<invocation-id>/<case-id>.json
  quarantine/<invocation-id>/...
```

An object digest is the domain-separated SHA-256 of its canonical payload.
Objects are immutable. All local opens are bounded, regular-file-only,
no-follow operations resolved beneath the configured store root; validation
occurs while streaming and before unbounded allocation. Publication uses a
sibling `create_new` temporary file, full write, file `fsync`, byte re-read and
digest verification, an atomic **no-replace** publish primitive, and
parent-directory `fsync`. An ordinary rename that may overwrite the destination is
forbidden. If the platform lacks a trustworthy no-replace primitive, the
runner uses a verified directory-lock protocol or fails closed. If a concurrent
writer already published the same digest, the second writer rehashes the
existing regular file and accepts only identical bytes.

Every locally reusable candidate has a strict `LocalProducerReceiptV1`, separate
from a remote producer receipt. It binds the local issuer/store epoch and
sequence, action/object and source-snapshot digests, effective reuse scope,
proposed disclosure audience, and the observed prior local-authority
generation,
producer engine/build identity, sandbox capability/guard, execution mode, and
the invocation that produced it. It does not bind the replacement generation or
disclosure-history digest. A `PublicationEventV1` then binds that receipt and
the same prior generation, and the replacement generation makes both the event
and monotonically expanded history reachable. The protected local trust
capsule defines the issuer, store ownership/mode, admitted OS principal, key or
platform anchor, restart validation, and anti-rollback capability. A profile
requiring cross-restart authority fails closed when the backend cannot prove
its declared anchor. The tsc-rs local threat model may trust its sole OS account
against malicious rollback, but that limitation is explicit and never
misrepresented as protection from the same principal. Receipt sequence is
monotonic inside the same authority snapshot commit and is never selected from
candidate data.

The action index is a bounded candidate set from one pre-execution action key
to immutable `CandidateManifestV1` objects. A candidate manifest binds that
action key, one object digest, one authority-receipt digest, the corresponding
publication-event digest, object/receipt byte lengths, and their schemas; its
own digest names the final path component.
Authority-receipt bytes are independently canonical, content-addressed objects
under `authority-receipts/sha256/`. The current generation names the complete
ordered candidate-manifest set for each action key and one mandatory
`ConflictRegistryV1` plus `DisclosureHistoryV1` digest. The registry is an
ordered map from action key to
a nonempty ordered set of content-addressed conflict manifests. Two receipts for the same
object therefore produce two independently representable manifests rather than
competing for one path. The index is a mutable lookup hint, not authority or a
last-writer-wins map. Missing,
malformed, incorrect, or invalid references are misses. Consumers never trust
an entry without opening and verifying the named manifest, object, receipt, and
authority. Two
authority-valid distinct canonical object digests for one action key are the
same-key nondeterminism violation from section 5.2; neither may be selected. A
key present in the verified current conflict registry is ineligible before
candidate acquisition or execution, even if zero or one candidate is currently
visible. A missing, corrupt, rolled-back, or over-limit registry is
infrastructure failure, not a miss.
`verify-cache` may produce an exact-restore plan or `RolloverPlanV1` but cannot
authorize or apply either. A rollover plan is valid only while the complete
authenticated conflict and disclosure sets are available; a missing, corrupt,
rolled-back, or unverifiable conflict set permits exact protected restore only.
Capacity pressure permits membership-preserving authenticated compaction only.
FCI-6e may apply an unchanged valid rollover plan after taking the store-wide
exclusive epoch barrier, preventing new old-epoch leases/invocations, and
draining every existing old-epoch lease and invocation. Rollover advances only
the authority-store/locator epoch in the protected capsule; it does not change
`ApplicationNamespaceV1`, semantic closures, or action keys.
The new head permanently rejects the old head/issuer/locator epoch for future
lookup, publication, and capability minting, and requires complete fresh
requalification of every eligible nonconflicted action before reuse or
publication. A capability already returned by a completed old-epoch invocation
remains an immutable in-process fact; it cannot be serialized into or reused by
the new epoch and is not retroactively revoked. The new head retains the exact
old `ConflictRegistryV1` and `DisclosureHistoryV1` roots as predecessor
commitments, carries every tombstone under its unchanged action key, and
monotonically carries the audience union. Rollover therefore cannot make known
nondeterminism deterministic or previously disclosed bytes secret. It cannot
retain selected old evidence as reusable, claim a tombstoned key was repaired,
or be initiated by candidate content. A membership-preserving
authenticated compaction is allowed only when every old conflict membership
proof remains verifiable; ordinary retention/GC is not repair.
Readers consider only index references reached from the
`CandidateIndexGenerationV1` named by the valid canonical
`index-head/current.json`; a generation, candidate manifest, receipt,
publication event, or object that exists only as a loose file is never a
discoverable candidate. Objects, authority receipts, publication events,
candidate manifests, conflict
witnesses/tombstones/manifests/registries, disclosure histories, generation manifests, and their content-addressed
directories are immutable and publish no-replace. Within authoritative
CAS/index data, the head is the only file ever replaced in place. Staging,
locks, and lease lifecycle files confer no semantic authority, and GC may
delete unreachable immutable files but never modifies their contents.
`locks/index-head.lock` is an OS-lock rendezvous and is never parsed as
semantic state.

A local outcome generation-wide compare-and-swap commit runs under the
exclusive index-head lock and follows this exact protocol:

1. open and fully validate the current head, issuer state, complete conflict
   registry, and disclosure history, compare them with the expected state bound
   by the sealed snapshot, reject a candidate addition for any conflicted key,
   and reject any disclosure-history shrink;
2. stage, `fsync`, no-replace publish, and re-read every immutable replacement
   object, authority receipt, publication event, candidate manifest,
   inherited/monotonically expanded conflict/disclosure references, generation
   manifest, and parent directory; the receipt binds the prior generation, the
   event binds the receipt, and only the replacement generation binds the event
   and expanded history;
3. write a sibling temporary head containing only the replacement generation
   digest and its framing/version, then `fsync` and re-read that file;
4. atomically replace only `index-head/current.json`, then `fsync` its parent
   directory; and
5. reopen, decode, canonical re-encode, and verify the installed head and the
   reachable generation before returning
   `IndexCommitGuard<LocalAuthorityCommit>`.

A conflict uses the same lock, head, durability, and final re-read but a
different private delta and guard. After validating at least two distinct
authority-valid canonical witnesses or one repetition-inequality pair, the
coordinator reopens the current generation, builds only the monotonic union of
its registry and the new `ConflictTombstoneV1`, removes the affected key's
candidate set, then no-replace publishes and re-reads the witnesses, tombstone,
`ConflictAuthorityReceiptV1`, conflict manifest, registry, and replacement
generation. It atomically replaces/fsyncs/reopens the same head and returns
`IndexCommitGuard<LocalConflictCommit>`. Only after this final verification may
the invocation return `NondeterminismDetected`. There is no registry shrink or
tombstone deletion API, and the conflict guard is ineligible for every root
constructor. Concurrent outcome/conflict commits retry from the newly verified
complete state; an outcome can never reintroduce a candidate for a conflicted
key.

The platform must provide atomic replacement plus the required file and
directory durability semantics under the lock, or the authoritative local
backend fails closed. A concurrent head change returns no guard; the
coordinator reopens and revalidates the new generation and reruns the same-key
conflict rule before a bounded retry. It never overwrites an immutable
candidate or publishes a partial set of index winners. Crash tests cover every
boundary before and after each immutable `fsync`, head-temp write/`fsync`, head
replacement, parent `fsync`, and final re-read; after recovery readers observe
exactly the old complete generation or the new complete generation, never a
loose or partial one. For a reported conflict, the old state means the report
was not yet returned; the new state means subsequent reuse is durably disabled.

The generic `MerkleManifestV1` binds the adapter root spec, ordered leaf and
optional interior references, derived policy keys, and recomputed summary. A
composite-profile manifest binds the ordered verified adapter suboutcomes and
their exact global-id-qualified leaf membership. The H2 `RootManifestV1`
specializes an adapter envelope
and contains:

- schema, root kind, semantic profile/policy, impact-graph digest, and all
  input/plan/oracle digests;
- the ordered exact shard set;
- each shard's shard-spec digest, `ShardManifestV1` object digest, byte length,
  membership digest, and recomputed summary;
- the ordered union of every case id and case-observation action key reached
  through those shard manifests;
- the exact projection, profile-summary, and root-node action keys;
- the union-membership digest; and
- the aggregate summary recomputed from verified raw case observations.

The outcome does not contain its own digest. Its canonical bytes determine the
outcome digest and storage path. After all actions finish, the single
coordinator stages raw/verified objects first, optional interiors second, the
complete `Passed` or `Rejected` outcome bytes third, and its authenticated
receipt and replacement index manifests fourth. Those immutable CAS bytes
remain unreachable and confer no authority until one atomic
unchanged-generation commit installs the replacement `CandidateIndexGenerationV1` that
references them. The returned `IndexCommitGuard<LocalAuthorityCommit>` is
rechecked before a capability is minted. A process crash or failed generation
check may leave
unreachable objects, interiors, outcome, receipt, or index-manifest bytes, but
never a discoverable partial authoritative outcome. Only `Passed` may be
exposed through a verified-root acceptance capability.

Outcome verification rehashes the outcome, every interior, raw observation,
and derived record; validates every typed schema, current exact action closure,
adapter plan/topology, and action key; reconstructs ordered membership; reruns
the current verifier; and recomputes every projection, summary, and passed or
rejected state from raw observations. A full graph, verifier, projection,
profile-summary, or H2 shard-plan change therefore produces the required new
interiors/outcome after complete revalidation without requiring execution for
raw observations acquired under unchanged exact keys. A serialized
`verified`, `ready`, `fresh`, or `cache_hit` boolean grants no authority.

Every active publisher and reader first acquires the store-wide shared
reader/publisher barrier, then creates a durable lease naming its invocation,
staging roots, and already published references, and holds both the shared
barrier and an OS-backed exclusive liveness lock for the lease lifetime. PID,
timestamp, or heartbeat text alone is not liveness authority. Mutating GC must
acquire the corresponding store-wide exclusive barrier before its final
snapshot revalidation and retain it through index removal, sweep, and directory
`fsync`. Consequently no reader or publisher can create a lease between the
last recheck and deletion. A backend may use an equivalent generation protocol
only if its proof establishes the same exclusion; otherwise mutating GC fails
closed.

GC treats a lock-held lease as active; an unlocked crash residue becomes
eligible only under `GcPolicyV1`. If the backend cannot distinguish active from
stale safely, mutating GC fails closed. GC freezes a mark epoch,
first applies checked-in `GcPolicyV1` to select retained action-index candidates
and expired index references, then marks pinned outcomes, live receipts,
retained candidates, the current conflict registry and every reachable
conflict witness/tombstone/manifest/authority receipt, active leases, and their
complete Merkle closures, plus the complete disclosure history and its
receipts. Conflict-control/disclosure objects reachable from the current
registry never expire under v1. The
retention order uses receipt sequence and stable digests, never filesystem
access time. It sweeps only unmarked objects older than the epoch after
rechecking the lease set; if pinned/live bytes alone exceed the requested
ceiling it fails without deletion. It cannot race a reader or delete an object
between child staging and the authoritative outcome-generation commit. A dry
run writes canonical `GcPlanV1`
bytes covering both index mutations and object deletions plus a plan digest;
apply accepts that exact digest, acquires the exclusive barrier, revalidates the
store/lease/index-generation snapshot, and refuses mutation if it changed.
Apply removes/fsyncs expired index references before deleting their
now-unreachable objects while retaining that barrier through both phases.
Quarantine has a bounded, explicit retention policy and cannot evade the
local-store ceiling.

## 10. Rust policy capabilities

Policy is represented by verified Rust values, not a caller-selected string or
an implementable authority trait. The pure `VerifiedMerkle` and verified
policy-spec mechanism are defined by `ci-core`; `ci-runner` owns the
effect-bound wrappers and non-serializable
run/cache proofs and re-exports the consuming surface. A policy marker is only
a type label and deliberately has no sealed trait bound:

```rust
pub enum FreshOnly {}
pub enum ReuseAllowed {}

pub struct AuthorizedRoot<P> {
    root: VerifiedMerkle,
    proof: PolicyProof,
    marker: PhantomData<fn() -> P>,
}
```

`ci-runner` defines the generic `AuthorizedRoot<P>` mechanism. The xtask hosted
composite-profile layer defines its own zero-sized `HostedTsTestsPolicy` and
local alias
`type HostedVerifiedRoot = AuthorizedRoot<HostedTsTestsPolicy>`; neither the
marker nor ts-tests-specific fields appear in a generic crate. Naming,
implementing, or importing a new `P` grants no authority and no policy behavior
is dispatched through `P`.

All fields and constructors are private. These types implement neither
`Deserialize` nor `Clone` nor a public unchecked conversion. The sole internal
constructor has the conceptual signature
`authorize<P>(VerifiedMerkle, VerifiedPolicySpec<P>,
VerifiedConsumerEngine, VerifiedExecutionEvidence<P>,
IndexCommitGuard<LocalAuthorityCommit>) ->
AuthorizedRoot<P>`. The policy spec, effect-verified consumer identity, and
typed execution evidence have private fields and are
non-cloneable/non-serializable; the policy spec is returned only after the current sealed profile
and its adapter policy have passed the generic verifier. The constructor
consumes the effect-bound values and commit guard and binds their
source/evidence snapshots, effective reuse scope, conflict-registry, policy-spec,
consumer-engine release/binary/channel/workflow, execution mode, and
committed-generation digests into the non-generic `PolicyProof`.
A conflict or remote-publication guard, a recorded engine identity without its
verified effect capability, verified Merkle bytes, or user-defined marker alone
cannot construct the wrapper.

`AuthorizedRoot<FreshOnly>` is minted only when the fresh verifier converts a
complete fresh trace plus a unique `FreshExecutionGuard`, created before
execution, into `VerifiedExecutionEvidence<FreshOnly>`. The guard and evidence
are non-cloneable and non-serializable, and the conversion rejects any selected
acquisition source. Prior receipts remain immutable but are ineligible for that
run. Every executable
leaf must satisfy its adapter-owned fresh-execution policy in that invocation;
for H2 this is exactly two isolated repetitions, followed by rebuilt shard
manifests. A carried or cached object, disk root, uploaded artifact, later
process, or cache restore can never contribute to `FreshOnly`.

`AuthorizedRoot<ReuseAllowed>` is minted only by the reusable verifier for a
profile that explicitly permits reuse. It rechecks the current action keys and
the complete Merkle tree. There is no `ReuseAllowed -> FreshOnly` conversion.
A fresh tree may be reconsidered under a `ReuseAllowed` profile only by passing
the same complete verifier.

`HostedVerifiedRoot` is a distinct adapter policy, not an alias for `FreshOnly`
or `ReuseAllowed`. Only `HostedVerifier::verify_or_run` may construct it. Its
private capability binds the hosted profile id, the digest of the exact
expected root/leaf action-key set, current graph and canonical-input digests,
root digest, hosted suite kind, evidence-availability/resource-policy digests,
consumer-engine identity, protected authority capsule, and the digest of every
leaf's verified origin receipt. The capability's
noncanonical origin set may
contain `authenticated-prior-root`, `authenticated-exact-cache`,
and `fresh-current-job` leaves in any exact
expected mix. A generic reusable root cannot be converted to it.

No `AuthorizedRoot<P>` constructor accepts `Rejected`; a rejected outcome is
canonical and exactly reusable only as a failure result, never as evidence of
acceptance.

## 11. Exact hosted-cache consumption

Hosted CI may consume a remote cache after the activation commit, subject to
all rules in this section. The public command and semantic scope remain the
single unsplit `cargo xtask acceptance` boundary sourced from `ts-tests`.
Owner-control runners, local evidence producers, readiness, performance,
stress, and non-`ts-tests` cases remain excluded.

At FCI-9b, `HostedVerifiedRoot` must cover the complete current ts-tests action
set reached by `cargo xtask acceptance`, including its conformance, H1, and
historical H2 acceptance subcalls. An H2-only root or a root that silently
omits one of those current subcalls cannot authorize the hosted command.
`CompositeProfileV1` binds those ordered adapter subroots and their exact leaf
membership into the hosted root action key and outcome.
Earlier migration packets may shadow and prove subsets incrementally, but hosted
activation waits until every current hosted subcall is represented by an exact
fresh-or-verified action and included in the hosted root. This completeness
requirement does not admit owner controls or any other local-only action.

The hosted suite has its own profile and root kind, `hosted-ts-tests`. A local
closure root that includes owner controls is a different type and action key;
it cannot satisfy the hosted profile. The cache may contain only data used by
the hosted ts-tests projection and the receipts required to authenticate that
data.

Remote lookup uses an exact logical locator and a separately authenticated,
bounded candidate index. Physical candidate objects are immutable and include
their own object and receipt digests:

```text
functional-ci-v1-index-<namespace-kind>-<namespace-epoch>-<platform-scope-digest>-<action-key>
functional-ci-v1-object-<action-key>-<object-digest>-<producer-receipt-digest>
```

`AuthenticatedCandidateIndexV1` binds the exact logical locator, protected
namespace epoch, trust-policy/effective-scope digests, its publishing authority
generation, and an ordered size-bounded set of immutable `(object digest,
producer receipt digest, publication event reference, physical locator)`
candidates. It does **not** require an old candidate index's authority-head
digest to equal the current global head. It is a discovery structure, not
semantic authority; each candidate still passes both proofs below.

`RemoteAuthorityHeadV1` binds its monotonic authority generation, epoch,
predecessor-head commitment, append-only publication-event root, conflict root,
and disclosure-history root. Starting from the mandatory current authenticated
head, a consumer verifies that a candidate's publication event is included in
that head's append-only event history (and that its publishing generation is an
authenticated ancestor), then verifies the **current** action-key conflict
proof and **current** object/audience disclosure proof. An unrelated later
publication may advance the head without invalidating the candidate; a later
conflict or insufficient current disclosure makes it ineligible. A stale index
cannot hide current conflicts because failure to obtain and authenticate the
current authority head is infrastructure failure.

The generic runner exposes only the single
`AtomicSnapshotPublisher` SPI from section 3: compare one opaque observed
publication generation and atomically publish one verified immutable snapshot
root. FCI-8c selects and freezes exactly one provider-internal realization;
candidate code cannot select it and generic code has no strategy enum or
fallback branch. Two possible research outcomes are:

- `CompareAndSwapIndex` maintains a mutable authenticated generation head. A
  commit atomically compares the complete generation captured by the snapshot
  and installs one replacement generation that names the new immutable indices.
- `EpochRotatedImmutableIndex` never mutates an index or epoch head supplied by
  candidate content. A protected, authenticated `AuthorizedEpochRotation`
  compares the current epoch-map generation and publishes a new immutable epoch
  plus index set in one provider transaction. Consumers use only the exact new
  epoch named by the protected epoch map.

This normal publication strategy is distinct from corruption-recovery epoch
rollover. Recovery rollover requires a provider-equivalent store-wide exclusive
epoch barrier: it prevents new old-epoch reads that could mint authority and
all old-epoch publication, drains registered readers/publishers/invocations,
then advances only the protected storage/locator epoch map, never the semantic
application namespace or action keys. It is allowed only from fully verified
conflict/disclosure authority state; otherwise exact restore is required. The
new authority head commits both old conflict and disclosure roots/head as
predecessors, carries every tombstone under the unchanged action key and the
full audience union, and makes old candidates/receipts ineligible for future
authority. A local in-process capability returned before the barrier is an
immutable historical result and is not retroactively revoked.

The implemented provider adapter contains only the selected realization, and
no runner path may silently fall back to the other. FCI-8c is a
read-only provider-capability freeze: it records the exact provider/API version,
selects one strategy, maps its atomic primitive and durability/visibility
semantics, namespace/auth/attestation, quotas and retention,
repair/rotation/recovery procedure, and typed miss/retry/infrastructure-failure classification.
No provider backend or workflow code may be written before that packet is
reviewed and frozen. FCI-8e implements only the selected strategy and proves
its compare/rotation, crash, conflict, quota, and recovery semantics before
activation. A physical locator
is a typed provider-local digest key constrained to the repository/namespace in
`TrustRootV1`, never an arbitrary URL, host, absolute path, or
credential-bearing string. A single immutable GitHub cache entry keyed only by
`ActionKeyV1` is not an acceptable backend because one corrupt or squatted
entry could permanently block a later valid publication. An invalid candidate
can coexist with a later valid immutable candidate, and a poisoned index can be
bypassed only by the selected protected repair/epoch mechanism, never by an
inexact fallback.

After the local outcome commit, a trusted publisher opens a distinct
`RemotePublicationSnapshotV1`. It binds the selected remote strategy and the
exact candidate-index/epoch-map and protected authority-head generation then
observed; it is not the
earlier semantic `EvidenceAvailabilitySnapshotV1` and cannot alter the inputs
already used by that invocation. Publication first stages the outcome,
immutable candidate objects, and a producer receipt that binds the observed
prior authority generation rather than the replacement head. It next builds a
`PublicationEventV1` referencing that receipt and its audience delta, the
replacement index manifests referencing the event, the monotonic
disclosure-history union, and a replacement `RemoteAuthorityHeadV1` whose predecessor is
the observed head and whose event root includes the new event. The replacement
head commits the receipt/event/history; none of them commits the replacement
head, so the graph is acyclic. Only then does it invoke the selected strategy's
atomic commit. That operation compares the complete
remote generation against the generation captured by this publication
snapshot and installs one replacement snapshot root containing both candidate
and authority heads only on equality. A remote
generation conflict forces a bounded refetch, authority validation, and
same-key uniqueness check before a replacement publication snapshot may be
sealed; it never overwrites or silently appends to stale indices. A newly
observed distinct authority-valid candidate aborts remote publication as
`NondeterminismDetected` only after the provider atomically commits its
protected monotonic conflict registry and removes that logical key from the
published candidate root; it does not revoke the already linearized local
capability. The conflict path returns only
`IndexCommitGuard<RemoteConflictCommit>` and cannot publish a semantic outcome
or mint a root. A provider that cannot prove monotonic conflict publication and
rollback rejection is ineligible. Bytes written before this check remain
unreachable. Successful
commit is the single shared-cache authority-publication linearization point and
returns a non-serializable
`IndexCommitGuard<RemotePublicationCommit>`. It does not mint or alter the
locally committed in-process capability. A different valid object
discovered in a later generation follows that same conflict commit before
later snapshots report `NondeterminismDetected`; it does not rewrite the
immutable receipt or
retroactively change the linearization of a capability returned under the
earlier local generation.

The cache/transport threat model permits corruption, replay, omission, and
temporary denial of service. Exact authentication and verification preserve
semantic safety; no cache design can force a malicious transport to return
available bytes. Suppression of an optional candidate causes an exact miss;
suppression, corruption, or rollback of the protected remote authority head for
a shared profile is infrastructure failure. Neither becomes a false pass.
Integrity does not imply confidentiality: the protected effective
`ReuseScopeV1` and audience authorize which immutable objects a PR reader may
discover or open. Only explicitly public evidence is PR-readable;
local/sensitive evidence and a different trust tier are indistinguishable from
an exact miss. A provider that cannot issue the required read-only,
audience-scoped capability is unavailable for that run. Removing a public locator after
scope narrowing is future eligibility control, not erasure: receipts and the
monotonic disclosure-history head continue to state every audience that could
already have copied the immutable bytes.

Workflow configuration supplies only computed exact locators. Prefix matching,
`restore-keys`, most-recent fallback, cross-profile fallback, and a key derived
from a cached file are forbidden. The H2 case key binds its exact reviewed
closure and raw payload contract rather than a shard or full profile, so a
boundary, job assignment, verifier, projection, or unrelated graph-node edit
does not hide reusable raw evidence. The full graph, impacted set, required
cases, exact current keys, and candidate indices are computed and sealed in
`EvidenceAvailabilitySnapshotV1` before execution. A root cache hit cannot
establish that the impact graph, case set, or partition is current.

FCI v1 requests and verifies each candidate object through its exact locator;
provider-side batch or archive transport is not part of the framework contract
and cannot be surfaced as evidence.

Prior-root carry-forward uses only a root digest named by an explicit,
authenticated baseline receipt, such as the selected base revision's recorded
root. Absence of that exact receipt means there is no prior candidate. It never
uses a newest-root query, prefix lookup, branch-name fallback, or a root chosen
from cache contents. A prior root with a different full graph or root action
key is only an authenticated index of candidate cases; it cannot be
accepted as the current root. Its children are checked independently against
their exact current case keys.

Restored bytes are untrusted. The tsc-rs filesystem backend extracts them only
under its configured ignored, invocation-specific
`target/functional-ci/v1/incoming/` directory. No restored
file is executed, loaded as a library, copied into source, or added to `PATH`.
Symlinks, special files, absolute paths, `..`, unexpected entries, duplicate
entries, and files outside declared size/count ceilings reject the cache.

Every accepted remote action and any reused root candidate require two
independent proofs:

1. **Producer authority.** `ProducerReceiptV1` binds a stable repository id,
   commit/tree digest, immutable workflow ref and workflow-file digest, event
   and trust tier, issuer/subject/audience, exact action key and object digest,
   effective reuse scope/audience, producer engine release/binary/channel and
   build-artifact identities, graph schema, reviewed node-spec/closure digest,
   payload schema, `ExecutionPlatformV1`/`ToolchainSetV1` digests, hosted suite
   namespace/epoch, trust-policy digest, and the observed prior authority
   generation. It never binds the replacement authority head or
   disclosure-history digest. Its enclosing `PublicationEventV1` binds the receipt digest
   and is proved included from the mandatory current authority head as
   described above. An outcome receipt additionally
   binds the complete graph, profile, plan, derived action keys, outcome kind,
   and exact interior/leaf set. A leaf receipt may record its producer's full
   graph for audit, but unrelated full-graph equality is not required when its
   exact current node closure matches. Likewise, the producer build-artifact id
   must be attested and internally consistent but need not equal the current
   whole-executable id when the reviewed current semantic closure is identical.
   Signatures/attestations and candidate indices are verified only against the
   protected-base `TrustRootV1`, never an issuer/subject policy supplied by the
   candidate checkout. Untrusted pull-request jobs receive only a proven
   read-only credential for their exact public audience, and it is closed before
   candidate-controlled execution; they cannot populate, repair, or rotate a
   shared namespace. A cache entry without valid producer authority is not
   evidence.
2. **Semantic verification.** Rust rehashes canonical bytes, validates the
   current exact node spec/closure and action key, invokes each owning adapter's
   current verifier, verifies both repetitions for every H2 raw case, and, after
   acquisition, reconstructs every adapter suboutcome, exact composite ts-tests
   membership, H2 shard manifests, comparisons, projections, and summaries. It
   also proves that no owner-control or other forbidden action is present.

Only after both proofs pass for every reused leaf, every exact-current-key miss
is satisfied by that adapter's complete fresh-execution policy, every raw
observation passes current revalidation, every
adapter interior/suboutcome and the composite root are verified/rebuilt, the
new complete root verifies, and the invocation's local authoritative generation
commit returns its guard may Rust mint
`HostedVerifiedRoot`. The cache provider's hit flag, key match, transport
checksum, successful read, or old root is never sufficient.

If the exact complete result instead verifies as `Rejected`, the hosted command
may return that deterministic failure without compiler execution, but constructs
no `HostedVerifiedRoot`. It must reverify the same complete current action set
and cannot use a cached failure as fail-fast permission to omit siblings.

An action miss or invalid entry runs that action's complete policy in the
current job; for H2 this means two isolated case executions. This is independent of
the semantic `impacted` set. Conversely, an impacted action with independently
valid exact-current-key evidence need not execute. An invalid entry is
quarantined and reported as `cache-rejected`; no byte or row from it is
partially reused. The runner rebuilds affected adapter interiors/suboutcomes and
aggregates a composite root from the exact expected mix of reverified prior,
cache, and fresh leaves. If any required fresh action cannot complete
its policy, the job fails as infrastructure failure.

Shared-cache publication occurs only after the coordinator has constructed and
locally committed a complete canonical outcome and authenticated producer
receipt. A trusted writer seals the separate remote-publication snapshot,
stages the immutable remote objects, receipts, and replacement index manifests,
then invokes the selected remote strategy's atomic unchanged-generation commit
against that snapshot. Authority-valid raw and derived evidence
from either complete outcome kind enters the shared semantic-evidence
generation only at that commit and is always reinterpreted by a consumer's
current verifier. Only a `Passed` outcome may enter the accepted-root namespace.
A trusted profile may publish an exact `Rejected` outcome to a typed rejection
namespace so the same canonical failure can be reused without minting any root
capability. A post-local-outcome upload or remote-generation conflict leaves the
local semantic bytes/capability intact but publishes no shared authoritative
receipt or reusable remote generation; unreachable remote bytes may remain for
provider retention. Infrastructure failure before local commit publishes no
local authority. The remote cache is transport; Rust verification is authority.

Namespace separation is capability routing, not semantic-key separation. For
one action/root key the sealed snapshot unions every authority-valid candidate
from semantic-evidence, rejection, and accepted-root indices before applying
the same-key conflict rule; a producer cannot hide two different outcomes by
placing them in different namespaces.

### 11.1 Resource and sandbox policy

Every execution profile names a checked-in `ResourcePolicyV1`. It contains
maximum worker count, total/process CPU quota or duty ceiling, weighted
CPU/memory slots, per-action memory weight,
process timeout, bounded infrastructure retry count, cache-transport
availability policy, maximum open files,
maximum action/interior/outcome/log bytes (including H2 case/shard limits),
maximum incoming object count/total bytes, local CAS ceiling, and temporary-disk ceiling.
Hosted `TrustRootV1` supplies hard upper ceilings; candidate content may request
stricter limits but cannot raise worker, CPU, memory, network, disk,
incoming-object, or open-file authority.

The policy covers the control plane as well as sandbox children. It therefore
also fixes aggregate runner/coordinator CPU-duty and RSS ceilings; graph and
inventory resident-byte ceilings; hashing, canonical encode/decode, CAS
verification, incoming-object decode, and explanation concurrency and weights;
bytes read/hashed/decoded per invocation (and, where the
backend supports it, per-second ceilings); and bounded queue capacities. At
most one heavyweight control-plane task or semantic action process may occupy
each configured heavy slot. A large file set, cache hit set, corrupt object,
or explanation request cannot bypass admission merely because it does not
spawn a compiler child.

The scheduler admits an action only when its declared weights fit the remaining
budget. It uses bounded channels and returns invocation-private staged results
to the single coordinator in fixed input order. Workers have no publication or
action-index authority. A resource limit, timeout, or retry changes execution
receipts but not canonical semantic bytes. Exceeding a limit is infrastructure
failure. A resource policy
must never reduce case membership or convert a missing result into a semantic
pass; a policy that would do so is invalid.

CPU/memory weights are admission controls, not assumed enforcement. Hosted and
local backends must also enforce the declared process-group/cgroup/job-object
quota so a child that spawns threads cannot consume unbounded CPU or memory.

FCI-4 and FCI-7 each freeze a versioned resource-evidence artifact containing
the exact machine/profile identity plus cold and warm planning wall time, CPU,
peak RSS, bytes processed, and concurrency observations, with baseline and
hard ceiling for every control-plane class above. Activation uses the current
artifact rather than an anecdotal run. For an activated fully reusable profile,
the mandatory warm no-impact fixture proves all of the following
simultaneously: semantic action processes spawned equals zero; compiler,
oracle, and test subprocesses spawned equals zero; every required reusable
action is reopened and reverified; the complete root is rebuilt and committed;
and only bounded control-plane work within the frozen CPU/RSS/byte ceilings
occurs. A generic profile containing `NonReusable` actions has a different
fixture: it freezes their exact sorted ids and process count, proves that only
that set executes, and cannot claim the process-count-zero result. The tsc-rs
`local-full` and hosted activation profiles are required below to have an empty
such set.

Filesystem mtime is never authority. An in-process metadata memo may be a
discardable planning hint, but a reuse decision must revalidate semantic bytes
and digests or consume an independently authenticated content identity whose
trust contract is part of the sealed snapshot.

The sandbox receives the exact read-only `SourceSnapshotV1` mount named by
`ExecutionSpecV1` and a fresh private output directory. Its environment is
an explicit non-secret allowlist with fixed locale, timezone, umask, temporary
directory, and platform contract. Semantic actions have no network; remote
cache transport and credentials remain outside the action sandbox. The runner
records every opened declared input and rejects observed access outside the
modeled closure. A platform that can neither enforce nor completely audit the
declared closure cannot produce hosted-reusable evidence for that action. Nix
may provide the outer sandbox, but `ci-runner` still owns the declaration,
receipt, and validation.

`SandboxCapabilitiesV1` classifies every nondeterministic/effect channel as one
of `DeclaredInput`, `Denied`, `FixedVirtualValue`, or `CompletelyAudited`.
Required channels include wall/monotonic clock, entropy devices and runtime RNG,
PID/process enumeration and host `/proc`, hostname/machine identity, IPC and
background daemons, network/name service, dynamic-loader search/configuration,
CPU feature/runtime dispatch, filesystem metadata/enumeration, locale/timezone,
and inherited descriptors/environment. A repetition policy is a detection
fixture, not proof that an uncontrolled channel is absent. Only the runner may
create a private, nonserializable `SandboxExecutionGuard` after the backend
proves the exact capability set; the fresh observation receipt consumes that
guard and binds its digest. An action/backend with an unclassified or
unenforceable channel is `NonReusable` or fails closed according to the frozen
profile and can never publish shared evidence.

## 12. Pure projections and single execution

Each generic action executes its adapter-owned repetition policy once. Each H2
case therefore produces its required two ordered raw repetition records once.
All H2 user-facing results are pure projections over verified case objects:

```text
acceptance(outcome) -> pass or deterministic failure
inventory(outcome)  -> ordered mismatch rows
summary(outcome)    -> exact counters
hosted(outcome)     -> ts-tests-only acceptance result
local(outcome)      -> corpus result plus separately verified owner controls
```

Projection functions accept typed verified data and configuration values only.
They do not receive a workspace path, compiler executor, process runner, clock,
random generator, cache client, or mutable global counter. They perform no
compiler execution and write no semantic object. Inventory and acceptance
therefore cannot traverse the corpus independently or disagree about a case
whose one two-repetition case object was acquired once.

Owner controls remain a separate fixed plan and root. The local command may
project a typed pair `(corpus_root, owner_root)`. Hosted code has no function
whose signature accepts `owner_root`.

### 12.1 Complete `local-full` profile

`local-full` is a versioned `CompositeProfileV1`, not an informal synonym for
"whatever was convenient to run." `CompletePhaseRegistryV1` is the sole
authoritative denominator and dispatcher input for both fresh and reusable
`cargo xtask ci`. It records the ordered duplicate-free phase ids,
dependencies, adapter/legacy implementation descriptors, required leaves,
projection, cacheability/reuse scope, and legacy-tail disposition. Its fields
and complete constructor are private; the command receives only a
`VerifiedCompletePhaseRegistry` produced by the protected profile verifier.

Only the sealed dispatcher owns `PhaseExecutionAuthority`, process spawning,
or the ability to return the final local capability. A phase implementation may
compute its typed result but cannot mark itself dispatched or complete. The
workspace dependency/call-graph/source audit forbids another authoritative
`ci` entry, direct phase/process invocation outside the dispatcher, alternate
phase arrays, and a conversion from a partial trace. Thus adding an imperative
call without a registry entry cannot silently extend the gate; it either is
unreachable from the authoritative command or fails the audit. For a revision,
the registry includes all of these classes:

1. workspace/dependency/policy audit;
2. Rust formatting;
3. Clippy for every selected workspace target and feature set;
4. oracle, schema, generated-file, and code-generation freshness checks;
5. every selected workspace test target;
6. semantic, ledger, compatibility, and historical regression gates;
7. the complete ts-tests acceptance corpus; and
8. every local-only owner-control, qualification, resource, and integrity
   control selected by the versioned local plan.

The profile manifest binds the exact `CompletePhaseRegistryV1` digest.
"Full" means complete **logical membership**, not that every action executed
fresh. A `ReuseAllowed` action may be satisfied only by exact, current,
reopened and reverified evidence. A `NonReusable` action always executes fresh
in the current invocation and cannot be bypassed by an old receipt. If one
selected `cargo xtask ci` phase has not yet been modeled, that phase remains a
mandatory freshly executed legacy tail and `local-full` cannot claim complete
functional coverage or mint its final profile capability until the tail also
passes. Omission is never represented as a cache hit.

FCI-8a first routes the existing fresh implementations through this same sealed
dispatcher. Each legacy implementation emits a typed `PhaseTraceV1`; the
functional shadow consumes the same registry/trace denominator and proves exact
membership, order, and pass/fail equality. There is never a second imperative
fallback list. FCI-9a may activate reuse only after those projections and
required process/resource evidence agree, `legacy_tail_count == 0`, and the
complete selected `NonReusable` action set is empty. Generic `NonReusable` support remains tested
with a separate synthetic adapter; it is not an excuse to leave a deterministic
tsc-rs phase outside exact reuse. If a later revision adds an unmodeled phase or
reclassifies one of those actions as `NonReusable`, the activated profile no
longer matches that revision: it mints no final capability, invalidates its
zero-process proof, and the same complete registry dispatcher selects
`FreshAll` for every phase until a new shadow and activation record closes the
gap. Hosted activation is
separate: FCI-9b keeps the one unsplit
`cargo xtask acceptance` command strictly ts-tests-only and excludes every
local-only owner-control action from its type and graph.

## 13. Exact invalidation and failure rules

For every adapter, a typed kind/spec, `ExecutionSpecV1`, direct input,
dependency edge, or owning implementation change changes that node and exactly
its two-graph reverse closure. The following are the H2 interpretation-layer
instances of that generic rule:

- a case-observation dependency's schema, typed identity, direct input, negative
  lookup, generated output, `ExecutionSpecV1`, command, feature, platform,
  toolchain, allowlisted non-secret semantic environment, oracle artifact,
  fixture, or reviewed compiler/harness file/module owner changes the consuming
  raw case keys;
- a raw canonical-payload schema, capture contract, or encoder change changes
  consuming raw case keys; FCI-1 through FCI-10 execute the resulting exact
  misses fresh;
- a verifier change invalidates derived verification records and requires all
  in-scope raw cases to be rehashed and reverified, but does not change their
  case keys;
- a projection or profile-summary change invalidates that derived projection
  or summary and the root, but does not change raw case keys;
- a shard membership or boundary change invalidates the affected
  `ShardManifestV1` interiors and root but no `CaseObservationSpecV1` or raw
  case key; and
- a valid unrelated graph-node edit changes the full graph/root identity and
  triggers root reconstruction, while only node closures in its reverse
  dependency closure change action keys.

A changed Cargo manifest, lockfile, toolchain, build script, or generated output
changes the Cargo/build nodes that own it and their reverse closure. A changed
full executable digest always changes `BuildArtifactIdV1`, but does not by
itself invalidate unrelated raw cases. An unknown production owner or
unbounded raw shared owner conservatively impacts all raw cases when a
validated all-raw mapping is available. A graph validator/integrity change
requires complete graph validation, raw-case revalidation, deterministic shard
repacking where references/specs change, reprojection, and root reconstruction,
but not compiler execution for cases whose exact current keys can be acquired.
An invalid graph or unknown input without a validated safe mapping fails before
execution.

Node/edge additions and removals are compared through both graphs as specified
in section 5.2. A candidate graph or trust-policy edit that lacks protected
transition authority cannot narrow this invalidation set.

Any changed bound raw, derived, or root byte rejects that stored object.
Storage corruption is not semantic impact: the runner reacquires the exact
current-key action or executes that action's full repetition policy while
retaining other independently verified actions, whether impacted or unchanged.

The verifier must report the first field-level reason and the expected and
observed digest. It must never repair a semantic payload, copy a stored
summary, accept a VCS ancestor commit as semantic freshness, or fall back to a
broader cache key. This does not prohibit the authenticated authority-head
ancestor/inclusion proof required for an already published candidate.

Failure classes are exact:

| Class | Examples | Canonical/cache behavior |
| --- | --- | --- |
| Deterministic semantic rejection | Output/diagnostic mismatch, wrong typed child-failure boundary, unexpected write | Complete canonical `Rejected` outcome may be exactly reused in its non-acceptance namespace; never mints an accepted root |
| Nondeterminism violation | Repetition inequality, or two authority-valid distinct canonical objects for one action key in the sealed snapshot/fresh result set | Commit the monotonic conflict-control generation and remove that key's candidates; choose neither and publish no semantic outcome/capability; the conflict guard cannot mint a root |
| Cache rejection | Wrong key binding, malformed schema, digest mismatch, invalid attestation/index, forbidden scope/path | Quarantine; try another candidate from the same exact authenticated index, then run the complete action miss; never inexact fallback |
| Infrastructure failure | Required I/O/transport, spawn, runner panic, signal, OOM, timeout, cancellation, source-snapshot mutation/guard failure | No authoritative receipt/capability or reusable action-index generation; unreachable staged/CAS bytes may remain; bounded retry only where policy says |
| Policy/configuration failure | Unknown profile, illegal plan, absent required input, unsupported platform, unapproved graph narrowing with no representable safe superset | Fail before execution; no cache publication |

### 13.1 Operator tooling and explanations

`ci-runner` exposes generic operations through the `xtask` host composition;
the registered repository adapter supplies semantics but does not own a second
CLI or evaluation loop. Every command prints deterministic text by default and
supports canonical JSON for tests and automation:

```text
cargo xtask functional-ci graph --profile <id> [--format text|json|dot]
cargo xtask functional-ci snapshot --profile <id> --base <sha> --head <sha> --out <path>
cargo xtask functional-ci affected --profile <id> --evidence-snapshot <path-or-digest>
cargo xtask functional-ci why-miss --profile <id> --action <node-id> --evidence-snapshot <path-or-digest> --candidate <source-ref>
cargo xtask functional-ci verify-plan --profile <id>
cargo xtask functional-ci verify-outcome --profile <id> --outcome <digest>
cargo xtask functional-ci verify-cache --profile <id> --locator <exact-locator>
cargo xtask functional-ci gc --dry-run [--max-bytes <n>]
cargo xtask functional-ci gc --apply-plan <plan-digest>
```

- `graph` renders typed nodes, direct inputs, dependency edges, closure digests,
  semantic owners, and shared/conservative-all-raw markers.
- `snapshot` performs no semantic action. It validates both graphs, inventory,
  protected transition/trust authority, source snapshot/guard, conflict
  registries, and exact bounded candidate indices, freezes the profile-selected
  provider and exact generation for every
  locator, then writes canonical `EvidenceAvailabilitySnapshotV1` bytes and its
  digest.
- `affected` performs no semantic action. It prints exact changed, impacted,
  prior-root carry-forward, exact-cache reuse, execute,
  revalidate, repack, and rebuild sets, the adapter schedule, and at least one
  lexicographically least shortest reason path per impacted action and every
  unknown input for its explicit immutable evidence snapshot. It also prints
  the protected-base/current graph, transition, trust-root, namespace-epoch-map,
  and snapshot digests. It does not refetch ambient cache state and never
  derives case `execute` from `impacted` or shard membership alone.
- `why-miss` compares the current expected semantic key with an available prior
  candidate explicitly named inside that snapshot, field by field in canonical
  field order, and prints the first difference plus the full dependency reason
  chain without consulting ambient candidates. It distinguishes
  build-identity-only differences, derived verifier/projection or shard-manifest
  invalidation, absence of exact-current-case-key evidence, and raw case semantic invalidation.
- `verify-plan` performs no semantic action. It validates the selected
  adapter-owned plan schema, stable ordering, exact denominator, membership and
  union digests, graph/profile binding, and every optional bundle partition. For
  H2 it additionally rejects empty shards, gaps, overlaps, duplicate/reordered
  cases, unknown ids, and a union different from the qualified case list. An
  adapter with no bundle plan verifies its explicit flat-leaf plan without an
  H2 branch in `ci-core`.
- `verify-outcome` rehashes and recomputes one complete `Passed` or `Rejected`
  outcome and reports its typed status; only the in-process `Passed` path may
  continue to a policy capability.
- `verify-cache` retrieves the exact authenticated candidate index for the
  locator and its bounded declared action/outcome candidates into the isolated
  incoming area. It runs authority, path, canonical,
  per-object digest, graph, conflict, and payload verification without executing
  compiler bytes. Because its process then exits, its report is diagnostic and
  cannot be used later as a `HostedVerifiedRoot` capability.
- `gc` is the leased local mark-and-sweep from section 9. Dry-run is the default
  and writes the exact `GcPlanV1` path/digest/byte set. Mutation requires an
  explicit `--apply-plan` digest, the store-wide exclusive barrier, and an
  unchanged store/lease/index-generation snapshot; it preserves quarantine only
  within its bounded retention policy, refuses symlinks, and never deletes
  remote cache data.

Explanations never print secret material or unredacted environment values.
They print typed field names and digests. Given the same graphs, candidate
index, and evidence-snapshot bytes, their text and JSON are byte-identical.

Path-filter output may be shown as a discovery hint, but none of these commands
may label a node unaffected without the typed graph and current direct-input
checks.

## 14. Migration stages and packets

`FCI-0` through `FCI-10` are dependency stages, not implementation packets and
not permission to implement a whole table row. The authorization unit is one
versioned packet linked from the slice-packet index with machine-checked state
`ready`. No production file may change merely because its stage appears below.
Read-only design, upstream/provider research, and inventory may run early only
inside a packet whose allowed paths and commands say so. The packet-control
bootstrap is the sole pre-closure exception: it can mark only the explicitly
listed FCI-1a through FCI-5c.1 shadow packets ready, and it cannot authorize an
H2.5g authority or any FCI-6+ effect/publication surface.

Every packet freezes, without `TBD` or an implementer-selected alternative:

- trusted base, prerequisites, exact allowed and forbidden files, and final
  symbols/signatures/visibility;
- every new or changed schema field, canonical ordering/default, error variant,
  corruption/failure behavior, and upgrade rule;
- exact golden fixtures, adjacent-negative and fault/crash controls, expected
  bytes/digests/sets/counters, and resource ceilings;
- the ordered implementation steps and integration call sites; and
- proof commands with expected results and the immutable evidence paths they
  produce.

If those facts do not fit one bounded change, the design owner adds another
lettered or numeric subpacket before implementation; "or smaller", "as
needed", and "follow the design" do not delegate an architectural decision to a
lower-capability implementation agent. No packet may be combined with production
emitter behavior. Commands below are future FCI requirements, not current
H2.5g commands. Proof uses the repository's role-based
`cargo xtask test <role>` route; direct Cargo package selectors are not an
authoritative automation route.

| Stage / proposed packet boundary | Frozen implementation boundary | Required proof before its successor |
| --- | --- | --- |
| FCI-0a framework boundary record | Documentation only: freeze the framework charter, v1 non-goals, qualification ladder, package/dependency/trust map, tsc-rs reference-adapter role, and unchanged hard gate in this canonical document. | Diff review proves no production/workflow/profile/evidence file changed and every current H2.5g command, count, hosted scope, and status is unchanged. This row is never a runtime-ready packet. |
| FCI-0b extension API-manifest record | Documentation only: freeze the public/sealed ownership table, final conceptual API, blocking/threading/cancellation/panic contract, error ownership, registry seal, `PreparedExecutionV1`, and sole runner-entry ownership. It declares no Rust item or placeholder. | Cross-section review maps every conceptual symbol to exactly one later owning packet with no unresolved owner or implementation-selected alternative. This row is never a runtime-ready packet. |
| FCI-1a core identifiers and dependency boundary | After packet-control bootstrap and its own ready packet, add private `crates/ci-core`, generic protocol/application/schema identifiers, digest/input skeletons, tests tree, and the negative-dependency/domain-literal audit. This is pre-closure shadow infrastructure only. Do not add canonical encode/decode behavior, graph types, adapter registration, outcome, aggregate, or effect behavior. | `cargo xtask test ci-core`; identifier ordering/type separation and negative dependency/literal tests pass with no production dependency or repository noun. |
| FCI-1b inert adapter descriptors | Add `AdapterDescriptorV1`, typed adapter ids/schema references, and checked inert descriptor records only. Do not declare `ActionModel`, `AdapterCodec`, `AdapterRegistration`, a monomorphized function, registry builder, decode, prepare, dispatch, or executable callback. | Duplicate/opaque descriptor/id shapes fail their checked record constructors; no trait bound references a future canonical decoder/model type and no candidate/runtime registration API exists. |
| FCI-1c graph/profile/typestate record seam | Add `NodeClass`, generic node/action/root record shapes, composite references, and pending/complete membership record declarations without adapter traits or constructors that can claim completeness. Do not add graph evaluation, codec, verification, aggregation, outcome, or effects. | Pending records cannot satisfy complete record APIs; case-shaped and flat fake data compile through the same generic records without downcast/id branch; every future typed constructor remains unavailable. |
| FCI-2a blocking runner and error boundary | Add private `crates/ci-runner`, the closed `InfraError`/effect-phase taxonomy, explicit `RunCancellation` vocabulary, blocking/no-async public-SPI contract, panic ownership, dependency tests, and no other effect interface. Do not add `RunContext`, worker, snapshot, invocation, sandbox, resource, staging, publication, CAS/cache, or H2 placeholders. | `cargo xtask test ci-runner`; error-family conversion and panic/cancellation tests prove infrastructure failure cannot become model data, rejection, success, or miss, and crate dependency direction is enforced. |
| FCI-2b bounded effect-result seam | Add bounded chunk-source/effect-result interfaces, invocation-private staging vocabulary, and test fakes only. Do not add a scheduler, live runner entry, source snapshot, action invocation, sandbox guard, publication, resource policy, CAS/cache, or H2 type owned by later packets. | Bounded read/truncate/over-limit/staging-abandon tests pass; no worker/publication/live-evaluation API exists and no undefined future type is forward-declared. |
| FCI-3a canonical bytes and hashes | Complete bounded streaming `CanonicalSink`, strict decode/re-encode, the exhaustive wire-object `ProtocolDomainV1` registry, purpose-specific digest newtypes including `PublicationEventDigest`, application namespace/fork/rename rules, exact framing, and v1 Node fixtures. This reserves framing, not a future event schema. | Golden byte/digest tests cover every escape/order/integer/framing/domain boundary, cross-namespace non-aliasing, digest-type substitution compile failures, bounded incremental hashing, unknown/duplicate fields, and exact Node parity. |
| FCI-3b execution, tool, build, reuse, disclosure, and sandbox identity | Add execution/invocation and sandbox-identity value schemas, generic tool/build/platform identities, separate current `ReuseScopeV1` and monotonic `DisclosureHistoryV1` schemas, sandbox capabilities/guard/observation values, secret exclusion, and adapter mapping. Do not declare the `Sandbox` trait before its mounted-source argument exists. | One-field invalidation and platform/toolchain/secret/audience tests pass; candidate widening/history shrink and retroactive-secrecy claims fail; every nondeterministic channel is classified; guard ownership/control-vs-harness tests pass; a non-Rust adapter compiles. |
| FCI-3c source, path, sandbox, and resource primitives | Add immutable `SourceSnapshotV1`/guard plus `SourceSnapshotProvider`, `MountedSourceSnapshot` and the `Sandbox` trait, bounded regular-file/no-follow path reads and immutable no-replace staging, bounded scheduler/queues, and full child/control-plane `ResourcePolicyV1` as one invariant set. Do not declare CAS, cache, authority-publication, candidate, or remote-publication traits. | Mixed/live-state, symlink/traversal/special-file, mutation-after-seal, concurrent no-clobber, byte/allocation, child quota, control-plane CPU/RSS/concurrency, and fail-closed platform-capability tests pass. |
| FCI-4a.1 graph schema and canonical rendering | Add generic `ActionGraph<I,K,S>`, stored typed `NodeSpec`, composite-profile schema, and pure canonical graph rendering for both fake data shapes. Do not add structural closure validation, runtime registry dispatch, prepared execution, or complete membership construction. | Missing/duplicate record fields and schema/digest errors fail; both fake graphs render byte-identically without repository nouns or a codec callback. |
| FCI-4a.2 graph/model structural validation | Add closure recomputation, cycles/edge/spec/closure/global-id validation, typed root/action/execution/derived proposal records, and stable evaluation-plan derivation over already decoded fake values. Do not add adapter traits, runtime registry dispatch, prepared execution construction, or complete membership construction. | Cycles, invalid edges, stale closures, missing specs, global-id collisions, invalid topology, and invalid repetition/resource/scope proposal values fail without a downcast/`Any`/id branch. |
| FCI-4a.3 sealed adapter preparation, membership, and testkit | Now that strict decode and every pure signature type exist, add the final `ActionModel`/`AdapterCodec` bounds, `AdapterRegistration::of`, `AdapterRegistryBuilder`, exact expected descriptor set, consuming `seal -> VerifiedAdapterRegistry`, private monomorphized decode/re-encode/dispatch, core-only `PreparedExecutionV1`, pure `LeafVerdict`/`DerivedVerdict`/`AdapterVerdict`, required-membership and pending-to-complete adapter/composite inputs, and dev-only `crates/ci-testkit`. Do not add an outcome manifest, verified outcome view, projection trait, CAS, or live runner behavior. | Unknown/duplicate/late/candidate registrations, wrong schema/implementation/registry digest, public prepared/complete construction, missing/duplicate/unexpected/wrong-key membership, and incomplete dependencies fail; both fake adapters pass reusable conformance without future-type placeholders; the generic seam reaches the first qualification-ladder proof. |
| FCI-4b snapshot inventory, negative, and generated ownership | Add `WorkspaceInventorySpecV1`, global dispositions, negative lookups, generated/build-system ownership and unknown policy over one immutable source snapshot; Git tree plus dirty overlay is only the H2 provider implementation. | Exact tracked/untracked/delete/symlink/case/Unicode/submodule/negative/generated fixtures pass from one snapshot; a concurrent edit cannot form a mixed root; unsafe unknown or opaque producer fails closed. |
| FCI-4c paired impact and protected transition | Add prior/current graph comparison, exact reverse closures, `TrustRootV1`/`GraphTransitionV1`, genesis and protected narrowing rules, and `impact-cases.v1.json` with synthetic availability only. | Exact node/edge add/remove/rename, no-impact, shared-owner, boundary/verifier-only, approved/unapproved narrowing, and candidate self-approval fixtures pass with no under- or over-impact. |
| FCI-4d pure explanations and planning budgets | Add deterministic affected/reason-path/why-miss pure values over explicit synthetic evidence; freeze versioned cold/warm graph, inventory, hashing, decode, explanation CPU/RSS/byte/concurrency baselines and ceilings. No live cache or snapshot command. | Text/JSON replay is byte-identical, tie-breaking is exact, warm no-impact planning stays within the frozen control-plane envelope, and no semantic subprocess is available to the pure test harness. |
| FCI-5a tsc-rs protocol/control packages and fixed plan | Add `ci-adapter-tsc-rs-protocol` and `ci-adapter-tsc-rs-control`; move/refactor H2 invocation/observation/root schemas, protected graph/plan/verification inputs, and checked-in plan from the indexed legacy forwarding owner. Control links no production/compiler crate. Do not register an incomplete H2 `ActionModel`/`AdapterCodec`, add a candidate harness executable, generic outcomes, or CAS; FCI-5c owns the first complete H2 registration. | Protocol golden fixtures and `verify-plan` pass over typed plan values; old/new totals and union membership agree; dependency audit proves protocol/control have no production link, no placeholder adapter callback exists, and `xtask` is only composition/legacy forwarding. |
| FCI-5b tsc-rs miss-only candidate harness | Add `ci-harness-tsc-rs` as the sole candidate-side executable for `ActionInvocationV1`; connect it to the protocol and production/compiler crates across the process boundary. It owns no verifier, graph, cache, registry, root, or authority API. | Control-to-production negative dependency and process-boundary tests pass; a warm synthetic lookup cannot build/spawn the harness; malformed invocation/output, nonzero/signal/timeout, sandbox escape, and over-limit cases fail in their frozen error family. |
| FCI-5c.1 H2.5g inventory complete-profile shadow | Before H2.5g closes, bind the reviewed 9,027-case H2.5g inventory plan as a deliberately narrow complete profile through the FCI-5a/5b seams. Keep the legacy qualification/profile/acceptance commands authoritative; this packet may emit only shadow observations and mismatch evidence. It owns no CAS, cache, outcome manifest, projection, capability, or live scheduler. | Exact denominator `9027`, dispositions `8511 admitted / 6 h2_8a_deferred / 510 h2_9_deferred`, strict membership, two isolated observations per admitted case, deterministic bytes, and old-summary comparison all pass. A mismatch blocks the successor but cannot mint H2.5g authority. |
| FCI-5c.2 complete H2 registration and verifier/aggregate shadow | After the final H2.5g validation reference, close/merge lineage, and packet rebind, complete the remaining H2 `ActionModel`/`AdapterCodec`, strict observation decoder/verifier, pure adapter derived/aggregate verdicts over the already complete typed input, fixed two-isolated-repetition policy, and the observation shadow connection through `VerifiedAdapterRegistry`/`PreparedExecutionV1`. User-facing summary comparison remains an adapter-local test oracle; do not add `VerifiedOutcomeView`, the framework `Projection` trait, an outcome manifest, or CAS. | Every fresh H2 case has exactly two isolated repetitions; old/new adapter verdicts, summary oracle, and dispositions match; union/partition/adjacent-negative controls pass; no H2 semantic implementation remains authored in `xtask` and no future outcome/projection type is referenced. |
| FCI-6a immutable local CAS and local issuer | Add immutable objects, the `CasBackend` bounded/no-replace object interface, the initially empty local authority-generation envelope with mandatory conflict/disclosure heads, strict `LocalProducerReceiptV1`, issuer/anti-rollback state, `index-head/current.json`, exclusive lock, exact atomic durability protocol, bounded verification, and recovery. Do not add candidate or authority-publication interfaces. | Every publication/crash boundary yields exactly old/new state; restart receipt/prior-generation/disclosure validation, history-shrink, rollback/replay, permissions, loose files, no-replace, tamper/truncate/swap, and unsupported authority capability fail closed. |
| FCI-6b candidate generations and durable conflicts | Add the local/remote-neutral `PublicationEventV1` schema, `CandidateRef`/`VerifiedCandidate`, bounded candidate sets, sealed generations, `ExactCacheBackend`, same-key uniqueness, monotonic conflict witnesses/tombstones/registry, `LocalConflictPublisher` and its conflict delta/guard, unchanged-head retry, read-only exact-restore and `RolloverPlanV1` validation, and `verify-cache`; remote remains absent. The plan requires unchanged semantic action keys plus exact predecessor conflict/disclosure roots and cannot apply a rollover. | Cached/fresh/repetition conflicts, acyclic prior-generation/receipt/event/replacement-generation construction, later candidate counts, commit races, registry corruption/capacity/deletion/readdition, exact-restore planning, refusal to plan from an unverifiable conflict set, membership-preserving capacity compaction, duplicate-same-object receipts, and no-winner behavior pass. No lease, barrier, capability, or rollover-apply API exists yet. |
| FCI-6c complete typed outcomes, projections, and Merkle assembly | Consume the FCI-4a.3 complete inputs/verdicts; add core sealing of adapter payloads, complete composite outcome collection, lease-bound `VerifiedOutcomeView`, typed `Projection` registration, the tsc-rs user-facing projections, passed/rejected manifests, interiors, reconstruction, `LocalOutcomePublisher` plus its outcome delta/guard, and `verify-outcome`. | Compile-fail/runtime tests reject unverified outcome/projection inputs and pending/missing/duplicate/unexpected/wrong-key observations/adapters; every derived/aggregate slot evaluates once in stable order, rejection never omits siblings, raw/tree/mixed/rejected/projection proofs pass, and only an unchanged generation returns the outcome guard. |
| FCI-6d authority capabilities | Add private `VerifiedPolicySpec<P>`, effect-bound verified consumer-engine seam, non-generic `PolicyProof`, `AuthorizedRoot<P>`, `FreshOnly`, `ReuseAllowed`, a private typed execution-evidence witness including `FreshExecutionGuard`, and local outcome-guard consumption. | Markers/recorded identities grant no authority; private constructors cannot be bypassed; relevant types are noncloneable/nonserializable; conflict/remote guards cannot mint a root; fresh authority requires the pre-execution guard plus complete fresh trace; engine/source/scope/registry bindings are mandatory. |
| FCI-6e leases, rollover apply, and GC | Add durable leases/liveness locks, the store-wide shared/exclusive epoch and GC barrier, guarded application of a still-exact `RolloverPlanV1`, permanent predecessor conflict/disclosure commitments, `GcPlanV1`, permanent live conflict-control pinning, bounded ordinary quarantine/retention, dry-run and exact-plan apply; GC is never repair. | Reader/publisher/GC/rollover races, crash/PID reuse, refusal of a changed or unverifiable rollover plan, old-lease/invocation drain, old-epoch lookup/publication/mint rejection, unchanged semantic keys and conflict membership, preservation of an already-returned in-process capability and old disclosure union, conflict tombstone deletion/forgetful-compaction attempts, changed GC plan/symlink refusal, mark/sweep ordering, and durability tests pass. |
| FCI-7a sealed live planning | Add `EvidenceAvailabilitySnapshotV1` and live commands over exact source snapshots/guards, local head plus conflict registry, protected transition/trust/scope state, and bounded candidate inventory. | Every command binds one immutable source/evidence snapshot, replay is exact, ambient edits/late candidates cannot change sets, conflicted keys never execute/reuse, and planning meets resource ceilings. |
| FCI-7b demand-driven local evaluation | Carry forward exact-current-key actions, build/spawn action harnesses only for misses, then revalidate/repack/rebuild and commit through the single coordinator beside existing authoritative commands. | Fresh/mixed roots agree; boundary/verifier-only and warm no-impact cases spawn and build zero semantic/compiler/oracle/test/harness processes while reopening all required evidence and rebuilding the root. |
| FCI-7c.1 second adapter and composite shadow | Add separate shard/repetition/compiler-free `ci-adapter-workspace-audit` and compose it with H2 through the same sealed registry, typestate collectors, core, and runner; use `ci-testkit` only from its conformance tests. Existing local commands remain authoritative. | No generic branch/downcast appears; both real adapter shapes produce complete suboutcomes through one API; adapter dependency and contract suites pass without changing an authoritative command or adding testkit to a runtime dependency closure. |
| FCI-7c.2 framework qualification and API freeze | Freeze the exact repository/core/local-runner portion of the workspace-public API manifest after both real adapters, run the reusable conformance suite, remove any duplicated generic mechanism from adapter crates, and record package/dependency/domain/branch audits. The FCI-8e provider-publication SPI remains separately owned and cannot reopen the adapter surface. A required shared API change first amends FCI-0b and reruns both adapters. | Both adapters and every then-existing fake local runner backend pass one frozen API and replay byte-identically; no generic crate contains repository/provider nouns or dependencies; the qualification ladder may first record `workspace framework qualified`. |
| FCI-8a `local-full` complete dispatcher and shadow | Add and freeze the host-API manifest partition containing `CompletePhaseRegistryV1` as the sole denominator/dispatcher for fresh and reusable `cargo xtask ci`, move every existing implementation behind its typed entry/trace, forbid alternate authoritative spawn/call/list paths by dependency/call-graph/source audit, and shadow the complete profile without reopening the FCI-7c.2 adapter/local surface. | Removing/reordering/unclassifying a phase, adding a direct call/spawn, using a partial trace, diverging fresh/reuse denominators, or changing an earlier API partition fails; `FreshAll` fallback uses the same registry; exact legacy-tail/`NonReusable` sets and complete projections agree, and activation readiness requires both sets empty plus resource proofs. |
| FCI-8b protected-host/bootstrap capability research | Read-only. Freeze required-workflow ownership, base/head/tested-tree identity, protected control directory and exact external `cargo xtask acceptance` resolution, signed engine channel/release/build attestation, candidate-as-data mounts, fork credential/log/process isolation, complete fresh fallback, and N+1 promotion. | Captured provider/workflow probes prove a PR cannot replace workflow/bootstrap/cargo/xtask/verifier/exit code, the visible semantic `run:` remains exactly the one command, and every required identity/attestation/isolation primitive is available or activation fails closed; no workflow/bootstrap implementation. |
| FCI-8c hosted-provider capability research | Read-only. Freeze provider/API and one private atomic publisher realization; separately mandatory authenticated monotonic conflict/disclosure/event-history heads and ancestor/inclusion proofs; optional candidate roots; the receipt-to-event-to-head DAG; namespace/audience auth; quotas/retention; exact restore or epoch-barrier/drain rollover with unchanged semantic action keys and exact predecessor conflict/disclosure commitments; and failure classes. | Probes prove optional candidate outage can miss while authority-head outage fails, old-candidate inclusion against a newer current head, current conflict/disclosure proofs, atomic publication/rollback rejection, acyclic content references, monotonic conflict/audience unions with no tombstone bypass or retroactive secrecy, refusal to rollover an unverifiable conflict set, writer separation, repair/rollover drain and old-epoch rejection, or reject the provider; no backend code. |
| FCI-8d protected consumer bootstrap and promotion | Only after FCI-8b, implement protected envelope, candidate snapshot acquisition, exact attested engine selection, control/action-harness split, complete fresh fallback, effect-verified consumer identity, and N+1 promotion shadow without cache writes or hosted activation. | Alias/PATH/fake xtask/verifier/exit-code, wrong issuer/audience/workflow/base/tree/channel/binary, replay, self-promotion, candidate process escape, and fallback-membership fixtures pass; candidate authority/write count is zero. |
| FCI-8e trust, receipts, hosted backend, and provider API partition | Only after FCI-8c/8d, add and freeze the provider-publication API partition containing protected trust/transition verification, engine/scope/disclosure-bound receipts, the provider-neutral `AtomicSnapshotPublisher` and remote guards; apply the FCI-6b `PublicationEventV1` schema to remote authority, optional candidates, mandatory monotonic conflict/disclosure/event-history heads, isolated extraction, read/write separation, and the frozen restore/rollover mechanism. It cannot change the FCI-7c.2 or FCI-8a partitions. Transfer packs remain outside v1 and expose no placeholder. | Optional-candidate versus mandatory-head outage, candidate-event ancestor/inclusion against a newer head, current conflict/disclosure proof, acyclic receipt/event/head generation, crash/conflict/disclosure rollback, history shrink/retroactive secrecy, forgery, wrong engine/scope/audience, quota, restore/rollover barrier and drain with exact predecessor conflict/disclosure roots, unchanged semantic action keys, already-returned capability stability, poisoned/squatted, credential revocation, untrusted-PR, and prior-partition API checks pass. |
| FCI-8f complete hosted shadow and final extension-manifest freeze, disabled | Add exact protected adapters for every current conformance/H1/historical-H2 `cargo xtask acceptance` subcall, compose the complete hosted root, run authenticated exact-hit/miss/mixed shadow without enabling workflow consumption, and record the exact FCI-7c.2/FCI-8a/FCI-8e union as the final workspace-public API manifest. | Shadow membership equals the complete current ts-tests action union; fresh/mixed/rejected projections agree; all earlier adapter/local/host/provider conformance suites pass without signature drift; corrupt/conflict/bootstrap attacks fail; owner roots and `NonReusable` actions are unrepresentable at hosted call sites. |
| FCI-9a local-full activation | After separate approval, and only with zero legacy-tail and zero selected `NonReusable` actions, make the complete FCI-8a `local-full` projection authoritative inside `cargo xtask ci`. A later unmodeled or newly nonreusable phase invalidates this activation for that revision and restores the complete fresh fallback until re-shadowed. | Fresh clone, warm no-impact process-count-zero proof, representative one-owner/shared-owner changes, forced misses, corrupt local evidence recovery, complete local gate, and recorded graph/root/resource digests pass. |
| FCI-9b hosted activation | After FCI-9a, FCI-8f, and separate approval, enable exact authenticated reuse only inside the unchanged unsplit protected-engine `cargo xtask acceptance` ts-tests boundary through `HostedVerifiedRoot`; an H2-only subset cannot activate. | Fresh clone, no-impact, one-case hit/miss, boundary/shared-core/verifier/engine changes, forced miss, corrupt/poisoned recovery, protected trust/transition/bootstrap, and complete hosted acceptance pass with every current subcall represented and no owner control. |
| FCI-10 cleanup | Remove duplicate qualifying traversals and demote arbitrary ranges to diagnostics; retain v1 ratchets/readers as immutable history. | No current profile points to a retired route; historical fixtures remain byte-identical; local-full and hosted commands retain their distinct complete memberships. |

Every packet first updates the relevant input/profile pins, then implements its
bounded change, then records tests and immutable evidence. A failed proof stops
the sequence; the next packet does not weaken the invariant or regenerate
expected values from the Rust result. FCI-8b and FCI-8c are deliberately
read-only. Bootstrap implementation begins only at FCI-8d and provider implementation
only at FCI-8e, after their independent reviewed freezes have made every
security/provider choice explicit.
FCI-8a through FCI-8f remain blocked until FCI-7c.2 closes; splitting the
earlier stages does not insert, remove, or reorder a hard-gate stage.

### 14.1 Framework extraction criteria

The three generic packages (`ci-core`, `ci-runner`, and dev-only `ci-testkit`)
remain small repository-internal libraries throughout this migration.
Repository semantics move from legacy `xtask` ownership into the dedicated
protocol/control/harness adapter packages, never into a generic crate.
Framework mechanism enters `ci-core` or `ci-runner`, and shared conformance
mechanism enters `ci-testkit`, only when all of these are true:

1. the API and implementation contain no tsc/H2 case, compiler, oracle,
   `EmitOutcome`, owner-control, slice, or workspace-role-specific type or
   branch;
2. neither runtime framework crate gains a dependency on production, oracle,
   harness, conformance, or xtask crates;
3. behavior is covered by the generic adversarial suite using in-memory fake
   graphs, executors, CAS, caches, sandbox, clock, and authority verifier;
4. errors remain separated into pure model/verification errors and runner
   infrastructure errors;
5. the H2 adapter needs no downcast, stringly typed escape, or core special
   case; and
6. the `workspace-audit` adapter, which has no corpus shards or compiler
   observations, uses the same graph, action key, outcome, runner, explanation,
   verified-root, and composite-profile abstractions without adding an H2
   branch.

The second adapter is the proof of reuse; H2 alone is insufficient evidence
that an abstraction is generic. Only the bounded mechanics explicitly
authorized by FCI-1 through FCI-7 enter the generic crates before that proof;
unproven H2 behavior remains in the tsc-rs protocol/control/harness adapter
packages (or its explicitly temporary indexed `xtask` forwarding path), and
the workspace-audit adapter remains in its own package. FCI-7c.1 proves the
second real shape; FCI-7c.2 freezes and records the repository/core/local
runner API partition, while FCI-8a/FCI-8e/FCI-8f append and freeze the later
host/provider partitions without reopening it. Passing the criteria does not
authorize crates.io publication, generic branding, a public stability promise,
or use outside this workspace.

Promotion from an adapter into a framework crate follows this exact sequence:

1. implement the first use adapter-locally; do not add a generic option, trait,
   enum variant, or hook for a hypothetical consumer;
2. name two current consumers with the same invariant, including
   workspace-audit for repository semantics, and record the duplicated
   adapter-local inputs, outputs, errors, and effect boundary;
3. freeze one candidate generic signature, owning crate, visibility, allowed
   dependencies, error variants, canonical-byte impact, and forbidden domain
   vocabulary in a ready packet;
4. add adapter-independent conformance fixtures for both consumers before
   moving behavior;
5. move only the shared mechanism, wire both adapters without a downcast,
   kind/id match, opaque string dispatch, defaulted semantic field, or optional
   callback that one adapter alone understands;
6. run the generic adversarial suite, both adapter contract suites, the
   negative-dependency audit, and the domain-literal/branch audit; then remove
   the duplicated adapter-local mechanism; and
7. if either consumer needs a different invariant or the frozen API grows an
   adapter-specific escape, stop promotion and keep the behavior local until a
   new design packet proves a smaller common mechanism.

Two call sites in H2, anticipated future reuse, or similar names are not a
second consumer. A provider-specific primitive remains in its provider adapter
unless `ci-runner` needs only the already frozen generic capability semantics.
This sequence is the implementation checklist; an implementation packet must
not replace any step with “generalize as needed.”

## 15. Mandatory adversarial tests

Before either FCI-9 activation, the repository must contain automated tests for
every row below:

1. generic core fixtures implement both an H2-shaped adapter and a shard-free,
   repetition-free workspace-audit adapter without a core branch, downcast,
   opaque kind string, or H2/Cargo/case/shard type in a generic crate, and
   compose their verified suboutcomes through `CompositeProfileV1`;
2. changing each `ExecutionSpecV1` or raw semantic input field one at a time
   changes exactly the owning closure and action keys in its reverse closure;
   an unrelated graph-node edit changes the full graph/root key but not those
   raw keys;
3. generic `ToolchainSetV1`/`BuildArtifactIdV1` fixtures contain only ordered
   `ToolRefV1`/`BuildComponentV1` values and compile for an adapter with no
   Cargo/Rust/Node concepts; the H2 adapter maps its exact Rust/Cargo/Node/tsc
   tool and build inputs into those values; an unrelated xtask/module change
   changes the H2 build id but not an H2 semantic key, while a semantic
   implementation, classifier, generated/build-script output, compile flag,
   runtime, or toolchain change affects exactly its declared consumers;
4. paired-graph fixtures cover node and edge addition, deletion, replacement,
   and rename; their exact `changed_prior`, `changed_current`, prior/current
   reverse closures, current impacted set, and deleted-node explanations match;
5. every `impact-cases.v1.json` row names exact prior/cache evidence and sealed
   snapshot bytes and equals the complete changed, impacted, carry-forward,
   cache-reuse, execute, schedule, revalidate, repack, and
   rebuild sets for narrative docs, unrelated tests, one fixture/case, one
   slice, shared core, boundary-only, verifier-only, negative, and unknown
   changes;
6. `WorkspaceInventorySpecV1` discovers tracked/untracked additions, deletions,
   ignored/generated roots, symlinks, case/Unicode collisions, submodules, and
   negative lookups; an unknown production owner maps conservatively to all raw
   cases or fails before acquisition, and candidate content cannot classify a
   selected-target input as owned only by another profile;
7. a candidate checkout cannot approve its own graph-edge/owner removal,
   ignore expansion, trust-policy widening, issuer, or cache-write authority;
   exact protected-base transition approval succeeds;
8. adding a file that satisfies a recorded absence changes the negative-lookup
   node and exact reverse closure;
9. an opaque build script forbids hosted reuse for every consuming target, and
   a fully modeled build script invalidates only its reverse closure;
10. Cargo package, target, feature, dev-dependency, build-script, and integration
    test changes select the exact expected target nodes;
11. Rust and Node v1 fixtures have identical canonical bytes/digests for raw
    UTF-8 key ordering, every short escape, every remaining U+0000-U+001F
    escape, slash, non-ASCII, U+2028/U+2029, and valid surrogate-pair input;
    they reject unpaired surrogates, duplicate decoded keys, alternate escapes,
    uppercase hex, and all other noncanonical encodings; every wire object has
    one registered domain and purpose-specific digest type, bounded streaming
    fails before its ceiling, and old-schema/current-key substitution executes
    fresh because v1 exposes no payload-migration type, registry, or hook;
12. every fresh H2 case has exactly two isolated repetitions, while the second
    generic adapter follows its different policy; map insertion, directory
    enumeration, worker count/completion, job assignment, shard partition,
    neighbors, and acquisition order do not change semantic bytes;
13. changing an unrelated recorded-compiler-plan row leaves an H2 case key
    unchanged, while changing its exact selected row changes that key; missing,
    duplicate, ambiguous, or mismatched rows reject planning;
14. `verify-plan` rejects plan gaps, overlaps, duplicates, reordering, unknown
    ids, and a wrong union before execution; a boundary-only change preserves
    every case key/raw digest while rebuilding the exact new interiors and root;
15. missing, truncated, extended, swapped, or digest-path-mismatched raw,
    verified, or interior objects fail without invalidating another
    independently verified leaf; forged summaries fail raw recomputation;
16. a cached root with a stale graph, wrong expected action set/partition, or
    otherwise valid self-consistent bytes cannot mint acceptance authority;
17. an impacted action with authenticated exact-current evidence is reused, an
    unchanged miss executes its full policy, one changed H2 case repacks only
    its shard, and the mixed semantic root equals a fully fresh root;
18. cache entries arriving after the sealed availability snapshot do not alter
    its execute set or the current local capability; a local index-head
    generation change before the local commit withholds every authoritative
    local receipt/index/capability and requires a validated replacement
    evidence snapshot or infrastructure failure; a late remote generation is
    ignored by current semantic evaluation and detected by a future evidence
    snapshot or the distinct remote-publication snapshot; optional cache
    candidate/index transport failure freezes an explicit miss, while
    missing/corrupt/rolled-back mandatory remote authority head is infrastructure
    failure and cannot fresh-fallback; an older valid candidate remains eligible
    only when its publication event/generation has an authenticated
    inclusion/ancestor proof from the current head and the current conflict/disclosure
    proofs admit its key and requested audience; post-outcome upload failure
    leaves semantic bytes intact, and worker order cannot alter publication;
19. two independently authority-valid distinct objects for one action key in
    the sealed snapshot/fresh result set, including fresh-versus-cached and
    cross-producer and repetition conflicts, choose neither, publish no semantic
    receipt/index winner/capability, atomically remove the key's candidates and
    commit its monotonic conflict tombstone before reporting
    `NondeterminismDetected`; zero/one/two candidates in every later invocation
    remain ineligible, duplicate receipts for one digest are not conflicts, and
    the durable update does not retroactively change an earlier capability;
20. a deterministic candidate-versus-oracle mismatch produces a complete,
    stable `Rejected` outcome; semantic rejection does not fail-fast/cancel
    siblings, and exact reuse returns the same failure tree without acceptance;
    repetition inequality instead produces no outcome and reports
    `NondeterminismDetected`;
21. infrastructure failure or external cancellation creates no authoritative
    receipt/capability or reusable action-index generation; a crash may leave
    unreachable immutable objects or outcome bytes but no discoverable partial
    authoritative result;
22. concurrent identical CAS publication succeeds only with identical bytes;
    no-replace publication never overwrites a destination, and only an
    unchanged-generation commit guard can make staged objects discoverable;
    two authority-valid receipts for one object occupy distinct
    candidate-manifest paths and both remain reachable without overwrite;
23. bounded streaming rejects oversized input before allocation, and symlink,
    traversal, absolute, special-file, duplicate/unexpected incoming object,
    per-object digest, object-count, and byte-ceiling attacks cannot access
    paths outside incoming/CAS roots;
24. worker/memory/time limits yield infrastructure failure without reducing
    action membership or publishing a semantic result, and candidate policy
    cannot exceed protected hosted resource ceilings; changing only resource,
    retry, worker, or cache policy changes no semantic action key/outcome, and
    a thread-spawning child remains inside the process-group quota;
25. `FreshOnly` cannot be deserialized, cloned, restored, migrated into, or
    constructed by a later process; `ReuseAllowed` and a generic reusable root
    cannot convert to `FreshOnly` or `HostedVerifiedRoot`; no commit guard is
    cloneable/serializable, and neither a conflict guard nor
    `IndexCommitGuard<RemotePublicationCommit>` can construct any
    `AuthorizedRoot<P>`;
26. platform/toolchain/ABI/filesystem differences change platform-bound keys;
    only a tested platform-independent action crosses classes, and secrets are
    rejected from reusable action env, keys, CAS, receipts, and explanations;
27. hosted prefix/fallback keys, candidate-controlled trust, invalid producer
    authority, wrong graph/profile/platform/toolchain/closure/payload, or
    inconsistent build receipt are rejected, while an unrelated producer graph
    or whole-build-id difference alone does not reject an exact raw action;
28. a corrupt or squatted candidate/index uses no inexact fallback, does not
    block a later valid immutable candidate through the profile-selected
    authenticated CAS repair or protected immutable-epoch rotation strategy,
    cannot redirect transport outside its typed trusted provider namespace, and
    cannot grant authority; strategy-specific tests prove that no backend path
    requires or silently falls back to the other strategy;
29. an untrusted PR receives only a proven read-only credential for the
    protected public audience, closed before candidate execution, and cannot
    discover local/sensitive tiers or publish, repair, or rotate any shared
    semantic, rejection, conflict, or accepted-root namespace; scope narrowing
    stops future eligibility but cannot shrink the monotonic audience union or
    claim already copied immutable bytes became secret;
30. every valid cached raw action is fully rehashed and processed by the current
    verifier/projection before hosted authority exists;
    verifier/projection/summary-only changes execute no compiler cases when exact raw keys exist;
31. the FCI-7 `snapshot`, `affected`, and `why-miss` commands bind exact source
    snapshot/guard, graph, transition, trust, scope/audience, conflict registry,
    namespace, selected provider/generation, and evidence-snapshot digests;
    candidate selection and shortest-path tie-breaking are explicit and output
    is byte-identical on replay;
32. `verify-outcome` handles both outcome kinds, and neither it nor
    `verify-cache` can export a process capability;
33. GC retains active reader/publisher leases and every live Merkle child,
    distinguishes a crash residue/PID reuse by the OS lock, and holds the
    store-wide exclusive barrier from final snapshot validation through sweep;
    a reader/publisher cannot register between recheck and deletion, no
    post-mark object is swept, symlinks are refused, and only the exact unchanged
    `GcPlanV1` applies; it never names a remote object for deletion and never
    removes conflict/disclosure authority state; corrupt, missing, rolled-back,
    or unverifiable conflict authority requires exact protected restore,
    capacity uses membership-preserving compaction, and an otherwise valid
    storage rollover uses an exclusive barrier, old-lease/invocation drain,
    unchanged action keys, and exact predecessor conflict/disclosure
    commitments, never GC or a candidate-selected tombstone/history deletion;
34. hosted mixed exact acquisition and fully fresh execution produce the same
    canonical semantic root and acceptance projection, while origin
    capabilities/receipts remain distinct;
35. the hosted root contains the complete current ts-tests action set and no
    owner-control id or `NonReusable` action; rejection in one adapter does not
    omit sibling adapter actions, while the local typed pair still requires the
    separate owner root;
36. every graph node persists its complete strongly typed `NodeSpec`; missing
    bytes, a valid digest over the wrong typed schema, unknown kind/spec fields,
    noncanonical re-encoding, and a `kind_spec_digest` mismatch all fail before
    impact planning;
37. action, build, and root golden fixtures bind both their generic
    `ProtocolDomainV1` and canonical `ApplicationNamespaceV1`; equal payloads
    under two application namespaces never alias, and an implementation/schema
    audit finds no tsc-rs/H2/branch literal or branch in generic crates outside
    named fixtures and the inert Cargo package/library-name declarations;
38. local-store crash fixtures interrupt every immutable publish and
    `index-head/current.json` replacement boundary; readers see only the old or
    new complete reachable generation, loose manifests remain invisible, and a
    platform without proven atomic-replace/durability support fails closed;
    exact repair restores the authenticated authority root, while an allowed
    rollover refuses an unverifiable conflict set, takes the exclusive epoch
    barrier, admits no new old-epoch work, drains old leases/invocations,
    rejects every old issuer/head/locator/candidate for future authority,
    retains exact predecessor conflict/disclosure commitments under unchanged
    action keys, requalifies only eligible nonconflicted actions, and does not
    retroactively alter a capability already returned by a completed
    invocation;
39. an arbitrary adapter-owned marker can name
    `AuthorizedRoot<AdapterMarker>` but cannot construct it; only a private
    constructor consuming both `VerifiedPolicySpec<AdapterMarker>` and
    effect-verified consumer identity plus
    `IndexCommitGuard<LocalAuthorityCommit>` succeeds; a recorded identity or
    conflict/remote guard fails, and no input/root is cloneable/deserializable;
40. cold/warm graph planning, inventory, hashing, canonical encode/decode, CAS
    verification, incoming-object decode, explanation, and coordinator fixtures obey
    their frozen CPU-duty/RSS/byte/concurrency ceilings; each activated fully
    reusable warm no-impact fixture builds or spawns exactly zero action
    harness, semantic, compiler, oracle, or test subprocesses while reverifying all required reusable
    actions and rebuilding the root, while a separate generic synthetic
    `NonReusable` fixture executes exactly its frozen action set and cannot
    claim that zero-process result;
41. fresh, shadow, reusable, and `FreshAll` fallback `local-full` all consume
    one `VerifiedCompletePhaseRegistry`; removing/reordering/unclassifying a
    phase, adding an alternate array or direct authoritative call/spawn, or
    constructing a final capability from a partial `PhaseTraceV1` fails the
    compile/dependency/call-graph/source and runtime audits; FCI-9a rejects a
    nonzero legacy tail or any selected `NonReusable` action, and the hosted
    type continues to reject local-only controls;
42. the FCI-8c provider contract fixture pins the researched API/version,
    selected atomic strategy, mandatory conflict/disclosure/event-history
    authority head and optional candidate channels, receipt/event/head DAG,
    ancestor/inclusion proofs, namespace/auth/attestation, limits, retention,
    exact restore or barrier-and-drain rollover with unchanged semantic keys
    and exact predecessor conflict/disclosure commitments, refusal when the
    conflict set is unverifiable, and failure classes; packet checking rejects provider
    implementation authorization until every field and probe result is frozen;
43. one immutable `SourceSnapshotV1` supplies graph/profile, inventory, direct
    files, directories, negative lookups, generated/build inputs, and the
    sandbox mount; concurrent edits, symlink/special-file archive entries,
    acquisition-after-mutation, and mixed Git-tree/dirty-overlay states fail
    before authority rather than hashing a state that never existed;
44. sandbox fixtures classify
    clock/entropy/PID/hostname/host-proc/IPC/network/loader/CPU/filesystem/environment
    channels as declared, denied, virtualized,
    or completely audited; only a runner-created guard can authorize a reusable
    observation, and repetition alone cannot satisfy the capability;
45. compile-fail/runtime fixtures prove pending observation/composite collectors
    cannot aggregate, missing/duplicate/unexpected/wrong-key leaves and adapter
    instances fail, candidate bytes cannot register adapter code, strict typed
    prepare precedes sealed erasure, and H2/workspace-audit compose without
    `Any`, downcast, `unsafe`, or id/kind branching in generic code;
46. local restart fixtures verify issuer/store epoch and sequence, receipt
    engine/source/sandbox/scope/proposed-audience/observed-prior-generation
    binding, acyclic receipt/event/replacement-generation construction,
    permissions and rollback/replay policy;
    candidate content cannot widen `LocalReusable`/audience to public, and a PR
    with no provably narrow read credential receives an exact miss rather than
    sensitive bytes; an already public digest cannot be relabeled secret and
    new sensitive bytes use a new key/namespace while preserving old disclosure;
47. the FCI-8b protected-host fixture pins required-workflow identity, exact
    tested tree, signed engine channel/release/attestation and fallback plan;
    candidate `.cargo/config.toml`, `PATH`, fake cargo/xtask, verifier, graph,
    policy, or unconditional exit zero cannot change the protected verdict;
48. tampered/wrong-issuer/subject/audience/workflow/base/tree/platform/binary or
    replayed engine releases fail before candidate execution; engine-owned
    changes use complete fresh fallback with zero reuse/write or return
    `RequiresStagedEnginePromotion`, and only `E_n` plus a protected signer may
    promote `E_(n+1)` for a later invocation; and
49. a fork's malicious build script cannot read credentials, parent env,
    control directory, host `/proc`, provider sockets, network, or workflow
    command files, cannot leave a process or publish, and the workflow fixture
    exposes exactly one semantic `run:` command, `cargo xtask acceptance`, from
    the protected control directory with no owner-control id.

The minimum cutover commands introduced by the migration are:

```text
cargo xtask test ci-core
cargo xtask test ci-runner
cargo xtask test xtask functional_ci
cargo xtask functional-ci graph --profile hosted-ts-tests --format text
cargo xtask functional-ci snapshot --profile hosted-ts-tests --base <sha> --head <sha> --out <path>
cargo xtask functional-ci affected --profile hosted-ts-tests --evidence-snapshot <path-or-digest>
cargo xtask functional-ci why-miss --profile hosted-ts-tests --action <node-id> --evidence-snapshot <path-or-digest> --candidate <source-ref>
cargo xtask functional-ci verify-plan --profile hosted-ts-tests
cargo xtask functional-ci verify-outcome --profile hosted-ts-tests --outcome <digest>
cargo xtask functional-ci verify-cache --profile hosted-ts-tests --locator <exact-locator>
cargo xtask functional-ci gc --dry-run
cargo xtask functional-ci gc --apply-plan <plan-digest>
cargo xtask ci --baseline <trusted-base-sha>
cargo xtask acceptance
```

`cargo xtask ci` and `cargo xtask acceptance` already exist, but their current
H2.5g meanings must not be replaced during FCI shadowing. The other commands do
not exist yet and must not be substituted into the current H2.5g closure.
FCI-1/FCI-2 introduce their role-test commands, FCI-4a.1 introduces `graph`,
FCI-5a introduces `verify-plan`, FCI-6b/FCI-6c/FCI-6e introduce local
`verify-cache`/`verify-outcome`/`gc`, and FCI-7a introduces the live
`snapshot`/`affected`/`why-miss` commands. FCI-8a and FCI-8f complete the local
and hosted shadows; FCI-9a and FCI-9b activate them separately. Every command
documents its exact output contract before its activation packet may proceed.
