# tsc-rs

`tsc-rs` is an independent Rust implementation of the **batch diagnostics
checker** in TypeScript 6.0.3. It aims to reproduce the diagnostics of one
exact, vendored `tsc` bundle—from diagnostic identity through rendered CLI
output—and measures progress by differential testing against that bundle on
the TypeScript conformance corpus.

> **Development status:** M8 batch-diagnostics compatibility is complete on
> the frozen supported scope. M9's typed outcome and true-replay foundations
> have landed; its production fuzzer, burn-in, and 14-window qualification
> are intentionally paused. The active H0 track is turning the completed
> prepared-program checker into a filesystem-hosted `--noEmit` compiler.
> Emit, language-service, and stable public-API tracks remain separate. The
> active implementation and verification tools live in an Oxc-style virtual
> Cargo workspace at the repository root: the workspace manifest is
> [`Cargo.toml`](Cargo.toml), member crates live under [`crates/`](crates/),
> and there is intentionally no top-level `src/` directory.

## Compatibility Target

In this project, “TypeScript compatibility” always means
**TypeScript 6.0.3 exactly**: the checked-in
[`_tsc.js`](vendor/typescript-6.0.3/lib/_tsc.js) bundle, its library
files, and the matching conformance corpus. The vendored compiler is both the
source-level specification for porting and the executable oracle used by the
test harness.

The completed M8 compatibility surface covers the following on its frozen
supported scope:

- batch syntactic and semantic diagnostics;
- emit-free suggestion diagnostics;
- grammar and unused diagnostic families;
- checked JavaScript, including tsc-compatible JSDoc parsing, binding, type
  resolution, and diagnostics;
- multi-file programs with in-program and ambient/pattern-ambient module
  resolution;
- exact diagnostic comparison from source location to message chains,
  related information, ordering, and rendered output.

The following are deliberately out of scope:

- JavaScript or declaration-file emission;
- general host-backed module resolution such as filesystem
  `node_modules`, `paths`/`baseUrl`, project references, and triple-slash
  redirects; the in-memory checker models only the package metadata needed
  by supported diagnostic producers;
- LSP, watch, and incremental operation;
- a public `TypeChecker` API;
- compatibility with TypeScript versions newer than 6.0.3.

These boundaries are diagnostic-identity based: excluded oracle rows remain
visible in the all-corpus results and may not be hidden by fixture, code, or
glob exclusions. See the normative
[definition of done](docs/design/greenfield/definition-of-done.md) for the
full contract.

These remain outside M8 even when implemented later. General host-backed
`--noEmit` execution is now the active
[H0 follow-on track](docs/design/greenfield/noemit-cli.md). Emission,
LSP/watch/incremental operation, and a public `TypeChecker` API retain
separate compatibility surfaces, evidence contracts, performance budgets,
and definitions of done; none changes the batch-diagnostics M8 denominator.

## Status

Milestones M0–M8 are complete. M9's fuzzer foundation is partially landed
and paused after true replay; H0 filesystem-hosted `--noEmit` is the active
frontier. M8 closed the frozen batch-diagnostics scope with:

- supported T0, T1, T2, and T3 each at **48,783 / 48,783**;
- T4 rendered output at **7,691 / 7,691** supported cases;
- zero false positives across the full corpus;
- zero recovery or untagged escape sites; and
- completion rows 1–10 green, with only the M9 steady-state row pending.

The all-corpus visibility denominator deliberately retains 241 reviewed
identity-level exclusions, all in the 2XXX band and all owned by
host-resolution. That is why the generated 2XXX row is below 100% while the
frozen supported 2XXX scope is complete. H0 uses those exact rows as its
initial owner inventory; no fixture, diagnostic code, or glob exclusion can
hide them.

M8 readiness is 10/10. The Rust function ledger, all-band emitter inventory,
runtime coverage, differential-fuzzer smoke, performance baseline, family
rollup, D2 emitter dependency closure, and full-corpus invariant attestation
are green; the invariant attestation covers 5,908 fixtures / 7,691 programs.
All 5,513 exact declarations have an owner, disposition, and immutable
evidence in the frozen reviewed snapshot.

The complete TypeScript 6.0.3 JSDoc subsystem port has landed. Its scanner,
parser, arena nodes, binder paths, checker utilities, and diagnostic
dispatch now form one dependency-complete chain; the older checker-side
comment projections have been removed. This architecture follows an
earlier bounded materialization experiment that showed individual tag
activation changes real symbol behavior unless template, import, signature,
and host-scope dependencies land together. See the
[complete JSDoc port contract](docs/design/greenfield/m8-jsdoc-ast-materialization.md).

The exact T1–T3 accepted sets and T4 rendered-diagnostic pipeline are active.
T4 uses the genuine vendored TypeScript formatter as its pinned producer and
an independently ported Rust formatter for the steady-state Node-free gate.
Schema-3 goldens preserve the observable present-but-empty
`relatedInformation` state through a sparse sidecar, so formatter equality
does not collapse a distinction exposed by `tsc`.

