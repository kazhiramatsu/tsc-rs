# Completion convergence plan

Status: active execution plan. Adopted after the 2026-07-16
full-project review.

This document sequences the work needed to make tsrs2 not only
implementable, but mechanically certain to converge on its normative
completion contract. [definition-of-done.md](definition-of-done.md)
remains the authority for WHAT Done means. This document owns the order,
artifacts, and acceptance gates for getting there. Existing M4-M9 step
docs continue to own TypeScript semantics inside their stages.

The plan has one governing rule:

> Correctness progress is a growing set of proven matches, not a growing
> integer that can exchange one correct diagnostic for another.

## 1. Starting point

Baseline at `main` commit `52c47bbb` (after M4 5.9c):

| Signal | Baseline |
|---|---:|
| All-corpus T0 | 20,052 / 48,719 (41.1585%) |
| All-corpus T1 / T2 / T3 shadow | 41.1523% / 38.3177% / 37.0820% |
| All-corpus FP / FN | 0 / 28,667 |
| 2XXX T0 | 10,921 / 20,916 (52.2136%) |
| 2XXX T3 shadow | 47.0214% |
| Relation pins | 415 / 415 |
| Escape sites | 258 (stale 0, untagged 9, recovery 112) |
| M8 readiness | 2 / 9 gates ready |
| Rust function dispositions | 273 unresolved |
| tsc dependency-closure functions | 5,204 unaccounted |
| Direct emitter runtime coverage | 0 / 607 |

The corpus also contains 48,821 raw oracle diagnostic records but only
48,719 T0 buckets. Sixty-eight T0 buckets in 58 cases contain multiple
diagnostics, accounting for 102 records beyond the T0 set. Any
diagnostic-level scope mechanism must preserve this multiplicity.

These numbers are a planning baseline, not a second ratchet. Machine
artifacts introduced below become the only accepted regression state.

## 2. Non-negotiable invariants

Every slice in this plan preserves the existing rules and adds the
following ones as their producers land:

1. All-corpus FP remains zero.
2. A previously matched diagnostic bucket may not become unmatched at
   the same tier. Improvements cannot compensate for regressions.
3. Syntactic T0 is independently gated on every merge.
4. Scope exclusions select one exact oracle diagnostic record, never a
   whole T0 bucket by accident.
5. Evidence is produced by a command and verified from its artifact;
   hand-authored booleans and counts are not evidence.
6. M8 readiness is an entry gate. A separate completion gate enforces
   the final T3/T4/escape/fuzzer contract.
7. A stage marker advances only after every expiring escape has either
   been implemented or moved to a later owner with reviewed evidence.

## 3. Workstreams and dependencies

The work is split into five independently reviewable tracks. Track A is
the immediate prerequisite. Tracks B, D, and E may then progress beside
the semantic milestone track, but all of their M8-entry rows must finish
before M7 closes.

```text
A: trustworthy measurement
  A1 set ratchet + syntactic CI
    -> A2 exact scope identity
      -> A3 real T4 pipeline
        -> A4 executable Done gate

C: semantic implementation
  M4 close -> M5 flow -> M6 inference/calls -> M7 tail -> M8 long tail
                     ^                         |
                     |                         v
B: produced evidence + fuzzer ----------------+
D: ledger / emitter closure / runtime coverage+
E: hosted CI / toolchain / current docs -------+

M8 bounded slices -> recovery zero -> T3/T4 100% -> M9 nightly steady state
```

### Track A — trustworthy measurement

#### A1. Set-monotone ratchets and permanent syntactic gate

Branch: `infra/set-ratchet`

Add a versioned, compressed accepted-match artifact, for example:

```text
ratchets/conformance-matches.v1.json.zst
```

The artifact pins:

- vendored `_tsc.js` SHA-256;
- golden corpus manifest SHA-256;
- comparator schema version;
- per fixture and matrix key, the T0 keys currently matched;
- separate matched sets for All, 2XXX, and syntactic passes;
- T1/T2/T3 matched bucket sets once each tier activates.

For T1-T3, the stored value is the T0 bucket identity whose complete
oracle/tsrs multisets agree at that tier. The fixed oracle golden supplies
the record contents; the ratchet need not duplicate message chains.

`cargo xtask conformance` must reject:

```text
accepted_matched_tier - current_matched_tier != empty
```

New matches are allowed. `cargo xtask ratchet update` writes additions
only and refuses to remove accepted matches. There is no regression
allowlist: the oracle and corpus are fixed, so a temporarily regressing
refactor stays off `main`.

