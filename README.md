# tsc-rs

`tsc-rs` is an independent Rust implementation of the TypeScript batch
diagnostics checker. Its compatibility target is one exact release,
**TypeScript 6.0.3**, and its results are continuously compared with the
checked-in TypeScript compiler and conformance corpus.

> **Current availability:** `tsc-rs` is not yet a drop-in replacement for
> the `tsc` command. The parser, binder, checker, and contextual diagnostic
> formatter are exercised through the repository's existing in-memory test
> and conformance harness. An internal one-shot `ProgramSession` now connects
> the owned `PreparedProgram` contract to a no-emit diagnostic pass, but it is
> not an end-user command and does not yet consume the authoritative resolution
> table. A production filesystem-hosted `--noEmit` command, tsconfig loading,
> and general package resolution are under active development. Emission, watch
> mode, and a stable public checker API are not currently provided.

## What Works Today

The repository's current test harness demonstrates that the checker can:

- scan and parse TypeScript, JavaScript, JSX, JSON, and JSDoc syntax;
- bind and type-check multi-file programs;
- produce syntactic, semantic, suggestion, grammar, unused, and checked-JS
  diagnostics;
- handle imports whose targets are already part of the program, together
  with ambient and pattern-ambient module declarations;
- preserve diagnostic codes, locations, spans, message chains, related
  information, ordering, and deduplication; and
- reproduce the currently gated, color-free contextual diagnostic format.

