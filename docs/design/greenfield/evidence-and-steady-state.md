# Evidence and M9 steady-state contract

Status: normative support contract for B1-B4 and M9.

This document owns how runtime, fuzz, performance, and nightly evidence
is produced and verified. The
[completion convergence plan](completion-convergence-plan.md) owns
landing order; [measurement-integrity.md](measurement-integrity.md)
owns shared history-anchor and identity rules.

## 1. B1 — one evidence protocol

`tsrs2/m8-evidence.json` contains producer configuration, reviewed
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
incomplete candidate is regenerated. B3 and B4 are always produced, and
the common manifest is always rewritten from the artifacts actually
consumed. A fresh local clone regenerates B2; hosted CI may restore the
exact content-addressed B2 artifact populated by a prior successful run.
M9 history is separately versioned in-repo.

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
cargo xtask fuzz run --seed <u64> --cases <n> --artifact <path>
cargo xtask fuzz replay <case>
cargo xtask fuzz reduce <case>
cargo xtask fuzz nightly --policy ratchets/fuzz-steady-state-policy.toml
cargo xtask fuzz steady-state [--require-ready]
```

Use grammar-aware generation and corpus mutation, including compiler
options, multi-file structure, checked JavaScript, and JSDoc syntax and
semantics. The generated domain is the supported batch checker. Filesystem
package-host resolution outside the in-memory program model, project
references, and emit-dependent behavior are excluded by construction.
Generating an out-of-domain case fails the window; it is not silently
discarded.

Every case runs tsrs and the pinned oracle through T0-T4. The canonical
signature is:

```text
schema + tier + pass + divergence side/class
+ sorted one-sided multiset of (code, normalized message head)
```

The normalized head is the exact first T2 message line after virtual-path
and LF normalization. T4 adds the first applicable renderer class in
fixed precedence `order`, `dedupe`, `path`, `newline`, `text`, plus the
first affected diagnostic key. Paths, generated names, positions, seeds,
and raw hashes do not enter the signature. Exact outputs remain in the
repro artifact. Renderer class is derived by comparing the structured
pre-render diagnostic sequence before falling back to `text`. Node and
Rust golden tests pin classifier and signature bytes.

Minimal reproducers deduplicate by signature. Reduction must retain both
that signature and the exact failing comparator. PR CI runs fixed-seed
smoke; scheduled CI rotates seeds. If every generated case is already
oracle-exact, PR CI records zero natural divergences and exercises the same
reducer/deduper with a separate deterministic one-sided mutation canary.
The canary is identified in the artifact and is never inserted into the
generated case observations. M8 readiness requires non-zero cases, equal
generated/compared counts, reducer smoke, and dedupe evidence with the
current B1 fingerprint.

### 3.1 M9 steady state

Three versioned artifacts make the final rate gate executable:

- `ratchets/fuzz-steady-state-policy.toml` fixes 14 consecutive UTC
  windows and the per-window minima of two hours and 100,000 completed
  cases, plus exact generator domains and the CI attestation key/policy.
  These three thresholds are contract constants, not tunable defaults.
  Any other valid policy change changes the fingerprint and restarts the
  streak.
- `ratchets/fuzz-nightly-history.v1.json.zst` appends non-overlapping
  window records: times, seeds, cases, runtime, input fingerprint,
  artifact hash, and new signature ids. It uses append-only lineage and
  trusted-base comparison. Prior rows remain byte-identical.
- `ratchets/fuzz-signatures.v1.json.zst` appends canonical signatures,
  first window, repro hash, owner, and state. Entries are never deleted.
  Only `open -> resolved` is allowed, with a cited conformance universe
  transition and A1 acceptance; evidence cannot be replaced.

Each history row references a compact checked-in artifact under
`ratchets/fuzz-windows/` containing per-seed outcome digests,
failure/signature membership, aggregates, and repro hashes. Scheduled CI
signs its manifest. Aggregation verifies repository/workflow identity,
producer commit, current input fingerprint, artifact hash, signature,
and raw-to-summary recomputation before appending it. An unsigned or
hand-authored row cannot count.

`fuzz steady-state --require-ready` requires:

1. all policy, history, signature, and window artifacts verify;
2. the last 14 windows are consecutive, current-fingerprint, successful,
   non-overlapping, and each meets the two fixed minima;
3. `distinct new signatures / 14 < 1.0` from raw membership;
4. the signature registry has zero open entries.

A checker, oracle, generator, reducer, signature schema, policy, or
attestation-key change resets the streak. A missing, failed,
under-budget, overlapping, stale, rewritten, or unsigned window breaks
it. A docs-only change outside the fingerprint does not.

## 4. B4 — performance and RSS

`cargo xtask perf conformance --artifact <path>` launches conformance as
a child and records raw wall time and maximum RSS. Configuration
enumerates approved runner profiles including OS/architecture, CPU/core
policy, memory, and measurement backend. A machine name alone is not an
approved profile.

The fingerprint pins executable, full-corpus command/options, immutable
oracle inputs, toolchains, and runner profile. The wall ceiling is at
most 60 seconds. The first RSS ceiling comes from measured evidence plus
a reviewed margin. Readiness and completion require the observations,
not only configured ceilings, to pass on an approved profile.

The ceiling-bearing full-corpus observation uses the normal bounded
lib-bundle cache. Also run a separately recorded
`TSRS_LIB_BUNDLE_CACHE=0` smoke in a short-lived child with an explicit
fixture limit; its purpose is to exercise the no-reuse path without
letting the batch checker's intentionally leaked lib bundles grow once
per corpus case. The smoke cannot replace the full-corpus wall/RSS
observation. Give fuzz and coverage workers explicit process lifetimes
for the same reason, and keep coverage concurrency at the configured bounded
worker cap.

## 5. Required CI topology

Required PR CI:

- fetches enough history for every A1/A2/A5/M9 anchor;
- runs recursive and trusted-base integrity checks;
- runs the permanent syntactic and ordinary conformance gates;
- builds once, verifies/reuses or produces B2, produces B3-smoke/B4,
  and invokes M8 readiness in that workspace;
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
  consumption;
- a final job named `gates` succeeds only when both lanes succeed.

Thus no evidence producer/consumer or A1/A2/A5 ordering crosses a job
boundary. The ordinary local `cargo xtask ci` remains the sequential
union of both lanes and is still the required pre-PR/pre-merge gate.
Main-branch runs populate the cache scope that later pull requests may
restore. Lockfile-keyed Cargo caches contain dependency archives only;
a pinned content-addressed compiler cache handles build outputs without
trusting checkout timestamps. A separate exact-fingerprint cache may
contain only the B2 raw runtime artifact; the semantic lane revalidates
it and rewrites the manifest. Conformance, readiness, B3, B4, and other
semantic evidence artifacts are never restored from that cache.

Scheduled CI runs the two-hour/100,000-case fuzz window and retains raw
output. A reviewed aggregation verifies and appends it without rewriting
history.

The final release job uses the approved performance runner, regenerates
B1-B4 evidence, runs full-corpus invariants, verifies M9 history, and
then runs `cargo xtask completion --require-done` in the same workspace.
Gate logic stays in local commands; YAML only executes it.

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
- an out-of-domain generated case, unequal comparison count, signature
  classifier drift, or reducer changing signature fails;
- a deleted/rewritten/unsigned/stale/under-budget nightly window or a
  signature entry deletion/reopen fails the M9 lineage check;
- a policy encoding thresholds other than 14 windows, two hours, and
  100,000 cases fails before evaluating history;
- an unapproved runner, declared-but-unobserved ceiling, wall over 60 s,
  RSS over its ceiling, or producer fingerprint mismatch fails.
