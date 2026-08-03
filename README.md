# tsc-rs

`tsc-rs` is an independent Rust implementation of the TypeScript batch
diagnostics checker. Its compatibility target is one exact release,
**TypeScript 6.0.3**, and its results are continuously compared with the
checked-in TypeScript compiler and conformance corpus.

> **Current availability:** `tsc-rs` is not yet a drop-in replacement for
> the `tsc` command. The parser, binder, checker, and contextual diagnostic
> formatter are exercised through the repository's existing in-memory test
> and conformance harness. An internal one-shot `ProgramSession` now connects
> the owned `PreparedProgram` contract and its exact authoritative resolution
> table to a no-emit diagnostic pass for reviewed `MemoryCompilerHost` package
> exports slices. It is not an end-user command. A production filesystem-hosted
> `--noEmit` command, tsconfig loading, and general package resolution are under
> active development. Emission, watch mode, and a stable public checker API are
> not currently provided.

## What Works Today

The repository's current test harness demonstrates that the checker can:

- scan and parse TypeScript, JavaScript, JSX, JSON, and JSDoc syntax;
- bind and type-check multi-file programs;
- produce syntactic, semantic, suggestion, grammar, unused, and checked-JS
  diagnostics;
- handle imports whose targets are already part of the program, together
  with ambient and pattern-ambient module declarations;
- consume reviewed package `exports`, conditional-target, `typesVersions`,
  and untyped-JavaScript results from an exact authoritative resolution table;
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
resolution facts through the harness and checker APIs. General filesystem
discovery, `tsconfig.json` handling, broad `node_modules` traversal and package
maps, filesystem-backed type-reference discovery, and CLI exit behavior are
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
| Owned `PreparedProgram` execution path | Internal one-shot no-emit path available; exact table consumption is active for reviewed H0.2 package-map slices |
| Color-free contextual diagnostic formatting | Available in the conformance harness |
| Filesystem-hosted `--noEmit` command | In development |
| tsconfig discovery and JSONC configuration | In development |
| `node_modules`, package `exports`/`imports`, `paths`, and `typeRoots` resolution | Bounded in-memory package maps, legacy package fields/`typesVersions`, and reviewed `@types`/type-reference slices are available internally; general resolution remains in development |
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

H0.0 now freezes the 241 exact host-resolution identities in a dedicated
machine-checked registry. Each row pins its owner family, vendored TypeScript
primary/dependency/diagnostic declarations and hashes, effective
module-resolution kind, exact vendored source/specifier/request-mode and
anchor chain, Rust seam, and a positive canary plus reviewed typed feature
control. The semantic CI lane checks the registry against the same trusted A2
baseline. A closed row must match its non-lapsed A2 tombstone, name an
authoritative Rust route, and retain historical accepted-set proof at T0--T4;
oracle-correction tombstones remain explicit `lapsed` rows. Initial bounded
pre-H0 local and GitHub-hosted CPU, wall-time, and RSS observations are
recorded with the registry; H0.6 still owns the final resolver/CLI resource
profiles and budgets.

H0.1 has landed five prerequisite slices without changing accepted
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
  buckets and their gates, and excludes suggestions from no-emit results; and
- **H0.1e:** the legacy harness library cache treats fingerprints only as
  bucket indexes, verifies exact ordered names, texts, and binder options, and
  uses locally owned parse/bind state when cache reuse is disabled.

H0.0 and H0.1 are complete. H0.2 is active and partial: its reviewed
`MemoryCompilerHost` route now connects the checker to the authoritative table
and closes all 179 package-exports-patterns-and-blocked-subpaths rows plus all
36 package-imports-self-references-and-conditions rows. The reviewed legacy
package-fields slice also covers manifestless `node_modules`,
`typings`/`types`/`main`, `typesVersions` package-root and back-reference
handling, Node ESM directory rejection, and source emit eligibility for
TS2877. The reviewed types slice closes all 3
types-type-roots-and-reference-directives rows: JSDoc import/require modes now
select the matching `@types` conditional export, while exact-spelling
reference directives probe explicit and default type roots case-sensitively.
The reviewed H0.3 consumer slices close both ambient const-enum module binding
rows and all 4 external-helper consumer rows. Import and re-export aliases
consume the authoritative resolved target without double-reporting imported
access sites, while private get/set transforms consume the synthetic `tslib`
request in the source file's static resolution mode and validate the resolved
helper shape. The reviewed alternate-resolution slice closes all 7
resolution-mode-and-message-selection rows: Classic mode owns the 6 TS2792
diagnostics across import-type and type-only import matrices, while Node10
preserves the alternate package location needed for its single TS2307
diagnostic. The reviewed untyped-package consumer slice closes the final 4
rows: checked-JavaScript literal `require` requests load their resolved module
symbols for 3 exact TS2339 member diagnostics, while an external-module
augmentation consumes an unloaded JavaScript target for the exact TS2665
diagnostic. All 241 host-resolution rows are now closed at T0--T4 with typed
Bundler controls and full-corpus FP=0. General filesystem-backed program
construction is not complete. H0.4 is active: its raw `FsCompilerHost`
primitive preserves bytes, typed host failures, deterministic directory
observations, native case profiles, and realpaths. A shared program-layer
decoder now applies the vendored Node host's BOM, UTF-16 endian/odd-byte, and
invalid-UTF-8 rules to package metadata. Leading path, type, and lib
references are observed once by the parser and retained by the source request
plan. The bounded `noLib` loader now recursively discovers TypeScript-family
sources through relative requests, `paths`, and `baseUrl`, while preserving
vendored discovery and failure order. Same-tree Unix canaries prove both
`PreparedProgram` and five-bucket `ProgramSession` diagnostic equivalence
between MemoryHost and FsHost. Default/explicit libs, automatic `types`,
JavaScript membership, `rootDirs`, config roots, and the remaining platform
matrix remain.

| Phase | State | Focus |
| --- | --- | --- |
| M0–M6 | Complete | Harness, syntax, binding, types and relations, core checking, flow, inference, and overloads |
| Phase-9 2XXX | Complete | Supported-scope 2XXX closure using exact diagnostic ownership |
| M7 | Complete | Non-2XXX diagnostic families closed on the supported scope |
| M8 | Complete | Supported-scope T0–T4 closure, full-corpus FP=0, and recovery/escapes zero |
| M9 | Paused after 1b | Typed outcomes and true replay landed; production generator, burn-in, freeze, and qualification deferred |
| H0 | Active (H0.0–H0.1 complete; reviewed H0.2/H0.3 rows closed 241/241; H0.4 bounded loader and optional resolution partial) | Filesystem-hosted `--noEmit`: program construction, config/CLI, rendering, and exit behavior |

The exact accepted-state summary below is generated by
`cargo xtask readme-status` and must not be edited by hand.

<!-- STATUS:BEGIN — generated by `cargo xtask readme-status`; do not edit by hand -->
Accepted conformance state at stage marker `M8` — the checked-in
`ratchet.toml` summaries, verified against the accepted-set
artifacts by every `cargo xtask ci` run:

| View | Exact diagnostic match (T0) |
| --- | --- |
| All bands | **100.0000%** (49,024 / 49,024) |
| 2xxx all-corpus visibility | **100.0000%** (21,051 / 21,051) |
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
