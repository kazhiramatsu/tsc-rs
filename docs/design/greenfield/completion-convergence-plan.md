# Completion convergence plan

Status: active execution plan. Adopted after the 2026-07-16 full-project
review.

This document owns delivery order, dependencies, and acceptance gates.
It intentionally does not repeat supporting schemas:

- [definition-of-done.md](definition-of-done.md) owns WHAT Done means;
- [measurement-integrity.md](measurement-integrity.md) owns A1/A2/A3/A5
  and D2 identity, lineage, freeze, and adversarial-test details;
- [evidence-and-steady-state.md](evidence-and-steady-state.md) owns
  B1-B4 production and M9 nightly evidence;
- milestone steps docs own TypeScript semantics inside their stages.

The governing rule is:

> Correctness progress is a growing set of proven matches, not a growing
> integer that can exchange one correct diagnostic for another.

## 1. Starting point

Planning baseline: `main` commit `52c47bbb`, after M4 5.9c.

| Signal | Baseline |
|---|---:|
| All-corpus T0 | 20,052 / 48,719 (41.1585%) |
| T1 / T2 / T3 shadow | 41.1523% / 38.3177% / 37.0820% |
| All-corpus FP / FN | 0 / 28,667 |
| 2XXX T0 | 10,921 / 20,916 (52.2136%) |
| Non-2XXX T0 | 9,131 / 27,803 (32.8418%) |
| Relation pins | 415 / 415 |
| Escape sites | 258 (stale 0, untagged 9, recovery 112) |
| M8 readiness at adoption | 2 / 9 legacy rows ready |
| Rust function dispositions | 273 unresolved |

There are 48,821 oracle records but 48,719 T0 buckets. Sixty-eight
buckets in 58 cases are duplicates, with 102 extra records. Scope and
resolution proof therefore operate on diagnostic occurrences, not only
T0 keys. Legacy name-collapsed closure counts are migration canaries;
D2 regenerates declaration-identity denominators.

## 2. Non-negotiable invariants

All work preserves these invariants:

1. all-corpus FP remains zero;
2. every accepted match and multiplicity-complete bucket is monotonic;
3. syntactic T0 is independently gated on every merge;
4. scope selects exact oracle records and never hides a neighbor;
5. a producer artifact, not a hand-written count/boolean, proves evidence;
6. M8 readiness and final completion are separate executable gates;
7. a stage advances only after its expiring escapes are implemented or
   reassigned with reviewed evidence;
8. every exercised `(code, pass)` row has exactly one family owner: the
   2XXX band is the sole range owner and all non-2XXX rows are enumerated.

## 3. Workstreams and dependencies

```text
A1 accepted sets -> A2 exact scope -> A3 T4 -> A4 Done gate
                 \-> A5 family rollup --------------------+
                                                          |
M4 -> M5 -> M6 -> 2XXX sweep -> M7 -> bounded M8 -> recovery zero -> M9
                      ^          |
B1 evidence -> B2 coverage ------+
            -> B3 fuzzer --------+
            -> B4 perf/RSS ------+
D1 Rust ledger ----------------------------------------------+
D2 declarations -> D3 runtime/static converse ---------------+
E1 CI/toolchains + E2 current docs --------------------------+
```

A1 lands first. Later A slices follow §4; B, D, and E may then run beside
semantic work, but their readiness rows must finish before M7 closes.

### Track A — trustworthy measurement

#### A1. Set-monotone conformance

