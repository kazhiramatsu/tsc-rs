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
| Legacy schema-1 tsc dependency-closure names | 5,204 unaccounted |
| Legacy schema-1 direct-emitter names runtime coverage | 0 / 607 |

The corpus also contains 48,821 raw oracle diagnostic records but only
48,719 T0 buckets. Sixty-eight T0 buckets in 58 cases contain multiple
diagnostics, accounting for 102 records beyond the T0 set. Any
diagnostic-level scope mechanism must preserve this multiplicity.
The two emitter/closure rows are the current name-collapsed migration
census only; D2 replaces their denominator with declaration identities
before either can become a readiness gate.

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

Add versioned, compressed accepted-match and immutable-input artifacts,
for example:

```text
ratchets/conformance-matches.v1.json.zst
ratchets/oracle-inputs.v1.json.zst
```

The artifact pins:

- vendored `_tsc.js` SHA-256;
- an immutable oracle-input manifest SHA-256. That manifest enumerates
  the corpus fixture bytes, matrix expansion/options/libs, every exact
  oracle diagnostic record, and — after A3's additive schema activation
  — the genuine oracle T4 rendered hash;
  it contains no tsrs output or accepted-tsrs baseline;
- per-tier comparator schema versions (an inactive future tier is an
  explicit absent entry, not silently the current comparator);
- per fixture and matrix key, the T0 keys currently matched;
- separate matched sets for All, 2XXX, and syntactic passes;
- per fixture and matrix key and comparison view (All, 2XXX, syntactic,
  and any explicitly supported fixed band/pass intersection), the
  multiplicity-complete T0 buckets — keys where the oracle and tsrs
  record counts agree exactly after that view's fixed predicate —
  stored from the first artifact version because they are the
  duplicate-bucket tombstone proof (A2), independent of tier
  activation;
- T1/T2/T3 matched bucket sets once each tier activates;
- the T4 matched-case set once T4 activates. T4 acceptance lives in
  this lineage, not in a mutable field beside the oracle golden.

For T1-T3, the stored value is the T0 bucket identity whose complete
oracle/tsrs multisets agree at that tier. The fixed oracle golden supplies
the record contents; the ratchet need not duplicate message chains. For
T4 the stored value is the fixture/matrix/case identity whose supported
rendered output is byte-exact. The immutable oracle-input manifest is the
denominator authority; changing the ratchet's input pin cannot redefine
it silently.

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
allowlist: an ordinary semantic slice sees a fixed oracle universe, and
the only growth path is the append-only input-transition protocol below,
so a temporarily regressing refactor stays off `main`.

The additions-only rule must hold against history, not only against
the file in the working tree — replacing the artifact wholesale and
refreshing the derived summaries would otherwise delete identities
from the left-hand side of the rejection check while the
implementation regresses in step. The artifact therefore records its
lineage: a `previous` field, stamped by `ratchet update`, carrying
the commit and SHA-256 of the version it grew from. The first version
is marked as the explicit bootstrap baseline reviewed at the A1
landing.

`ratchet check` validates the whole chain, not merely the current
artifact's one `previous` edge. Starting at the current artifact
version, it repeats the following until the bootstrap:

```text
current = current artifact-path version
while current is not bootstrap:
    expected = immediate preceding artifact-path version before current
    require current.previous.commit == expected.commit
    require current.previous.sha256 == SHA-256(expected bytes)
    require current.input_pins == expected.input_pins
            OR a valid append-only input transition from expected
    require every accepted set in expected ⊆ the corresponding set in current
    current = expected
require current is the unique oldest artifact-path version and is marked bootstrap
```

"Immediate preceding" is evaluated relative to each historical
`current` commit, never repeatedly relative to HEAD. The subset rule
covers every matched-tier set (including T4) and every view's
multiplicity-complete set. Equality of input pins covers the vendored
bundle, immutable oracle-input manifest, and every already-active tier's
comparator schema.
Consequently an unverified intermediate commit cannot launder a
removal by becoming the final artifact's `previous`: the checker
continues through that intermediate version and detects the shrinking
edge behind it. A bootstrap marker anywhere except the unique oldest
artifact-path version fails. Artifact-changing commits form one
ancestry chain; concurrent ratchet updates must rebase and regenerate
before merge. A merge commit that merely carries an unchanged artifact
does not create a lineage version. Missing history anywhere between
HEAD and the bootstrap fails rather than passing vacuously.

