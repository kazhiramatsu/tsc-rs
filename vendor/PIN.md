# Vendored TypeScript Pin

- npm package: `typescript`
- version: `6.0.3`
- source: `oracle/node_modules/typescript`
- `_tsc.js` sha256:
  `1c59e77a54b186ec43fa7f3e0d3c4bb15ca5eb5ba43e96b1d3a267139eddd3e3`

The vendored implementation lives under
`vendor/typescript-6.0.3/lib/`. It contains `_tsc.js`,
`typescript.js`, `typescript.d.ts`, `diagnosticMessages.json`, and the
108 layered `lib*.d.ts` files used by the oracle and future codegen.

The conformance corpus is vendored under
`ts-tests/tests/cases/conformance/`.

The upstream `compiler`, `project`, and `projects` test trees are vendored
byte-for-byte under `ts-tests/tests/cases/` from TypeScript commit
`050880ce59e30b356b686bd3144efe24f875ebc8`. Their exact Git tree IDs, blob
inventory digests, file counts, byte counts, and executable modes are pinned
in `vendor/typescript-6.0.3/test-suites-pin.v1.json`. The harness integration
contract recursively verifies every entry; the three trees are not sampled or
filtered. This is an inventory-integrity contract: it does not execute those
upstream suites or claim that the compiler passes them.

H1's additive source-universe transition is pinned separately at
`vendor/typescript-6.0.3/test-suites-pin.v2.json`. It binds the complete v1
pin by path and SHA-256, preserves its three suite entries exactly, adds the
exact 22-file `transpile` tree, and pins
`src/testRunner/transpileRunner.ts` by Git blob ID. Its SHA-256 is
`83f8edbb6f4535a19e61cf872532a46722f8cedbd2d746a0922dc507addc0879`.
The harness recursively verifies all four v2 trees. This transition records
inputs and runner identity only; it deliberately does not change the v1
expansion or claim a transpile execution result.

The v1 suites' deterministic case expansion is pinned at
`vendor/typescript-6.0.3/test-suite-expansion.v1.json`: 7,276 `compiler`
cases plus 632 `project` runner cases backed by the shared `projects` tree, for
7,908 total. Every case starts as `not-run`; inclusion in the manifest records
neither execution nor a passing result. The only command shape is
`cargo xtask upstream-suites manifest --check|--write`, with no subset,
filter, limit, or output-path option (`--suite`, `--filter`, `--limit`, or
`--out`).

`tsc_harness::upstream_suites::execution` reconstructs immutable execution
inputs from that manifest and the same `ts-tests` trees. It verifies all 7,086
pinned source paths and bytes, interns raw and decoded data by Git blob ID, and
shares each parsed fixture across its matrix/module variants. Ordered settings,
unit occurrences, descriptor properties, and the two symlink phases remain
observable.

The 103 compiler fixtures containing a virtual `tsconfig.json` are frozen
separately in
`vendor/typescript-6.0.3/compiler-config-plans.v1.json`. The artifact records
the root-planning projection produced by the vendored TypeScript 6.0.3
`parseJsonSourceFileConfigFileContent` for 106 case expansions: converted raw
config values, ordered `fileNames`, extended-source identities and contents,
effective `allowJs`/`resolveJsonModule`/`outDir`/`declarationDir`, parsed errors,
and the original-unit root/other/program-root partitions. It is not a serialized
or fully compared `ParsedCommandLine`. Regeneration and freshness checking use
only the pinned vendor and corpus:

```text
node crates/oracle/compiler-config-plans.mjs > vendor/typescript-6.0.3/compiler-config-plans.v1.json
node crates/oracle/compiler-config-plans.mjs --check
```

The producer requires Node 25.2.1 and verifies, before import, the
`typescript.js` SHA-256
`569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39`.
It also verifies the expansion-manifest SHA-256
`9c6e991103b571f7a8800dc5e1ef66088017689f3769de8fcdb408b2dc125188`.

