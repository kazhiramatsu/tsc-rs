# new-CI Phase 0 — current-oracle shadow packet (lane 3)

Status: **NORMATIVE DESIGN** for amendment item 3's pre-H2.9 maximum. Parent
authority is `new-ci-evidence-dag.md`; completed M1-M3 is consumed, not redesigned.

## 1. Identity and boundary

Phase 0 is a REPORT-ONLY adapter over the current H2 oracle. That oracle remains
the sole authority; shadow output is evidence about, never accepted from, a successor.

- Phase 0 adds zero `crates/*.rs` bytes and incurs zero oracle-ladder byte tax.
- The adapter remains under standalone `new-ci/`; it is not a root-workspace member
  and root Cargo commands do not build it.
- Scheduled runs use the canonical clean `main` checkout after its landing gate and
  walk release the machine. They never occupy the gate's critical machine window.
- No local gate, hosted check, ratchet, qualification policy, walk, or merge decision
  may use a Phase 0 report, receipt, severity, or exit status for pass/fail.
- Shadow exit status is consumed only by its scheduler after status is recorded;
  it is never forwarded to a gate. Failure to record is a scheduler incident.
- Evidence is uncommitted under `target/new-ci-shadow/`: `RUN` is
  `runs/<run-id>/report.{json,md}`, `REVIEW` is `review/<run-id>.json`, and
  `INCIDENT` is `incidents/<run-id>/`. There is no mutable `latest` identity.

## 2. Current-oracle adapter

### 2.1 Dynamic discovery

Every run inventories the checked-out tree; it carries no H2.9-final rung array.

1. Enumerate every `crates/oracle/h2-*.mjs` file in byte-sorted path order.
2. Classify each on-disk contract as producing rung, checked sidecar, imported
   helper, or restricted producer. Producers declare artifact and check entrypoint;
   helpers become implementation inputs rather than silently disappearing.
3. Union declared outputs, extracted pins, and checked-in `ratchets/h2-*.json`.
   Each artifact has one producer; orphans, duplicates, or an executable with no
   understood declaration are topology/schema drift.
4. Build M1 typed-pin/producer edges and topologically sort. Parse the canonical
   walk's `ORDER` only as a cross-check; neither it nor `plan.rs`'s frozen array is
   copied into the adapter.
5. A new rung appears in the next inventory without an adapter edit. Unknown
   grammar is visible drift, never permission to omit the rung.

Owner controls are checked sidecars. `h2-baseline.mjs` and future approved-runner
producers are restricted: observe bytes/certificate, but do not invoke them.
Non-producing helpers bind into the importing action's implementation digest.

### 2.2 Projection into the M1-M3 contract

Each discovered check is projected without changing the substrate types:

- `Action.tool` is `node-current-oracle`; version binds `.node-version` and the
  selected runtime. Definition binds normalized `--check` argv, root cwd,
  environment allowlist, timeout, child policy, artifact/verdict kind, and schema.
- Implementation binds the script with only M1-classified pins masked plus loaded
  helpers. Each mask remains a labelled envelope term; unclassified path-adjacent
  digests make the action uncomparable and are never guessed.
- `SemanticInputManifest` binds source tree, declared non-produced inputs,
  configuration/schema, output-affecting platform facts, and checked-in target
  consumed by `--check`. Paths label canonical byte identities.
- Produced ratchets/scripts are labelled `DependencyOutput` edges: evidence uses
  `core`; pins, lineage, and fingerprints use `envelope`. Baseline is explicitly
  `Some(run source-root digest)`, not a branch or guessed H2.9 root.
- `ReceiptKey` uses the exact parent formula. Workspace path, run ID, time, PID,
  retry, priority, and shard layout remain execution-only.

`StatusReceipt` records each attempt; only both-projection-verified bytes carry
verified success. Failures, skips, timeouts, and comparisons are diagnostic.
`TransactionManifest` closes the observed run only for report integrity: its
generation is not trusted and cannot satisfy another cache lookup in Phase 0.

### 2.3 Observation record

Every sampled or full run records:

- source commit/tree, full inventory, and action/manifest/edge/key/comparator digests;
- exact artifact digest/size, canonical semantic digest when defined,
  `core`/`envelope` digests, and schema verdict;
- invoked `--check` exit class, typed machine verdict, stdout/stderr digests,
  elapsed time, and verified/diagnostic status;
- the exact `target/chain-walk/converged-run-id` certificate, never `runs/latest`:
  `converged-crates.sha256`, `summary.log`, rung logs, qualification verdicts,
  overrides, rounds, and minted set in `runs/<certificate-id>/`;
- selected/completed/skipped/certificate-only rungs, wall time, byte counters,
  report bytes, and maximum child concurrency.

The certificate crate-tree must match. Samples hash the entire graph but invoke
only their sample; full runs classify every producer, including certificate-only.

## 3. Comparison rules

