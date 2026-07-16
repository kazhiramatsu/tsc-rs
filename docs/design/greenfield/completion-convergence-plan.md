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
| Non-2XXX T0 | 9,131 / 27,803 (32.8418%) |
| Non-2XXX T3 shadow | 29.6047% |
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
8. Every (code, pass) row the corpus exercises maps to exactly one
   owner family; an unmapped row fails the family-map check. Non-2XXX
   rows map through the enumerated A5 map; rows in codes 2000-2999
   belong wholesale to the 2XXX band family, the single range-keyed
   owner (its per-function ownership is the emission map).

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
    -> A5 non-2XXX family map + rollup (feeds C4 stage gates, D2 owners)

C: semantic implementation
  M4 close -> M5 flow -> M6 inference/calls -> 2XXX sweep -> M7 tail -> M8 long tail
                     ^                                       |
                     |                                       v
B: produced evidence + fuzzer ------------------------------+
D: ledger / emitter closure / runtime coverage -------------+
E: hosted CI / toolchain / current docs --------------------+

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
- per fixture and matrix key, the multiplicity-complete T0 buckets —
  keys where the oracle and tsrs record counts agree exactly —
  stored from the first artifact version because they are the
  duplicate-bucket tombstone proof (A2), independent of tier
  activation;
- T1/T2/T3 matched bucket sets once each tier activates.

For T1-T3, the stored value is the T0 bucket identity whose complete
oracle/tsrs multisets agree at that tier. The fixed oracle golden supplies
the record contents; the ratchet need not duplicate message chains.

`cargo xtask conformance` must reject:

```text
accepted_matched_tier - current_matched_tier != empty
accepted_multiplicity_complete - current_multiplicity_complete != empty
```

New matches and newly complete buckets are allowed. `cargo xtask
ratchet update` writes additions only and refuses to remove accepted
matches or accepted multiplicity-complete buckets. The complete-bucket
set is ratcheted in its own right because the matched-tier gate cannot
see its regressions: a bucket accepted at 2/2 that falls to 2/1 keeps
its T0 key in the matched set, yet the fall would silently void any
tombstone standing on that bucket. There is no regression
allowlist: the oracle and corpus are fixed, so a temporarily regressing
refactor stays off `main`.

The additions-only rule must hold against history, not only against
the file in the working tree — replacing the artifact wholesale and
refreshing the derived summaries would otherwise delete identities
from the left-hand side of the rejection check while the
implementation regresses in step. The artifact therefore records its
lineage: a `previous` field, stamped by `ratchet update`, carrying
the commit and SHA-256 of the version it grew from, the first
version marked as the explicit bootstrap baseline reviewed at the A1
landing. `ratchet check` verifies that the recorded previous commit
is an ancestor of HEAD and is the most recent commit in
`git log HEAD -- <artifact path>` other than the change under review
— the immediate predecessor version, so the pointer is not
chooseable and chaining from an older version to hide a removal
fails — that the artifact committed there hashes to the recorded
SHA-256, and that every accepted set at that version (all
matched-tier sets and the multiplicity-complete set) is a subset of
the corresponding current set. A history too shallow to reach the predecessor fails rather
than passing vacuously. This lineage is what the A2 tombstone proofs
and the A5 rollup stand on: artifact membership stays durable even
against a coordinated edit of artifact, summaries, and
implementation.

Partial runs (`--limit`, explicit fixture lists) compare only the
fixtures actually executed against their accepted subsets — the
matched-tier and multiplicity-complete sets under the same subset
rule — so a regression in an executed fixture still fails while
unexecuted fixtures cannot false-positive. The full-corpus set gate
remains the merge authority through `cargo xtask ci`.

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
- a bucket accepted at 2/2 regressing to 2/1 fails and names the
  bucket, even though its T0 key remains in the matched set;
- deleting an accepted identity from the artifact while regressing
  the implementation and refreshing the derived `ratchet.toml`
  summaries in the same change fails: the lineage subset check
  names the removed identity;