The expensive B2 Node coverage sweep is content-addressed. CI and local
verification reuse the exact verified artifact when the Node pin, vendored
compiler, corpus, inventory, and producer inputs are unchanged; the full
7,462-program AST visit is not repeated for unrelated Rust or documentation
changes. Coverage execution is capped at one worker with bounded
per-process program lifetime and library-cache buckets.

M7 used the same evidence-led strategy that made the 2XXX sweep effective:
measure exact oracle rows, group them into A5 owner families, trace each
family through its emitting `tsc` declaration and Rust boundary, and port
one bounded producer slice at a time. M8 applied that discipline with exact
D2 declaration identities, static dependency paths, and B2 runtime
evidence—same-named declarations never closed one another implicitly.
`cargo xtask m8 trace` and `cargo xtask m8 plan draft` remain available for
auditing the frozen close record without revisiting the full Node AST.

M9 does not start by running the current smoke for 14 nights. That smoke is
32 cases from eight templates and proves only the entry machinery. The
[M9 execution contract](docs/design/greenfield/m9-execution-and-close.md)
first requires a generator-domain and resource preflight, true replay and
reduction, pass-aware multiplicity-preserving divergence classes, a bounded
streaming producer, and non-qualifying burn-in. Every discovered witness is
then split into exact owner tasks: diagnostic producers use their A5/2XXX
family, D2 declaration/SCC, and Rust boundary, while terminal or
pipeline-native failures use their exact phase/control-flow owner. Only
after every task closes and the semantic fingerprint freezes does the
14-window qualification begin.

M9.1b has now landed canonical true replay and bounded one-case Node/Rust
adapters. The remaining oracle-deviation registry, fixpoint reducer,
production generator, history, burn-in, freeze, and qualification work is
paused rather than treated as complete.

The active
[H0 execution contract](docs/design/greenfield/noemit-cli.md) addresses the
practical boundary outside M8: filesystem and package resolution,
program construction, tsconfig/options diagnostics, the no-emit batch gate,
rendering, and exit status. It applies the same exact-row/owner method to the
241 host-resolution identities, beginning with the 144-row package
`exports`-pattern cluster.

<!-- STATUS:BEGIN — generated by `cargo xtask readme-status`; do not edit by hand -->
Accepted conformance state at stage marker `M8` — the checked-in
`ratchet.toml` summaries, verified against the accepted-set
artifacts by every `cargo xtask ci` run:

| View | Exact diagnostic match (T0) |
| --- | --- |
| All bands | **99.5084%** (48,783 / 49,024) |
| 2xxx all-corpus visibility | **98.8552%** (20,810 / 21,051) |
| Syntactic | **100.0000%** (2,246 / 2,246) |

The 2XXX supported scope is **100% complete** with zero T0 false
negatives. Its all-corpus row above deliberately retains reviewed
out-of-scope oracle diagnostics in the denominator.

False positives are a hard gate: 0 on every merge. Escape
ceilings: untagged 0, recovery 0. Non-2XXX family
map: frozen, 15 families / 433 rows.

M8 readiness: 10/10 gates ready.
Ready: m7-gate, shadow-tiers, scope-frozen, rust-function-dispositions, emitter-inventory, emitter-dependency-closure, runtime-coverage, differential-fuzzer, performance-baseline, m7-family-rollup. Pending: none.
<!-- STATUS:END -->

The table is the **all-corpus visibility view**. It intentionally includes
reviewed out-of-scope oracle rows. Completion is judged on the separate
supported-scope view, while `FP = 0` remains absolute across the full corpus.
Accepted exact matches are identity-ratcheted: a change may add matches but
may not silently trade away an existing one.

## How Correctness Is Measured

The harness matrix-expands the checked-in TypeScript fixtures, runs both
checkers with the same files and compiler options, and compares diagnostics
at progressively stricter tiers:

| Tier | Required equality |
| --- | --- |
| T0 | file, diagnostic code, line, and column |
| T1 | T0 plus diagnostic category |
| T2 | T1 plus full span and top-level message text |
| T3 | T2 plus message chains and related information |
| T4 | byte-equivalent rendered CLI output per case, including order and deduplication |

The active merge gate verifies the accepted T0–T4 artifacts, absolute
all-corpus `FP = 0`, and the frozen exact scope. Determinism, idempotence,
job independence, encoding independence, unsupported-unwind coverage,
ledger/evidence freshness, and zero escape inventories are separate
mandatory gates.

## Roadmap

| Phase | State | Focus |
| --- | --- | --- |
| M0–M6 | Complete | Harness, syntax, binding, types/relations, core checking, flow, inference, and overloads |
| Phase-9 2XXX | Complete | Supported-scope 2XXX T0 closure using emitter ownership and exact-row mining |
| M7 | Complete | Six A5 virtual bands closed with supported FN=0, all canaries passing, and T1 active |
| M8 | Complete | Supported-scope T0–T4 closure, full-corpus FP=0, and recovery/escapes zero |
| M9 | Paused after 1b | Typed outcomes and true replay landed; production generator, burn-in, freeze, and 14-window qualification deferred |
| H0 | Active | Filesystem-hosted `--noEmit`: host/session seam, exact module resolution, program/config loading, CLI diagnostics, and exit behavior |