Byte equality is recorded first; semantics never erase the raw digest.

| Artifact kind | Byte rule | Semantic rule |
|---|---|---|
| Oracle script/action | Preserve raw digest | Mask only typed pin spans; compare implementation `core` and ordered pin `envelope` separately |
| JSON ratchet/evidence | Preserve exact UTF-8 bytes | Validate the declared schema, sort object keys, preserve array order and JSON number/string types, then compare canonical values |
| Opaque or unknown artifact | Exact bytes required | No normalization; a comparator must be designed before semantic equality exists |
| Check verdict | Logs are provenance, not equality | Compare `(rung, artifact digest, exit class, machine verdict, term)` |
| Walk certificate/log | Preserve every file digest | Compare source/crate root, discovered coverage, rung verdicts, rounds, minted set, overrides, and final-green state |

EXPECTED fields require comparator-schema locations: timestamps, run/certificate
IDs, PID, worker/host, absolute workspace/temp-log path, attempt, duration,
priority, and shard/batch layout after its ordered union agrees. JSON whitespace
and object-key order may be serialization-only; typed pin-only rewrites may alter
the envelope with equal implementation core. Log prefixes may carry those fields.

There is no global name ignore. Arrays sort only when schema-declared sets;
semantic timestamps, roots, case IDs, counts, verdicts, hashes, overrides, and
tool versions are never expected. A new ignored pointer changes comparator schema.

| Class | Named severity | Definition | Destination |
|---|---|---|---|
| `BYTE_EQUAL` | `CLEAN` | Raw bytes and semantic projection agree | `RUN` summary |
| `SEMANTIC_EQUAL` | `EXPECTED` | Core agrees and every byte/envelope difference is allowed above | `RUN` expected-difference ledger |
| `BUDGET_SKIP` | `INCOMPLETE` | A selected observation did not run because a hard budget or machine window closed | `RUN` + `REVIEW` |
| `SCHEMA_DRIFT` | `BLOCKER` | Parse/schema/declaration/comparator version is unknown or invalid | `RUN` + `REVIEW` schema queue |
| `BYTE_DRIFT` | `BLOCKER` | Semantic equality is unavailable, or byte drift exceeds its allowlist | `RUN` + `REVIEW` |
| `CONTENT_DIVERGENCE` | `INCIDENT` | Canonical evidence content differs | `RUN` + `INCIDENT` bundle |
| `VERDICT_DIVERGENCE` | `INCIDENT` | Current-oracle and projected typed verdicts disagree | `RUN` + `INCIDENT` bundle |
| `MISSING_RUNG` | `INCIDENT` | Script, declared artifact, graph row, check row, or certificate row is absent/duplicated | `RUN` + `INCIDENT` bundle |
| `STALE_RECEIPT` | `BLOCKER` | Recomputed key/output/root disagrees, success is unverified, or the receipt predates its pinned source/certificate | `RUN` + `REVIEW` receipt queue |
| `CERTIFICATE_DIVERGENCE` | `INCIDENT` | Certificate root, coverage, override, or final-green claim cannot be reconciled | `RUN` + `INCIDENT` bundle |

Every non-clean row names rung, both digests, first semantic/key difference,
reason path, comparator, and reproduction. `BLOCKER`/`INCIDENT` block only
promotion qualification; they cannot turn a Phase 0 gate red.

## 4. Budget and schedule

Measured inputs: the 9,027-case 5g observation cost 89 minutes; a crate-byte
landing costs 50-95 minutes; about 10 minutes/gate is gt7's usefulness bar.
Phase 0 gets a short post-merge slot or reserved full window, never a landing.

| Run | Trigger and frequency cap | Hard wall | Work cap |
|---|---|---:|---|
| Sample | 15 minutes after each `main` merge; coalesce within 60 minutes; at most 4 per rolling 24 hours | 10 minutes total | entire inventory; at most 8 invoked rung checks; one child |
| Full | Tuesday and Friday at 02:00 Asia/Tokyo; at most 2 per rolling 7 days | 100 minutes total | every dynamically discovered report-safe check; one child |

Changed items and their one-hop graph enter first; a commit-digest rotation fills
slots. Over-eight cones partition across regular samples with explicit unobserved
rows; no catch-up job is created.

Before launch/between children require the intended canonical clean checkout and
no gate, walk lock, performance job, or critical owner. If one appears, wall
expires, or frequency caps, stop the child, close `BUDGET_SKIP`, and drop the
trigger. Never queue, retain a gate lock, extend the window, or delay the gate;
retry only on the next regular trigger.

The adapter never initiates 5g observation. It uses the same-tree canonical walk
certificate/outcome; missing or hit-ineligible means incomplete/stale. Only
canonical `scripts/chain-walk.sh` may authorize observation, never an ahead/Phase 0
worktree.

## 5. Promotion and rollback policy

