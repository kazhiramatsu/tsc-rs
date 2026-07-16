# Measurement integrity contract

Status: normative support contract for the completion plan.

This document owns the schemas and anti-regression rules behind A1, A2,
A5, and the declaration inventory used by D2. The
[completion convergence plan](completion-convergence-plan.md) owns when
they land; [definition of done](definition-of-done.md) owns the final
product bar.

The rules below deliberately use one vocabulary. A versioned machine
artifact has:

- **inputs** — the exact vendor, corpus, comparator, producer, policy,
  and toolchain material that gives its observations meaning;
- **content** — identities or rows, never a hand-written aggregate;
- **state** — `draft`, a reviewed pin, or `frozen` as applicable;
- **anchor** — history proving that protected content was not silently
  replaced.

Counts and hashes are derived coherence fields. They are never the
authority for an identity set.

| Artifact | State path | Protected change rule |
|---|---|---|
| A1 accepted state | bootstrap -> append-only versions | accepted identities only grow |
| A2 scope | draft -> optional band pin -> global frozen | pinned deletion needs a tombstone; global additions/edits never occur |
| A5 family map | draft -> frozen base + universe extensions | old ownership is byte-stable; extensions add new domain rows only |
| M9 histories | bootstrap -> append-only windows/signatures | prior windows/fields are byte-stable; signature state is one-way |

## 1. Common anchor protocol

There are two anchor shapes.

### 1.1 Append-only lineage

A growing artifact records `previous = {commit, sha256}`. The checker
walks every artifact-path version back to the unique oldest version,
which alone is marked `bootstrap`. At each edge it requires:

1. `previous.commit` is the immediate preceding version of that path,
   evaluated relative to the historical version being checked;
2. `previous.sha256` equals that version's bytes;
3. protected content is a subset of the current content, or is
   byte-identical when the artifact does not permit growth;
4. input pins are equal, except for an explicitly allowed transition.

A merge that carries unchanged bytes creates no lineage version.
Concurrent updates rebase and regenerate. Missing history, a second
bootstrap, a pointer to an older-but-not-immediate version, or any
shrinking edge fails.

Hosted PR CI also compares HEAD directly with the resolved PR-base
artifact. This prevents a rewritten branch from manufacturing a smaller
self-consistent chain. The only missing-base exception is the initial
bootstrap PR: the base has no artifact and the candidate has exactly one
oldest bootstrap version. After that, absence is an error.

### 1.2 Reviewed snapshot anchor

An adjudicated set freezes in two changes:

1. the reviewed content lands while the artifact is `draft`;
2. a follow-up change records that adjudication commit and the complete
   enumerated identity set.

The checker reads the artifact at the recorded ancestor commit and
compares identities, not only a self-hash. A later re-baseline is an
explicit reviewed event; it cannot ride an implementation slice. Hosted
PR CI compares a global frozen snapshot with the trusted base so an
add-and-reanchor pair of branch commits cannot redefine it.

Every anchor check fails on insufficient clone depth. CI must fetch the
unique bootstrap and every recorded adjudication or transition commit.

## 2. A1 — accepted conformance state

The versioned artifacts are:

```text
ratchets/conformance-matches.v1.json.zst
ratchets/oracle-inputs.v1.json.zst
```

The immutable oracle-input manifest pins fixture bytes, matrix
expansion/options/libs, every oracle diagnostic record, genuine oracle
T4 hashes after A3 activation, the vendored `_tsc.js` SHA-256, and each
comparator schema version. It contains no tsrs output or accepted-tsrs
baseline.

The accepted artifact uses append-only lineage and stores, per fixture
and matrix key:

- matched T0 bucket identities for the fixed All, 2XXX, syntactic, and
  explicitly declared band/pass intersection views;
- T1-T3 matched buckets once each comparator activates;
- multiplicity-complete T0 buckets for every fixed view from the first
  version, independent of tier activation;
- T4 matched case identities after A3 activation.

For T1-T3, a bucket is accepted only when its complete oracle/tsrs
multisets agree. A multiplicity-complete bucket has equal oracle and
tsrs record counts after that view's fixed predicate. It is ratcheted
separately because a 2/2 bucket can regress to 2/1 while its T0 key stays
matched. T4 acceptance is a case identity in this lineage, never a
mutable accepted hash beside the golden.

Every gating run rejects either difference:

```text
accepted_matched_tier - current_matched_tier
accepted_multiplicity_complete - current_multiplicity_complete
```

Both must be empty. `ratchet update` adds only. `ratchet.toml` counts are
derived summaries and are verified against the artifact.

`--band 2xxx`, `--syntactic-only`, and All select their recorded fixed
views. A supported fixed intersection needs its own view. Exact A2 scope
is not a ratchet view: all-corpus accepted sets remain the monotonic
authority and supported metrics are recomputed live. `--limit` and
fixture-list runs project the selected view to executed fixtures and
still enforce both accepted subsets there. Ad-hoc filters are
report-only and cannot update or satisfy a ratchet.