M7's machine-readable A5 family map remains the permanent set of virtual
bands for non-2XXX ownership. M8 added the all-band D2 declaration graph:
5,513 exact identities, 643 direct emitters, exact Rust ledger joins, B2
runtime execution/zero-hit evidence, and static shortest paths.

## Getting Started

The corpus, oracle, library files, and goldens are checked in; no bootstrap or
package-install step is required.

Requirements:

- [Rust](https://www.rust-lang.org/tools/install) via `rustup`. The repository
  pins the toolchain and required `rustfmt`/`clippy` components in
  [`rust-toolchain.toml`](rust-toolchain.toml).
- Node.js for oracle probes, oracle-driver tests, and golden refreshes. The
  accepted version is pinned in [`.node-version`](.node-version).

Build and run the primary checks from the active workspace:

```sh
git clone https://github.com/kazhiramatsu/tsc-rs.git
cd tsc-rs

CARGO_BUILD_JOBS=2 cargo build --workspace
CARGO_BUILD_JOBS=2 cargo test --workspace -- --test-threads=2
CARGO_BUILD_JOBS=2 cargo xtask conformance --band 2xxx
CARGO_BUILD_JOBS=2 cargo xtask ci --baseline origin/main
CARGO_BUILD_JOBS=2 cargo xtask completion
```

`cargo xtask ci` is the full local merge gate and includes formatting,
Clippy, build/tests, generated artifacts, accepted-state and scope/family
audits, all/2XXX/syntactic conformance, invariants, ledgers, escapes, and
README status freshness. See [setup and verification](docs/setup.md) for
focused commands and environment details. `cargo xtask completion` writes
the report-only M8/M9 completion matrix; its `--require-done` form is reserved
for the post-M9 release gate.

When every changed path relative to the trusted base is Markdown and the
generated README `STATUS` block is unchanged, skip all local
Cargo/Node/full-corpus CI and validate the diff, links, anchors, and
generated-block boundaries instead. Hosted Actions runs only a lightweight
classifier and required `gates` sentinel; any workflow, config, schema,
golden, generated-artifact, or generated-status change uses the full gate.

Normal implementation work keeps Cargo at two build jobs and two test
threads, groups related focused tests into batches, and runs the complete
local CI once for the candidate branch. Full conformance and the
content-addressed B2 Node coverage sweep are not editing-loop commands.

## Repository Layout

| Path | Responsibility |
| --- | --- |
| `Cargo.toml` | Virtual workspace manifest; there is no root package or top-level `src/` |
| `crates/syntax` | Scanner, parser, generated AST schema, traversal, and source-file representation |
| `crates/binder` | Symbols, scopes, declarations, assignments, and flow graph construction |
| `crates/types` | Shared compiler options, symbols, types, signatures, and relation data |
| `crates/checker` | Program assembly and semantic diagnostic checking |
| `crates/diags` | Generated diagnostic messages, diagnostic structures, line maps, sorting, and deduplication |
| `crates/harness` | TypeScript fixture expansion and program-input construction |
| `crates/oracle` | Node-based access to the vendored `tsc` oracle |
| `crates/conformance` | Differential comparison, exact identities, ratchets, scope, and family reports |
| `crates/fuzz` | Differential-fuzzing foundations for M8/M9 |
| `crates/xtask` | Code generation, audits, conformance commands, and the complete CI gate |
| `ts-tests` | Checked-in TypeScript conformance corpus |
| `vendor/typescript-6.0.3` | Pinned compiler bundle and standard library files |
| `docs/design/greenfield` | Authoritative architecture, execution plan, and completion contracts |

The original v1 implementation is no longer in the working tree and is
preserved at the `v1-final` tag.

## Contributing

Porting is evidence-led: read the matching vendored `tsc` function, probe the
oracle when behavior is ambiguous, capture immutable before evidence, and
then implement the smallest dependency-complete producer slice. As the
completed JSDoc port demonstrated, a subsystem whose parser, binder, and
checker semantics are inseparable lands only when that whole semantic chain
is coherent. Expected diagnostics and messages come from the oracle rather
than memory.

Start with:

- [repository workflow and verification rules](CLAUDE.md);
- [greenfield execution guide](docs/design/greenfield/README.md);
- [completion convergence plan](docs/design/greenfield/completion-convergence-plan.md);
- [M8 execution and close contract](docs/design/greenfield/m8-execution-and-close.md);
- [M9 execution and close contract](docs/design/greenfield/m9-execution-and-close.md);
- [H0 filesystem-hosted no-emit contract](docs/design/greenfield/noemit-cli.md);
- [M7 band and owner strategy](docs/design/greenfield/m7-band-and-owner-strategy.md).

The status block in this README is generated. After changing accepted state or
readiness evidence, run `cargo xtask readme-status` from the repository root
instead of editing the block by hand.

## License

Licensed under the [MIT License](LICENSE).
