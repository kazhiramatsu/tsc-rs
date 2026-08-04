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

Their deterministic case expansion is pinned at
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