This policy creates eligibility only. Decision/enforcement need a post-H2.9 packet.

### 5.1 Eligibility window

Before ANY trust promotion, Phase 0 must complete a continuous 14-calendar-day
window; it may start earlier, but only a post-H2.9 report may bind the stable root:

1. At least 12 clean samples over eight `main` heads and four owner-reviewed full
   sweeps. Every producer is covered by all four; the final sweep binds the stable
   H2.9 root.
2. Zero unexplained `BLOCKER` or `INCIDENT` rows. Every `EXPECTED` row has an
   existing comparator rule and review provenance. A real divergence or its
   fix restarts the 14-day clock at the next clean report.
3. The full retained incident registry replays. Minimum cases are `a8aa644b`
   (44/44 envelope-only, 9,027-case consumer HIT, and printer-core invalidation),
   `e8e32f61` (1,124/1,124 indexed pin changes), `e1957f77` (6/6), and
   the gate-tax-4 event (55/61 stale ladder rungs and 6/6 named surfaces,
   precision/recall 1.000/1.000).
4. Seeded fixtures exercise every non-clean taxonomy class and its destination;
   forged outputs, torn records, CAS races, stale-owner fencing, and both
   transaction crash windows retain the M3 adversarial coverage.

### 5.2 Reconciliation checklist

The README caveats are promotion prerequisites, not historical notes:

- replace the local SHA-256 with `sha2`, cross-check old/new vectors and stored
  identities, and define a versioned migration rather than reinterpreting keys;
- implement and adversarially verify the digest-addressed blob CAS, quotas,
  complete GC roots, leases, retention grace periods, and recovery; the no-op
  GC-root stub is not trusted storage;
- implement remote object/receipt paths with digest verification, signatures,
  attestation, trusted/untrusted namespaces, poisoning tests, and no PR write
  path into trusted state;
- reconcile `README.md` and `STATUS.md` line by line, wire the modeled 5g
  observation through the canonical contract, and reverify M1-M3 against the
  final production dependencies.

No checklist item may be waived merely because the shadow window is clean.

### 5.3 Rollback triggers and action

After a future promotion, any false HIT, current-oracle/shadow semantic or
verdict incident, missing rung, accepted stale/forged receipt, digest/key
collision, corrupt transaction accepted as complete, lease/fencing/CAS
violation, provenance/signature/namespace breach, two consecutive incomplete
full sweeps, or seven days without one clean full sweep triggers rollback.

Rollback occurs before the next trusted decision: atomically select the last
immutable current-oracle root, disable successor reads/writes/uploads, and run
the current oracle canonically. Preserve the newer receipts and incident
bundle for reconciliation; do not rewrite or delete either generation. Resume
requires incident replay, a new clean eligibility window, and a new explicit
promotion decision. During Phase 0 the equivalent trigger only stops the
qualification clock and routes a report, because no trust has moved.

## 6. Acceptance for landing Phase 0

Phase 0 is LANDED only when all are true:

1. The adapter is installed on the sample/full schedule above and its schedule
   wrapper is demonstrably invisible to gate pass/fail.
2. Dynamic-discovery tests add and remove a synthetic rung/artifact without a
   code/list edit and obtain the corresponding graph/topology row.
3. **N = 10** consecutive clean sampled reports cover at least five distinct
   merged `main` heads, remain inside the 10-minute budget, and account for
   every discovered item as invoked, certificate-only, or intentionally
   unselected.
4. One complete full-sweep report accounts for every discovered producer,
   stays inside 100 minutes, and has its report digest recorded by the CI
   evidence owner after review.
5. At least one isolated seeded `CONTENT_DIVERGENCE` reaches `INCIDENT` with
   reproduction data while the gate remains green and no oracle/ratchet byte
   changes.
6. The final diff proves zero `crates/*.rs` bytes, no workspace membership,
   no gate consumer, and no committed generated report.

## 7. Prohibitions

- No trusted promotion, trusted cache read/write, or enforcement before H2.9
  and the complete policy in section 5; eligibility never promotes by itself.
- No hosted cache authority, remote trusted namespace, or PR-to-trusted
  promotion during Phase 0.
- No tsgo producer, stable H2.9-root comparison, public API, or LSP work during
  Phase 0. All remain post-H2.9 reservations.
- No gate coupling: no gate, walk, hosted check, ratchet, or merge policy may
  consume shadow output or wait for its scheduler.
- No oracle/ratchet mutation and no oracle `--write`; the current oracle is
  observed read-only.
- No 5g observation re-run from a non-canonical path, ahead worktree, or
  shadow command. Existing canonical certificates are the Phase 0 source.
- No root-workspace membership, root Cargo dependency, `crates/*.rs` byte, or
  hardcoded final rung/artifact list.
- No remote upload, mutable `latest` key, silent comparator fallback, guessed
  pin masking, or conversion of a diagnostic status into a cache HIT.