Hosted merge CI adds an independent trusted-base check:

```text
HEAD input pins == <PR base artifact> input pins
    OR HEAD carries a valid append-only input transition from that base
every accepted set in <PR base artifact> ⊆ the corresponding set in HEAD artifact
```

The workflow passes the resolved PR base SHA (`origin/main` is the
local shorthand) to `cargo xtask ratchet check --baseline <ref>`.
This direct comparison is required even when the branch's recursive
chain is internally valid, so rewriting several branch commits cannot
manufacture a smaller self-consistent lineage. The sole missing-base
exception is the A1 bootstrap PR itself: the base ref must contain no
artifact, and the candidate must contain exactly the one oldest version
marked bootstrap. Once the base contains an artifact, its absence or an
attempt to bootstrap again fails. This recursive lineage plus the
trusted-base comparison is what the A2 tombstone proofs and the A5
rollup stand on: artifact membership stays durable even against a
coordinated edit of artifact, summaries, implementation, and
intermediate branch history.

The normal ratchet path never changes the oracle universe or input
schema. Two named append-only input transitions are permitted and are
compared directly with the PR-base manifest:

- a fuzzer repro graduates through `universe-transition`: the old
  oracle-input manifest is an identity-and-bytes subset of the new one,
  every old fixture/matrix record and oracle T4 hash is unchanged, and
  only enumerated fixtures/records are added. After A2 global freeze,
  every added record must be in supported scope; a transition cannot
  create a new exclusion or reopen the scope baseline;
- A2 and A3 may each land one reviewed `input-schema-extension`: every
  pre-existing manifest byte and identity remains unchanged, while the
  extension adds only its declared derived fields/index (A2 canonical
  identity hashes; A3 genuine oracle T4 hashes and the new T4 comparator
  entry) to every applicable existing record. Every prior tier comparator
  entry remains byte-identical. An explicit absent marker in the prior
  schema makes omission distinguishable from an empty value.

Neither transition may delete or edit an existing oracle value, change
vendor/comparator semantics, or remove an accepted set. A TypeScript
re-vendor or comparator semantic change is the separate project defined
by the completion authority. A coordinated artifact/pin/implementation
edit that does not satisfy one of these projections fails.

Every ratcheted run has an explicit comparison-view id. `--band 2xxx`
selects the stored all-corpus 2XXX matched and multiplicity-complete
sets; `--syntactic-only` selects the syntactic sets; All selects All. A
supported fixed combination such as 2XXX+syntactic must have its own
recorded intersection view, never an implicit global set. Exact A2 scope
selection is not a ratchet view — all-corpus accepted sets remain the
monotonic authority while supported metrics are recomputed live. `--limit`
and explicit fixture lists then project the selected fixed view to only
the fixtures actually executed. A regression in an executed fixture
still fails while an out-of-view or unexecuted fixture cannot
false-positive. An ad-hoc filter with no stored view is report-only and
cannot update or satisfy the ratchet. The full-corpus All, 2XXX, and
syntactic views remain merge authority through `cargo xtask ci`.

Add `cargo xtask conformance --syntactic-only` to `cargo xtask ci`.
Retain integer counts in `ratchet.toml` as readable summaries, but derive
and verify them against the accepted-match artifact so they cannot drift.

Acceptance:

```sh
cargo xtask ratchet check
cargo xtask ratchet check --baseline origin/main
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
- deleting an identity in intermediate commit C1 and then stamping
  C1 as C2's immediate `previous` still fails: recursive validation
  reaches the pre-C1 edge and names the removed identity;
- an artifact change whose recorded previous is a valid ancestor
  but not the immediate predecessor of the artifact path fails;
- a previous-hash mismatch fails as stale; changing a non-oldest
  version to the bootstrap marker fails; a predecessor or the
  bootstrap unreachable in a shallow history fails rather than
  passing;
- a branch-local chain that is internally self-consistent but omits
  an identity accepted by the PR-base artifact fails the trusted-base
  comparison and names that identity;
- a base ref without an artifact is accepted only for the one-version
  bootstrap creation; the same absence or a second bootstrap fails
  after a baseline exists;
- a multi-version chain containing additions only passes both the
  recursive and trusted-base checks;
- deleting or editing an unmatched oracle record while updating the
  oracle-input pin, ratchet summaries, and implementation still fails
  the PR-base input-universe comparison; an enumerated append-only corpus
  transition passes and preserves every old record byte-for-byte;
- the one-time A2/A3 schema extensions may add only their declared
  derived fields; changing an existing diagnostic or oracle T4 value in
  the same change fails the PR-base projection;
- a previously accepted T4 case cannot be removed by changing both the
  implementation and any review summary: the T4 lineage subset check
  names the case;
- All, 2XXX, syntactic, and declared fixed band/pass intersection views
  compare the corresponding multiplicity-complete sets; a filtered run
  neither reports out-of-view buckets missing nor skips the view's
  regression check;
- a syntactic FN cannot be hidden by a semantic addition;
- a comparator or oracle-input-manifest hash mismatch fails as stale.

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

The two hashes are not implementation-defined strings. Rust and Node use
one schema-tagged canonical byte encoding: UTF-8; no insignificant
whitespace; fixed object-field order; decimal integers; JSON string
escaping; message-chain `next` arrays and related-information arrays in
their oracle-observable order; and lowercase SHA-256 hex over exactly
those bytes. The message-chain encoder recursively includes text, code,
category, and children. The related-information encoder includes the
same diagnostic fields plus normalized virtual file/span data.
`occurrence` is assigned zero-based after stable sorting by the complete
canonical record bytes excluding occurrence; byte-identical neighbors
retain oracle input order. The canonical encoder has a version in the A1
oracle-input pin through A2's one-time input-schema extension, so a
hash-semantic change is not a silent identity edit.

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
  vendored-tsc and immutable oracle-input-manifest pins give the claim
  its meaning.
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
  The `2xxx` early pin reads A1's full-corpus 2XXX view; the global
  freeze reads the All view. A partial-fixture or supported-scope
  projection can never prove a tombstone.
  Tombstones deliberately ride A1 (§4 row 1), not B1 — B1 lands
  after the phase-9 sweep, and a `target/` artifact from a past
  commit is unverifiable from a fresh clone. A pinned band's live
  set only ever shrinks, and every shrink is proven, not asserted.

Unpinned bands stay mutable only while the manifest status is `draft`.
The global `frozen` transition at M7 close is itself a two-step history
anchor, not a boolean assertion. First, all remaining adjudicated
exclusions land under normal review while status is `draft`. A follow-up
change adds a `global-freeze` record containing that adjudication commit
and the enumerated exact identity set of every then-live exclusion, and
changes status to `frozen`; count and SHA-256 remain derived fields.
`scope audit` requires the recorded commit to be an ancestor, the pinned
global set to equal the exact manifest set at that commit, and every
current live exclusion to be identity-for-identity in that pinned set.
Thus an addition or edit after global freeze fails even in a band that
never had an early band pin and even if count/hash are refreshed. A
global-pinned identity missing from the current live set is accepted
only with the same A1 standing-proof tombstone required for a
pinned-band deletion. A current identity outside the global pinned set
is always an unauthorized addition. The global pinned set never changes;
status downgrade fails.
Hosted PR CI independently compares the global-freeze record with the
resolved PR base: the first transition is allowed only when the base is
still `draft` and the candidate has one valid global record; once the
base is frozen, the pinned set and adjudication commit must be
byte-identical in HEAD. A two-commit add-and-reanchor sequence therefore
cannot manufacture a new self-consistent global baseline.
The transition also re-verifies every early band-freeze record and
requires every live identity in such a band to satisfy both anchors; it
does not retroactively claim that an unpinned band was reviewed earlier.
§4 row 9 consumes the early mechanism: the 2XXX sweep requires a
verifying `2xxx` band-freeze record while the manifest is still `draft`.

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
- Rust and Node canonical encoders produce the same message-chain and
  related-information hashes for Unicode, CRLF-normalized virtual paths,
  nested chains, and reordered related-information canaries; changing
  an observable array order changes the identity;
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
  vendored-tsc / immutable-oracle-input-manifest pins mismatch the tree;
- adding a non-2XXX exclusion still passes while only `2xxx` is
  pinned;
- the global freeze fails while any band-freeze record fails
  re-verification;
- a global-freeze record whose adjudication commit is absent, not an
  ancestor, or does not contain exactly its pinned identity set fails;
  so does a status downgrade;
- after global freeze, adding or editing a non-2XXX exclusion fails even
  when the edit also refreshes count, hash, and the current file's pinned
  set; the history anchor names the unauthorized identity;
- a two-commit branch that adds an exclusion and reanchors global freeze
  to the first branch commit fails the PR-base global-record comparison;
- a post-freeze deletion without a valid A1 tombstone fails, while the
  same proven resolved deletion succeeds.

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

Golden schema 3 carries only genuine oracle rendered hashes and the
oracle records needed to reproduce them. A3 adds those hashes to A1's
immutable oracle-input manifest through the one-time
`input-schema-extension`: every pre-existing fixture/matrix/diagnostic
byte remains the PR-base value, and only the previously absent T4 field
is populated from the explicit vendored formatter path. After that
activation, `oracle-refresh --render-hashes` is check-only in the fixed
universe; changing an existing oracle hash is neither a ratchet update
nor a baseline edit — it fails. There is no committed
accepted-tsrs hash beside the oracle golden. Conformance always computes
the current tsrs hash, and additions to the reviewable accepted baseline
are T4 case identities written by the A1 ratchet writer.

Supported-scope T4 formats the supported oracle and tsrs diagnostic
records after exact schema-2 scope selection. All-corpus output remains
visible and FP=0 remains absolute.

M7 acceptance:

```sh
cargo xtask oracle-refresh --render-hashes --check
cargo xtask conformance --tier t4 --report-only
cargo xtask ci
```

M8 activates A1's set-monotone T4 matched-case set. A previously
byte-exact case may not regress while another case improves; recursive
lineage and the PR-base comparison cover T4 exactly as they cover T0-T3.
Activation occurs only after A2's global freeze is anchored and the
current run reports no live exclusion as `resolved`. A later proven
scope deletion only enlarges supported scope; the change that introduces
the newly graded record must keep every already accepted T4 case exact or
the T4 ratchet rejects it.

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
8. The declaration-identity schema-2 all-band emitter inventory and
   dispositions are frozen and fresh.
9. Runtime, fuzzer, performance, and RSS artifacts match the current
   producer-input fingerprints, pass schema/provenance checks, and their
   measured wall/RSS observations are within the approved ceilings on
   the approved reference runner.
10. `cargo xtask invariants --suite all --full-corpus` passes, covering
    idempotence, jobs-independence, prefix-determinism, encodings,
    matrix-independence, and unsupported-unwind over the complete fixed
    corpus rather than the PR sample.
11. `cargo xtask fuzz steady-state --require-ready` verifies the M9
    window/history policy and reports no known-open divergence class.

Required regression test: make any one full-scope invariant red while
rows 1-9 and 11 are green; `completion --require-done` must fail and name
the invariant. A sampled `cargo xtask ci` invariant run cannot satisfy
row 10.

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
- `cargo xtask families report`: a rollup recomputed from one current
  full-conformance observation plus four verified inputs — A1's
  accepted-match ratchet, the immutable oracle-input manifest, the exact
  A2 scope manifest, and the frozen family map. The command first applies
  exact-record scope selection to the oracle and tsrs multisets, then
  grades the resulting supported buckets at each tier, and only then
  groups records by the map's (code, pass) owner. It never infers current
  supported matches from A1's all-corpus bucket set: excluding all of a
  bucket, one neighbor of a duplicate bucket, or a mismatching excluded
  neighbor can each change the supported projection. A1 remains the
  monotonic guard; the current observation supplies matched / total / FN
  and canary rows. No second ratchet is introduced;
- adjudication of the provisional owners recorded in the family doc
  (implicit-any, JSX, 7016, override validation), updating the doc
  from the frozen map;
- the freeze itself: the map file starts `draft` while the
  adjudication backlog resolves and turns `frozen` in the slice's
  closing change, which records the adjudication commit. A frozen
  map is identity-anchored the same way as the A2 band-freeze
  record, tightened from subset to full equality for the frozen oracle
  universe: `families check` re-reads the map at the recorded
  adjudication commit (an ancestor of HEAD) and requires the current
  base rows and canary set to be identical to those at that commit — the
  freeze metadata itself is outside the comparison, and a status
  downgrade back to `draft` is rejected outright — so any post-freeze
  owner or canary edit fails even though coverage and uniqueness still
  hold.
  An A1 append-only corpus universe transition may introduce a previously
  unseen (code, pass) row. It cannot rewrite the base anchor: a separate
  history-anchored `universe-extension` record enumerates only the new
  domain rows and their adjudicated owners, and the current map is the
  immutable base union of all verifying extensions. Every pre-existing row
  and canary remains byte-identical, an extension key must be new in the
  oracle-input manifest transition, and readiness fails until all new
  non-2XXX rows are assigned exactly once. A repro using only existing
  rows needs no extension.
  A legitimate ownership correction is its own reviewed re-baseline
  event recording a new adjudication commit, never an edit riding an
  implementation slice. M8 readiness row 10 reads M7
  ownership from this anchored map, so a red row cannot be moved
  out of an M7 family to fake completion.

A1 and the A2 exact-scope schema are prerequisites. The report may show
the current reviewed draft scope during M5-M7, but readiness row 10
accepts only the globally anchored `frozen` scope and a full-conformance
artifact whose producer fingerprint matches the current tree.
Acceptance:

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
- an append-only A1 universe transition that adds a new non-2XXX
  (code, pass) row fails until a matching anchored map extension lands;
  the extension succeeds without changing any old row, while disguising
  an old owner change as an extension fails;
- a code split across passes with different owners is accepted — the
  map key is the row, never the bare code (1453 arrives syntactic
  and semantic, 6133 semantic and suggestion);
- rollup counts recompute from current conformance and the exact
  supported projection, never from the map or A1 summaries;
- a wholly excluded bucket contributes zero supported denominator; an
  exclusion of one record in a duplicate bucket preserves its neighbor;
  and removing an excluded mismatching neighbor can expose a supported
  match even when the all-corpus A1 bucket is unmatched;
- a stale conformance artifact or one produced against a different A2
  scope/oracle-input fingerprint fails rather than yielding a rollup;
- the cross-band exemplars (7027, 6053, 6133-both-passes) are
  represented in the shipped map.

### Track B — produced evidence and differential fuzzing

#### B1. Evidence artifact protocol

Branch: `infra/evidence-schema-v2`

Replace duplicated hand-written claims in `m8-evidence.json` with
producer configuration plus references to produced artifacts. The
checked-in file contains commands, reviewed ceilings, and approved runner
profile ids; it contains no hand-editable `ready` boolean or copied
observation count. Every produced artifact includes:

- schema and producer version;
- git commit as provenance, never as the sole freshness test;
- command and relevant arguments;
- an exact producer-input fingerprint over the built tsrs executable and
  all producer-relevant inputs: selected source-tree manifests,
  `Cargo.lock`, `rust-toolchain.toml`, Node pin, vendored bundle,
  immutable oracle-input manifest, comparator/instrumenter/generator
  code, inventory, scope, policy, and arguments as applicable;
- start/end timestamps and exit status;
- raw observations from which the summary is recomputed;
- artifact SHA-256 recorded in the manifest.

`artifact_exists` becomes `read_and_verify_artifact`. Readiness derives
counts and booleans from artifact contents. A text file with the right
path cannot satisfy a gate. It recomputes the current producer-input
fingerprint and requires exact equality; an ancestor commit alone is
insufficient. A docs-only commit outside the producer's declared source
manifest does not stale the artifact, while any checker, harness,
producer, toolchain, option, corpus, oracle, scope, or inventory input
that can affect its observation does. A dirty relevant path fails.

Artifacts under `target/` are ephemeral products, not versioned
evidence. The required M7-readiness workflow builds once, runs B2, B3's
smoke, and B4 in the same workspace, then invokes `m8 readiness
--require-ready` against exactly those artifacts. A fresh clone therefore
regenerates evidence instead of relying on a past `target/` file. The M9
nightly history is the separate versioned lineage described in B3.

```sh
cargo xtask m8 evidence produce --all
cargo xtask m8 readiness --require-ready
```

The orchestration command invokes the named B2/B3/B4 producers and writes
only their common manifest; it cannot synthesize observations itself.

Required tests change one checker source, producer source, `Cargo.lock`,
toolchain pin, scope identity, and command argument independently and
require the affected artifact to fail stale; a docs-only change outside
the fingerprint remains valid. Deleting an ephemeral artifact fails, and
a fresh-clone workflow that runs all declared producers then readiness
passes without any checked-in `target/` state.

#### B2. Runtime emitter coverage producer

Branches:

1. `m8/runtime-instrumentation`
2. `m8/runtime-zero-hit-review`

Generate an instrumented copy of `_tsc.js` under `target/`; never edit the
vendored bundle. Instrument direct-emitter function entries using the
same schema-2 declaration identities as `m8-emitter-inventory.json`, run
the full oracle corpus, and write one hit count per declaration. Two
same-named declarations never share a counter. The `<top>` identity is
not a function entry; account for it with a distinct module-evaluation
marker at the top of the instrumented copy.

The readiness reader derives `executed_emitters` from non-zero counts.
Zero-hit emitters require explicit evidence tied to the inventory hash.
Unknown, duplicate, overlapping, name-collapsed, or unaccounted
identities fail. The artifact fingerprint additionally pins the
instrumenter source, Node pin, vendored bundle, declaration inventory,
immutable oracle-input manifest, and full-corpus command.

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
cargo xtask fuzz nightly --policy ratchets/fuzz-steady-state-policy.toml
cargo xtask fuzz steady-state [--require-ready]
```