- an artifact change whose recorded previous is a valid ancestor
  but not the immediate predecessor of the artifact path fails;
- a previous-hash mismatch fails as stale; a predecessor
  unreachable in a shallow history fails rather than passing;
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

A band can be pinned before the manifest freezes. A band-freeze
record inside `m8-scope.json` holds the band, the adjudication
commit, and the pinned identity set — the exact A2 identity of every
exclusion in the band at pin time, enumerated; the aggregate SHA-256
and count are derived fields, recomputed for coherence and never the
authority. A hash of the current set alone cannot reject a forged
add-and-rehash edit, so the pin is created in two steps — the
adjudicated band exclusions land first under normal review, then a
follow-up change records the pin against that commit — and `scope
audit` enforces three rules from defined inputs:

- subset, never equality-only: every current exclusion in a pinned
  band must appear identity-for-identity in the pinned set; an
  identity outside it (an addition) or one whose fields differ (an
  edit) fails, forcing a forger to edit the pinned set itself, which
  the next rule catches;
- history anchor: the pinned set must equal the band subset of
  `m8-scope.json` as committed at the recorded adjudication commit,
  and that commit must be an ancestor of HEAD (the audit reads the
  file at that commit; hosted CI fetches enough history). Any later
  change to the pinned set or to the adjudication-commit field
  fails; a deliberate re-pin is a baseline event with its own
  reviewed adjudication, never silent;
- tombstoned deletion: an identity present in the pinned set but
  absent from the current set must carry a tombstone — the identity
  plus the resolving commit, which must be an ancestor of HEAD — and
  its proof lives in the A1 accepted-match artifact, whose
  vendored-tsc and golden-manifest pins give the claim its meaning.
  T0-key membership alone proves resolution only for a singleton
  bucket, where the excluded identity is the key's sole oracle
  record. In a duplicate T0 bucket a T0 match cannot say which
  record resolved (2XXX alone has 65 duplicate buckets carrying 99
  extra records — TS2695 span variants, TS2537 message variants,
  TS2343 at 35 buckets), so there the tombstone additionally
  requires the bucket to be multiplicity-complete in the artifact:
  tsrs emits every occurrence of the key, or no identity in that
  bucket may be deleted. The proof is versioned in-repo, verifiable
  on a fresh clone without any historical build product, and
  standing: the A1 gate ratchets the matched-tier and
  multiplicity-complete sets alike (accepted minus current is empty
  for both, partial runs included) and the artifact itself is
  lineage-anchored against wholesale replacement, so neither the T0
  match nor the bucket's completeness can silently disappear — a
  later regression inside the bucket fails conformance itself, not
  merely the audit.
  Tombstones deliberately ride A1 (§4 row 1), not B1 — B1 lands
  after the phase-9 sweep, and a `target/` artifact from a past
  commit is unverifiable from a fresh clone. A pinned band's live
  set only ever shrinks, and every shrink is proven, not asserted.

Unpinned bands stay mutable and the manifest status stays `draft`.
The global `frozen` transition at M7 close re-verifies every
band-freeze record and never retroactively blesses an unpinned band.
§4 row 9 consumes this: the 2XXX sweep gate requires a verifying
`2xxx` band-freeze record — machine-checked with the manifest still
draft.

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
- stale, duplicate, and ambiguous exclusions fail;
- adding or editing a 2XXX exclusion after the `2xxx` pin fails even
  when the change also rewrites the pinned set, count, and hash (the
  history anchor no longer matches);
- a record whose adjudication commit does not contain exactly the
  pinned set, or is not an ancestor of HEAD, fails;
- a deletion in a pinned band without a tombstone fails; so does a
  tombstone whose identity is absent from the A1 accepted-match
  artifact or whose resolving commit is not an ancestor of HEAD; a
  proven tombstoned deletion passes;