Land versioned accepted-match and immutable-oracle-input artifacts.
Gate All, 2XXX, syntactic, active T1-T4, and multiplicity-complete sets
against recursive history and the trusted PR base. Add the permanent
syntactic CI gate. Updates add identities only; corpus growth and the A2
and A3 schema activations use the named append-only transitions in the
[integrity contract](measurement-integrity.md#2-a1--accepted-conformance-state).

Acceptance: `ratchet check`, its `--baseline` form, All/2XXX/syntactic
conformance, and `cargo xtask ci`.

#### A2. Exact scope identity

Replace schema-1 T0 keys with exact schema-2 diagnostic occurrences.
Implement canonical Node/Rust identity encoding, draft band pins,
standing A1 tombstones for resolved exclusions, and the two-step global
freeze. The 2XXX band pins during phase 9; global freeze occurs at M7
close. Details and required attacks are fixed by the
[integrity contract](measurement-integrity.md#3-a2--exact-scope-state).

Acceptance: `scope audit`, its trusted-base form, conformance, and the
conformance crate tests. Schema 1 cannot freeze or satisfy readiness.

#### A3. Real T4

M7 lands deterministic formatter structure; M8 activates byte parity.
Oracle T4 hashes come from the vendored formatter, enter the immutable
oracle manifest through A3's one schema extension, and remain check-only
afterwards. Accepted T4 cases grow through A1. See
[T4 activation](measurement-integrity.md#4-a3--t4-activation).

M7 acceptance:

```sh
cargo xtask oracle-refresh --render-hashes --check
cargo xtask conformance --tier t4 --report-only
cargo xtask ci
```

#### A4. Executable completion gate

Add `cargo xtask completion` early in report-only form and
`cargo xtask completion --require-done` as the post-M9 release gate. The
strict command requires:

1. all-corpus FP zero;
2. globally frozen, fresh exact scope with no resolved entry;
3. supported T0-T3 at 100%;
4. every supported T4 case byte-exact;
5. syntactic diagnostics fully in scope;
6. zero escapes and an empty escape manifest;
7. a fresh, complete Rust function ledger;
8. a fresh, frozen declaration-identity inventory and dispositions;
9. current B1-B4 evidence within approved performance/RSS ceilings;
10. `cargo xtask invariants --suite all --full-corpus` green;
11. M9 steady state green with zero open signature.

The report writes `target/completion/report.json`. A sampled PR
invariant run cannot satisfy row 10. A required regression keeps rows
1-9 and 11 green while making one full-corpus invariant red; strict mode
must fail and name that invariant.

#### A5. Family ownership and rollup

Turn [non-2xxx-first-order.md](non-2xxx-first-order.md) into a frozen
machine map and `families report/check`. Every non-2XXX `(code, pass)` row
is enumerated once; 2XXX is owned only by its band partition. The rollup
is recomputed from current full conformance after exact A2 scope, with A1
as monotonic guard. The map is identity-anchored and grows only through
reviewed universe extensions. See
[family ownership](measurement-integrity.md#5-a5--family-ownership-and-supported-rollup).

Acceptance: `families report`, `families check`, and CI.

### Track B — produced evidence

The [evidence contract](evidence-and-steady-state.md) is the single
authority for all four producers:

| ID | Deliverable | Required result |
|---|---|---|
| B1 | common producer fingerprint and artifact reader | fresh-clone, same-workspace evidence; no editable readiness claims |
| B2 | declaration-level runtime emitter coverage | every direct emitter executed or reviewed zero-hit |
| B3 | deterministic generator/oracle/reducer/deduper | current smoke before M8; versioned 14-window steady state at M9 |
| B4 | wall/RSS child-process measurement | observations within ceilings on an approved runner |

`cargo xtask m8 evidence produce --all` invokes producers; it cannot
synthesize their observations.

### Track C — semantic completion

#### C1. Close M4

After A1, finish 5.9d/5.9e: modules/JSX/interop residue, zero untagged
escapes, no owner `<= 5.8`, and the close-only `STAGE=5.9` bump. Gate:
T0 >= 35%, FP=0, accepted sets and syntactic gate green.

#### C2. M5 flow

Follow [m5-flow-steps.md](m5-flow-steps.md). TS2454 and adjacent
nullable/reachability families are the opening band. Aggregate 50% does
not replace flow canaries, set monotonicity, idempotence,
jobs-independence, or unsupported-unwind.

#### C3. M6 inference and overloads

First land the scoped speculation transaction API and rollback tests in
[m6-inference-calls-steps.md](m6-inference-calls-steps.md). Then land
stages 7.1-7.5. No direct link-table write is allowed under speculation;
the start gate proves both cache and diagnostic rollback.

#### C4. M7 tail

No M7 stage starts until §4 row 9 closes the 2XXX sweep: all-corpus band
FP=0, supported-scope band FN=0, every matrix point covered, and exact
band exclusions pinned under A2.

Follow stages 8.1-8.5. Each closes on its A5 family rows and canaries,
never the aggregate 63% calibration point:

- 8.1 checker grammar;
- 8.2 suppression audit and canaries;
- 8.3 unused error rows;
- 8.4 suggestion rows plus T1 activation;
- 8.5 program/resolution rows plus A3 formatter structure.

M7 closes only when A2 is globally frozen, A3 structure is live, B1-B4
evidence is current, D1-D3 are complete, every M7-owned family has
supported FN=0, and all ten
[M8 readiness rows](m8-readiness.md#machine-gate) pass.

#### C5. Bounded M8 mining

Each branch declares one family, oracle anchors, fixtures, expected
escape/disposition removals, and tier. The entry report fixes the family
residual snapshot; every slice reports its family before/after against
that snapshot, never against a moving top-FN list. Work in order: T0
family residue, T1 category, T2 span/top message, T3 chain/related
information, T4 rendering, recovery, then the final emitter/dependency
converse.

Every slice removes an exact mismatch or measured prerequisite. Three
probes exposing the same model ceiling trigger the
[stall playbook](../stall-playbook.md), not more local patches.

#### C6. Recovery zero

Move every graceful recovery off `Unsupported` using a deterministic
error/unknown type, missing-node control-flow result, oracle-compatible
no-diagnostic value, or proved parser invariant. The close empties
`escapes.toml`, sets both ceilings to zero, and runs fuzzer crash and
determinism suites.

### Track D — completeness converse

#### D1. Rust dispositions

Burn down the Rust function backlog with its owning semantic stages.
Native helpers require evidence. New undispositioned functions remain
forbidden; the backlog is zero before M8 readiness.

#### D2. Declaration-level tsc closure

Regenerate the tsc inventory with exact declaration identities, lexical
call resolution, conservative-but-distinct property candidates, shortest
emitter paths, review SCCs, exact ledger joins, and an explicit owner for
every direct emitter. Name-collapsed schema 1 remains migration input
only. The full identity and ownership rules live in
[the integrity contract](measurement-integrity.md#6-d2--declaration-identity-and-closure).

#### D3. Runtime/static reconciliation

Join B2 runtime hits to D2 closure. Every direct emitter is executed or
has zero-hit evidence; every closure identity is ported, deferred, or
not applicable. Contradictions—such as an executed not-applicable emitter
or an emitter without a family—fail.

### Track E — reproducible operation

#### E1. CI and toolchains

Pin Rust/clippy and Node; require `cargo xtask ci`; fetch all anchor
history; run trusted-base ratchet/scope checks; produce and consume
readiness evidence in one workspace; schedule signed fuzz windows; and
reserve an approved runner for performance and release. The exact job
topology is in the
[evidence contract](evidence-and-steady-state.md#5-required-ci-topology).
Local commands own gate logic; workflow YAML only invokes them.

#### E2. Current documentation

Archive or redirect v1 instructions, make setup and docs routing current,
remove stale active checklists, resolve landed 5.8 boxes, and generate
README status from conformance/readiness output.

## 4. Required landing order

Phase numbers are content identities. This table alone owns landing
order: a slice starts only after every earlier row required before it is
green.

| Order | Slice | Required before |
|---:|---|---|
| 1 | A1 set ratchet + syntactic CI | further large semantic work |
| 2 | M4 5.9d / 5.9e close | M5 |
| 3 | A2 exact scope identity | scope classification/freeze |
| 4 | A5 family map + non-2XXX rollup | M5 close |
| 5 | E1 hosted CI + toolchains | M5 close |
| 6 | E2 documentation cleanup | M5 close |
| 7 | M5 flow | M6 |
| 8 | M6 transaction precondition, then M6 | M7 |
| 9 | pin 2XXX scope, then sweep to all-corpus FP=0 and supported FN=0 | M7 |
| 10 | B1 evidence protocol + D2 closure tooling | M7 close |
| 11 | B2 coverage + B3 fuzzer + B4 performance | M7 close |
| 12 | M7 stages 8.1-8.5 including A3 structure; content complete, not closed | M7 close prerequisites |
| 13 | D1-D3 complete + A2 globally frozen | M7 close |
| 14 | M7 close: `m8 readiness --require-ready` | first M8 slice |
| 15 | A4 report-only completion gate | early M8 |
| 16 | bounded M8 tiers + recovery zero | M9 |
| 17 | M9 14-window steady state + zero open signatures | `completion --require-done` |

Rows 7-8 deliberately land flow before full inference: flow has the
larger measured unlock family and M6 has the transaction start gate. The
baseline counts and decision record remain in the
[2XXX phase plan](2xxx-first-order.md#phase-plan).
Evidence begins before M7 so milestone close is not an infrastructure
cliff.

## 5. Per-slice review template

Every implementation PR records:

```text
Owner milestone / tier:
tsc anchors and vendored hash:
Exact fixtures or generated signatures:
Accepted-set additions (removals MUST be 0):
All FP; All / 2XXX / syntactic T0; active/shadow T1-T4:
Escapes, ledgers, closure, and runtime rows before -> after:
Tests and evidence artifact hashes:
```

For docs-only or pure tooling work, mark semantic fields not applicable
but still run CI when gate behavior can change.

## 6. Stop conditions

Stop and review the design if an accepted identity must be removed, an
exclusion cannot select exactly one record, evidence can pass without its
producer, a fuzz failure cannot replay, closure loses an identity without
a rule, three fixes hit one model ceiling, an aggregate passes while its
family/canary is red, or required toolchain/CI pins do not cover `main`.

## 7. Completion checkpoints

- M4 close: T0 >= 35%, FP=0, untagged/stale zero.
- M5 close: flow canaries and invariants green.
- M6 start: scoped transaction rollback proof exists.
- M7 start: row 9's 2XXX band gate and A2 pin are green.
- M7 close: all ten M8 readiness rows are produced and verified.
- M8 midpoint: accepted sets still grow and no repeated architectural
  ceiling is being patched locally.
- M9: 14 consecutive current-fingerprint windows, fewer than one new
  distinct signature per night, and zero open signature.

A failed checkpoint triggers design review. It never permits a weaker
denominator, count-only ratchet, broad exclusion, or hand-written
evidence claim.