Start with two complementary producers:

- grammar-aware small TypeScript program generation;
- mutations of corpus fixtures that preserve or deliberately perturb
  syntax, compiler options, and multi-file structure.

The policy's generated domain is the supported batch-checker contract:
it excludes node_modules/package host resolution, project references,
JSDoc-driven semantics, and emit-dependent behavior by construction. A
producer that generates a case outside that declared domain fails the
window; it may not silently discard the case from the comparison count.

Every generated case runs both tsrs and the pinned oracle. Compare T0-T4
and select the first failing tier. The canonical signature is schema,
tier, pass, divergence side/class, and the sorted one-sided multiset of
`(code, normalized message head)` at that tier. T4 uses a deterministic
renderer class (`order`, `dedupe`, `path`, `newline`, or `text`) plus the
first affected diagnostic's same code/message-head key. Paths, generated
fixture names, positions, seeds, and raw rendered hashes are excluded
from the signature so reduction does not manufacture a new class. Exact
outputs remain in the repro artifact. Persist minimal reproducers and
deduplicate by this canonical signature; the reducer must prove that its
output retains the same signature and still fails the exact comparator.
`normalized message head` means the exact first line of the T2 top
message after virtual-path and LF normalization — no heuristic argument
stripping. Renderer classes use the fixed precedence written above and
are derived by comparing the structured pre-render diagnostic sequence
before falling back to `text`; Node and Rust golden tests pin the
classifier and canonical signature bytes.