The Rust production root planner matches this recorded projection for all 103
fixtures. The corpus has no nonempty config diagnostics, so this contract does
not establish general config-diagnostic, compiler-option conversion,
filesystem `matchFiles`, project/project-runner, or compiler-execution
compatibility. Every upstream case remains `not-run`; this layer therefore
claims no upstream test result.

The 16 contiguous official compiler fixtures
`moduleResolutionWithSuffixes_empty.ts` through
`moduleResolutionWithSuffixes_threeLastIsBlank4.ts` (compiler-fixture/source
indices 4293 through 4308) have a focused resolver oracle at
`vendor/typescript-6.0.3/compiler-module-suffixes.v1.json`. Its producer
reconstructs every virtual unit from the verified fixture bytes, checks the
expansion-manifest unit hashes, parses the embedded config with TypeScript
6.0.3, and resolves all 18 imported-module requests against a fresh in-memory
host. The artifact freezes the resolved/not-found result and ordered
`fileExists` observations, as well as upstream failed-lookup locations and
directory/read/realpath observations for review. Regeneration and freshness
checking use:

```text
node crates/oracle/compiler-module-suffixes.mjs > vendor/typescript-6.0.3/compiler-module-suffixes.v1.json
node crates/oracle/compiler-module-suffixes.mjs --check
```

The Rust harness contract compares each artifact source directly with its
manifest source and compiler-fixture rows, parses the same config through the
production root planner, then exactly compares the Rust resolver's resolution
record and ordered `fileExists` probes. The other upstream probe streams stay
frozen evidence rather than being treated as a cross-host API-equivalence
claim. The local oracle gate executes the pinned producer freshness check; the
GitHub guardrail performs syntax checking only. These 16 manifest cases remain
`not-run`: this focused artifact establishes module-suffix resolver semantics,
not complete compiler-baseline execution or a passing upstream test result.

The three official `NodeModulesSearch` project descriptors have a focused
config-to-loader oracle at
`vendor/typescript-6.0.3/project-node-modules-search.v1.json`. Its producer
verifies the descriptor and backing-project bytes against the expansion
manifest, reconstructs the pinned `projectsRunner.ts` host and option merge,
and records the CommonJS and AMD variants for six cases. The artifact freezes
config roots, effective loader-facing options, the project host's
`lib.es5.d.ts` default, exact program source order, and upstream pre-emit
diagnostics. Regeneration and freshness checking use:

```text
node crates/oracle/project-node-modules-search.mjs > vendor/typescript-6.0.3/project-node-modules-search.v1.json
node crates/oracle/project-node-modules-search.mjs --check
```

The Rust harness serves all 233 pinned `projects` files from one verified,
read-only, case-sensitive mount shared by every project variant. The focused
executor parses the selected config, applies the project-runner option and
default-library contract needed by the loader, and compares all six oracle
cases through bounded program construction. It adds an explicit `noEmit=true`
adapter because H0 does not own emit. Upstream project emit and baseline
comparison therefore remain `not-run`, as do the manifest cases themselves.
The local oracle gate executes the freshness check; GitHub Actions only
syntax-checks the producer.

`vendor/typescript-6.0.3/compiler-config-diagnostics.v1.json` separately pins
the 51 focused malformed/config-conversion fixtures and an options-diagnostic
subcorpus sourced mechanically from the official compiler cases
`pathsValidation1.ts` through `pathsValidation5.ts`. The latter verifies each
source against the expansion manifest plus byte, SHA-256, and Git-blob pins,
then records the nine filtered TS5061/5062/5063/5064/5066/5090 diagnostics from
`createProgram(...).getOptionsDiagnostics()`, including their config-file
locations. Regeneration and freshness checking use:

```text
node crates/oracle/compiler-config-diagnostics.mjs --write
node crates/oracle/compiler-config-diagnostics.mjs --check
```

This is diagnostic-oracle evidence only for those five named compiler
fixtures. Those five and every other compiler/project/project-runner case
remain `not-run` in the expansion manifest; this artifact claims no upstream
test pass.