The normal path does not change inputs. Only these reviewed transitions
are allowed:

- `universe-transition` adds enumerated fixtures/records while every old
  identity and byte remains unchanged. After A2 global freeze, additions
  must be supported; the transition cannot create an exclusion.
- One A2 and one A3 `input-schema-extension` may add only their declared
  derived identity fields or oracle T4 fields/comparator entry. Existing
  oracle values and active comparator entries remain byte-identical.

A vendor upgrade or comparator-semantic change is a separate project,
not one of these transitions.

Acceptance:

```sh
cargo xtask ratchet check
cargo xtask ratchet check --baseline origin/main
cargo xtask conformance
cargo xtask conformance --band 2xxx
cargo xtask conformance --syntactic-only
cargo xtask ci
```

## 3. A2 — exact scope state

Schema 2 identifies one oracle diagnostic occurrence as:

```text
fixture + matrix_key + pass + file + start + length + code + category
+ message-chain hash + related-information hash + occurrence
```

Line and column are redundant review fields verified against `start`;
they are never identity. `occurrence` is zero-based after stable sorting
by complete canonical record bytes excluding occurrence, with
byte-identical neighbors retaining oracle input order.

Rust and Node share a versioned canonical encoder: UTF-8, fixed object
field order, decimal integers, JSON string escaping, no insignificant
whitespace, and observable array order. Message-chain bytes recursively
contain text, code, category, and children. Related-information bytes
contain the same diagnostic fields plus normalized virtual file/span.
Hashes are lowercase SHA-256. Changing this encoding requires A2's one
schema extension; it is not a silent identity edit.

The selector removes exact oracle records before supported comparison.
It never removes an entire T0 bucket accidentally. Syntactic diagnostics
are non-excludable. Schema 1 is rejected with a migration message.

### 3.1 Draft band pins

A band-freeze record contains the band, adjudication commit, and complete
enumerated identity set. Count/hash are derived. It follows the reviewed
snapshot protocol. While the manifest remains `draft`:

- current identities in that band must be members of the pinned set, so
  additions and edits fail;
- the pinned set must equal the band subset at its adjudication commit;
- a pinned identity may disappear only with a tombstone.

The phase-9 2XXX sweep uses this mechanism before M7 starts. Other
unpinned bands remain reviewable.

### 3.2 Resolution tombstones

A tombstone contains the exact identity and a resolving commit that is
an ancestor of HEAD. Its standing proof is A1 membership using the
applicable full-corpus fixed view:

- T0 membership is sufficient for a singleton bucket;
- a duplicate T0 bucket must also be multiplicity-complete, otherwise a
  match cannot prove which occurrence resolved;
- the early 2XXX pin reads A1's 2XXX view; global freeze reads All;
  partial-fixture and supported projections cannot prove resolution.

A1's append-only lineage keeps this proof live on a fresh clone. A
resolved exclusion returns to the supported denominator; no historical
build artifact is needed. The proof is invalid unless A1's vendor,
oracle-input, and comparator pins verify against the current tree.

### 3.3 Global freeze

At M7 close, all remaining exclusions land while `draft`. A follow-up
change adds one global-freeze record with the adjudication commit and
complete live identity set, then changes status to `frozen`.

The audit re-verifies every band pin and the global snapshot. After
freeze, additions, edits, pinned-set changes, reanchoring, and status
downgrade fail. A deletion requires the same A1 tombstone. The global
set never changes. PR CI requires the base and HEAD global records to be
byte-identical after the first valid transition. That first transition
is allowed only when the trusted base is `draft` and the candidate
contains exactly one valid global record.

Acceptance:

```sh
cargo xtask scope audit
cargo xtask scope audit --baseline origin/main
cargo xtask conformance
cargo test -p tsrs2-conformance
```

The 68 duplicate T0 buckets in the adopted corpus are permanent
canaries; 65 are in 2XXX. The scope audit must exercise them.

## 4. A3 — T4 activation

The oracle formatter is the explicit vendored tsc formatter path, with
normalized virtual paths, fixed newlines, and oracle sort/dedupe order;
it is not a hash of serialized diagnostic JSON. Golden schema 3 stores
oracle records and genuine oracle rendered hashes only.

A3's one-time input-schema extension fills the previously absent T4
field without changing any earlier oracle byte. Afterwards
`oracle-refresh --render-hashes --check` is check-only for the fixed
universe. Conformance computes the current tsrs hash; accepted T4 case
identities grow through A1.

T4 activates only after A2 global freeze and zero live `resolved`
entries. Supported T4 formatting applies exact scope before rendering;
all-corpus output and absolute FP=0 remain visible.

Required formatter tests cover ordering, adjacent dedupe, UTF-16 spans,
chains, related information, file-less diagnostics, suggestions,
platform-independent paths, and CRLF input.

## 5. A5 — family ownership and supported rollup

The map's domain is every corpus-exercised `(code, pass)` row:

- codes 2000-2999 belong wholesale to the 2XXX band family and may not
  appear in the enumerated map;
- every other row appears exactly once under an enumerated owner family;
  bare code is not a key because one code may span passes.

The map starts `draft` and freezes through the reviewed snapshot
protocol. Its anchored base rows and canaries require full equality, not
subset. Status downgrade, owner movement, or canary edits fail. A
legitimate correction is a reviewed re-baseline event.

An A1 universe transition introducing a new `(code, pass)` row uses a
separate anchored `universe-extension` record. It adds only new domain
rows and owners; every old row remains byte-identical. Readiness fails
until each new non-2XXX row is assigned exactly once.

`families report` derives its rollup from one current full-conformance
observation and four verified inputs: A1, the immutable oracle manifest,
the exact A2 scope, and the frozen family map. It applies exact scope to
oracle and tsrs multisets, grades supported buckets, then groups by
owner. It never infers current supported matches from A1 summaries. A1
is the monotonic guard; the current observation supplies numerator,
denominator, FN, and canaries.

Acceptance:

```sh
cargo xtask families report
cargo xtask families check
cargo xtask ci
```

## 6. D2 — declaration identity and closure

Every named or anonymous function-like node uses:

```text
lexical-owner declaration path + declaration kind + name-or-anonymous
+ start + end + source-slice SHA-256
```

`name` is a review alias, never a key. Anonymous callbacks own their
diagnostics; the module body is the single `<top>` identity. Identifier
calls resolve through lexical bindings. Conservative property dispatch
may add every candidate, but candidates remain distinct nodes.

The inventory records exact declaration identities, shortest
direct-emitter paths, presentation SCCs, and hash-pinned generated/helper
rules. A direct site is a `Diagnostics.*` use except a `.code` membership
read. A `tsc-port` auto-match requires exact `tsc-span` and `tsc-hash`.
Every direct emitter has a milestone/family owner or reviewed out-of-scope
cause.

The schema-1 counts (607 direct-emitter names, 6,587 closure names,
5,204 unaccounted) are migration canaries only. They collapse 353
multi-declaration names, including 19 direct emitters; `visit` alone has
seven declarations. D2 regenerates all counts before adjudication.
Schema 1 cannot freeze or satisfy readiness.

The schema-1 code-census recipe also remains an explicit migration
cross-check: 1,731 distinct direct-emitter codes are the union of 1,551
codes present at real-function sites and 180 additional `<top>`-only
codes. The corpus has 746 codes (336 in 2XXX and 410 outside it), all of
which occur at a real-function direct site. `<top>` is a separate
module-evaluation marker and is never assigned as a B2 function.

Mechanical ownership is limited to:

1. 2XXX sites joined through the emission map after exact span/hash
   expansion;
2. exercised non-2XXX sites joined to A5 by `(code, pass)`; multi-pass
   ambiguity remains manual;
3. unexercised sites closed by declaration-level zero-hit evidence and
   D3 adjudication.

Every identity remains enumerated in the frozen dispositions. Executing,
porting, or dispositioning one declaration cannot close a same-named
sibling. The dispositions pin the exact inventory hash. Each D2 tooling
slice must reduce `unaccounted` monotonically and may not shrink the
inventory through an unexplained parser heuristic.

## 7. Required adversarial tests

The implementations must pin at least these failure classes:

| Contract | Must fail |
|---|---|
| A1 lineage | matched or multiplicity-complete identity removed while counts, implementation, and artifact are edited together |
| A1 lineage | shrinking intermediate version; non-immediate predecessor; stale hash; second bootstrap; missing history; branch chain smaller than PR base |
| A1 inputs | old oracle bytes edited/deleted; undeclared schema change; vendor/comparator pin drift |
| A1 views | 2/2 to 2/1 regression; syntactic FN hidden by semantic gain; fixed or partial view skipping its accepted subset |
| A2 identity | same T0 key but different span/message/occurrence conflated; Node/Rust canonical bytes differ |
| A2 pin | add/edit plus rewritten set/count/hash; non-ancestor or mismatching adjudication commit |
| A2 tombstone | proof absent, partial-view only, stale A1 pin, or duplicate bucket not multiplicity-complete |
| A2 global | unpinned-band edit after freeze; status downgrade; branch add-and-reanchor; unverified band pin |
| A5 map | unmapped/duplicate row; enumerated 2XXX row; owner/canary change after freeze; old owner change disguised as extension |
| A5 rollup | stale conformance/scope fingerprint; excluded duplicate neighbor lost; A1 summary substituted for current supported grading |
| D2 | same-name declarations share id, ledger closure, runtime counter, property-call evidence, or disposition |

Positive companions cover additions-only A1 updates, append-only universe
extensions, proven tombstone deletions, non-2XXX draft edits while only
2XXX is pinned, anchored A5 extensions, and a fresh-clone full check.
The frozen family-map canaries include cross-band ownership for 7027 and
6053 and both passes of 6133.