- with two excluded records sharing one T0 key and tsrs emitting
  only one, the tombstone for either identity fails
  (multiplicity-incomplete bucket); with both emitted, the resolved
  deletions pass;
- the audit fails outright when the A1 artifact is missing or its
  vendored-tsc / golden-manifest pins mismatch the tree;
- adding a non-2XXX exclusion still passes while only `2xxx` is
  pinned;
- the global freeze fails while any band-freeze record fails
  re-verification.

The full-corpus audit records the 68 duplicate T0 buckets as a permanent
canary; the duplicate-bucket tombstone rule above is exercised against
exactly that set (65 of the 68 sit inside the 2XXX band).
`m8-scope.json` cannot become `frozen` until this schema is live.

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

#### A5. Non-2XXX family map and rollup

Branch: `infra/family-map`

[non-2xxx-first-order.md](non-2xxx-first-order.md) owns the family
decomposition; this slice turns it into machine state:

- a reviewable enumerated map file assigning every corpus-exercised
  non-2XXX (code, pass) row to exactly one owner family — an
  enumeration table, never a numeric-range rule, because ownership
  crosses bands (7027 is M5 flow surfacing as a suggestion; 6053 is
  program machinery inside 6XXX; 6133 spans the suggestion and
  semantic passes). The single deliberate range key is the 2XXX band
  boundary itself: `families check` partitions corpus rows at codes
  2000-2999 — rows inside are owned wholesale by the band family
  (per-function ownership lives in the 2XXX emission map, D2 rule 1),
  rows outside must each appear exactly once in the enumerated map —
  so the union check leaves no corpus-exercised row of any band
  unowned;
- `cargo xtask families report`: a rollup derived from the A1
  accepted-match artifact plus the frozen map — per family, matched /
  total / FN at each tier, plus the canary rows. No second ratchet:
  the global set ratchet already forbids regressions; the rollup adds
  accounting and gate inputs;
- adjudication of the provisional owners recorded in the family doc
  (implicit-any, JSX, 7016, override validation), updating the doc
  from the frozen map;
- the freeze itself: the map file starts `draft` while the
  adjudication backlog resolves and turns `frozen` in the slice's
  closing change, which records the adjudication commit. A frozen
  map is identity-anchored the same way as the A2 band-freeze
  record, tightened from subset to full equality because the corpus
  is fixed and the coverage rule already pins the domain: `families
  check` re-reads the map at the recorded adjudication commit (an
  ancestor of HEAD) and requires the current enumerated rows and
  canary set to be identical to those at that commit — the freeze
  metadata itself is outside the comparison, and a status downgrade
  back to `draft` is rejected outright — so any post-freeze owner or
  canary edit fails even though coverage and uniqueness still hold.
  A legitimate ownership correction is its own reviewed re-baseline
  event recording a new adjudication commit, never an edit riding an
  implementation slice. M8 readiness row 10 reads M7
  ownership from this anchored map, so a red row cannot be moved
  out of an M7 family to fake completion.

Only A1's artifact is a prerequisite; the map file and doc carry no
tooling dependency. Acceptance:

```sh
cargo xtask families report
cargo xtask families check
cargo xtask ci
```

Required tests:

- an unmapped non-2XXX (code, pass) row in the corpus fails
  `families check`; 2XXX rows are covered wholesale by the band
  partition, and a 2000-2999 code appearing in the enumerated map
  also fails — band ownership is never re-enumerated;
- the same (code, pass) row mapped to two families fails;
- after the freeze, changing any row's owner fails against the
  anchored map even though coverage and uniqueness still hold — the
  gaming case is pinned: reassigning a red M7-owned row such as
  (1206, semantic) from checker-grammar to an M8 family fails
  `families check`, so readiness row 10 cannot be satisfied by
  shrinking M7 ownership;
- post-freeze canary edits or a status downgrade back to `draft`
  fail the same way; the only path is a re-baseline event recording
  a new reviewed adjudication commit;
- a code split across passes with different owners is accepted — the
  map key is the row, never the bare code (1453 arrives syntactic
  and semantic, 6133 semantic and suggestion);