CI runs a fixed-seed smoke set. Scheduled CI runs a rotating seed and
stores artifacts. M8 readiness requires a real artifact with at least one
generator case, equal generated/oracle-comparison counts, a reducer smoke,
and dedupe evidence.

M9 makes the rate gate executable with three versioned files:

- `ratchets/fuzz-steady-state-policy.toml` fixes 14 consecutive UTC
  nightly windows, at least two hours and 100,000 completed generated
  cases per window, and the exact supported generator domains. These are
  normative minima enforced by the command, not editable defaults; a
  policy below them fails. Any valid policy change, including a
  tightening, changes the fingerprint and requires a new full streak;
- `ratchets/fuzz-nightly-history.v1.json.zst` records each window's start
  and end, seed range, cases, runtime, producer-input fingerprint,
  artifact SHA-256, and newly observed signature ids. It uses the same
  recursive lineage and PR-base trusted comparison shape as A1, with the
  stronger history rule that every prior window record remains
  byte-identical and only later non-overlapping windows append. Windows
  cannot be deleted or rewritten. A missing, failed, under-budget,
  overlapping, or stale-fingerprint night breaks the consecutive streak;
- `ratchets/fuzz-signatures.v1.json.zst` is an append-only registry of
  canonical signatures, first-seen window, minimal-repro SHA-256, owner,
  and `open`/`resolved` state. Entries are never deleted. Resolution must
  cite the conformance fixture/universe-transition and A1 acceptance that
  keeps the fix live. Every prior field is byte-identical across lineage
  except the one-way `open -> resolved` transition with that proof;
  `resolved -> open` and evidence replacement fail.