These capabilities are conformance-gated only on the project's frozen
supported scope, summarized under [Development Status and
Roadmap](#development-status-and-roadmap). The `PreparedProgram` type in
`crates/program` can be consumed by the internal `ProgramSession` in
`crates/compiler`; the session owns the program, keeps parser/binder/checker
borrows within one run, and separates the five no-emit diagnostic buckets.
Existing conformance tests still assemble files, libraries, options, and
resolution facts through the harness and checker APIs. Normal filesystem
discovery, authoritative resolution-table consumption, `tsconfig.json`
handling, `node_modules` traversal, package maps, and CLI exit behavior are
not wired into a production executable yet.

## Build and Explore

The corpus, oracle, library files, and accepted artifacts are checked in. No
bootstrap or package-install step is required.

Requirements:

- [Rust](https://www.rust-lang.org/tools/install) via `rustup`. The pinned
  toolchain and required `rustfmt` and `clippy` components are declared in
  [`rust-toolchain.toml`](rust-toolchain.toml).
- Node.js for oracle probes, conformance comparisons, and golden refreshes.
  The accepted version is pinned in [`.node-version`](.node-version).

Clone the repository and run the workspace checks:

```sh
git clone https://github.com/kazhiramatsu/tsc-rs.git
cd tsc-rs

CARGO_BUILD_JOBS=2 cargo build --workspace
CARGO_BUILD_JOBS=2 cargo test --workspace -- --test-threads=2
```

To exercise the diagnostic engine through its current developer-facing test
surface:

```sh
CARGO_BUILD_JOBS=2 cargo xtask test checker --lib -- --test-threads=2
```

Focused checks for the host and prepared-program boundaries are:

```sh
CARGO_BUILD_JOBS=2 cargo xtask test host -- --test-threads=2
CARGO_BUILD_JOBS=2 cargo xtask test program -- --test-threads=2
CARGO_BUILD_JOBS=2 cargo xtask test compiler -- --test-threads=2
```

`checker`, `host`, `program`, and `compiler` are stable workspace roles rather
than Cargo package names. Contributor commands and CI use
`cargo xtask test <role>`, so an internal package rename does not require every
workflow and document to be rewritten. `cargo xtask workspace audit` verifies
the role metadata and rejects direct package or binary selectors in repository
automation.

These commands run internal libraries and tests; there is no end-user command
that accepts a project or source file yet, and no production
`cargo run -- ...` workflow. The complete local acceptance gate is intended
for contributors and is substantially more expensive than the commands
above:

```sh
CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=2 cargo xtask ci --baseline origin/main
```

See [setup and verification](docs/setup.md) for focused commands and
environment details.

## Compatibility and Limits

“TypeScript compatibility” in this repository means the checked-in
[`_tsc.js`](vendor/typescript-6.0.3/lib/_tsc.js) from TypeScript 6.0.3,
its matching standard libraries, and its matching conformance corpus. It
does not imply compatibility with newer TypeScript releases.

| Capability | Availability |
| --- | --- |
| Existing harness-assembled, in-memory batch diagnostics | Available and conformance-gated |
| Owned `PreparedProgram` execution path | Internal one-shot no-emit path available; authoritative resolution-table consumption remains in development |
| Color-free contextual diagnostic formatting | Available in the conformance harness |
| Filesystem-hosted `--noEmit` command | In development |
| tsconfig discovery and JSONC configuration | In development |
| `node_modules`, package `exports`/`imports`, `paths`, and `typeRoots` resolution | In development |
| Output emission (`.js`, `.d.ts`, source maps, or build info) | Not implemented |
| Watch, incremental, project-reference, or solution builds | Outside the current scope |
| Language server and stable public `TypeChecker` API | Outside the current scope |
| TypeScript versions other than 6.0.3 | Unsupported |

The active filesystem work is limited to a single-project, mandatory
`--noEmit` flow. Unsupported operations must fail closed rather than check
only part of a project or silently ignore an option.

## How Correctness Is Measured

The harness expands the checked-in TypeScript fixtures, runs the Rust checker
and the vendored compiler with the same prepared inputs, and compares
diagnostics at progressively stricter tiers:

| Tier | Required equality |
| --- | --- |
| T0 | file, diagnostic code, line, and column |
| T1 | T0 plus diagnostic category |
| T2 | T1 plus full span and top-level message text |
| T3 | T2 plus message chains and related information |
| T4 | byte-equivalent output from the gated contextual formatter per case, including order and deduplication |

False positives are forbidden on every merge. Accepted diagnostic identities
are ratcheted, so a change may add exact matches but may not silently trade
away an existing match. Reviewed rows outside the completed checker boundary
remain visible in the all-corpus denominator; they are not hidden by fixture,
diagnostic-code, or glob exclusions.

The full gate also checks determinism, idempotence, build-job independence,
encoding independence, unsupported-unwind behavior, generated artifacts,
owner ledgers, and escape inventories.

## Workspace Layout

The active implementation is an Oxc-style virtual Cargo workspace at the
repository root. There is intentionally no top-level `src/` directory.

| Path | Responsibility |
| --- | --- |
| `Cargo.toml` | Virtual workspace manifest |
| `crates/syntax` | Scanner, parser, generated AST schema, traversal, and source-file representation |
| `crates/binder` | Symbols, scopes, declarations, assignments, and flow graph construction |
| `crates/types` | Shared compiler options, symbols, types, signatures, and relation data |
| `crates/host` | Read-only compiler-host contract and deterministic in-memory host adapter |
| `crates/program` | Owned prepared-program, path-identity, and authoritative resolution contracts |
| `crates/compiler` | One-shot owned program execution and no-emit diagnostic buckets |
| `crates/checker` | Parser/binder assembly and semantic diagnostic checking |
| `crates/diagnostics` | Diagnostic messages, structures, line maps, sorting, and deduplication |
| `crates/harness` | TypeScript fixture expansion and program-input construction |
| `crates/oracle` | Node-based access to the vendored TypeScript oracle |
| `crates/conformance` | Differential comparison, exact identities, ratchets, scope, and family reports |
| `crates/fuzz` | Differential-fuzzing foundations |
| `crates/xtask` | Code generation, audits, conformance commands, and the complete CI gate |
| `ts-tests` | Checked-in TypeScript conformance corpus |
| `vendor/typescript-6.0.3` | Pinned compiler bundle and standard library files |
| `docs/design/greenfield` | Authoritative architecture, execution plans, and completion contracts |

Internal Cargo packages currently follow `tsc-rs-<role>`, while shared
dependency aliases use `tsc-<role>` and Rust crate identifiers use
`tsc_<role>`. The full word `diagnostics` is used consistently. These names are
implementation details; contributor commands should use the stable roles shown
in the path table above. The `cargo xtask` alias selects the `tsc-rs-xtask`
package through the checked-in Cargo configuration. See
[setup and verification](docs/setup.md#workspace-package-roles) for the rename
and audit workflow.

The original v1 implementation is preserved at the `v1-final` tag and is no
longer present in the working tree.

## Contributing

Implementation is evidence-led: read the matching vendored TypeScript
function, probe the oracle when behavior is ambiguous, capture immutable
before evidence, and implement the smallest dependency-complete producer
slice. Expected diagnostics and messages come from the oracle rather than
memory.

Start with:

- [repository workflow and verification rules](CLAUDE.md);
- [greenfield execution guide](docs/design/greenfield/README.md);
- [definition of done](docs/design/greenfield/definition-of-done.md);
- [H0 filesystem-hosted no-emit contract](docs/design/greenfield/noemit-cli.md);
- [M8 execution and close contract](docs/design/greenfield/m8-execution-and-close.md);
- [M9 execution and close contract](docs/design/greenfield/m9-execution-and-close.md); and
- [M7 band and owner strategy](docs/design/greenfield/m7-band-and-owner-strategy.md).

`main` must remain green. Normal implementation work uses a short-lived
branch, runs the complete local gate once for the final candidate, and lands
through a merge-commit GitHub PR. When every changed path is Markdown and the
generated status block below is unchanged, the documentation-only exception
uses rendered-diff, link, anchor, and whitespace checks instead of Cargo,
Node, or full-corpus CI.

## Development Status and Roadmap

Milestones M0–M8 are complete on the frozen supported batch-diagnostics
scope. M9 is paused after its typed-outcome and canonical true-replay
foundations. H0, the filesystem-hosted `--noEmit` track, is the active
frontier.

H0.1 has landed four prerequisite slices without changing accepted
diagnostics:

- **H0.1a:** node and array ownership no longer depends on final program file
  order;
- **H0.1b:** a fail-closed `CompilerHost` contract and deterministic
  `MemoryCompilerHost` provide the read-only host boundary;
- **H0.1c:** `crates/program` owns trusted path identities, ordered sources,
  roots and libraries, package metadata, renderable diagnostic text, and
  authoritative typed resolution outcomes; and
- **H0.1d:** `crates/compiler` consumes an owned `PreparedProgram` through a
  one-shot session, uses a non-leaking checker path, preserves five diagnostic
  buckets and their gates, and excludes suggestions from no-emit results.

The remaining H0.1 work makes the existing library cache collision-safe. H0.2
follows with general host-backed module and type-reference resolution and
connects the checker to the authoritative resolution table.

| Phase | State | Focus |
| --- | --- | --- |
| M0–M6 | Complete | Harness, syntax, binding, types and relations, core checking, flow, inference, and overloads |
| Phase-9 2XXX | Complete | Supported-scope 2XXX closure using exact diagnostic ownership |
| M7 | Complete | Non-2XXX diagnostic families closed on the supported scope |
| M8 | Complete | Supported-scope T0–T4 closure, full-corpus FP=0, and recovery/escapes zero |
| M9 | Paused after 1b | Typed outcomes and true replay landed; production generator, burn-in, freeze, and qualification deferred |
| H0 | Active (H0.1a–d landed) | Filesystem-hosted `--noEmit`: cache hardening, resolution, config/CLI, rendering, and exit behavior |

The exact accepted-state summary below is generated by
`cargo xtask readme-status` and must not be edited by hand.

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

The generated table is the all-corpus visibility view. Completion is judged
on the separate supported-scope view, while `FP = 0` remains absolute across
the full corpus.

## License

Licensed under the [MIT License](LICENSE).
