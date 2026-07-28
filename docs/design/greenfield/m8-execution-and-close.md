# M8 execution and close contract

Status: active execution contract after M7 close.

This page owns how M8 is executed and closed. It starts only after
[`m8 readiness --require-ready`](m8-readiness.md#machine-gate) is green.
The [completion convergence plan](completion-convergence-plan.md) still owns
cross-track landing order, [measurement integrity](measurement-integrity.md)
owns accepted sets, exact scope, T4 activation, and D2 identities, and the
[definition of done](definition-of-done.md) still owns the final project
contract.

Readiness and completion are deliberately different:

- `ready=true` authorizes the first M8 slice;
- M8 closes the supported diagnostic and recovery work;
- M9 proves differential-fuzzer steady state;
- only then may `cargo xtask completion --require-done` pass.

The checked-in `STAGE` marker records the last closed milestone. It therefore
remains `M7` throughout M8 and changes to `M8` only in the M8 close slice.
`m8 readiness --require-ready` opening M8 does not move the marker.

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

## Required landing order

M8 executes these steps in order:

1. **Completion report** — implement `cargo xtask completion` in report-only
   form. It must enumerate every strict completion row and show why
   `--require-done` cannot yet pass.
2. **Frozen residual plan** — materialize the exact M8 entry residual and map
   every supported mismatch to its family, exact D2 producer cluster, Rust
   boundary, live tier blockers, and owning slice.
3. **Bounded T0 closure** — close the supported T0 residual one
   dependency-closed producer slice at a time while preserving all accepted
   identities and all-corpus `FP=0`.
4. **Formal tier closure** — activate and close the exact T1, T2, and T3
   accepted sets in the convergence plan's order. A touched family still
   follows the vertical slice-fidelity rule; the global sweep is not
   permission to add a knowingly wrong category, span, message, chain, or
   related-information shape.
5. **A3 T4 closure** — activate the rendered-output comparator through the
   reviewed A3 universe transition, then close byte parity for every
   supported case, including ordering and deduplication.
6. **Recovery and converse close** — empty `escapes.toml`, keep the frozen D2
   inventory/dispositions and current B1-B4 evidence fresh, and run the full
   invariant suite.
7. **M8 close** — require completion rows 1-10 to be green, with only M9
   steady state pending; update `STAGE` from `M7` to `M8` in this close slice.

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
cargo xtask m8 trace \
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
cargo xtask conformance \
  --band all \
  --out-json target/m8/entry-conformance.json
cargo xtask m8 plan draft \
  --conformance-json target/m8/entry-conformance.json \
  --out target/m8/owner-plan-draft.json
cargo xtask m8 plan check \
  --plan target/m8/owner-plan-draft.json
```

The full conformance command is run once for the entry snapshot, not as an
editing loop. `plan draft` expands only the 109 exact programs named by the
333 entry identities, invokes the targeted no-AST-visit trace, and reuses its
content-addressed raw trace on later draft checks. The draft partitions the
entry universe exactly; an identity-side cluster reference that disagrees
with cluster membership, a stale exact-identity hash, a cross-family cluster,
or a stale program/code/summary count fails `plan check`.

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

Focused probes, crate tests, and the target family report are the iteration
loop. Generated artifacts and README status are refreshed before the final
verification. Run the complete local `cargo xtask ci --baseline origin/main`
once on the clean candidate branch, then let the required GitHub Actions
lanes verify the same slice. Do not repeatedly run full conformance, the B2
Node sweep, or long fuzz windows while editing.

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

The M9 policy, append-only history/signature formats, attestation path, and
scheduled producer may be implemented and tested during M8. The qualifying
14-window streak starts only after the checker/oracle/generator/reducer and
policy fingerprint is frozen: changing any of them resets the streak.

## Separate follow-on design tracks

M8 is exclusively the TypeScript 6.0.3 batch-diagnostics completion phase.
It neither implements nor silently reserves acceptance credit for:

- **Emitter track** — JavaScript and declaration-file emission;
- **L-track** — LSP, watch, and incremental operation;
- **Public-API track** — a public `TypeChecker` API.

Each track requires a separate design, goal, compatibility surface, oracle or
reference contract, performance bounds, and definition of done. Its design
may reuse the batch engine, but it may not change M8 scope identities,
denominators, evidence, or completion criteria. The existing
[LSP and incremental notes](lsp-and-incremental.md) are preliminary design
input for the future L-track, not an active M8 execution plan.