Each history row references a checked-in compact producer artifact under
`ratchets/fuzz-windows/`, not an expired CI download. It contains the raw
per-seed outcome digests, exact failure/signature memberships, aggregate
counts, and hashes of separately stored minimal reproducers. The
scheduled workflow signs its artifact manifest with the CI attestation
key pinned by policy; the checked-in bundle includes the signature and
verification material. The aggregator and `steady-state` command verify
repository/workflow identity, producer commit, current input fingerprint,
artifact SHA-256, signature, and raw-to-summary recomputation. An
unsigned or hand-authored history row cannot count as a window. Key or
attestation-policy rotation changes the fingerprint and resets the
streak.

`fuzz steady-state --require-ready` verifies all three lineages, requires
the 14 windows to share the current release-candidate input fingerprint,
computes `distinct new signatures / 14 < 1.0` from raw window membership,
and requires zero `open` registry entries. The scheduled workflow uploads
raw artifacts; a reviewed aggregation change records each verified
window and its compact signed artifact in-repo so the command works on a
fresh clone. A checker, oracle, generator, reducer, signature-schema, or
policy change resets the streak;
a docs-only change outside the fingerprint does not.

#### B4. Performance and RSS producer

Branch: `infra/perf-evidence`

Add `cargo xtask perf conformance --artifact <path>`, which launches
conformance as a child process and records wall time and maximum RSS in a
structured artifact. The checked-in configuration enumerates approved
reference-runner profile ids with OS/architecture, CPU model/core policy,
memory, and measurement backend; merely recording an arbitrary machine
name cannot satisfy readiness. Pin the exact executable, full-corpus
command/options, immutable oracle-input manifest, toolchains, and runner
profile in the producer fingerprint. Keep the normative wall ceiling at
60 s; set the first RSS ceiling from measured evidence plus a reviewed
margin. Readiness and completion require the measured values themselves,
not only the declared ceilings, to pass on an approved profile.

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
- A2 exact scope is globally identity-anchored and frozen after review;
- A3 formatter structure is live;
- B1-B4 evidence producers have current-fingerprint artifacts, with B4
  produced on an approved reference runner;
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