- rollup counts recompute from the artifact, never from the map;
- the cross-band exemplars (7027, 6053, 6133-both-passes) are
  represented in the shipped map.

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

C4 begins only after the 2XXX completion sweep closes the band (§4
row 9; the phase-9 checklist: all-corpus band FP zero,
supported-scope band FN zero, exclusions pinned as exact A2
identities under the `2xxx` band-freeze record, all matrix points).
Follow stages 8.1-8.5. TS6133 and
TS6196 currently account for 14,266
FNs, so the 63% aggregate gate is calibration only: it is reachable
from the unused family alone and certifies nothing about the other
M7 families. Each stage therefore closes on its own family rows from
the A5 map ([non-2xxx-first-order.md](non-2xxx-first-order.md)):

- 8.1 grammar — the checker-grammar family (semantic-pass 1XXX plus
  the grammar rows of 17XXX/18XXX);
- 8.2 suppression — behavioral, no code set: closes on the audit
  artifact plus the suppression canaries named in the map;
- 8.3 unused — the unused family's error-mode rows;
- 8.4 suggestions — the suggestion-pass rows (unused suggestion half,
  infer-from-usage residue, 80XXX, deprecations, flow-derived
  surfacing) and T1 activation;
- 8.5 options/program — the program/resolution family rows and the
  T4 formatter structure (A3).

A stage's gate is its family rollup reaching the acceptance recorded
in the map (canary set complete, then supported FN = 0 for its rows);
the aggregate rate is never a substitute.

M7 cannot close until:

- T1 set ratchet is active;
- A2 exact scope is frozen after review;
- A3 formatter structure is live;
- B1-B4 evidence producers have current artifacts;
- D1-D3 are complete;
- every M7-owned family reports complete in the A5 rollup
  (readiness row 10 — enforced by `--require-ready` itself, not only
  by the separate `families report`);
- `cargo xtask m8 readiness --require-ready` passes; the same command
  is re-run unchanged as the M8 entry gate.

#### C5. Bounded M8 mining

Replace an open-ended “top code until done” phase with bounded slices.
Each M8 branch declares one owner family, its oracle anchors, its input
fixture list, the expected escape/disposition removals, and its tier.
At M8 entry the readiness report fixes the per-family residual
snapshot — unfinished families, unowned emitters, and per-family
T3-mismatch buckets — and every M8 slice cites its family's
before/after against that snapshot, not against a moving top-FN list.

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
- assign every direct emitter an owner (milestone, M7 stage, M8
  family, or out-of-scope with cause) as a column in the dispositions
  file. The A5 join alone cannot complete the column. Measured from
  this inventory's `direct_emitter` sites joined to the golden oracle
  records: the 607 direct emitters span 1,731 distinct site codes —
  1,551 in real functions plus 180 referenced only from `<top>`, the
  module-evaluation marker that is not an assignable function (B2).
  The corpus exercises 746 of the 1,551 (336 2XXX + 410 non-2XXX);
  every corpus code has at least one real-function site in this
  inventory, and a re-vendor that breaks that property is a D3
  contradiction report, never a silent default. The remaining 805
  real-function codes and all 180 `<top>`-only codes have no corpus
  row. The mechanical pass is three rules, a site matching none stays
  unassigned, and manual review is limited to cross-family helpers,
  multi-pass sites, and `unexercised` adjudication:
  - 2XXX site codes resolve through the 2XXX emission map
    (impl-checker-2xxx.md, backed by the 2xxx-emitter-inventory.md
    function-to-module table), which names the owning milestone per
    emitter function;
  - corpus-exercised non-2XXX codes join the A5 family map on
    (code, pass), the pass implied when the corpus sees the code in
    exactly one pass; a multi-pass code (1453, 6133) never
    auto-assigns by bare code — its sites carry their candidate rows
    into manual review;
  - codes with no corpus row (the 805 real-function codes and the
    180 `<top>`-only codes above) take the explicit owner
    `unexercised`, closed by B2 zero-hit evidence plus D3
    adjudication into a family or out-of-scope with cause;
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
emitter marked not applicable, or an executed emitter with no family
owner.

