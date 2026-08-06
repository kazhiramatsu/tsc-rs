# M9 execution and close contract

Status: paused after M9.1b. M9.0's draft preflight inventory, M9.1a's typed
outcome/class model, and M9.1b's canonical true replay plus bounded one-case
Node/Rust adapters have landed. M9.1c through M9.7 remain pending; no
burn-in, frozen fingerprint, or qualifying window is running. The separate
[H0 filesystem-hosted `--noEmit` track](noemit-cli.md) is complete, and
[H1 JavaScript emit](h1-emit.md) is now in design without earning M9 evidence
or qualification credit. This contract remains the authority when M9 resumes.

This page owns how M9 is investigated, implemented, qualified, and closed.
The [definition of done](definition-of-done.md) still owns WHAT project
completion means, the
[evidence contract](evidence-and-steady-state.md) owns the machine artifact
and CI rules, and
[measurement integrity](measurement-integrity.md) owns A1/A2/A5 and D2
identity/lineage. This page owns the M9 ordering, owner strategy, resource
policy, and stop conditions.

The governing lesson from M8 is:

> Discover the complete producer surface before a long campaign, and repair
> each divergence through its exact tsc owner and dependency closure. A
> symptom signature is not an implementation boundary.

## 1. Outcome, entry, and scope

M9 establishes bounded corpus-external confidence in the completed
TypeScript 6.0.3 batch diagnostics checker. It does not claim a proof of
equivalence. It never removes, reclassifies, or weakens an M8 corpus
identity; resolved fuzz repros may extend that universe only through A1's
reviewed append-only transition.

M9 starts only when:

- `STAGE=M8`;
- `cargo xtask completion` reports rows 1-10 green and only
  `m9-steady-state` pending;
- all-corpus `FP=0`, all accepted sets, the frozen exact scope, A5, D2,
  B1-B4, and the full-corpus invariant attestation remain green; and
- the checked-in M8 close record is current.