#### D2. Make the declaration-level closure review tractable

Before writing thousands of manual `not-applicable` rows, improve the
inventory review surface without weakening its conservative closure:

- replace schema 1's bare-name identity with a schema-2 declaration
  identity for every named or anonymous function-like node:
  `lexical-owner declaration path + declaration kind + name-or-anonymous
  + start + end + source-slice SHA-256`. Diagnostics in an anonymous
  callback belong to that callback, never its nearest named owner. The
  module body remains the one explicit `<top>` identity. `name` is a
  review/ledger alias and never the primary key;
- resolve identifier calls through lexical bindings to exact declaration
  identities. Conservative property-call dispatch may add edges to every
  candidate declaration with that property name, but it keeps those
  candidates as separate nodes and dispositions; a hit or port of one
  cannot account for another;
- emit the shortest direct-emitter path for every closure identity;
- collapse strongly connected components for review presentation;
- identify generated/runtime helper families with exact hash-pinned
  rules;
- auto-account a fresh `tsc-port` ledger entry only when its `tsc-span`
  and `tsc-hash` select that exact declaration. A matching bare function
  name is only a candidate and never closes another same-named
  declaration;
- assign every direct emitter an owner (milestone, M7 stage, M8
  family, or out-of-scope with cause) as a column in the dispositions
  file. The A5 join alone cannot complete the column.

The checked-in schema-1 census — 607 name-collapsed direct emitters,
6,587 closure names, and the 1,731/746 site-code decomposition — is a
migration canary, not a schema-2 acceptance baseline. It currently merges
353 identities with multiple declarations, including 19 direct-emitter
identities (`visit` alone merges seven declarations and three unrelated
diagnostic sites), and attributes anonymous callbacks to a named owner or
`<top>`. D2 therefore regenerates every direct-declaration, closure,
`<top>`-only, real-function, executed, and unaccounted count before owner
adjudication. No final gate or zero-hit decision may reuse the legacy
607/5,204/name-only denominators. The overall diagnostic-site code census
remains a migration cross-check, but its owning-declaration split is
rederived.