Partial runs (`--limit`, explicit fixture lists) compare only the
fixtures actually executed against their accepted subsets, so a
regression in an executed fixture still fails while unexecuted
fixtures cannot false-positive. The full-corpus set gate remains the
merge authority through `cargo xtask ci`.

Add `cargo xtask conformance --syntactic-only` to `cargo xtask ci`.
Retain integer counts in `ratchet.toml` as readable summaries, but derive
and verify them against the accepted-match artifact so they cannot drift.

Acceptance:

```sh
cargo xtask ratchet check
cargo xtask conformance
cargo xtask conformance --band 2xxx
cargo xtask conformance --syntactic-only
cargo xtask ci
```

Required regression tests:

- remove one accepted match and add a different match at the same total:
  the gate fails and names the removed identity;
- a pure addition passes before and after `ratchet update`;
- a syntactic FN cannot be hidden by a semantic addition;
- a comparator or corpus hash mismatch fails as stale.

This slice lands before any further large semantic slice. It is the
finite-convergence foundation for everything that follows.

#### A2. Scope schema 2: exact diagnostic identities

Branch: `infra/scope-identity-v2`

Replace the schema-1 T0-only key with a stable diagnostic identity:

```text
fixture + matrix_key + pass + file + start + length + code + category
+ message-chain hash + related-information hash + occurrence
```

Keep line and column as review-facing redundant fields and verify them
against `start`; do not use them as the sole identity. `occurrence`
distinguishes truly identical repeated records after stable oracle
sorting.

The scope selector returns exact record identities, not `BTreeSet<T0Key>`.
Supported-scope comparison subtracts only those exact oracle record
occurrences and preserves every neighboring record in the same T0
bucket. If tsrs emits the selected excluded identity, that case is
reported as `resolved` and the disposition must be deleted.

Because the current scope manifest is empty and draft, no compatibility
loader is needed. Reject schema 1 with a migration message rather than
silently widening an exclusion.

Acceptance:

```sh
cargo xtask scope audit
cargo xtask conformance
cargo test -p tsrs2-conformance
```

Required tests:

- two diagnostics sharing file/code/line/col but differing in span;
- two sharing span and code but differing in rendered message arguments;
- two identical records distinguished by occurrence;
- excluding one record leaves its neighbor in the supported denominator;
- syntactic records remain non-excludable;
- stale, duplicate, and ambiguous exclusions fail.

The full-corpus audit records the 68 duplicate T0 buckets as a permanent
canary. `m8-scope.json` cannot become `frozen` until this schema is live.

#### A3. Real T4 rendering and per-case comparison

Branch sequence:

1. `m7/t4-renderer-structure`
2. `m8/t4-byte-parity`

M7 implements the deterministic formatter structure already assigned to
stage 8.5. The comparison surface is color-free, uses normalized virtual
paths and fixed newlines, and applies the same sort/dedupe order as the
oracle contract. The oracle side must be produced by an explicit vendored
tsc formatter path, not by hashing serialized diagnostic JSON.

Golden schema 3 carries genuine oracle and accepted-tsrs rendered hashes.
Conformance always computes the current tsrs hash; it never trusts the
committed tsrs hash as the current result. The committed value is the
reviewable accepted baseline and is updated only through the ratchet
writer.

Supported-scope T4 formats the supported oracle and tsrs diagnostic
records after exact schema-2 scope selection. All-corpus output remains
visible and FP=0 remains absolute.

M7 acceptance:

```sh
cargo xtask oracle-refresh --render-hashes
cargo xtask conformance --tier t4 --report-only
cargo xtask ci
```

M8 acceptance adds a set-monotone T4 matched-case ratchet. A previously
byte-exact case may not regress while another case improves.

Required tests cover ordering, adjacent dedupe, UTF-16 spans, chains,
related information, file-less diagnostics, suggestion category,
platform-independent paths, and CRLF input.

#### A4. Executable final completion gate

Branch: `infra/completion-gate`

Add:

```sh
cargo xtask completion
cargo xtask completion --require-done
```

The report form always writes `target/completion/report.json`. The strict
form fails unless all of the following are true:

1. All-corpus FP is zero.
2. The exact scope manifest is frozen, fresh, and has no resolved entry.
3. Supported-scope T0, T1, T2, and T3 numerators equal their denominators.
4. Every supported case has current T4 hash equal to the oracle hash.
5. Syntactic diagnostics remain fully in scope.
6. `escapes` reports zero sites and `escapes.toml` contains no entries.
7. The Rust function ledger has no backlog and every hash is fresh.
8. The all-band emitter inventory and dispositions are frozen and fresh.
9. Runtime, fuzzer, performance, and RSS artifacts pass their schema and
   provenance checks.
10. M9 reports fewer than one new signature per nightly window and no
    known-open divergence class.

The command is added early in report-only form. Rows turn green as work
lands; the strict form becomes the release gate only after M9.

### Track B — produced evidence and differential fuzzing

#### B1. Evidence artifact protocol

Branch: `infra/evidence-schema-v2`

Replace duplicated hand-written claims in `m8-evidence.json` with
references to produced artifacts. Every artifact includes:

- schema and producer version;
- git commit;
- command and relevant arguments;
- vendored bundle, corpus, golden, and inventory hashes as applicable;
- start/end timestamps and exit status;
- raw observations from which the summary is recomputed;
- artifact SHA-256 recorded in the manifest.

`artifact_exists` becomes `read_and_verify_artifact`. Readiness derives
counts and booleans from artifact contents. A text file with the right
path cannot satisfy a gate.

#### B2. Runtime emitter coverage producer

Branches:

1. `m8/runtime-instrumentation`
2. `m8/runtime-zero-hit-review`

Generate an instrumented copy of `_tsc.js` under `target/`; never edit the
vendored bundle. Instrument direct-emitter function entries using the
same generated identities as `m8-emitter-inventory.json`, run the full
oracle corpus, and write hit counts. The `<top>` identity is not a
function entry; account for it with a module-evaluation marker at the
top of the instrumented copy.

The readiness reader derives `executed_emitters` from non-zero counts.
Zero-hit emitters require explicit evidence tied to the inventory hash.
Unknown, duplicate, overlapping, or unaccounted identities fail.

Acceptance:

```sh
cargo xtask coverage emitters --corpus
cargo xtask m8 readiness
cargo xtask codegen band-inventory --by-function --band all --check
```

#### B3. Differential fuzzer before M8

Branch sequence:

1. `m7/fuzz-generator`
2. `m7/fuzz-oracle-compare`
3. `m7/fuzz-reducer-dedupe`
4. `infra/fuzz-ci`

Add a deterministic seeded command:

```sh
cargo xtask fuzz run --seed <u64> --cases <n> --artifact <path>
cargo xtask fuzz replay <case>
cargo xtask fuzz reduce <case>
```

Start with two complementary producers:

- grammar-aware small TypeScript program generation;
- mutations of corpus fixtures that preserve or deliberately perturb
  syntax, compiler options, and multi-file structure.

Every generated case runs both tsrs and the pinned oracle. Compare T0-T4,
classify signatures by the first stable divergence identity, persist
minimal reproducers, and deduplicate by signature. The reducer must prove
that its output retains the same signature.

CI runs a fixed-seed smoke set. Scheduled CI runs a rotating seed and
stores artifacts. M8 readiness requires a real artifact with at least one
generator case, equal generated/oracle-comparison counts, a reducer smoke,
and dedupe evidence. M9 changes the scheduled run into the rolling
`< 1 new signature/night` gate.

#### B4. Performance and RSS producer

Branch: `infra/perf-evidence`

Add a command that launches conformance as a child process and records
wall time and maximum RSS in a structured artifact. Record the reference
machine identity and toolchain. Keep the normative wall ceiling at 60 s;
set the first RSS ceiling from measured evidence plus a reviewed margin.

The corpus run currently benefits from a process-lifetime leaked lib
cache. Include an A/B run with `TSRS_LIB_BUNDLE_CACHE=0`, and ensure fuzz
workers have an explicit process lifetime so generated option/lib
combinations cannot grow a cache without bound.

### Track C — semantic milestone completion

The existing stage docs remain authoritative. This plan adds the
measurement/evidence prerequisites around them.

#### C1. Close M4

Proceed with 5.9d and 5.9e after A1 lands:

- implement the modules/JSX/interop residue;
- reduce untagged escapes to zero;
- expire every owner `<= 5.8`;
- bump `STAGE` to `5.9` only in the close slice;
- record the M4 go/no-go and top residual codes.

M4 acceptance remains T0 >= 35% and FP=0, now strengthened by set
ratchets and the permanent syntactic gate.

#### C2. M5 flow

