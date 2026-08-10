# tsc-rs

`tsc-rs` is an independent Rust implementation of the TypeScript batch
diagnostics checker. Its compatibility target is one exact release,
**TypeScript 6.0.3**, and its results are continuously compared with the
checked-in TypeScript compiler and conformance corpus.

> **Current availability:** `tsc-rs` is not yet a drop-in replacement for
> the `tsc` command. The parser, binder, checker, and contextual diagnostic
> formatter are exercised through the repository's existing in-memory test
> and conformance harness. A bounded filesystem-hosted `--noEmit` command is
> now available through the `tsc-rs` compiler binary: it discovers
> `tsconfig.json`, accepts `-p` and explicit files, renders contextual
> diagnostics, and fails closed on options outside the supported surface.
> General package/project behavior, emission, watch mode, and a stable public
> checker API are still under development or outside the current scope.

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
borrows within one run, separates the five no-emit diagnostic buckets, and
now exposes a distinct fail-closed emit entry backed by the typed protocols in
`crates/emitter`. Transformer/printer execution and JavaScript output are not
yet connected.
Existing conformance tests still assemble files, libraries, options, and
  resolution facts through the harness and checker APIs. The bounded
  filesystem/config/CLI path is available through the production binary;
  general package/project discovery and the remaining broad config surface
  are still being closed.

## Build and Explore

The corpus, oracle, library files, and accepted artifacts are checked in. No
bootstrap or package-install step is required.

Requirements:

- [Rust](https://www.rust-lang.org/tools/install) via `rustup`. The pinned
  toolchain and required `rustfmt` and `clippy` components are declared in
  [`rust-toolchain.toml`](rust-toolchain.toml).
- Node.js for oracle probes, conformance comparisons, and golden refreshes.
  The accepted version is pinned in [`.node-version`](.node-version).

Clone the repository first:

```sh
git clone https://github.com/kazhiramatsu/tsc-rs.git
cd tsc-rs
```

### Build the tagged H0 no-emit snapshot

`h0-noemit-v1` is the frozen TypeScript 6.0.3-compatible, single-project
`--noEmit` snapshot. Build that exact tag as follows:

```sh
git fetch --tags
git switch --detach h0-noemit-v1

cargo build \
  --release \
  --locked \
  --manifest-path crates/compiler/Cargo.toml
```

The executable is `target/release/tsc-rs`:

```sh
./target/release/tsc-rs --version
./target/release/tsc-rs --noEmit -p /path/to/project
```

To install the same tagged binary into Cargo's executable directory instead:

```sh
cargo install --locked --path crates/compiler
tsc-rs --noEmit -p /path/to/project
```

The build reads the checked-in `vendor/typescript-6.0.3/lib` catalog and
embeds it in the binary; it performs no package/bootstrap download and does
not require Node.js. After the binary is built or installed, execution does
not require either Node.js or the repository's `vendor/` directory.

### Contributor checkout

To return to current development and run the workspace checks:

```sh
git switch main
git pull --ff-only

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
CARGO_BUILD_JOBS=2 cargo xtask test emitter -- --test-threads=2
CARGO_BUILD_JOBS=2 cargo xtask test compiler -- --test-threads=2
```

`checker`, `host`, `program`, `emitter`, and `compiler` are stable workspace roles rather
than Cargo package names. Contributor commands and CI use
`cargo xtask test <role>`, so an internal package rename does not require every
workflow and document to be rewritten. `cargo xtask workspace audit` verifies
the role metadata and rejects direct package or binary selectors in repository
automation.

These commands run internal libraries and tests; the bounded no-emit binary can
also accept a project or source file. The complete local acceptance gate is intended
for contributors and is substantially more expensive than the commands
above:

```sh
CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=2 cargo xtask ci --baseline origin/main
```

If that local command fails, rerunning the exact command automatically reuses
only successful phases whose repository inputs, toolchain, environment,
baseline, and required outputs still match. `--fresh` discards the failed-run
journal. A green run deletes the journal, so the next independent invocation
is a full gate again. Generated-evidence freshness runs before the expensive
workspace-test phase, and unrelated ratchet/corpus edits do not invalidate the
workspace-layout audit. Rust tests still fail closed on verification inputs
because several test targets consume checked-in ratchets and schemas directly.

GitHub Actions deliberately runs only `cargo xtask acceptance`. That stable
entrypoint consumes the checked-in `ts-tests` acceptance corpus and currently
runs all 5,908 diagnostic fixtures / 7,691 expanded cases. Formatting,
workspace tests, fine-grained phase controls, stress, and evidence production
remain in the complete local gate.

For example, from a project containing a compatible `tsconfig.json`:

```sh
cargo run --manifest-path crates/compiler/Cargo.toml -- --noEmit -p .
```

The built `tsc-rs` binary contains the exact TypeScript 6.0.3 standard-library
catalog and does not look up `vendor/` or launch Node at runtime.

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
| Owned `PreparedProgram` execution path | Available for the frozen one-shot H0 no-emit profile |
| Color-free contextual diagnostic formatting | Available in the conformance harness |
| Filesystem-hosted `--noEmit` command | Available for the frozen H0 command-line and config profile |
| tsconfig discovery and JSONC configuration | Available for the frozen H0 option/root profile; other recognized fields fail closed |
| `node_modules`, package `exports`/`imports`, `paths`, and `typeRoots` resolution | Available for the frozen Classic-through-Bundler H0 profile |
| Output emission (`.js`, `.d.ts`, source maps, or build info) | Not implemented |
| Watch, incremental, project-reference, or solution builds | Outside the current scope |
| Language server and stable public `TypeChecker` API | Outside the current scope |
| TypeScript versions other than 6.0.3 | Unsupported |

The frozen filesystem profile is limited to a single-project, mandatory
`--noEmit` flow. Unsupported operations fail closed rather than check
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
| `crates/emitter` | Typed emit artifacts, output topology, outcomes, failures, and in-memory sink |
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
- [H1 JavaScript emit contract](docs/design/greenfield/h1-emit.md);
- [post-H1 completion slices](docs/design/greenfield/post-h1-completion-slices.md);
- [persistent Program and incremental-parser design](docs/design/greenfield/lsp-and-incremental.md);
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

Milestones M0–M8, the frozen H0 filesystem-hosted `--noEmit` profile, the
L0/L1 persistent-source and incremental-parser prerequisites, and the bounded
H1 JavaScript-emit profile are complete. H1.0a–H1.6 close the reviewed emit
owner graph, no-emit non-regression boundary, typed execution protocols,
detached factory/transform/printer pipeline, scoped resolver, output planning,
memory/filesystem sinks, CLI behavior, failure semantics, upstream
qualification, and resource evidence. The final qualification dispositions
all 15,680 pinned upstream cases, executes the sole compatible compiler case
with exact diagnostics, JavaScript bytes, callback metadata, write order,
result presence, and exit status, and keeps seven adjacent controls fail-closed
before their first sink write.

H1 is deliberately narrower than broad one-shot `tsc`: its active runtime is
whole-Program `.ts` emit at `target=ESNext`, `module=Preserve`, without maps,
declarations, bundles, downlevel transforms, JavaScript/JSX-family inputs, or
the wider option/config/host matrix. The selected route toward complete
TypeScript 6.0.3 compiler and tooling coverage is:

1. expand one-shot JavaScript emit, starting with effective
   `module=ESNext`/implied-format ESM and CommonJS closure at
   `target=ESNext`, then file kinds, option families, JSX/decorators, helpers,
   and targets from newest to oldest; close config, host/System, library
   replacement, CLI, and output behavior on the same path;
2. implement source maps alongside transformer expansion, then declaration
   emit, bundles/`outFile`, and declaration maps;
3. land one shared versioned Program/resolution/invalidation substrate for
   full `DocumentRegistry`, `isProgramUptoDate`, old-Program reuse, resolution
   caches, and watcher dependency sets;
4. build deterministic signatures/`.tsbuildinfo`, project references,
   solution build, and watch on that substrate;
5. stabilize cancellation and the public compiler/custom-transform API, then
   implement Language Service and tsserver before the independent Rust-native
   LSP adapter; and
6. close M9 confidence production, locales/platform/package matrices,
   reproducible release artifacts, and only then begin a post-6.0.3 transition.

This order follows the pinned compiler ownership: ordinary CLI compilation
constructs a Program before emit; builder/watch adds signatures, affected-file
state, build info, project orchestration, and invalidation around that Program;
Language Service synchronizes through `isProgramUptoDate` and
`createProgram(oldProgram)`; and LSP is not an upstream TypeScript protocol.
Sharing the Program/resolution invalidation layer keeps builder/watch and
service work from growing separate caches with incompatible stale-state rules.
The branch-sized order and per-slice gates are fixed by the
[post-H1 completion slices](docs/design/greenfield/post-h1-completion-slices.md);
the audited
[compiler compatibility residual](docs/design/greenfield/compiler-compatibility-residual.md)
and [LSP/incremental design](docs/design/greenfield/lsp-and-incremental.md)
define the detailed finish lines. Ordinary GitHub CI remains the single
`cargo xtask acceptance` entrypoint while the complete gate remains local.

H0.0 now freezes the 241 exact host-resolution identities in a dedicated
machine-checked registry. Each row pins its owner family, vendored TypeScript
primary/dependency/diagnostic declarations and hashes, effective
module-resolution kind, exact vendored source/specifier/request-mode and
anchor chain, Rust seam, and a positive canary plus reviewed typed feature
control. The local semantic gate checks the registry against the same trusted A2
baseline. A closed row must match its non-lapsed A2 tombstone, name an
authoritative Rust route, and retain historical accepted-set proof at T0--T4;
oracle-correction tombstones remain explicit `lapsed` rows. Initial bounded
pre-H0 local CPU, wall-time, and RSS observations are recorded with the
registry. H0.6 freezes the final CLI/local-gate profiles and budgets in
`ratchets/h0-qualification.v1.json`.

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

H0.0 and H0.1 are complete. H0.2 is complete for the frozen H0 profile: its
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
The general legacy type-reference slice now admits Classic and Node10 on the
same node-style primary/secondary spine: custom and default roots, nearest
`node_modules`/`@types`, package identity, lexical-to-realpath transitions,
and explicit-mode secondary exports retain the vendored lookup order and
failure boundary. General file resolution now also applies the complete
TypeScript 6.0.3 written-extension replacement groups for TS, JS, MTS, and CTS
families. Non-Node-ESM misses continue through the separate implicit-addition
stage, while package-map targets retain their exact-only TS boundary and never
receive that second stage. The two stages preserve their distinct
`resolvedUsingTsExtension` provenance, and `.d.json.ts` twins retain their
arbitrary-extension identity. Such `.d.*.ts` twins join source membership
when `allowArbitraryExtensions` is enabled or the importer is a declaration
file; otherwise the authoritative row remains unloaded and drives TS6263.
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
diagnostic. All 241 host-resolution rows are closed at T0--T4 with typed
Bundler controls and full-corpus FP=0. H0.4 is complete for the frozen
host/platform profile: its raw `FsCompilerHost`
primitive preserves bytes, typed host failures, deterministic directory
observations in JavaScript UTF-16 display-name order, native case profiles,
and realpaths. A shared program-layer
decoder now applies the vendored Node host's BOM, UTF-16 endian/odd-byte, and
invalid-UTF-8 rules to package metadata; resolver and automatic-type consumers
also share strict-JSON-then-JSONC `readJson` semantics while retaining exact
manifest text. Leading path, type, and lib
references are observed once by the parser and retained by the source request
plan. The bounded loaders now recursively discover TypeScript-family sources
through relative requests, ordered `rootDirs`, `paths`, and `baseUrl`, while
preserving vendored discovery and failure order. An explicit extensionless
root retains its requested spelling while probing `.ts`, `.tsx`, and `.d.ts`;
`allowJs` appends only `.js` and `.jsx`, and a complete miss reports TS6231.
With `allowJs`, explicit JavaScript roots, local JavaScript module dependencies,
and supported JavaScript path references join that same owned source graph.
JavaScript targets found while searching `node_modules` are admitted through
the inclusive `maxNodeModuleJsDepth` boundary; deeper rows remain unloaded and
retain their admission reason. Config-derived depth values preserve TypeScript's
fractional and infinite JavaScript-number comparisons, including the
`jsconfig.json` default; programmatic NaN keeps its unordered comparisons.
The focused official `NodeModulesSearch` project bridge now serves the complete
233-file upstream `projects` tree from one verified shared mount and executes
the three descriptors under both CommonJS and AMD. It matches the pinned
project runner's config roots, loader-facing option projection, ES5 host
default library, and exact source order through bounded no-emit program
construction. The upstream project runner still owns emit and baseline
comparison, so all six manifest cases deliberately remain `not-run`.
The general project no-emit adapter also classifies all 632 recorded project
cases: all 82 H0-compatible plans load and execute successfully, while the
remaining 550 request only the declared emit/watch non-scope and fail closed.
Later shallower and root discoveries reprocess
exactly the imports or full reference phases required by TypeScript. A `.jsx`
module target without an active JSX mode is not
read and produces TS6142, while an explicit `.jsx` root or path reference can
still join membership. Effective `resolveJsonModule` also admits explicit JSON
roots. `rootDirs` uses the longest display-path prefix, probes the original
location first, and then visits alternate roots in declaration order.
Default/explicit libraries and
post-root explicit or wildcard automatic `types` participate in the same
owned program graph; wildcard discovery is stable across case profiles because
it consumes that JavaScript-compatible host order.
Same-tree Unix canaries prove both `PreparedProgram` and five-bucket
`ProgramSession` diagnostic equivalence between MemoryHost and FsHost; the
program-level canaries exercise Classic, Node10, Bundler, ordered optional
settings including `rootDirs`, and wildcard `@types` discovery. Loaded roots
preserve their lexical identity without a blanket realpath observation.
External resolver transitions instead load the physical `resolvedFileName`
and retain the lexical `originalPath` on each loaded or unloaded resolution;
extension classification remains tied to that lexical spelling;
the checker consumes the validated source id for loaded rows and the physical
path for TS7016. An explicit `preserveSymlinks=true` instead keeps external
non-relative modules and type references on their distinct lexical link
identities, while absent or false retains physical-source deduplication.
General config-derived root selection and exact package-ID redirects have also
landed; the focused case-only alias diagnostics match the pinned TypeScript
oracle, and all 7,276 recorded compiler plans load and execute through the Rust
no-emit session in the local structural audit. The declared platform profiles
are full local macOS qualification and a focused Windows x64 host/program
filesystem canary. Ambiguous raw drive-relative roots remain outside the
profile and fail closed.

| Phase | State | Focus |
| --- | --- | --- |
| M0–M6 | Complete | Harness, syntax, binding, types and relations, core checking, flow, inference, and overloads |
| Phase-9 2XXX | Complete | Supported-scope 2XXX closure using exact diagnostic ownership |
| M7 | Complete | Non-2XXX diagnostic families closed on the supported scope |
| M8 | Complete | Supported-scope T0–T4 closure, full-corpus FP=0, and recovery/escapes zero |
| M9 | Paused after 1b | Typed outcomes and true replay landed; production generator, burn-in, freeze, and qualification deferred |
| H0 | Complete (frozen single-project no-emit profile; 241/241 host rows, 7,276/7,276 compiler plans, 82/82 compatible project plans, exact CLI/program oracles) | Filesystem-hosted `--noEmit`: program construction, config/CLI, embedded libraries, rendering, and exit behavior |
| L0/L1 | Complete and performance-qualified | Shared text/position snapshots, domain-scoped identity leases, owned bind/Program snapshots, immutable incremental parsing/rebinding, registry reuse, exact fresh equivalence, reclamation stress, and approved large-edit evidence |
| H1 | Complete and performance-qualified (H1.0a–H1.6) | Bounded `ESNext`/`Preserve` whole-Program `.ts` JavaScript emit is exact and fail-closed |
| H2 | H2.0a/H2.0b, H2.1a–H2.1e module formats, H2.2a–H2.2d TypeScript transforms, H2.3a JavaScript/JSX/JSON source-output families, H2.4a/H2.4b decorators/class fields, H2.5a ESNext lowering, and H2.5b ES2021 lowering complete; H2.5c ES2020 lowering next | [Branch-sized broad one-shot compiler slices](docs/design/greenfield/post-h1-completion-slices.md), with 595 fully exact rows, 1,221 exact reported diagnostics, 849 exact writes, 5 output-exact diagnostic controls, twelve explicit later-owned source deferrals, 20 ES2021-target, 20 ESNext-target, 19 legacy-decorator, 19 standard-decorator/class-field, and 14 JSON owner controls, all five JSX modes, and immutable H0/H1/L1/resource lineage |

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
