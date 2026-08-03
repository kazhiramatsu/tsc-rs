# Evidence and M9 steady-state contract

Status: normative support contract for B1-B4 and M9.

This document owns how runtime, fuzz, performance, and nightly evidence
is produced and verified. The
[completion convergence plan](completion-convergence-plan.md) owns
landing order; [measurement-integrity.md](measurement-integrity.md)
owns shared history-anchor and identity rules.

## 1. B1 — one evidence protocol

`m8-evidence.json` contains producer configuration, reviewed
ceilings, and approved runner profile ids. It contains no editable
`ready` boolean or copied observation count.

Every produced artifact contains:

- schema and producer version;
- producer commit, command, arguments, timestamps, and exit status;
- an exact input fingerprint over the built executable and all relevant
  source manifests, `Cargo.lock`, toolchain/Node pins, vendor, immutable
  oracle inputs, comparator, producer/generator/instrumenter code,
  inventory, scope, policy, runner, and arguments as applicable;
- raw observations from which every summary is recomputed;
- artifact SHA-256 recorded in the common manifest.

Freshness means fingerprint equality, not merely an ancestor commit. A
dirty relevant path, missing artifact, wrong schema, failed exit, or
raw/summary mismatch fails. A docs-only change outside a producer's
declared inputs need not stale it. Artifact paths are workspace-relative
and may not escape the workspace.

Artifacts under `target/` are ephemeral. The required workflow builds
once, reuses current verified producer output where allowed (currently
B2 only), runs every stale or non-reusable producer, then consumes those
artifacts in the same workspace:

```sh
cargo xtask m8 evidence produce --all
cargo xtask m8 readiness --require-ready
```

The orchestration command validates a candidate B2 artifact's schema,
producer-input fingerprint, inventory hash, raw declaration counts, and
exact zero-hit reviews before reuse. A missing, stale, malformed, or
incomplete candidate is regenerated. Before the reviewed M9.1 schema
transition, B3 and B4 are always produced. At that transition, one bounded
M9 PR-smoke artifact replaces the old B3 entry artifact: M8 readiness
consumes its compatibility projection and M9 consumes its domain/
classifier/replay/reducer records. The orchestration must not execute a
second legacy smoke. B4 remains separately produced, and the common
manifest is always rewritten from the artifacts actually consumed. A fresh
local clone regenerates B2; hosted CI may restore the exact content-
addressed B2 artifact populated by a prior successful run. M9 history is
separately versioned in-repo.

## 2. B2 — runtime emitter coverage

Generate an instrumented `_tsc.js` under `target/`; never edit the
vendor. Counters use D2 declaration identities, so same-named and
anonymous functions remain separate. `<top>` uses a module-evaluation
marker, not a function-entry counter.

Run the full oracle corpus and record one count per direct-emitter
declaration. Readiness derives executed emitters from non-zero counts.
At least one emitter must execute. Every zero-hit identity needs reviewed
evidence tied to the exact inventory hash. Unknown, duplicate,
overlapping, name-collapsed, or unaccounted identities fail.

The runtime artifact is an execution-presence ledger, not a profiler:
each declaration counter saturates at `1`. Instrumentation writes an
inline declaration-index byte in a `Uint8Array` and resolves indexes back
to D2 identities only when the process finishes. This avoids doing a
long-ID object lookup and arithmetic update on every scanner/checker AST
visit while preserving the exact ready/not-ready predicate.

The producer limits itself to the configured `runtime_coverage.max_workers`
(currently one) even when more logical cores are available, and starts the
coverage Node process in single-threaded mode. This keeps the full-corpus
evidence run from monopolizing a developer machine or a shared CI runner;
changing the cap affects throughput only, not the corpus or acceptance
criteria. Within that process, vendor lib SourceFiles are reused only
under TypeScript's own `sourceFileAffectingCompilerOptions` key and a
bounded LRU (`max_lib_cache_buckets`, currently eight). The producer
ends each Node process after `programs_per_process` programs (currently
500) and ORs the per-process hit-sets, bounding retained AST/heap state.
Before the sweep, `diagnostic_canary_programs` programs (currently 32)
run twice and must produce byte-identical serialized diagnostics through
the cached, instrumented driver and the ordinary uncached oracle driver.
The second pass forces reuse and guards the optimization against
cross-program SourceFile state leakage.