Follow `m5-flow-steps.md`. The 3,962 TS2454 FNs and adjacent nullable /
reachability families are the opening measurement band. Do not use the
50% aggregate threshold as a substitute for the per-family canaries and
set ratchet.

Acceptance includes idempotence, jobs-independence, unsupported-unwind,
and all accepted T0/T1 sets.

#### C3. M6 inference and overloads

Before M6 starts, land the scoped speculation transaction API and failed
candidate rollback tests required by `m6-inference-calls-steps.md`.
Inference, contextual typing, overload selection, and failed-candidate
diagnostics then land as bounded stages 7.1-7.5.

No direct link-table write is permitted under speculation. The M6 start
gate tests both cache rollback and diagnostic rollback.

#### C4. M7 grammar, unused, suggestions, and program diagnostics

Follow stages 8.1-8.5. TS6133 and TS6196 currently account for 14,266
FNs, so the 63% calibration gate is plausible but not sufficient by
itself.

M7 cannot close until:

- T1 set ratchet is active;
- A2 exact scope is frozen after review;
- A3 formatter structure is live;
- B1-B4 evidence producers have current artifacts;
- D1-D3 are complete;
- `cargo xtask m8 readiness --require-ready` passes; the same command
  is re-run unchanged as the M8 entry gate.

#### C5. Bounded M8 mining

Replace an open-ended “top code until done” phase with bounded slices.
Each M8 branch declares one owner family, its oracle anchors, its input
fixture list, the expected escape/disposition removals, and its tier.

Recommended order:

1. T0 residue by largest supported FN family;
2. remaining T1 category disagreements;
3. T2 span and top-message disagreements;
4. T3 chain and related-information disagreements;
5. T4-only ordering/rendering cases;
6. recovery-channel elimination;
7. final emitter/dependency converse audit.

Every slice must reduce at least one exact mismatch identity or retire a
measured escape/disposition prerequisite. If three consecutive probes in
one family expose a shared data-model ceiling, stop local patching and
apply the [stall playbook](../stall-playbook.md).

Produce `docs/NOTES-m8-<family>.md` only when the information is not
already captured by the mismatch artifact or commit body. Avoid an
unbounded prose backlog that can drift from machine state.

#### C6. Recovery-zero stage

Recovery behavior may remain graceful, but it must leave the
`Unsupported` channel. Give this work its own branch series rather than
leaving 112 current sites to an unspecified Done cleanup.

For each recovery family choose and pin one of:

- a deterministic error/unknown type;
- a missing-node control-flow return;
- a no-diagnostic recovery value matching tsc's crash boundary policy;
- an explicit parser invariant where the shape is proven unreachable.

The final slice empties `escapes.toml` and sets both escape ceilings to
zero. Fuzzer crash and determinism suites are mandatory here.

### Track D — completeness ledgers and closure reduction

#### D1. Rust function disposition burn-down

Start before M5 and remove backlog entries with the semantic stages that
own them. A disposition-only slice is allowed for clearly native helpers,
but it must cite why no tsc port identity applies. New undispositioned
functions remain forbidden.

Checkpoint: zero entries before M8 readiness is required.

#### D2. Make the 5,204-function closure review tractable

Before writing thousands of manual `not-applicable` rows, improve the
inventory review surface without weakening its conservative closure:

- emit the shortest direct-emitter path for every closure identity;
- collapse strongly connected components for review presentation;
- identify generated/runtime helper families with exact hash-pinned
  rules;
- auto-account fresh `tsc-port` ledger names;
- expand reviewed rules into exact generated entries so the frozen file
  remains identity-complete.

Every infrastructure slice must monotonically reduce `unaccounted` and
must not reduce the inventory through an unexplained parser heuristic.
The final generated dispositions still enumerate every identity and pin
the inventory hash.

#### D3. Runtime coverage and static converse reconciliation

Join B2's runtime hits to D2's static closure. Every direct emitter is
executed or has zero-hit evidence. Every closure identity is ported,
deferred, or not applicable. Report contradictions such as an executed
emitter marked not applicable.

### Track E — reproducible operation and current documentation

#### E1. Hosted CI and toolchain pin

Branch: `infra/hosted-ci`

- add `rust-toolchain.toml` with the reviewed Rust/clippy version;
- pin the supported Node major/minor for the oracle;
- run `cargo xtask ci` in a required GitHub Actions check;
- run the syntactic gate explicitly until it is folded into `ci`;
- add a scheduled fuzz workflow when B3 lands;
- upload mismatch, readiness, completion, and fuzz artifacts on failure.

