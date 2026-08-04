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
observable. Config-driven compiler and project roots stay explicitly unresolved
until the matching TypeScript config parser is applied, so this planning layer
does not change any case's `not-run` state or claim a test result.
