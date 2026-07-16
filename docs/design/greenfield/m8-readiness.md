# M8 scope and readiness contract

This is the executable contract for entering M8. The normative end
state remains [definition-of-done.md](definition-of-done.md); this
page defines how its supported-scope and M8-start clauses are
measured. M5-M7 implementation is a prerequisite, not part of this
work.

## Two views of the same corpus

Every conformance run reports both views. Neither is optional.

1. **All-corpus visibility** is the existing fixed 5,908-fixture
   denominator. It keeps the T0 ratchet, top FN codes, and the
   absolute `FP=0` gate. Out-of-scope work remains visible here.
2. **Supported scope** removes only reviewed oracle diagnostics in
   `tsrs2/m8-scope.json`. M8's T1-T4 completion target uses this
   denominator.

Scope dispositions are exact diagnostic identities:
`fixture + matrix_key + file + code + line + col`. There are no
fixture, directory, code, or glob exclusions. One host-resolution or
JSDoc diagnostic therefore cannot hide another diagnostic in the
same program. Syntactic diagnostics can never be excluded.

The only accepted reasons are:

- `host-resolution`: behavior requiring node_modules/package.json,
  paths/baseUrl host lookup, project references, or redirects;
- `jsdoc-semantics`: checking whose result depends on a JSDoc type;
- `emit-dependent`: a diagnostic whose observability depends on the
  deliberately absent emitter.

Every entry needs non-empty evidence. The manifest starts `draft`.
A band can be pinned earlier without freezing the manifest: an A2
band-freeze record (the band's enumerated pinned identity set inside
`m8-scope.json`, anchored to its adjudication commit) makes that
band immutable while status stays `draft` — the `2xxx` band pins at
the phase-9 sweep (convergence plan §4 row 9; the A2 section defines
the subset / history-anchor / tombstone rules). At M7 close,
classify the remaining known out-of-scope FNs, review the exact
diff, and change it to `frozen`; the freeze re-verifies every
band-freeze record. If tsrs later emits an excluded diagnostic at
T0, conformance reports it as `resolved-t0`; delete the disposition
immediately so T1-T4 begin grading it (in a pinned band, the
deletion carries a tombstone whose proof is the identity's
membership in the A1 accepted-match artifact — in a duplicate T0
bucket, the whole bucket must be multiplicity-complete; the pinned
set itself never changes).

`cargo xtask conformance` prints both metric sets and writes both to
the mismatch JSON. The all-corpus FP gate and T0 ratchet are
unchanged.

## All-band inventory and dependency closure

Run:

```sh
cargo xtask codegen band-inventory --by-function --band all
cargo xtask codegen band-inventory --by-function --band all --check
```

The generated `tsrs2/m8-emitter-inventory.json` is pinned to the
SHA-256 of the vendored `_tsc.js`. It inventories every direct
`Diagnostics.*` use except `.code` membership reads and computes a
transitive, deliberately conservative identifier/property-call
dependency closure. Identities use the same tsc function-name
vocabulary as `tsc-port` ledger headers. Duplicate bundle
declarations with the same function name form one identity.

`m8-emitter-dispositions.json` is the tsc-to-Rust converse of the
Rust function ledger:

- the file pins the exact generated inventory SHA-256, so generator
  changes require a reviewed refresh even when `_tsc.js` is unchanged;
- a matching `tsc-port` name is classified automatically;
- remaining closure functions require exact `deferred` or
  `not-applicable` entries with evidence;
- no missing or stale entry is accepted when the file is `frozen`.

The name-call closure is an over-approximation, so a dynamic-dispatch
false dependency may be `not-applicable`, but only with evidence.
This makes the conservative edge reviewable rather than silently
dropped.

## Runtime, fuzzer, and performance evidence

`tsrs2/m8-evidence.json` holds the readiness summaries. `draft`
never passes. A producer must write an artifact under its declared
path before changing a section to `ready`.

- Runtime coverage pins the exact inventory SHA-256 and lists every
  direct-emitter identity exactly once as
  executed or zero-hit-with-evidence and must have at least one executed
  emitter. Unknown, duplicate, overlapping, or evidence-free identities fail.
- The differential fuzzer must run a generator against both tsrs and
  the pinned oracle, compare every generated case, exercise its
  reducer and signature deduper, and name the CI command that keeps it
  running.
- Performance records wall time, maximum RSS, and reviewed ceilings.
  The wall ceiling may not exceed 60 seconds. Artifact paths are
  workspace-relative and may not escape the workspace.

The minimal differential loop is an **M8-start** prerequisite. M9
does not introduce the fuzzer; it hardens the already-running loop
to the steady-state `< 1 new signature/night` gate.

## Machine gate

```sh
cargo xtask m8 readiness
cargo xtask m8 readiness --require-ready
```

The first command is a report and succeeds while work remains. The
second is the M7-close/M8-entry gate and fails unless all nine rows
are green:

1. M7 conformance gate (`T0 >= 63%`, `FP=0`, configured exact T1 ratchet,
   enforced by All-band conformance);
2. live T1-T3 shadow metrics;
3. frozen, non-stale scope manifest;
4. zero undispositioned Rust checker functions;
5. fresh all-band emitter inventory;
6. frozen and complete dependency dispositions;
7. complete runtime coverage evidence;
8. an actually running differential fuzzer;
9. recorded performance and RSS ceilings.

The JSON report is written to `target/m8/readiness.json`. The command
does not pretend that the current M4 branch is M8-ready; it makes the
remaining prerequisites explicit and mechanically testable.

## Escape end state

Recovery guards may remain a separately ratcheted `Unsupported`
class through M5-M7, but they are not a permanent exception to Done.
Before final completion they must move to deterministic recovery
values/control flow that do not use the `Unsupported` containment
channel. The final escape requirement remains `sites=0` and an empty
manifest.