A generated case does not inherit an A2 exclusion from a fixed-corpus
fixture. Every generated case admitted by the M9 domain requires exact
T0-T4 parity, apart from the exact reviewed oracle-crash shapes enumerated
by the [M8 readiness contract](m8-readiness.md#recorded-tsc-603-crash-deviations-differential-classification).
M9.0 must materialize those prose records as the frozen machine registry
described below before any one can count. A generator branch that needs
emission, general host-backed module resolution, project references,
LSP/watch/incremental state, or a public `TypeChecker` API is outside the
domain by construction. Producing such a case invalidates the window; it is
never silently discarded or turned into a new exclusion.

“Complete subsystem port” in this document means the dependency-complete
tsc 6.0.3 subsystem needed by the supported batch-diagnostics surface. It
does not authorize emit, LSP, host, public-API, or upstream-version scope.

## 2. Audited M9 entry state

The M8 B3 artifact proves that generation, comparison, a reducer path, and
signature deduplication can be invoked in CI. It deliberately does not
satisfy an M9 window:

| Surface | M8 entry implementation | Required before M9 qualification |
|---|---|---|
| Generator | 32 cases from eight mostly single-file templates | grammar/corpus mutation with a frozen, measured domain matrix |
| Commands | `fuzz run`, `replay`, and `reduce` | preflight, nightly, aggregate, and steady-state verification too |
| Replay | validates saved JSON structure | reruns the exact case through both current engines |
| Reducer | line deletion around one selected result | program/syntax-aware fixpoint reduction with real replay |
| Signature | aggregate pass; T0-T3 set projection | pass-aware, multiplicity-preserving canonical class |
| Dedupe proof | repeats one saved signature value | at least two independently executed cases in the same class |
| Storage | retains all cases and per-case scratch trees | streaming digests and bounded scratch; full bytes only for failures |
| Completion | row 11 is explicitly false | data-driven, Node-free history verifier |
| Hosted operation | no scheduled workflow | attested scheduled producer and reviewed aggregation |

The entry implementation also stores the rotating seed/case arguments in
the producer fingerprint and keeps the M9 code inside the broad xtask
producer. Those boundaries must be split before history bootstrap:
different UTC seeds must share one semantic fingerprint, while a checker,
oracle, generator, comparator, reducer, domain, workflow, or policy change
must reset the streak.

No qualifying window may be recorded until every discrepancy in this table
has a machine-enforced disposition.

## 3. Required landing order

M9 executes in this order:

1. **M9.0 preflight inventory** — materialize the implementation-versus-
   contract gap report, generator domain, outcome model, resource pilot,
   and tsc/D2 planning inputs.
2. **M9.1 fuzzer foundation** — isolate the producer, implement true replay
   and reduction, correct the pass/tier comparator and canonical class, and
   land the adversarial tests.
3. **M9.2 bounded domain producer** — implement grammar-aware generation,
   corpus mutation, domain quotas, streaming execution, bounded
   child/Node lifetimes, and the coverage ledger.
4. **M9.3 history and CI foundation** — land window/history/class/witness
   schemas, recurrence and owner triage, attestation verification,
   scheduled production, aggregation, the Node-free steady-state verifier,
   the data-driven completion row, and code-level acceptance of the future
   `STAGE=M9` close marker while the checked-in marker remains `M8`.
5. **M9.4 non-qualifying burn-in** — exercise every domain shard, reduce and
   register every incident, and close them through owner/declaration
   slices. Burn-in never earns a qualifying window.
6. **M9.5 fingerprint freeze** — freeze the exact producer inputs, policy,
   domain quotas, scheduled workflow, and attestation policy only after
   the final complete burn-in has zero newly discovered class, zero
   untriaged incident, and zero unresolved owner task.
7. **M9.6 qualification** — append 14 consecutive qualifying UTC windows
   under that one frozen fingerprint.
8. **M9.7 close** — create the close candidate by updating `STAGE` from
   `M8` to `M9` and recording the status, then run the final release gate on
   that exact tree without changing a producer input.

All code and gate behavior needed to turn zero history into row 11 red and
14 valid windows into row 11 green lands before M9.5. In particular,
replacing a hard-coded completion result after the streak would change the
producer or verifier boundary and is forbidden. The final close contains no
checker, oracle, generator, comparator, reducer, policy, workflow, or
history-schema change.

## 4. M9.0 preflight inventory

The first implementation slice adds a report-only `fuzz preflight` view.
It reports facts; it neither creates history nor decides that a gap is
acceptable.

The initial M9.0a landing is deliberately draft-only. Its schema loader
accepts and reports the three `draft` inventories, but rejects a premature
`frozen` status and cannot satisfy `--require-ready`. A later slice may
enable `frozen` only together with the complete required-identity and
cardinality set, typed raw evidence, and recomputation needed to prevent an
edited or truncated inventory from becoming ready.

The report inventories:

- every current generator branch and its stable identity;
- TypeScript, TSX, checked JavaScript, and JSX script kinds;
- syntax/recovery, declarations/statements/expressions, relations,
  generics/inference/overloads, flow, modules, JSX, checked-JS/JSDoc, and
  renderer strata;
- single- and multi-file topology, allowed compiler-option dimensions,
  and corpus-mutation seed classes;
- syntactic, semantic, and suggestion passes and T0-T4 observation paths;
- every terminal outcome and timeout boundary;
- a frozen oracle-deviation registry containing every reviewed M8 crash
  shape, exact input/outcome hashes, the declared Rust outcome, and positive
  plus adjacent-negative replay canaries;
- classifier, reducer, deduper, replay, history, and attestation
  adversarial-test coverage;
- exact D2 inventory/fingerprint availability and the owner join used after
  a discovery; and
- cases/second, wall time, peak RSS, scratch bytes, artifact bytes, and
  Node/process lifetime on the standard hosted runner profile, including
  child and aggregate CPU time, Node's single-threaded launch policy, and the
  Rust internal-worker cap.

The machine records use the fixed workspace-relative paths defined by the
[evidence contract](evidence-and-steady-state.md#31-m9-steady-state).
M9.0 lands `ratchets/fuzz-domain.v1.toml`,
`ratchets/fuzz-oracle-deviations.v1.json`, and
`ratchets/fuzz-preflight.v1.json`; the optimized hosted pilot later lands
`ratchets/fuzz-calibration.v1.json`; the final complete burn-in lands
`ratchets/fuzz-burn-in-zero.v1.json.zst`; and M9.5 freezes
`ratchets/fuzz-producer-inputs.v1.json` plus
`ratchets/fuzz-steady-state-policy.toml`. The verifier starts from a fresh
clone and rejects an absent, draft, schema-mismatched, or hash-mismatched
record; an ephemeral file under `target/` cannot substitute for one.

The domain is a versioned manifest. Each production branch has a stable
identity and at least one deterministic witness seed. The manifest fixes
per-window minimums for every stratum and selected cross-stratum pairs,
plus a minimum unique-normalized-program ratio. A large count of repeated
simple assignments cannot satisfy it. A zero-hit branch is either fixed or
removed through a reviewed domain correction before freeze; runtime absence
does not silently shrink the manifest.

These generator strata are M9's discovery **bands**. Numeric diagnostic
bands remain useful reporting axes, but the fixed corpus has already closed
them and they do not describe what the fuzzer failed to generate. Before
implementation, preflight performs only a static candidate join: it maps
each stratum's witness shapes against the frozen D2 graph and existing B2
evidence to highlight potentially reachable `deferred` declarations and
unresolved property-dispatch candidates. It does not run generator-wide
instrumented Node/V8 tracing. Exact targeted D2 trace is allowed only after
a real divergence supplies a witness. This is a bounded survey of the
supported generated domain, not a blind re-audit of every tsc declaration.
The bands are quota and discovery-scheduling axes only; they never define
scope, an exclusion, acceptance credit, or an implementation owner. Once a
divergence exists, its exact diagnostic-D2 or pipeline-native owner—not its
domain band or diagnostic number—defines the implementation slice.

The acceptance work unit remains exactly 100,000 valid cases per UTC window:
ordinary completed comparisons (exact or divergent), tsrs terminal
divergences with a valid oracle result, plus any exact recorded M8
oracle-deviation outcomes. After bounded streaming lands, the hosted
resource pilot measures this work unit and all domain quotas on the standard
runner. M9's
optimization target is at most 120 minutes, but that unmeasured target is
not fabricated into the acceptance artifact. The frozen policy records the
observed optimized wall/RSS/disk values plus a reviewed margin as hard
ceilings. Missing the 120-minute target triggers performance/design review
before accepting a higher measured ceiling. Making the producer faster must
shorten the run; wall time is never a minimum to consume, and the frozen
ceiling cannot be loosened after seeing a qualifying result.

## 5. Fuzzer foundation and bounded execution

### 5.1 Producer boundary and fingerprints

Generator, comparator, reducer, classifier, replay, and raw window writing
live in the `tsc-rs-fuzz` crate and a dedicated producer binary. Xtask may
dispatch it, but unrelated xtask/completion/documentation bytes are not
producer inputs.

The **semantic fingerprint** includes:

- the checker and every relevant syntax/binder/types/diagnostics input;
- the exact oracle bundle, driver, Node pin, library set, and host adapter;
- generator, the exact corpus-mutation inputs, domain manifest, and
  option/topology adapter;
- comparator, renderer, reducer, classifier, deduper, and outcome schemas;
- the M9 history/attestation verifier and pinned trust policy;
- worker/process/timeout policy and scheduled workflow/attestation policy;
- fixed cases-per-window and domain quotas; and
- stable toolchain and runner-profile fields that can affect outcomes.

UTC slot, derived seeds, attempt/run id, timestamps, and output artifact
hash are **window identity**, not semantic inputs. Checked-in history,
class/witness registry, window summaries, close documentation, and `STAGE`
are verifier inputs but not producer inputs. Tests pin both sides so an
aggregation or close commit does not reset the streak, while any behavior-
relevant change does.

The stable runner profile is OS/architecture ABI, standard-runner class,
pinned Rust/Node/TypeScript toolchains, worker/process policy, and a pinned
container digest if one is adopted. Exact hosted-image revision, CPU model/
frequency, runner id, and available-core observations are diagnostic
metadata, not semantic fingerprint fields. Their normal rotation does not
reset 14 days; a stable-field change resets the streak, while an observation
outside the stable profile or resource ceiling invalidates that window.

### 5.2 Comparison and terminal outcomes

Every case records its real pass and compares both engines through T0-T4.
The first failing tier selects the diagnostic class. The terminal outcome
model distinguishes:

- ordinary exact completion;
- a tsrs panic/crash, timeout, OOM, or unsupported unwind;
- one exact row in the M9.0 frozen oracle-deviation registry derived from
  the M8 readiness contract;
- an oracle crash/timeout or malformed response;
- a generator/domain/harness failure; and
- controller/worker interruption.

A tsrs terminal failure with a completed oracle result is a divergence and
gets a replayable class/witness. An oracle crash counts only when real replay
matches one exact reviewed M8 deviation shape and the declared Rust-side
outcome; it is recorded as `recorded-oracle-deviation`, never as parity.
Any other oracle, generator, domain, harness, or controller failure makes
the window unsuccessful because no valid comparison exists. It cannot be
counted as an exact case or silently resampled.

Each raw execution is a versioned canonical envelope bound to the exact
canonical `CaseSpec` hash. It retains both engine observations or the typed
producer failure before any comparison or class projection. Authoritative
producer and verifier code validates/indexes `CaseSpec` once and atomically
derives that raw envelope and digest, the structured comparison, and the
canonical class from the same execution. Independently supplied or mixed
raw bytes, `Comparison`, and class values are not acceptance evidence.
Schema 1 bounds every diagnostic message-chain tree to depth 32 and 4,096
total nodes before recursive comparison/serialization. An adapter response
over either limit is a typed malformed response, never a partially compared
diagnostic.
Within one completed engine outcome, `renderer.assembled` is the exact
ordered, multiplicity-preserving projection of `diagnostics`; only the
renderer-owned `canonical_head` sidecar may be added. Sorting, deduplication,
and formatting begin after that bound input. Every final `deduped` row must
select an assembled row, but selection order and multiplicity remain raw
observations. This prevents two executions from being spliced into one
canonical envelope without validating away dropped, inflated, or reordered
final rows.

`fuzz replay` reconstructs the exact files, options, cwd, seed decisions,
and process policy, reruns both engines, and requires the saved comparator
and class. `fuzz reduce` repeatedly performs that real replay. A structural
or program-level reduction first removes files/options/declarations, then a
syntax-aware pass shrinks statements and expressions to a fixpoint. It must
preserve the exact class, terminal outcome, domain validity, and failing
comparator; checking two stored strings is not replay.

### 5.3 Canonical class

The versioned canonical class is the rate/deduplication key:

```text
schema + first failing tier-or-terminal phase
+ real pass-or-terminal sentinel + divergence side/outcome class
+ sorted one-sided multiset of (code, normalized message head)
  or closed terminal kind/boundary key
```

T4 adds the first renderer class in fixed precedence and the first affected
diagnostic key. Multiplicity is retained. The normalized head is computed
from the complete T2 record before any T0/T1 projection, so early-tier
classes do not collapse to an empty message. Virtual paths and
generator-owned identifiers are normalized by a versioned, one-way
raw-to-normalized algorithm. Schema 1 encodes every literal raw `<` as
`<<`; only a typed path or generator-identifier replacement emits a single
canonical `<@...>` or `<#...>` placeholder. A normalized string is never
fed back as raw input. This keeps literal text such as `<@2:0@>` distinct
from an owned path placeholder without excluding valid source or diagnostic
text. `CaseSpec` rejects an exact raw source claimed by two path/identifier
roles while allowing ordinary prefix overlap. Positions, seeds, timestamps,
and raw hashes do not enter the class.
The comparator evaluates tier before pass: T0 is a set, while T1-T3 are
complete multisets inside each T0 bucket. It computes the failing-tier
count difference before mapping surviving raw occurrences to their
position-free T2 heads and never re-differences those mapped heads. When
unequal heads compete for a smaller failing-tier residual, schema 1 first
cancels identical `(code, normalized head)` occurrences, orders the
remaining occurrences by numeric code and UTF-8 head bytes, pairs them in
that order across sides, and retains only the original count surplus. This
tie-break is part of the Rust/Node canonical-vector contract.
T0-T3 uses the real syntactic/semantic/suggestion pass. Pure T4 compares the
captured final deduped render sequence and uses the explicit
`pass=aggregate-render` sentinel; the pre-dedupe assembled sequence is
case-bound provenance, not the order/dedupe comparator input. Empty
rendered segments, dropped final rows, and inflated final rows remain
representable raw observations so the schema cannot validate a renderer
defect away. When no diagnostic/pass exists, the class uses
`pass=terminal`, a fixed `parse|bind|check|format` phase, and
`terminal kind + adapter-owned boundary_id`. The boundary is a schema enum
with a closed phase/kind
allowlist, not caller-provided text; volatile process text, paths, seeds,
timestamps, addresses, and hashes remain only in raw `detail` and never
enter the class. T4 tries
structured order, structured dedupe, whole-aggregate path normalization,
whole-aggregate newline normalization, then text in that fixed order. A
path- or newline-only segment does not outrank another residual text
difference when the whole aggregates still differ after that normalization.

Node/oracle fixtures and Rust production-path tests derive identical
classifier bytes from the same typed raw vectors for duplicate diagnostics,
pass separation, generated names, literal-placeholder collisions, paths,
every terminal outcome, and renderer precedence. The window verifier
recomputes classes and dedupe membership from raw outcome digests. A
producer-provided summary or a vector containing the same class twice is
not evidence.

### 5.4 Streaming and process lifetime

The nightly controller divides the deterministic case sequence into bounded
serial child shards. Concurrency is one producer child and one oracle/
renderer Node worker, launched with the frozen single-threaded Node/V8
flags, unless a separately reviewed performance experiment changes the
policy before freeze. Rust's internal worker/thread count is also capped.
Each child owns its Rust arenas/caches and one persistent Node worker, exits
after a fixed number of cases, and merges by case/seed identity rather than
completion order. The artifact records child and aggregate CPU time, peak
RSS, scratch bytes, and process rollovers in addition to wall time.

Successful cases stream compact canonical records directly to compressed
output:

```text
case id + generator decision/domain ids + options/topology
+ input/output digests + outcome + class membership
```

They are not retained in a 100,000-element in-memory vector and do not each
create a directory/program JSON tree. Full source, exact structured/rendered
outputs, process logs, and reducer state are retained only for a divergence
or terminal failure. Scratch space has a fixed upper bound and is removed on
success and failure.

The immutable compiler/library bundle may be shared inside one bounded
child. Generated programs, mutable checker state, and SourceFiles are not
shared across cases. If profiling justifies any broader lib/SourceFile
cache, its finite capacity and exact key—at least oracle/library hashes,
source text digest, script kind, and every source-file-affecting compiler
option—must be frozen in policy. A cold-cache/warm-cache diagnostic canary
must remain byte-identical, and a cache-policy change resets the semantic
fingerprint.

Cold/warm and process-rollover canaries must be byte-identical. Tests run the
same seeds with different legal shard boundaries and require identical
ordered digests, class membership, and witnesses. Per-engine deadlines kill
and replace the bounded child rather than leaving shared state behind.

The B2 full-corpus AST instrumentation sweep is not part of a nightly
window. M9 reuses its frozen D2 identities and invokes targeted trace only
after a witness exists.

## 6. Class, witness, recurrence, and owner triage

A canonical class is deliberately coarser than an implementation owner.
The registry therefore has four append-only levels:

1. **class** — immutable canonical bytes and first-seen observation id
   (burn-in campaign or qualifying window); this is the new-class-rate
   identity;
2. **witness** — exact canonical source/files/options repro hash (without
   class-level generated-identifier/path normalization), outcome/output
   hashes, reducer proof, and class membership; and
3. **incident** — one immutable discovery or recurrence event for one
   witness; and
4. **owner task** — one exact pipeline/D2/Rust semantic owner or fuzzer-
   producer owner boundary whose slice must resolve that incident.

An identical or different witness that recurs after an earlier incident was
resolved appends a new incident; simultaneous distinct witnesses also get
separate incidents. State changes are append-only transition events, never
field edits: discovery appends the incident, reviewed triage appends one or
more owner-task assignments, and resolution appends one event per owner
task. Current state is derived. An incident with no task is untriaged; one
with any unresolved task is open; only a non-empty all-resolved task set is
resolved. A class is open when any incident is untriaged/open. The
14-window rate counts distinct classes first seen in those windows, while
every task independently blocks zero-open.

Every owner-task assignment records:

- first failing pass, tier, side/outcome, and pipeline layer;
- an owner kind: `diagnostic-producer`, `pipeline-native`, or
  `producer-defect`;
- for a diagnostic producer, the A5 `(code, pass)` family, or the 2XXX band
  plus its exact producer cluster, exact tsc D2 declaration identities,
  source spans/hashes, SCC/static boundary, and emitting/non-emitting probes;
- for a terminal/no-diagnostic, parser/binder/program, or pure-T4 incident,
  its exact phase/domain owner, tsc control-path declaration(s) where
  applicable, and the tsrs-native or port-ledger Rust path/function/hash;
- for a tsrs-only row, the exact Rust emitter plus the corresponding tsc
  non-emitting control/static owner;
- for a `producer-defect`, the exact generator/domain-validator/oracle-
  adapter/harness/controller/comparator/reducer/classifier/registry-history
  path, function/schema/hash, the independent low-level observation that
  disproves the original classification, and the adversarial canary that
  prevents recurrence;
- exact Rust ledger boundary and any unresolved property-call candidates;
  and
- the regression fixture and acceptance transition that will prove
  resolution, or the explicit A1-not-applicable producer-defect proof.

Printed function names, diagnostic code alone, a canonical class alone, or
a moving “top signature” count are not owner identities.

If fuzzing introduces a previously unexercised non-2XXX `(code, pass)`, its
D2 emitter supplies a provisional owner. The resolved regression fixture
adds that row through an A5 universe extension. A resolved witness graduates
to the append-only conformance universe only after the implementation is
exact; after A2 global freeze the new fixture cannot introduce an exclusion.
Semantic resolution requires the real replay to pass, all incidents in the
owning slice to pass, every owner task for the incident to be resolved, and
the A1 transition to accept the T0-T4 regression.

If later adjudication proves that a generator, domain validator, oracle
adapter, harness/controller, comparator, reducer, classifier, or registry/
history defect fabricated the apparent semantic class, the original class/
witness/incident is never deleted or rewritten. Append a `producer-defect`
owner task and resolution citing the producer fix, independent raw
observation, exact replay, and an accepted adversarial canary; A1 is
explicitly not applicable. Any affected burn-in or qualifying window is
append-only invalidated, the semantic fingerprint changes, and qualification
restarts. A failure already classified at execution time as generator/
domain/harness/controller failure creates no semantic class and simply
invalidates its window.

## 7. Divergence implementation loop

M9 retains the owner strategy that closed 2XXX, M7, and M8:

```text
exact witness
-> first failing pass/tier/outcome and pipeline layer
-> A5/2XXX diagnostic family or exact pipeline-native owner
-> exact D2 emitter(s)/trace/non-emitting sibling when diagnostic-producing
-> static dependency closure/SCC
-> exact Rust boundary
-> one dependency-complete owner slice
-> regression fixture + A1/A5 transition
```

One owner family and dependency-closed cluster equals one PR. Several
classes/witnesses may share a PR only when the exact D2 closure and missing
semantic representation are the same. One class with two owners is two
slices represented by two owner tasks; the incident cannot resolve until
both close.

Before selecting a semantic owner, reproduce the structured observations
below the canonical classifier. If those bytes disprove the class, follow
the `producer-defect` disposition in §6 instead of porting checker behavior.

Before editing, the slice ports or probes the cited tsc 6.0.3 control flow;
expected diagnostics are never inferred from memory. Focused real replay,
reduction, unit pins, and the target regression are the editing loop.
Fixed-seed/domain smoke and the complete local CI run once on the clean
candidate branch are the merge evidence. A qualifying window is never an
editing-loop command.

### 7.1 Full-port trigger

Stop local symptom repair immediately—without waiting for three failed
patches—if any witness shows that:

- an observable tsc AST field/node, symbol, type, signature, flow, or
  program state has no Rust representation;
- a checker/binder-side semantic source-text rescan or projection outside
  the syntax owner, shadow parser, transient semantic object, hand-built
  diagnostic chain, or local activation guard would be needed;
- parser/binder/checker activation order changes real symbols, relations, or
  diagnostics; or
- two independent witnesses converge on the same absent representation or
  subsystem boundary.

The fallback trigger remains three probes/fixes hitting one unexplained
model ceiling. At either trigger, freeze the local patch, inventory the
complete supported-batch tsc dependency surface, and design one coherent
subsystem port. Dependency-ordered commits may exist on its branch, but a
partially active semantic subsystem does not merge. The complete JSDoc
parser/arena/binder/checker port is the precedent.

This does not prohibit the scanner/parser from reading source text to
construct the canonical AST. It prohibits a downstream semantic owner from
re-parsing or reconstructing syntax/state that should have been represented
by the parser, arena, binder, type, flow, or program model.

## 8. Nightly evidence, attestation, and CI cadence

The frozen policy derives every qualifying seed from:

```text
semantic policy fingerprint + UTC slot + shard id
```

A workflow input or maintainer cannot choose a qualifying seed. There is at
most one finalized slot per UTC date, and only the scheduled workflow's
attested `run_attempt == 1` may qualify. A failed, cancelled, or interrupted
first attempt breaks that date and therefore the consecutive streak.
Reruns use the same slot/seeds for diagnosis or burn-in but are always
non-qualifying. This rule lets the checked-in attestation prove offline that
a successful retry did not hide an artifact-less earlier failure.

A **successful window** means the controller completed exactly 100,000
valid cases (ordinary exact/divergent comparisons, tsrs terminal
divergences with a valid oracle, plus any explicitly recorded M8
oracle-deviation outcomes), every domain quota and uniqueness floor passed,
resource ceilings held, and raw-to-summary verification succeeded.
Discovering a divergence does not make infrastructure evidence disappear:
the class and incident are appended and block zero-open. An unknown
oracle/domain/harness/controller failure makes the window non-qualifying.

The scheduled producer runs only from protected `main` on a standard public
runner. It has read-only repository contents and the minimal OIDC/
attestation permissions needed to produce a GitHub artifact attestation;
there is no long-lived signing secret and no paid larger runner. The policy
pins repository identity, workflow path/hash, scheduled event, relevant
input fingerprint, runner profile, artifact digest, and attestation
authority, including `run_attempt == 1`. PR/manual/rerun jobs cannot mint a
qualifying window.

The attested subject is the compact raw window bundle. Each accepted slot is
checked in as `ratchets/fuzz-windows/<UTC-slot>/window.v1.json.zst` beside
`attestation.v1.json`, the complete signed statement/transparency bundle
whose subject digest must equal that window file. An aggregation command
verifies the sidecar against the frozen repository/workflow/authority policy,
recomputes every digest/class/quota and history edge, then appends canonical
window, class, witness, and incident records. It never reruns the long
producer or edits an old record. Several independently attested consecutive
windows may share one aggregation PR so local full acceptance and the hosted
PR guardrail are not repeated every night. Temporary hosted artifacts are
retained only long enough for verified aggregation; durable compact records,
attestation sidecars, and
divergence repros live in-repo.

Append-only authority is the decompressed canonical record bytes and their
previous-record hashes, not incidental compressed-container bytes. The
Node-free steady-state verifier rechecks the checked-in attestation bundles,
lineage, raw-to-summary calculations, relevant-input fingerprints, class/
incident state, and current policy.

After M9.2 lands and before M9.3 changes the CI/schema consumer, hosted
calibration freezes a bounded PR-smoke case count, exact seed/domain-canary
list, and wall/CPU/RSS/scratch ceilings. The list does not grow implicitly
with the nightly manifest. After that transition, the bounded hosted PR
guardrail executes it exactly once: one versioned artifact supplies both the
M8 B3 readiness projection and the M9 domain/classifier/replay/reducer/
registry checks. The legacy M8 smoke is retired at that schema transition and
is not run beside it. The 100,000-case producer is scheduled-only. The final
release job verifies the 14 checked-in windows; it does not create a
fifteenth.

After a candidate PR is pushed, its branch remains fixed while Actions
runs. Investigation and read-only preflight for the next owner slice may
continue in a separate worktree/branch. Do not push unrelated follow-up work
to the in-flight branch and restart its CI; do not run two local
full-corpus/Node-heavy gates concurrently. No implementation commit may
depend on the unmerged candidate. After it merges, create or rebase the next
slice on the latest `origin/main` and rerun its focused evidence before
editing.

## 9. Burn-in, freeze, qualification, and close

### 9.1 Non-qualifying burn-in

Burn-in runs the complete frozen-candidate domain across rotating seed
shards without creating qualifying history. Every discovery is registered,
reduced, split into its exact owner tasks, fixed through the §7 loop, and
promoted to a regression fixture. A checker/generator/policy correction may
freely reset burn-in evidence.

After the last behavior-relevant change, the final candidate fingerprint
must complete at least one non-qualifying 100,000-valid-case work unit with
every nightly domain/cross-domain quota, uniqueness floor, and resource
ceiling satisfied. That final campaign must discover zero new canonical
class and finish with zero untriaged incident and zero unresolved owner
task. A shorter shard, aggregate of partial runs, or campaign from an older
candidate fingerprint cannot satisfy the burn-in-zero record.

M9.5 freeze requires:

- the preflight gap/domain/resource report green;
- true replay/reduction and every adversarial test green;
- the bounded producer meeting the reviewed 100,000-case wall/RSS/disk
  ceilings established by the hosted calibration;
- scheduled production, attestation, aggregation, history, recurrence, and
  completion verification proven end to end with non-qualifying canaries;
- all close-path code, including parsing/auditing a future `STAGE=M9`,
  landed while the actual marker remains `M8`;
- the final full 100,000-case burn-in-zero record green, all earlier burn-in
  incidents resolved, and their regression fixtures accepted;
- completion row 11 data-driven and red only because qualifying history is
  shorter than 14; and
- an exact reviewed semantic fingerprint and policy snapshot.

### 9.2 Qualifying streak

`fuzz steady-state --require-ready` requires the last 14 UTC slots to be
consecutive and, for each slot:

- exactly 100,000 valid cases within the frozen measured wall ceiling;
- every frozen domain quota, uniqueness floor, process/resource bound, and
  attestation rule green;
- the same current semantic fingerprint and policy;
- one non-overlapping deterministic seed range; and
- complete append-only raw/class/witness/incident membership.

Across those windows, distinct newly discovered canonical classes divided
by 14 must be less than 1.0, and the complete registry must have zero
untriaged incident and zero unresolved owner task.

In practice a real discovery stops final qualification: fixing it and adding
its regression changes the semantic fingerprint, so the current 14-window
streak restarts at day one. The `< 1/window` rate is an engineering
measurement, not permission to defer known work. Missing, failed,
under-budget, over-time, overlapping, stale, manually seeded, rewritten, or
unattested slots break the streak.

### 9.3 M9 close

Appending the fourteenth valid window must make row 11 green without a code
change. The close candidate first updates `STAGE` to `M9` and refreshes only
the close documentation/generated status that was proven outside the M9
semantic fingerprint. The final release job then runs on that exact
candidate tree:

1. verifies M9 history/attestations, zero untriaged incident, and zero
   unresolved owner task;
2. regenerates current B1-B4 evidence;
3. runs every full-corpus invariant;
4. runs `cargo xtask completion --require-done`; and
5. records all 11 rows green in one workspace.

The close merges only after that exact `STAGE=M9` tree is green. This order
ensures completion row 10's invariant attestation fingerprints the final
stage marker rather than the preceding M8 tree. The release verifier
consumes the existing 14 windows and never spends another long window merely
to close.

## 10. Per-owner review template

Every semantic M9 PR records:

```text
Canonical class and exact witness/incident ids:
First failing pass, tier, side/outcome, and pipeline layer:
Generator domain strata and seed/replay hashes:
A5/2XXX diagnostic owner or exact pipeline-native owner:
Exact D2 declarations/SCC where diagnostic-producing:
Emitting and non-emitting probes:
Static closure and exact Rust ledger boundary:
Vendored tsc spans/hashes:
Subsystem completeness inventory:
Witnesses/incidents before -> after:
Regression fixture and A1/A5 transition:
Accepted identities lost: 0
All-corpus FP: 0
Focused replay/reducer/pins and full CI:
```

Infrastructure-only slices use the same template with semantic fields marked
not applicable, but must include fingerprint before/after, adversarial-test
ids, resource observations, and why no qualifying history can be minted
early. A Markdown-only contract slice with a byte-identical generated README
`STATUS` block instead follows the repository's documentation-only rule:
diff/link/anchor/generated-block validation only, with no Cargo, Node, B2,
or full-corpus CI. A workflow, policy, schema, artifact, executable, or
generated-status change is not documentation-only.

## 11. Stop conditions

Stop and review M9 design if:

- a valid generated case would need a new A2 exclusion;
- a class cannot be recomputed from raw evidence or a witness cannot truly
  replay/reduce;
- pass, multiplicity, terminal outcome, recurrence, or distinct owners
  collapse into one mutable record;
- a seed, failed attempt, out-of-domain case, quota, or resource failure can
  be discarded while the window still succeeds;
- a long producer keeps all cases in memory, creates unbounded scratch, or
  starts a Node process per case;
- the 100,000-case pilot cannot meet the frozen standard-runner budget;
- an owner cannot be named by exact diagnostic-D2 or pipeline-native Rust/
  tsc identities;
- a full-port trigger in §7.1 fires;
- an aggregation/history/close-only commit resets the semantic fingerprint,
  or a behavior change does not reset it;
- a PR or manual workflow can mint qualifying attestation; or
- emit, host, LSP, public-API, or upstream-version work is proposed as an M9
  divergence fix.

Hard or slow implementation alone is not a reason to weaken the domain,
case count, identity, or zero-open gate.

## 12. Separate follow-on tracks

M9 completes only TypeScript 6.0.3 batch-diagnostics confidence. JavaScript
and declaration emission, LSP/watch/incremental operation, general
filesystem/package hosts, a public `TypeChecker` API, and newer TypeScript
versions retain their separate design/goal/compatibility/evidence contracts.
No M9 artifact reserves acceptance credit for them.
