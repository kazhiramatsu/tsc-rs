# Terminal residue protocol

Status: active execution supplement, extracted from the Phase-9 2XXX
close on 2026-07-26.

This document defines how a parity sweep changes method when its
supported false-negative set becomes a small, heterogeneous tail. It
supplements the mining loop in
[README.md](README.md#the-loop-per-stage-never-deviate), the
slice-fidelity rules in
[definition-of-done.md](definition-of-done.md#milestone-gates-vs-slice-fidelity),
and the identity contracts in
[measurement-integrity.md](measurement-integrity.md). The definition
of done remains authoritative for completion; this protocol owns the
last-mile execution method.

The central rule is:

> In the terminal tail, classify an exact diagnostic identity by the
> pipeline layer that stopped it. Do not infer the implementation owner
> from the diagnostic code.

Phase 9 closed with many code-2339 rows that had different semantic
owners. The last supported row did not require a new missing-property
verdict: the verdict was already exact, but the checked-JavaScript
publication boundary hid it because unrelated JSDoc provenance was
present. Code-bucket mining alone cannot distinguish that shape.

## 1. Entry and exit

### 1.1 Entry trigger

Enter terminal-residue mode when either condition is true after a full
re-measure:

1. the active supported FN set has at most 32 exact identities; or
2. two consecutive slices leave no remaining semantic-owner cluster
   larger than three exact identities.

The trigger changes the investigation method, not the completion gate.
Once entered, the sweep stays in terminal-residue mode even if a fix
reveals additional downstream rows. Re-run the exact classification;
do not return to code-level batching merely because the count grew.

The threshold is an operating default. A phase plan may choose a lower
number before implementation begins, but it must not raise the
threshold after seeing the residue.

### 1.2 Frozen work queue

At entry, write one full-band and one active-band conformance snapshot
to distinct immutable paths. The queue is the supported false-negative
identities in those snapshots, including multiplicity. For every row,
record:

- fixture and matrix key;
- exact oracle identity and active comparison tier;
- code, pass, span, message-chain identity, and related information;
- supported/excluded disposition;
- current partial-boundary evidence, if any;
- tentative tsc declaration owner and pipeline layer.

The queue is a planning view, not a new scope artifact. A2 remains the
scope authority. A previously missed exclusion is still a stop
condition and cannot be repaired by editing the queue.

### 1.3 Exit

A band-focused phase closes only when its named gate is satisfied. A
typical terminal close requires:

- supported T0 for the active band is 100%;
- supported FN for the active band is zero;
- all-corpus FP is zero in every measured band;
- exact T1/T2/T3 diffs have no lost identities in the all and
  supported views;
- ratchet, scope, family, ledger, escape, invariant, and CI gates pass.

Excluded oracle rows deliberately remain false negatives in the
all-corpus view. Likewise, supported false negatives outside the active
band may remain for a later phase. Neither fact prevents a correctly
scoped phase close. The result section must state the numerator,
denominator, FP count, and supported FN count instead of saying only
"100%".

## 2. Diagnostic pipeline classification

Every terminal row gets exactly one current blocking layer. A later
probe may move it downstream, but one slice must not claim several
layers without showing the dependency.

| Layer | Question | Required evidence |
|---|---|---|
| Producer | Does the required node, symbol, member, signature, type, flow fact, or resolution result exist? | AST shape, symbol/value declaration, type flags/data, member/signature inventory, or resolver result |
| Verdict | Given those facts, does the checker select the same diagnostic branch as tsc? | Smallest oracle/tsrs probe, emitter declaration, diagnostic code and exact semantic operands |
| Renderer | Can the selected verdict render the required type, span, chain, and related information? | T1/T2/T3 comparison and the exact missing display/chain arm |
| Publication | Was an exact internal diagnostic suppressed, reclassified, or omitted at an output boundary? | Diagnostic stream immediately before and after the boundary, plus the narrow publication predicate |
| Grading | Is the emitted row assigned to the correct scope, multiplicity bucket, tier identity, and accepted set? | A1/A2/A5 reports, exact identity diff, and ratchet evidence |

Parser recovery or an incorrect program shape is a preflight failure
upstream of Producer. When the Rust and oracle trees differ, transfer
the row to the recovery plan rather than fabricating semantic facts in
the checker.

### 2.1 Classification order

Investigate in pipeline order:

1. confirm the expanded program and AST/recovery shape;
2. inspect producer facts;
3. determine whether the verdict exists internally;
4. inspect renderer fidelity;
5. inspect publication;
6. inspect grading only after an exact diagnostic is observable.

Do not begin by adding a publication exception. If the internal verdict
or its type operands are wrong, exposing it creates a false positive or
a higher-tier regression.

### 2.2 Publication is a first-class implementation boundary

Checked-JavaScript, JSDoc, host-dependent, recovery, and directive
surfaces may compute more internal diagnostics than the public result
contains. Treat this filtering as code with semantic obligations, not
as harness cleanup.

A publication slice must prove all of the following:

- the upstream verdict is already exact at every live tier;
- the receiver/container member set is complete for this syntax shape;
- the predicate is expressed in semantic and structural facts, never a
  fixture name or a diagnostic-code-wide allowlist;
- neighboring open-ended, host-dependent, JSDoc-driven, or
  recovery-dependent shapes remain suppressed;
- only diagnostics emitted by the bounded operation are marked for
  publication.

Where the Rust implementation has a native publication adapter around
a ported tsc semantic owner, record both boundaries. The D2
`port-plan` may correctly report no exact Rust ledger join for the
adapter. That is not a waiver: the PR still names the primary tsc
declaration, its D2 identity/SCC, and the native Rust predicate being
changed.

## 3. Required shape proof

Before editing behavior, reduce the row to the smallest source that
preserves its blocking layer. Capture the following facts when they are
relevant:

- syntax kind of the diagnostic node and receiver;
- parent chain through the containing expression/declaration;
- assignment-declaration kind and assignment-target status;
- nearest containing function/class and whether `this` is lexical;
- resolved and merged symbol identities, flags, declarations, and
  value declaration;
- containing type flags, symbol, apparent type, and complete member
  source;
- source-file mode and checked-JavaScript state;
- JSDoc, directive, module-resolution, or recovery provenance;
- diagnostic count and identities immediately before the output gate.

The proof belongs in a unit pin or a concise code comment when it is
load-bearing. Large ad hoc debug dumps remain scratch evidence and are
not committed.

### 3.1 Positive and negative pins

The smallest positive pin proves the exact oracle row: code, span,
message text, chain/related data where live, and container display.

Add a negative or adjacent control whenever widening could admit a
neighboring shape. Typical controls are:

- an existing member on the same receiver;
- an assignment rather than a read;
- a function expression rather than an arrow;
- a static side rather than an instance side;
- a valid JSDoc-driven type rather than an invalid ignored tag;
- an open-ended JS expando rather than a complete member table;
- an excluded sibling row that must remain excluded.

Do not freeze a known divergence merely to create a negative test. If a
useful counterexample is itself an in-scope FN, record it as another
queue identity and either implement it in the same semantic-owner slice
or keep the narrower proof.

## 4. Source trivia and JSDoc rules

The complete M8 JSDoc subsystem now represents comments, tags, types, and
declarations as arena nodes attached to their real hosts. Production
publication and semantic proofs therefore use those exact `NodeId`s, parent
chains, spans, binder symbols, and checker types. They must not rescan source
text or reconstruct a tag from trivia.

Raw trivia remains useful only in parser/attachment differential tests that
prove how tsc selected a comment range. It cannot choose checker behavior,
publication, symbol identity, a type, or a diagnostic.

The following rules are mandatory:

1. Start from the ordinary host's attached JSDoc array and select the exact
   parsed tag kind/name/span used by tsc.
2. Prove the JSDoc parent chain and host relationship; a fixture-wide or
   nearest-comment guess is not attachment evidence.
3. Obtain semantic types and declarations through the normal binder/checker
   paths, never a transient comment projection.
4. A parser attachment test may normalize `pos` with the same trivia/comment
   range rules as tsc, but its result must be the arena attachment under
   test, not a second semantic channel.
5. Never use `source.contains("@tag")`, an unbounded prefix search, or a
   local activation guard as production proof.
6. Keep the accepted syntax no broader than the oracle-backed shape.

For example, a class-field arrow annotated with `@this` retains lexical
class `this` because the tag is invalid for an arrow. Publishing a
missing-member verdict requires proof of the arrow/property/class
relationship as well as the exact attached `JSDocThisTag` node and its
normal checker interpretation. Raw `@this` text alone is insufficient.

## 5. Slice construction

### 5.1 One owner per slice

One semantic owner equals one branch and one PR. A shared diagnostic
code, fixture directory, or publication function is not by itself a
shared owner.

A terminal slice may contain several exact identities only when one
predicate and one complete producer/verdict proof decides all of them.
If the fabrication audit finds a second receiver shape, declaration
kind, provenance source, or tsc emitter, split the slice.

### 5.2 Minimum implementation rule

Implement the narrowest reusable semantic predicate that explains the
oracle. It may depend on:

- exact syntax and parent relationships;
- resolved symbol identity and flags;
- declaration kind/value-declaration identity;
- complete member-source identity;
- assignment kind and read/write position;
- exact attached provenance.

It must not depend on:

- fixture or file basename;
- source line/column constants;
- a diagnostic code without its semantic operands;
- a broad "checked JS" or "has JSDoc" flag when only one provenance
  path is proven;
- a current corpus count.

### 5.3 D2 evidence

After D2a, every semantic slice runs `cargo xtask port-plan` for its
primary tsc declaration. Record:

- declaration ID and lexical path;
- SCC ID and member count;
- exact Rust ledger joins;
- static callers/callees and unresolved calls;
- joined escape/dormant rows.

For a native publication adapter, add a separate line naming the Rust
helper and why it has no one-to-one tsc declaration. Do not create a
fake ledger join to make the report look complete.

## 6. Verification sequence

Run the sequence below without piping a gate through `head`, `tail`, or
`grep`. Before/after JSON paths are immutable evidence for the slice.
The `slice-evidence` commands in §7 automate the observations, exact
diffs, and read-only repository checks. The individual commands remain
documented here both as the contract and as a diagnostic fallback.

### 6.1 Before implementation

```text
cargo xtask conformance \
  --files <comma-separated-targets> \
  --band <active-band> \
  --out-json /tmp/<slice>-target-before.json

cargo xtask conformance \
  --band <active-band> \
  --out-json /tmp/<slice>-band-before.json

cargo xtask conformance \
  --band all \
  --out-json /tmp/<slice>-all-before.json
```

### 6.2 After the unit pin and implementation

Run the target test, then:

```text
cargo xtask conformance \
  --files <comma-separated-targets> \
  --band <active-band> \
  --out-json /tmp/<slice>-target-after.json

cargo xtask conformance-diff \
  /tmp/<slice>-target-before.json \
  /tmp/<slice>-target-after.json \
  --out-json /tmp/<slice>-target-diff.json

cargo xtask conformance \
  --band <active-band> \
  --out-json /tmp/<slice>-band-after.json

cargo xtask conformance-diff \
  /tmp/<slice>-band-before.json \
  /tmp/<slice>-band-after.json \
  --out-json /tmp/<slice>-band-diff.json

cargo xtask conformance \
  --band all \
  --out-json /tmp/<slice>-all-after.json

cargo xtask conformance-diff \
  /tmp/<slice>-all-before.json \
  /tmp/<slice>-all-after.json \
  --out-json /tmp/<slice>-all-diff.json
```

The target must move exactly as predicted. Full active-band and
all-band reports must have FP=0. Every live T1/T2/T3 view must report
lost=0; investigate every gained identity outside the target rather
than assuming it is beneficial.

### 6.3 Ratchet and repository gates

Only after the identity diffs are reviewed:

```text
cargo xtask ratchet update
cargo xtask ratchet check --baseline origin/main
cargo xtask scope audit --baseline origin/main
cargo xtask families check --baseline origin/main
cargo xtask ledger check
cargo xtask escapes --stale "$(cat STAGE)"
cargo xtask ci --baseline origin/main
```

`ratchet update` is never a regression repair. If it removes an
accepted identity or the diff shows an unexpected loss, fix the
implementation and regenerate from the clean accepted state.

### 6.4 PR evidence

Every terminal PR body and result section records:

- target before/after in all and supported views;
- active-band T0 numerator/denominator, FP, and supported FN;
- all-band T0 numerator/denominator and FP;
- T1/T2/T3 lost/gained counts for both scope views;
- accepted matched and multiplicity-complete deltas;
- relevant unit/binder/syntax counts;
- ledger and escape counts;
- D2 plan summary;
- the full CI command and exit status.

## 7. Evidence automation

`slice-evidence` implements the repeated report-only sequence as two
explicit lifecycle operations. Before editing behavior, capture the
immutable baseline:

```text
cargo xtask slice-evidence snapshot \
  --slice <name> \
  --targets <comma-separated-targets> \
  --band <active-band> \
  --out-dir /tmp/<slice>-before
```

After the unit pin and implementation, verify against that snapshot:

```text
cargo xtask slice-evidence verify \
  --before-dir /tmp/<slice>-before \
  --out-dir /tmp/<slice>-after \
  --baseline <trusted-ref>
```

Both output paths must be new directories outside the Git worktree.
The command refuses to overwrite an existing directory. Keeping the
two lifecycle operations separate prevents an implementation-time run
from silently replacing its own baseline.

The before manifest records the slice, sorted target set, active band,
Git commit and worktree fingerprint, controlled-input hashes, three
observation summaries, per-step exit codes, artifact hashes, and
complete stdout/stderr logs. `verify` first checks every before report
and log hash, then writes target, active-band, and all-band after
observations and exact T1/T2/T3 diffs. When the active band is `all`,
the identical active/all full-corpus observation is executed once and
copied with a separately hashed reuse log.

The final after manifest embeds the before manifest path and hash plus
its Git/input fingerprints, steps, and observation summaries. It is
therefore the single PR evidence index; the referenced before
directory still remains immutable and supplies the original reports
and logs.

Verification fails and preserves its partial manifest on:

- a non-zero observation, diff, or repository-check exit;
- any false positive;
- any lost T1/T2/T3 identity in either scope view;
- a changed supported oracle universe;
- a stale or modified before report or log;
- mutation of the controlled inputs or Git worktree while the command
  runs.

On clean diffs it runs `ratchet check`, `scope audit`, `families
check`, `ledger check`, the current-stage `escapes` audit, and the
invariant suite, each in its own logged child process. It never runs
`ratchet update` and never writes scope, golden, family, exclusion, or
ratchet state. Full `cargo xtask ci` remains the final merge gate
because repeating CI inside this report would duplicate the
full-corpus checker work already captured here.

Gains outside the target do not represent a mechanical regression, so
the command does not approve or reject them. It writes
`status = "review-required"` plus per-tier counts in the after manifest;
the implementer reviews the exact identities in the diff artifacts and
records the disposition in the PR evidence.

Automation reduces transcription and waiting overhead. It does not
choose the semantic owner, disposition a row, or approve a gain.

## 8. Stop conditions

Stop the slice and return to investigation when any of the following
occurs:

- the exact row cannot be assigned to one current pipeline layer;
- the Rust and oracle program/AST recovery shapes differ;
- publication would expose a verdict whose type, span, display, chain,
  or related information is not exact;
- the proposed predicate needs a fixture name, location constant, or
  diagnostic-code-wide exception;
- no negative boundary can be stated for an open-ended or
  provenance-sensitive widening;
- the supported universe changes unexpectedly;
- an accepted T1/T2/T3 identity is lost;
- a previously missed exclusion is discovered;
- source-trivia attachment cannot be proven without ambiguous text
  matching;
- the primary tsc semantic owner or native-adapter boundary cannot be
  named.

Hard, slow, or one-row work is not a stop condition. Terminal residue
is expected to consist of small, unrelated semantic proofs.

## 9. Phase-9 lessons carried forward

The Phase-9 close validated these choices:

- exact supported-scope pinning prevented late exclusions from
  redefining success;
- one semantic owner per PR kept dozens of small residue fixes
  reviewable and made regressions attributable;
- exact shadow-tier identity diffs prevented equal aggregate counts
  from hiding substitutions;
- all-corpus FP=0 kept narrow checked-JS publication predicates honest;
- excluded JSDoc rows remained visible in the all view while supported
  2XXX reached 100%.

It also exposed three corrections to the generic mining loop:

1. diagnostic code is too coarse for a heterogeneous terminal tail;
2. output publication must be classified separately from verdict
   generation;
3. source trivia positions were useful attachment evidence during the
   Phase-9 investigation, but M8 proved that production JSDoc semantics
   require the complete parser/arena/binder/checker representation; later
   slices inspect the attached nodes rather than preserving a trivia scanner.

Future phase plans should link this protocol when they define a
zero-supported-FN close gate.
