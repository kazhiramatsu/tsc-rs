# M8 execution and close contract

Status: M8 closed; M9 steady-state execution is active. The completed close
state and handoff are recorded below.

This page owns how M8 was executed and closed. M8 started only after
[`m8 readiness --require-ready`](m8-readiness.md#machine-gate) was green.
The [completion convergence plan](completion-convergence-plan.md) still owns
cross-track landing order, [measurement integrity](measurement-integrity.md)
owns accepted sets, exact scope, T4 activation, and D2 identities, and the
[definition of done](definition-of-done.md) still owns the final project
contract.

Readiness and completion are deliberately different:

- `ready=true` authorized the first M8 slice;
- M8 closed the supported diagnostic and recovery work;
- M9 proves differential-fuzzer steady state;
- only then may `cargo xtask completion --require-done` pass.

The checked-in `STAGE` marker records the last closed milestone. It remained
`M7` throughout M8 and changed to `M8` in the M8 close slice.
`m8 readiness --require-ready` opening M8 did not move the marker.

## Entry baseline

The M7-close observation at commit
`8873bb74a2911a38ca2da6b6c305f7353bd3b31d` is the human-readable planning
baseline:

| Signal | M8 entry |
|---|---:|
| Supported T0 | 48,108 / 48,441 (99.3126%) |
| Supported T0 residual | 333 |
| Supported T1 residual | 335 |
| Supported T2 residual | 1,516 |
| Supported T3 residual | 3,450 |
| All-corpus FP | 0 |
| Exact scope exclusions | 583 |
| Escape sites | 193 |
| Escape-manifest rows | 176 |
| T4 | inactive; A3 activation pending |

The 193 escape sites comprise 75 M8 stage occurrences, 117 recovery
occurrences, and one dormant-assumption occurrence; their manifest
representation has 65, 110, and one row respectively. Counts on this page are
not a ratchet and never replace exact identities. The first M8 planning slice
produces and freezes a machine-readable entry report against the current
accepted-set, scope, family-map, D2, and corpus fingerprints. Later slices
report before/after against that fixed entry universe rather than a moving
top-FN list.

The conformance JSON field `supported_false_negative_identities` is the
authoritative seed for that report. It carries every non-excluded oracle
occurrence in a missing supported T0 bucket as a schema-2 exact identity.
The entry baseline has 333 such unique identities: 320 semantic, nine
suggestion, and four syntactic. Plan generation must consume this field
directly; aggregate diagnostic-code lists and rendered T0 keys are
insufficient substitutes.

The supported T0 residual starts in six owner families:

| Family | Supported FN | Entry canaries |
|---|---:|---:|
| `parser-pragma-residue` | 4 | 0 / 1 |
| `flow-strict-nullability` | 11 | 3 / 3 |
| `module-semantics-tail` | 39 | 1 / 3 |
| `override-validation` | 42 | 0 / 2 |
| `checkjs-jsdoc` | 86 | 0 / 3 |
| `implicit-any` | 151 | 2 / 3 |

Family names are scheduling labels, not scope selectors. In particular,
`checkjs-jsdoc` contains supported exact identities even though other
JSDoc-driven identities have reviewed `jsdoc-semantics` exclusions. No whole
family, code, pass, fixture, or directory may be excluded.

## Completed landing order

M8 executed these steps in order:

1. **Completion report** — implement `cargo xtask completion` in report-only
   form. It must enumerate every strict completion row and show why
   `--require-done` cannot yet pass.
2. **Frozen residual plan** — materialize the exact M8 entry residual and map
   every supported mismatch to its family, exact D2 producer cluster, Rust
   boundary, live tier blockers, and owning slice.
3. **Bounded T0 closure** — close the supported T0 residual one
   dependency-closed producer slice at a time while preserving all accepted
   identities and all-corpus `FP=0`.
4. **Formal tier closure** — first close the report-only supported T1, T2,
   and T3 residuals in the convergence plan's order. Then run one fresh full
   measurement and record
   `ratchet update --transition tier1-3-input-schema-extension`. The
   transition changes the oracle-input manifest's T1-T3 entries atomically
   from explicit `absent` to comparator schema v1 and stores, for every
   fixed view / fixture / matrix / T0 bucket, the independently graded
   complete-multiset T1, T2, and T3 identities. The nesting
   `T3 ⊆ T2 ⊆ T1 ⊆ multiplicity-complete ⊆ T0` is validated, and all fixed
   views plus partial fixture projections thereafter reject exact
   `accepted - current` losses at every active tier. `ratchet.toml`'s
   T1-T3 matched/total/rate values are verified artifact-derived summaries,
   not another authority. A touched family still follows the vertical
   slice-fidelity rule; the global sweep is not permission to add a
   knowingly wrong category, span, message, chain, or related-information
   shape. The all-band conformance JSON includes
   `supported_tier_mismatches`, partitioned by `first_failed_tier`, with the
   complete expected and actual bucket shapes. This report-only residual was
   the scheduling input before activation; aggregate tier counts and
   matched-only identity lists are not sufficient to assign an
   implementation owner.
5. **A3 T4 closure** — activate the rendered-output comparator through the
   reserved one-time `t4-input-schema-extension`, then close byte parity for
   every supported case, including ordering and deduplication. It is not a
   universe transition and cannot ride corpus growth.
6. **Recovery and converse close** — empty `escapes.toml`, keep the frozen D2
   inventory/dispositions and current B1-B4 evidence fresh, and run the full
   invariant suite.
7. **M8 close** — required completion rows 1-10 to be green with only M9
   steady state pending, then updated `STAGE` from `M7` to `M8`.

Steps 3-5 describe the global activation order. Within them, a prerequisite
slice may legitimately add no accepted diagnostic, but it must name the
immediately consuming family, carry direct semantic pins, and leave every
active accepted set unchanged.

## Owner-cluster construction

M8 generalizes the successful 2XXX and M7 owner strategy. Numeric diagnostic
bands and same-named functions are review aids only; they are not slice
boundaries.

For each exact mismatch:

1. collect its diagnostic-time trace across applicable fixture matrices and
   minimal oracle probes;
2. compare declaration coverage with the nearest valid non-emitting sibling
   probe;
3. seed exact D2 declaration identities, never printed function names;
4. expand through the static D2 call graph to the nearest already-ported or
   reviewed-disposition boundary;
5. retain property-call candidates separately instead of name-collapsing
   them;
6. merge repeated stacks or static SCCs only when they describe one semantic
   subsystem;
7. assign the resulting dependency-closed cluster, exact Rust boundary, and
   upper-tier blockers to one slice.

Runtime evidence chooses and explains implementation clusters. It never
proves the static converse: an unexecuted declaration remains open until the
frozen inventory gives it an exact reviewed disposition.

The low-level evidence command is:

```bash
CARGO_BUILD_JOBS=2 cargo xtask m8 trace \
  --program-json target/emitting/program.json \
  --program-json target/non-emitting/program.json \
  --code 8020 \
  --out target/m8-trace-8020.json
```

It is deliberately targeted and report-only. The instrumenter validates
already-inventoried diagnostic-site offsets and performs no `_tsc.js` AST
parse or visit. Each probe records the diagnostic-time stack and exact D2
declarations observed by V8 precise coverage; the command also requires the
instrumented diagnostic JSON to equal the ordinary oracle byte-for-byte.
Library SourceFile caches reset between probes so coverage does not depend on
whether the emitting or non-emitting sibling ran first. Each probe also uses
its own single-threaded Node process; V8 lazy-compilation state from one probe
therefore cannot alter the next probe's declaration set.
Instrumentation is content-addressed by the source, D2 inventory, trace
tools, and selected codes, so repeated sibling comparisons reuse the bundle.
The command does not select or approve a sibling, truncate static closure, or
freeze an owner cluster. Those remain plan-generator and review decisions.

The entry-plan commands are:

```bash
CARGO_BUILD_JOBS=2 cargo xtask conformance \
  --band all \
  --out-json target/m8/entry-conformance.json
CARGO_BUILD_JOBS=2 cargo xtask m8 plan draft \
  --conformance-json target/m8/entry-conformance.json \
  --sibling-fixture 'conformance/node/nodeModulesTripleSlashReferenceModeOverride4.ts#module=node16' \
  --out target/m8/owner-plan-draft.json
CARGO_BUILD_JOBS=2 cargo xtask m8 plan apply-review \
  --plan target/m8/owner-plan-draft.json \
  --review m8-owner-plan-review.json \
  --out m8-owner-plan.json
# Land the reviewed draft, return to a clean main, then freeze it in place.
CARGO_BUILD_JOBS=2 cargo xtask m8 plan freeze \
  --plan m8-owner-plan.json
CARGO_BUILD_JOBS=2 cargo xtask m8 plan check \
  --plan m8-owner-plan.json \
  --baseline origin/main
```

The full conformance command is run once for the entry snapshot, not as an
editing loop. `plan draft` expands only the 109 exact programs named by the
333 entry identities, invokes the targeted no-AST-visit trace, and reuses its
content-addressed raw trace on later draft checks. The draft partitions the
entry universe exactly; an identity-side cluster reference that disagrees
with cluster membership, a stale exact-identity hash, a cross-family cluster,
or a stale program/code/summary count fails `plan check`.

`--sibling-fixture` adds a focused existing-corpus negative control without
changing the fixed entry identity universe. A multi-matrix fixture must name
one exact `#matrix-key`. Adding probes is incremental: matching program/hash
rows from the fresh raw trace are reused and only new or changed programs
launch isolated Node processes. A trace-tool, Node-pin, inventory, or vendor
fingerprint change invalidates reuse rather than mixing evidence vintages.

The review overlay enumerates the exact cluster set and records one selected
sibling, owner slice, rationale, every producer-SCC decision, and any
native-adjacent Rust boundary needed where no exact ledger join exists.
`apply-review` rejects unknown sibling selections, omitted clusters/SCCs,
collapsing a non-singleton SCC as a singleton, stale Rust paths/functions,
and boundary overrides that replace an available exact ledger join. The
reviewed plan remains a draft for one landed commit; the later freeze commit
may only anchor that identical reviewed content. `plan freeze` records the
full current commit, refuses an unlanded or incomplete review, and changes
only `status` plus the freeze record. Frozen `plan check` resolves that
commit, requires it to be an ancestor, compares the complete normalized JSON
to the anchored draft, verifies the anchored review-overlay hash, and allows
the trusted baseline transition exactly once. Later semantic CI checks the
frozen plan without rerunning Node or depending on ignored `target/` inputs.

Trace `execution_pass` and oracle output `pass` are intentionally separate.
For example, parser-created JSDoc diagnostics may be returned in the semantic
bucket, and a semantic 7016 may be returned in the suggestion bucket. The
plan joins an exact program and diagnostic code conservatively and retains
the execution phase for review; it must not discard a producer merely
because those two pass labels differ.

Static expansion follows exact lexical-call edges only to the first frozen
ported or reviewed-disposition boundary. Property-call candidates and
unresolved calls remain separate. A mechanical SCC is recorded in full with
`merge_status=review-required`; its members are not silently merged into the
implementation slice. Likewise, sibling candidates in the entry-residual
program set are proposals only. Missing or weak candidates require a
targeted minimal probe, and every sibling selection, SCC decision, Rust
boundary override, rationale, and owner-slice assignment is reviewed before
the plan is frozen.

The initial implementation-order hypothesis is parser pragma, strict
nullability, module semantics, override validation, then the larger
implicit-any and supported check-JS clusters. This is not a hard-coded order.
The frozen dependency graph may reorder it, and the owning plan records the
reason.

## Per-slice contract

Every semantic slice records:

```text
Owner family and tier:
Exact mismatch identities and fixed entry fingerprint:
Exact tsc D2 identities, spans, hashes, traces, and static closure:
Emitting probe and nearest non-emitting sibling:
Rust boundary and prerequisite/consumer relationship:
Supported T0-T4 before -> after:
Accepted identities lost: 0
All-corpus FP: 0
Canaries and focused tests:
Escapes, ledgers, dispositions, and evidence rows before -> after:
```

One PR owns one family and one dependency-closed cluster. A large family may
use several dependency-ordered PRs; a PR may not combine unrelated small
tails merely to reduce the displayed FN count. Three probes that expose the
same model ceiling trigger the stall playbook and a design review rather
than a fourth local patch.

The `checkjs-jsdoc` family exposed that ceiling: comments had been projected
from source text without declaration nodes, so exact relation chains and
related declaration sites required repeated fabrication. A first bounded
materialization experiment then showed that activating a subset of tags
before template, import, signature, and host-scope dependencies were present
changed real symbol and relation behavior. Its approved design correction was
therefore the
[complete M8 JSDoc subsystem port](m8-jsdoc-ast-materialization.md).
That dependency-complete parser/AST/binder/checker chain has landed, and
resolved historical scope rows return through A2 tombstones. The lasting
rule is unchanged: no partial semantic JSDoc slice, new checker-side comment
projection, or local activation guard is an accepted implementation unit.
The landed subsystem is a prerequisite for the formal A1/A3 corpus gates,
not evidence that either transition has already occurred.

Relation reporting follows the vendored control flow at every failure level.
Whenever tsc renders a source/target pair, the source is read-normalized and
the target is write-normalized at that same level before display. Applying
normalization only to the final diagnostic head, or repairing a nested chain
after the relation returns, is not equivalent and is not accepted.

Checked-JS object and member behavior is likewise producer-owned. The former
checker-side memberless/symbol-carrying empty-resolution admission heuristic
has been deleted and must not be restored. Object display and property
publication follow the TypeScript 6.0.3 binder/type/checker path; the
plain-JS nested-object to TypeScript-consumer canary therefore retains tsc's
2339 when that member is absent.

The disposition artifact itself remains byte-identical after D2b freeze.
When a slice ports a declaration frozen as `deferred`, the before/after row
records the new exact `tsc-span`/`tsc-hash` ledger join as monotone
implementation evidence; it does not rewrite the historical planning
disposition. Losing a join frozen as `ported`, or adding one to
`not-applicable`, remains a hard failure.

Focused probes, crate tests, and the target family report are the iteration
loop. Generated artifacts and README status are refreshed before the final
verification. Run the complete local
`CARGO_BUILD_JOBS=2 cargo xtask ci --baseline origin/main` once on the clean
candidate branch, then let the required GitHub Actions workflow verify the
same slice with its bounded compile/syntax guardrail; it does not repeat
clippy, static contracts, tests, receipt-bound B2-B4 evidence, full invariants,
readiness, or performance work. As soon as every required hosted check passes and the PR is mergeable,
merge it automatically with `gh pr merge --merge --delete-branch`; no fresh
per-PR approval is required unless the work introduces a substantial design
or scope change. Do not repeatedly run full conformance, the B2 Node sweep, or
long fuzz windows while editing.

Local M8 validation uses `CARGO_BUILD_JOBS=2` and at most two Rust test
threads. Related focused tests are batched into one invocation; repeatedly
starting one Cargo process per fixture is not the iteration model. Higher
parallelism is allowed only for an explicitly owned performance experiment,
not for semantic closure or routine CI preparation.

The B2 runtime artifact remains content-addressed. An unrelated Rust or
documentation change revalidates and reuses it; only an exact producer
fingerprint change may trigger the single-worker bounded Node AST visit.
Performance changes are measured and reviewed separately from semantic
slices unless the semantic change itself exceeds an existing ceiling.

## M8 exit and M9 handoff

M8 closes only when:

- supported T0-T3 are 100%;
- every supported case has byte-exact T4 output;
- syntactic diagnostics are fully in scope;
- all-corpus `FP=0` and every accepted set remains monotone;
- `escapes.toml` is empty and the Rust/D2 ledgers are fresh and complete;
- full-corpus determinism, idempotence, job, matrix, encoding, and prefix
  invariants are green;
- current B1-B4 evidence stays inside its validity and performance bounds;
- `cargo xtask completion` reports rows 1-10 green and only the M9 row
  pending.

### Close record

The 2026-07-30 accepted-state observation records the completed M8-owned
surface:

| Signal | M8 close |
|---|---:|
| Supported T0 | 48,783 / 48,783 |
| Supported T1 | 48,783 / 48,783 |
| Supported T2 | 48,783 / 48,783 |
| Supported T3 | 48,783 / 48,783 |
| Supported T4 cases | 7,691 / 7,691 |
| All-corpus FP | 0 |
| Escape sites / manifest rows | 0 / 0 |
| Exact tier state | T1-T3 active; schema-3 T4 active |
| Full-corpus invariants | green; 5,908 fixtures / 7,691 programs |
| Completion | 10 / 11 rows green; M9 pending |

The fresh machine close report confirms rows 1-10 green, row 11
`m9-steady-state` pending, and `STAGE=M8`. No M8 semantic or recovery
implementation remains; M9 is the sole pending project-completion row.

The [M9 execution contract](m9-execution-and-close.md) now owns the
implementation-versus-contract preflight, generator-domain audit, bounded
producer, class/witness/recurrence formats, diagnostic-D2/pipeline-native
owner-task closure, attestation path, non-qualifying burn-in, and qualifying
14-window streak. The landed M8 B3 artifact is only an entry smoke.
Qualification starts after every known incident closes and the checker/
oracle/generator/reducer/domain/workflow/policy fingerprint freezes;
changing any of those inputs resets it.

## Separate follow-on design tracks

M8 is exclusively the TypeScript 6.0.3 batch-diagnostics completion phase.
It neither implements nor silently reserves acceptance credit for:

- **H0 track** — the now-active filesystem-hosted
  [`--noEmit` compiler](noemit-cli.md), including general module/package
  resolution, tsconfig, and CLI behavior;
- **Emitter track** — JavaScript and declaration-file emission;
- **L-track** — LSP, watch, and incremental operation;
- **Public-API track** — a public `TypeChecker` API.

Each track requires a separate design, goal, compatibility surface, oracle or
reference contract, performance bounds, and definition of done. Its design
may reuse the batch engine, but it may not change M8 scope identities,
denominators, evidence, or completion criteria. The existing
[LSP and incremental notes](lsp-and-incremental.md) are preliminary design
input for the future L-track, not an active M8 execution plan.