The completed B2 instrumented runner provides selected-fixture trace
mode for
[D2 trace-assisted implementation clusters](measurement-integrity.md#62-trace-assisted-implementation-clusters).
Trace mode associates a diagnostic-time call stack and all-declaration
execution coverage with exact oracle diagnostic identities. It is
planning and review evidence: it identifies the dynamic seed for a
dependency-closed porting slice, but it does not replace the full-corpus
direct-emitter counters, the static call graph, or declaration
dispositions. In particular, a declaration absent from a trace may not
be classified as not applicable on that basis.

The fingerprint includes the instrumenter, Node pin, vendor, declaration
inventory, immutable oracle inputs, full-corpus command, harness, and
the B2 producer source/dependencies. It deliberately excludes unrelated
checker and xtask executable bytes so those changes do not force this
oracle-only sweep. A selected trace additionally fingerprints its
position map or shadow-stack producer, stack-depth policy, and
emitting/non-emitting probe pair.

Acceptance:

```sh
cargo xtask coverage emitters --corpus
cargo xtask codegen band-inventory --by-function --band all --check
cargo xtask m8 readiness
```

## 3. B3 — differential fuzzing

Commands:

```sh
cargo xtask fuzz preflight [--require-ready]
cargo xtask fuzz run --seed <u64> --cases <n> --artifact <path>
cargo xtask fuzz replay <case>
cargo xtask fuzz reduce <case>
cargo xtask fuzz nightly --policy ratchets/fuzz-steady-state-policy.toml
cargo xtask fuzz aggregate --window <path> --attestation <path>
cargo xtask fuzz steady-state [--require-ready]
```

The M8 B3 entry artifact is a 32-case, eight-template smoke. It proves only
that the generation/comparison/reducer/deduper path is callable. Its
aggregate-pass, line-reducer, saved-JSON replay, and in-memory/per-case
scratch representation earn no M9 window credit.

Before history bootstrap, M9 replaces that entry surface with grammar-aware
generation and corpus mutation under the ordered
[M9 execution contract](m9-execution-and-close.md). The versioned domain
manifest covers compiler options, single/multi-file topology, TypeScript,
TSX, checked JavaScript/JSX, JSDoc parser/binder/checker semantics, syntax
recovery, and the supported semantic model. It fixes stable generator-branch
identities, witness seeds, per-window stratum/cross-stratum minima, and a
unique-normalized-program floor. Raw observations recompute those values; a
large count of one simple template cannot satisfy them.

The generated domain is the supported batch checker. Filesystem package-host
resolution outside the in-memory program model, project references,
emit-dependent behavior, LSP/watch/incremental state, and public API calls
are excluded by construction. A generated case never inherits a
fixed-corpus A2 exclusion. Generating or silently discarding an out-of-domain
case fails the window.

Every case runs tsrs and the pinned oracle through T0-T4. The canonical
class is:

```text
schema + first failing tier-or-terminal phase
+ real pass-or-terminal sentinel + divergence side/outcome class
+ sorted one-sided multiset of (code, normalized message head)
  or closed terminal kind/boundary key
```

Raw diagnostic validation fixes message-chain depth at 32 and total nodes
at 4,096 before recursive projection. Either engine adapter treats an
over-limit tree as a typed malformed response rather than comparing a
truncated chain.
For each completed engine outcome, the renderer assembled sequence projects
the structured diagnostics one-for-one in the same order and multiplicity;
only its canonical-head sidecar is additional. Sort/dedupe/format
observations are downstream of that join. A final row must select an
assembled row, while its order and multiplicity remain observable. Thus
independently supplied execution fragments cannot masquerade as one raw
outcome.

The normalized head comes from the complete T2 record before T0/T1
projection, after versioned virtual-path, LF, and generator-identifier
normalization. Schema 1 is a one-way raw-to-normalized encoding: every
literal raw `<` becomes `<<`, while only typed owned-path and generated-
identifier replacements emit single canonical `<@...>` and `<#...>`
placeholders. Normalized text is never re-entered as raw text, and one exact
raw source cannot be owned by both path and identifier roles. Multiplicity
is retained. T4 adds the first applicable renderer class in fixed precedence
`order`, `dedupe`, `path`, `newline`, `text`, plus the first affected
diagnostic key. Positions, seeds, timestamps, and raw hashes do not enter
the class. Renderer `order` and `dedupe` are derived from the captured final
deduped sequence; the assembled sequence remains provenance. Empty
segments and dropped/inflated final rows remain representable before the
comparator falls back to whole-aggregate path/newline/text checks.
Independent Node fixtures and Rust production-path tests derive classifier
bytes from the same typed raw vectors.

T0-T3 retains the real syntactic/semantic/suggestion pass. A pure T4
failure compares the final deduped render sequence and uses
`pass=aggregate-render`. A no-diagnostic terminal class uses
`pass=terminal`, a fixed `parse|bind|check|format` phase, and terminal kind
plus adapter-owned `boundary_id` from a schema enum with a closed phase/kind
allowlist rather than an invented diagnostic code. Volatile raw process
detail never enters that class.

A tsrs crash/panic, timeout, OOM, or unsupported unwind after a valid oracle
result is a terminal divergence class. M9 preflight turns every prose crash
shape enumerated by M8 readiness into a frozen machine registry with exact
input/outcome hashes, the declared Rust outcome, and positive/adjacent-
negative replay canaries. An oracle crash counts only when real replay
matches one exact registry row; it is recorded as
`recorded-oracle-deviation`, not parity. Any other oracle, generator,
domain, harness, or controller failure makes the window unsuccessful because
no valid observation exists; it is not resampled. Exact outputs remain in
the witness artifact.

`fuzz replay` actually reruns both engines with the exact files, options,
cwd, generator decisions, and process policy. Reduction must retain the
class, terminal outcome, domain validity, and exact failing comparator
through real replay. Deduplication evidence requires independently executed
cases; repeating one stored class value is not evidence.

The long producer runs bounded serial child shards. Each child owns one
persistent oracle/renderer Node worker launched under the pinned single-
threaded Node/V8 policy, caps Rust's internal worker/test threads, and exits
after a fixed case count. Successful cases stream compact domain/input/
output/outcome digests to compressed output; only divergences and terminal
failures retain full sources, outputs, logs, and reducer state. It does not
retain all cases in memory, create one directory per successful case, start
Node per case, or run B2 AST instrumentation. The producer records child and
aggregate CPU time, peak RSS, scratch bytes, and process rollover.
Process-rollover and shard-partition canaries must be byte-identical.

Only the immutable compiler/library bundle may be shared by default inside
one bounded child; generated programs, mutable checker state, and SourceFiles
are case-local. Any broader lib/SourceFile cache needs a frozen finite
capacity and an exact key covering at least oracle/library hashes, source
text digest, script kind, and all source-file-affecting compiler options.
Cold/warm diagnostic canaries must be byte-identical, and changing this
cache policy resets the semantic fingerprint.

After M9.2 implements the bounded domain producer and before M9.3 changes the
CI/schema consumer, hosted calibration freezes the PR smoke's exact case
count, seed list, domain-canary ids, and wall/RSS/scratch ceilings. The list
is bounded and does not grow implicitly when the nightly domain manifest
grows. PR CI invokes that smoke exactly once; its single versioned artifact
supplies both the M8 B3 readiness projection and M9 classifier/replay/
reducer/domain evidence. Scheduled CI alone runs the full window. A separate
mutation canary may exercise the one-sided path when the smoke is exact, but
it is labeled, excluded from generated observations, and cannot substitute
for real two-case dedupe evidence. The old 32-case/eight-template producer
is retired at the reviewed schema transition and never runs beside the M9
smoke.

### 3.1 M9 steady state

The versioned contract is fully reconstructible from a fresh clone. The
policy hashes these checked-in, workspace-relative machine inputs and
results; a similarly named file under `target/` cannot satisfy them:

- `ratchets/fuzz-domain.v1.toml` owns stable generator branch/cross-stratum
  ids, witness seeds, compiler-option/topology dimensions, nightly quotas,
  uniqueness floor, and the independently bounded PR-smoke manifest;
- `ratchets/fuzz-oracle-deviations.v1.json` owns the exact reviewed M8 oracle
  crash inputs/outcomes, declared Rust outcomes, and positive/adjacent-
  negative replay canaries;
- `ratchets/fuzz-preflight.v1.json` owns the implementation-gap, outcome,
  adversarial-test, static D2-candidate, and resource-survey result;
- `ratchets/fuzz-calibration.v1.json` owns the standard-runner 100,000-case
  pilot plus PR-smoke wall/CPU/RSS/scratch/artifact observations and the
  reviewed ceiling derivation;
- `ratchets/fuzz-burn-in-zero.v1.json.zst` owns canonical per-case digests,
  quota/uniqueness/resource totals, discovery membership, artifact
  attestations, and owner-task closure for the final full-domain burn-in;
  and
- `ratchets/fuzz-producer-inputs.v1.json` owns the exact semantic-
  fingerprint path/hash manifest, schema versions, stable runner fields,
  worker/process policy, and verifier/attestation inputs.

The final rate gate then uses these versioned policy/history/registry
artifacts:

- `ratchets/fuzz-steady-state-policy.toml` moves once from `draft` to
  `frozen` through the reviewed-snapshot protocol. Its freeze record contains
  the adjudication commit and exact hashes of the green preflight/domain,
  oracle-deviation registry, bounded-runner calibration, burn-in-zero,
  producer/verifier input manifest, scheduled workflow, and attestation
  policy. A scheduled producer refuses to mint qualifying evidence while
  that anchor is missing, draft, or mismatched. The frozen policy fixes 14
  consecutive UTC windows, exactly 100,000 valid cases per window (ordinary
  exact/divergent comparisons, tsrs terminal divergences with a valid oracle,
  and exact recorded oracle-deviation outcomes), the measured standard-
  runner wall/RSS/disk ceilings, generator-domain quotas/uniqueness, worker/
  process/timeout limits, deterministic seed derivation, and CI attestation
  policy. Fourteen windows and 100,000 cases are contract constants, not
  tunable defaults. Wall time is a maximum, never a minimum to consume. Any
  later policy change is a new reviewed freeze, changes the semantic
  fingerprint, and restarts the streak.
- `ratchets/fuzz-nightly-history.v1.json.zst` appends non-overlapping
  UTC-slot/attempt records: derived seeds, cases, quotas, runtime/resources,
  semantic fingerprint, artifact/attestation hash, and new class/incident
  ids. It uses append-only lineage and trusted-base comparison. Prior
  canonical records remain byte-identical.
- `ratchets/fuzz-registry.v1.json.zst` appends immutable canonical
  classes, exact minimized witnesses, recurrence incidents, and exact owner
  tasks. Discovery, owner-task assignment, and per-task resolution are
  append-only transition events; current state is derived rather than
  written back. A recurrence appends a new incident instead of reopening or
  rewriting the old one. Diagnostic tasks use pipeline layer, A5/2XXX
  family, exact D2 declarations/SCC, and the Rust boundary. Terminal,
  parser/binder/program, and pure-T4 tasks use an exact pipeline-native tsc/
  Rust boundary rather than a fake diagnostic family. A later-proven
  generator/domain-validator/oracle-adapter/harness/controller/comparator/
  reducer/classifier/registry-history defect uses an exact `producer-defect`
  task without deleting the original observation. Semantic-task resolution
  cites a passing real replay plus the regression fixture's conformance-
  universe transition and A1 acceptance. Producer-defect resolution instead
  cites the producer fix, independent raw observation, exact replay,
  adversarial canary, and explicit A1-not-applicable disposition; every
  affected window is append-only invalidated and the fingerprint/streak
  resets.

Each history row references one slot directory
`ratchets/fuzz-windows/<UTC-slot>/`. Its `window.v1.json.zst` contains
per-seed outcome digests, failure/class/incident membership, domain
aggregates, and witness hashes. Its `attestation.v1.json` is the complete
signed statement/transparency bundle; the attested subject digest must equal
the window file, and repository/workflow/event/attempt/authority claims must
match frozen policy. Both sidecars are mandatory and hashed by history.
Append-only authority is the decompressed canonical record bytes plus
previous-record hashes, not incidental compressed-container bytes.

Qualifying seeds are derived from semantic policy fingerprint + UTC slot +
shard id; a workflow input cannot select them. There is at most one
finalized slot per UTC date, and only the scheduled workflow's attested
`run_attempt == 1` may qualify. A failed/cancelled/interrupted first attempt
breaks the date and streak. Reruns use the same seeds for diagnosis but are
non-qualifying, so an artifact-less first failure cannot be hidden offline
by a successful retry.

Protected-main scheduled CI produces a GitHub artifact attestation for the
compact raw bundle using minimal OIDC/attestation permissions and no
long-lived signing secret. The policy pins repository identity, workflow
path/hash, scheduled event, relevant-input fingerprint, runner profile, and
artifact digest, and `run_attempt == 1`. PR/manual/rerun jobs cannot mint
qualifying evidence.
Aggregation verifies that provenance and recomputes every raw digest,
class, incident, quota, and history edge before appending; it never reruns
the long producer or rewrites history. Multiple independently attested
consecutive windows may share one aggregation PR. An unsigned,
hand-authored, seed-selected, rerun-attempt, or reordered row cannot count.

`fuzz steady-state --require-ready` requires:

1. all policy, history, class/witness/incident, window, and attestation
   artifacts verify;
2. the last 14 windows are consecutive, current-fingerprint, successful,
   non-overlapping, each has exactly 100,000 valid cases within the frozen
   measured wall ceiling, and every domain/resource rule passes;
3. `distinct newly discovered canonical classes / 14 < 1.0` from raw
   membership;
4. the complete registry has zero untriaged incident and zero unresolved
   owner task.

A checker, oracle, generator, reducer, comparator/class/outcome schema,
domain/corpus-mutation input, process policy, M9 history/attestation
verifier, scheduled workflow, or attestation-policy change resets the
streak. UTC slot/seed/attempt metadata, append-only history/registry/window
records, and close-only docs/`STAGE` are outside that semantic fingerprint.
A missing, failed, under-budget, over-time, overlapping, stale, rewritten,
manually seeded, or unattested window breaks the streak. A docs-only change
outside the fingerprint does not.

The fingerprinted runner profile contains only stable, outcome-affecting
fields: OS/architecture ABI, standard-runner class, pinned Rust/Node/
TypeScript toolchains, worker/process policy, and a pinned container digest
if a container is adopted. Exact hosted-image revision, CPU model/frequency,
runner id, and available-core observations are recorded as diagnostic
metadata. Their ordinary rotation does not restart the streak, but a
metadata observation that violates the stable profile or a resource ceiling
invalidates that window. Changing a stable field is a reviewed policy change
and does restart it.

## 4. B4 — performance and RSS

`cargo xtask perf conformance --artifact <path>` launches the fixed CI
conformance producer as a child and records raw wall time and maximum RSS.
The producer expands each fixture and executes each checker case exactly
once. That case's aggregate and syntactic diagnostic streams feed the
`all`, `2xxx`, and `syntactic` accumulators in fixed order, while the `all`
stream also feeds A5. A scoped checker producer and the caller-thread grading
consumer overlap through a FIFO channel with capacity one. View grading and
callbacks remain sequential, so at most those two stages run concurrently;
there is no corpus-sized case cache. The consumer drops each view's ratchet
sets immediately after its gate. Configuration enumerates approved runner
profiles including OS/architecture, CPU/core policy, memory, and measurement
backend. A machine name alone is not an approved profile.

The fingerprint pins executable, full-corpus command/options, immutable
oracle inputs, toolchains, and runner profile. The wall ceiling is at
most 60 seconds. The first RSS ceiling comes from measured evidence plus
a reviewed margin. Readiness and completion require the observations,
not only configured ceilings, to pass on an approved profile.

The ceiling-bearing full-corpus observation uses the normal corpus-bounded
legacy lib-bundle cache. The harness retains an opaque prepared lookup hint
per immutable lib set and parser/binder option projection, avoiding a second
full-content fingerprint for every matrix case. This is only a hint: every
use still verifies the projected options plus the exact ordered lib names and
full texts, and any mismatch uses the ordinary exact cache path. Diagnostic
line starts are likewise built lazily once per case/file rather than once per
diagnostic. Also run a separately recorded
`TSRS_LIB_BUNDLE_CACHE=0` smoke in a short-lived child with an explicit
fixture limit; its purpose is to exercise the locally owned no-reuse path
while bounding repeated parse/bind cost and peak allocator retention. The
smoke cannot replace the full-corpus wall/RSS observation and cannot publish
CI conformance evidence. Give fuzz and coverage workers explicit process
lifetimes, and keep coverage concurrency at the configured bounded worker
cap.

Before the measured child starts, the parent invalidates the previous
performance manifest, receipt, and fixed outputs. After the full child, the
cache-off smoke, resource ceilings, input fingerprint, producer executable,
and repository HEAD all verify, the parent binds the exact `all`, `2xxx`,
`syntactic`, and standard `target/families/report.json` bytes and atomically
publishes the receipt last. The receipt fixes schema/producer, fresh nonce,
command, full-corpus/cache policy, fixed view order, HEAD, executable and
input fingerprints, and every output path/length/digest. Workspace-relative
paths and every existing parent component are checked without accepting
symlinks.

The bound summary files use a versioned CI-only projection. `all` retains the
complete exact T1-T3 identity observations used by the standing M8
conformance artifact. After each secondary view's authoritative accepted
`RunSets` have passed, `2xxx` and `syntactic` retain their aggregate fields and
mismatch detail but omit the redundant report-only identity vectors. Their
oracle-universe digests remain complete, and the projection decoder rejects
unknown bands, invalid nested observation schemas, and non-empty omitted
vectors. Tests require the remaining fields, universe digests, and gated
`RunSets` to equal independent full-view grading. Each view is
compact-streamed directly to its one bound path; the producer never
pretty-serializes or rewrites the same summary.

Publication returns a non-serializable, move-only token to the same xtask
process. The later semantic merge gate can consume only that token and
deserializes the already rehashed output bytes; it does not reopen summaries
or run conformance again. A missing, stale, reordered, replaced, or tampered
receipt/output is a hard failure with no duplicate-run fallback. The token
and receipt never cross a job, Actions cache, or uploaded artifact boundary.

## 5. Required CI topology

A trusted-base diff containing only `.md` paths and leaving README's
generated `STATUS` block byte-identical runs no Cargo, Node, B2, or full-
corpus work. A lightweight hosted classifier preserves the required `gates`
check while marking the Rust and semantic lanes skipped. Local validation is
`git diff --check` plus review of changed links/anchors and generated-block
boundaries. Any other path or generated-status change uses the required PR
CI below:

- fetches enough history for every A1/A2/A5/M9 anchor;
- runs recursive and trusted-base integrity checks;
- runs the permanent syntactic and ordinary conformance gates from B4's one
  fixed-order producer traversal and same-process receipt;
- builds once, verifies/reuses or produces B2, produces B4 and exactly one
  M9 PR-smoke artifact whose compatibility projection supplies B3, and
  invokes M8 readiness in that workspace;
- uploads mismatch, readiness, and fuzz/evidence artifacts on failure.

The ordinary PR semantic lane does not produce a completion report merely
for artifact upload. `cargo xtask completion` remains an explicit report-only
command during M8, and the final release job produces and consumes its strict
form in the same workspace.

The hosted implementation may place the independent Rust
format/clippy/build/test gates and the semantic evidence gates on two
standard runners. The boundary is fixed:

- `cargo xtask ci --lane rust` owns only format, clippy, build, and
  workspace tests;
- `cargo xtask ci --lane semantic --baseline <trusted-sha>` owns every
  recursive/trusted-base audit, all fixed conformance views, recovery
  census, invariant/ledger/escape gates, and readiness production and
  consumption. Its B4 producer and conformance consumer remain in the same
  process so the move-only receipt authority cannot cross this boundary;
- a final job named `gates` succeeds only when both lanes succeed.

Thus no evidence producer/consumer or A1/A2/A5 ordering crosses a job
boundary. Except for the exact documentation-only rule above, the ordinary
local `cargo xtask ci` remains the sequential union of both lanes and is
still the required pre-PR/pre-merge gate.
Main-branch runs populate the cache scope that later pull requests may
restore. Lockfile-keyed Cargo caches contain dependency archives only;
a pinned content-addressed compiler cache handles build outputs without
trusting checkout timestamps. A separate exact-fingerprint cache may
contain only the B2 raw runtime artifact; the semantic lane revalidates
it and rewrites the manifest. Conformance, readiness, B3, B4, and other
semantic evidence artifacts are never restored from that cache.

Normal PR CI runs the one short, calibrated, fixed-seed M9 domain/classifier/
replay/reducer smoke described above; the M8 B3 projection is derived from
the same artifact, and neither invocation nor case generation is duplicated.
It never runs a qualifying window. Protected-main scheduled CI runs exactly
100,000 valid cases within the frozen measured ceiling, streams the compact
raw bundle, and attests it. A reviewed aggregation verifies and appends one
or more independently attested windows without rerunning the producer or
rewriting history. B2 AST instrumentation is not part of that scheduled job.

The final release job uses the approved performance runner, regenerates
B1-B4 evidence, runs full-corpus invariants, verifies M9 history, and
then runs `cargo xtask completion --require-done` in the same workspace.
It consumes the existing 14 windows rather than producing a fifteenth. Gate
logic stays in local commands; YAML only executes it.

## 6. Required adversarial tests

- changing checker/producer source, lockfile, toolchain, scope, inventory,
  policy, or arguments independently stales the affected artifact;
- a docs-only path outside the fingerprint remains valid;
- missing ephemeral evidence fails, while a fresh-clone produce-then-read
  workflow passes;
- same-name declaration counters, anonymous counters, and `<top>` remain
  distinct; a hit for one never covers another;
- a trace joined by printed name, instrumented coordinates treated as
  vendor coordinates without a position map, or a truncated, unresolved,
  or external frame silently dropped instead of classified fails;
- trace or coverage absence used to shrink the static closure or justify
  a not-applicable disposition fails;
- an out-of-domain generated case, unequal comparison count, unmet domain
  quota/uniqueness floor, summary trusted instead of recomputed, classifier
  drift, or reducer changing class/outcome/comparator fails;
- pass aggregation, duplicate-diagnostic multiplicity loss, an empty T0/T1
  head caused by projection, generated-name/path instability, or a fake
  duplicate made by copying one class value fails the classifier/deduper
  tests;
- a saved-JSON-only replay, non-fixpoint/non-replaying reduction, per-case
  Node launch/directory, unbounded scratch/vector, process-rollover drift, or
  shard-partition-dependent output fails;
- a tsrs terminal failure treated as exact; an unknown oracle/domain/
  harness/controller failure silently discarded/resampled; or an oracle
  crash called a recorded deviation without exact frozen-registry replay
  fails the window;
- a deleted/rewritten/unattested/stale/under-budget/over-time nightly window,
  non-first workflow attempt, manually selected seed, duplicate UTC slot,
  or class/witness/incident deletion or rewrite fails M9 lineage;
- a recurrence that mutates/reopens an old incident instead of appending a
  new one, transition state written back instead of derived from append-only
  events, one canonical class treated as one owner, an incident with zero or
  unresolved owner tasks called resolved, a semantic task lacking real
  replay plus A1 regression acceptance, or a producer defect that deletes
  its original incident/keeps its window valid/lacks an adversarial canary
  fails;
- a policy encoding thresholds other than 14 windows or exactly 100,000
  valid cases; treating wall time as a minimum; or changing the
  calibration-anchored wall/RSS/disk ceiling fails before evaluating
  history;
- a PR/manual/wrong-workflow/wrong-repository attestation, or a qualifying
  seed not derived from policy fingerprint + UTC slot + shard, fails;
- an aggregation/history/close-only change that resets the semantic
  fingerprint, or a behavior-relevant source/workflow/policy change that
  does not reset it, fails;
- an unapproved runner, declared-but-unobserved ceiling, wall over 60 s,
  RSS over its ceiling, or producer fingerprint mismatch fails.