After regeneration, the mechanical owner pass is three rules. A site
matching none stays unassigned, and manual review is limited to
cross-family helpers, multi-pass sites, dynamic property candidates, and
`unexercised` adjudication:

- 2XXX site codes resolve through the 2XXX emission map
  (impl-checker-2xxx.md, backed by the 2xxx-emitter-inventory.md
  function-to-module table), expanded from its name candidates to exact
  declaration identities by source span/hash before it can assign an
  owner;
- corpus-exercised non-2XXX codes join the A5 family map on
  (code, pass), the pass implied when the corpus sees the code in
  exactly one pass; a multi-pass code (1453, 6133) never
  auto-assigns by bare code — its sites carry their candidate rows
  into manual review;
- declaration site codes with no corpus row take the explicit owner
  `unexercised`, closed by B2 declaration-level zero-hit evidence plus
  D3 adjudication into a family or out-of-scope with cause;
- expand reviewed rules into exact generated entries so the frozen file
  remains identity-complete.

Every infrastructure slice must monotonically reduce `unaccounted` and
must not reduce the inventory through an unexplained parser heuristic.
The final generated dispositions still enumerate every identity and pin
the inventory hash. Schema 1 is accepted only as the pre-D2 draft input;
it cannot be frozen and cannot satisfy M8 readiness.

Required tests:

- all seven current `visit` declarations receive distinct ids; executing,
  porting, or dispositioning one leaves the other six unaccounted;
- two declarations with the same name but different source spans/hashes
  cannot be closed by one ledger entry;
- an anonymous direct emitter receives its own counter and does not
  increment its named owner or `<top>`;
- a property-call over-approximation creates separate review edges for
  every candidate declaration and one candidate's runtime hit does not
  cover its siblings;
- schema-1 inventory/dispositions fail `--require-ready` with a migration
  message, and regenerated schema-2 counts are derived rather than pinned
  to the legacy census.

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
- check out enough git history to reach the A1 artifact's unique
  bootstrap and every recorded adjudication commit — the set-ratchet
  lineage/input-universe transitions (A1), scope band/global freezes
  (A2), family-map freeze (A5), and M9 history/signature lineages (B3)
  all fail on a too-shallow clone rather than passing vacuously;
- run `cargo xtask ratchet check --baseline <PR-base-SHA>` in the
  required pull-request check (`origin/main` is the local shorthand),
  independently of the recursive branch-lineage validation;
- run `cargo xtask scope audit --baseline <PR-base-SHA>` so the first
  global freeze is unique and every later global-freeze record remains
  byte-identical to the trusted base;
- run the syntactic gate explicitly until it is folded into `ci`;
- after B1-B4 land, make the required M7-readiness job build once, run
  runtime coverage, fixed-seed fuzz smoke, and performance on the
  approved reference-runner profile, then run `m8 readiness
  --require-ready` in that same workspace. No job may download an older
  `target/` artifact to satisfy readiness;
- add the scheduled two-hour/100,000-case fuzz workflow when B3 lands,
  retain its raw artifact, and require the reviewed aggregator to verify
  and append the window to the versioned M9 history without rewriting
  prior windows;
- provide an approved reference-runner job/profile for B4; an ordinary
  faster hosted runner may exercise the command but cannot certify the
  normative wall/RSS rows;
- make the final release job regenerate B1-B4 evidence on that approved
  runner, execute `invariants --suite all --full-corpus`, verify the
  checked-in M9 histories, and only then run `completion --require-done`
  in the same workspace;
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
| 12 | M7 semantic stages 8.1-8.5 including A3 formatter structure (stage content complete; milestone not yet closed) | M7 close prerequisites |
| 13 | D1-D3 zero/complete + A2 globally identity-anchored scope frozen | M7 close |
| 14 | M7 close: `m8 readiness --require-ready` | first M8 semantic slice |
| 15 | A4 report-only completion gate | early M8 |
| 16 | bounded M8 tiers + recovery zero | M9 |
| 17 | M9 14-window nightly steady state + zero open signatures | `completion --require-done` |

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
- M9: `cargo xtask fuzz steady-state --require-ready` verifies 14
  consecutive current-fingerprint windows, the rolling new-signature rate
  is below one per night, and the signature registry has no open entry.

Failure at a checkpoint is not permission to weaken a denominator,
replace a set ratchet with a count, or add a broad scope exclusion. It is
a design review trigger.