The local command remains the source of the gate logic. Hosted CI makes
its execution independently observable; it does not duplicate the rules
in YAML.

#### E2. Documentation authority cleanup

Branch: `docs/v1-archive-and-setup`

- move v1-only Phase 1, `src/`, `verify.sh`, and bootstrap instructions
  under an explicit v1 archive or replace them with links to tag
  `v1-final`;
- replace `docs/setup.md` with current tsrs2 setup and verification;
- make `docs/README.md` route directly to the greenfield execution
  authority;
- move completed/stale checklists out of Active Deep Designs;
- resolve unchecked 5.8 landing checklists against landed commits;
- generate the README status table from conformance/readiness output so
  metrics and pin counts cannot lag `main`.

## 4. Required landing order

The following order is normative unless a prerequisite itself is found
incorrect:

| Order | Slice | Required before |
|---:|---|---|
| 1 | A1 set ratchet + syntactic CI | any further large semantic slice |
| 2 | M4 5.9d / 5.9e close | M5 |
| 3 | A2 exact scope identity | any scope classification/freeze |
| 4 | E1 hosted CI + toolchain | M5 close |
| 5 | E2 documentation cleanup | M5 close |
| 6 | M5 flow | M6 |
| 7 | M6 transaction precondition, then M6 | M7 |
| 8 | B1 evidence protocol + D2 closure tooling | M7 close |
| 9 | B2 coverage + B3 fuzzer + B4 perf | M7 close |
| 10 | M7 including A3 formatter structure | M8 entry |
| 11 | D1-D3 zero/complete + scope frozen | M7 close |
| 12 | `m8 readiness --require-ready` | first M8 semantic slice |
| 13 | A4 report-only completion gate | early M8 |
| 14 | bounded M8 tiers + recovery zero | M9 |
| 15 | M9 nightly steady state | `completion --require-done` |

A1 is intentionally first because every later semantic result is harder
to trust without it. A2 is early because scope data becomes expensive to
migrate after classification starts. Evidence work begins before M7 to
avoid turning M7 close into a large infrastructure cliff.

## 5. Per-slice review template

Every implementation PR body records:

```text
Owner milestone / tier:
tsc anchors and vendored hash:
Exact fixtures or generated signatures:
Accepted matched-set additions:
Accepted matched-set removals: MUST BE 0
All FP: 0
All / 2XXX / syntactic T0:
T1 / T2 / T3 shadow or active rates:
T4 cases if applicable:
Escapes before -> after by owner/class:
Ledger / closure / runtime rows before -> after:
Tests and evidence artifact hashes:
```

For docs-only and pure tooling slices, use `not applicable` only where no
semantic output is exercised; still run `cargo xtask ci` when the tooling
can affect a gate.

## 6. Stop conditions

Stop the current slice and fix the plan or design when any of these
occurs:

- an accepted matched identity must be removed to land the change;
- a scope exclusion cannot identify exactly one intended oracle record;
- a readiness row can be satisfied without executing its producer;
- a fuzzer divergence cannot be reproduced from its artifact;
- a closure reduction drops identities without a reviewable rule and
  before/after path evidence;
- three consecutive local fixes in one family expose the same missing
  type/symbol/flow model;
- the M5/M6/M7 aggregate gate is met while its named canaries or
  prerequisite rows remain red;
- `main` depends on a toolchain version not represented by the pin and
  hosted check.

## 7. Completion forecast and decision points

With A1-A4 and B1 in place, completion becomes falsifiable and progress
becomes set-monotone. The current architecture is then adequate for the
fixed 6.0.3 batch-checker scope: M5 and M7 have large measured owner
families, M6 has an explicit transaction prerequisite, and M8 has a
complete tsc-side inventory rather than a guessed function list.

The project should continue after each checkpoint only if:

- M4 close: T0 >= 35%, FP=0, untagged/stale zero;
- M5 close: flow canaries and invariants green;
- M6 start: transaction rollback proof exists;
- M7 close: all nine M8 readiness rows are produced and verified;
- M8 midpoint: accepted T0/T1 sets still grow and no repeated
  architectural ceiling is being patched locally;
- M9: the rolling new-signature rate is below the normative bound.

Failure at a checkpoint is not permission to weaken a denominator,
replace a set ratchet with a count, or add a broad scope exclusion. It is
a design review trigger.