### Track E — reproducible operation and current documentation

#### E1. Hosted CI and toolchain pin

Branch: `infra/hosted-ci`

- add `rust-toolchain.toml` with the reviewed Rust/clippy version;
- pin the supported Node major/minor for the oracle;
- run `cargo xtask ci` in a required GitHub Actions check;
- check out enough git history to reach every recorded adjudication
  commit and the A1 artifact's lineage predecessor — the set-ratchet
  lineage (A1), scope band-freeze (A2), and family-map freeze (A5)
  history anchors all fail on a too-shallow clone rather than
  passing vacuously;
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
| 4 | A5 family map + non-2XXX rollup | M5 close |
| 5 | E1 hosted CI + toolchain | M5 close |
| 6 | E2 documentation cleanup | M5 close |
| 7 | M5 flow | M6 |
| 8 | M6 transaction precondition, then M6 | M7 |
| 9 | 2XXX scope adjudication pinned by the A2 `2xxx` band-freeze record, then completion sweep — all-corpus 2XXX FP = 0, supported-scope 2XXX FN = 0 (2xxx-first-order.md phase 9, first half) | M7 |
| 10 | B1 evidence protocol + D2 closure tooling | M7 close |
| 11 | B2 coverage + B3 fuzzer + B4 perf | M7 close |
| 12 | M7 including A3 formatter structure | M8 entry |
| 13 | D1-D3 zero/complete + scope frozen | M7 close |
| 14 | `m8 readiness --require-ready` | first M8 semantic slice |
| 15 | A4 report-only completion gate | early M8 |
| 16 | bounded M8 tiers + recovery zero | M9 |
| 17 | M9 nightly steady state | `completion --require-done` |

A1 is intentionally first because every later semantic result is harder
to trust without it. A2 is early because scope data becomes expensive to
migrate after classification starts. A5 sits before M5 close so the M5
flow family rows, C4's per-stage gates, and D2's owner column consume a
reviewed map instead of ad-hoc code lists. Rows 7-8 fix M5 flow before
M6 inference, exercising the swap-back clause 2xxx-first-order.md's
phase plan records for its phases 8/7: at the 52c47bbb baseline the
flow-unlock family carries 4,357 of the 9,995 band FNs (2454 alone
3,962, C2) against 477 for the calls/inference-unlock family, and M6
additionally carries the speculation-transaction start gate; both
orders are dependency-legal and phase numbering is unchanged. Evidence
work begins before M7 to avoid turning M7 close into a large
infrastructure cliff.

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

With A1-A5 and B1 in place, completion becomes falsifiable and progress
becomes set-monotone per family, not only in aggregate. The current architecture is then adequate for the
fixed 6.0.3 batch-checker scope: M5 and M7 have large measured owner
families, M6 has an explicit transaction prerequisite, and M8 has a
complete tsc-side inventory rather than a guessed function list.

The project should continue after each checkpoint only if:

- M4 close: T0 >= 35%, FP=0, untagged/stale zero;
- M5 close: flow canaries and invariants green;
- M6 start: transaction rollback proof exists;
- M7 start: the 2XXX completion sweep is closed — band FP zero
  corpus-wide, band FN zero on the supported scope, and the band's
  scope exclusions adjudicated under a verifying A2 band-freeze
  record, the manifest still draft (the phase-9 checklist);
- M7 close: all ten M8 readiness rows are produced and verified;
- M8 midpoint: accepted T0/T1 sets still grow and no repeated
  architectural ceiling is being patched locally;
- M9: the rolling new-signature rate is below the normative bound.

Failure at a checkpoint is not permission to weaken a denominator,
replace a set ratchet with a count, or add a broad scope exclusion. It is
a design review trigger.
