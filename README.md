# tsc-rs

`tsc-rs` is an independent Rust implementation of the **batch diagnostics
checker** in TypeScript 6.0.3. It aims to reproduce the diagnostics of one
exact, vendored `tsc` bundle—from diagnostic identity through rendered CLI
output—and measures progress by differential testing against that bundle on
the TypeScript conformance corpus.

> **Development status:** the checker is not yet complete or distributed as a
> drop-in `tsc` replacement. The active implementation and its verification
> tools live in the [`tsrs2/`](tsrs2/) Cargo workspace.

## Compatibility Target

In this project, “TypeScript compatibility” always means
**TypeScript 6.0.3 exactly**: the checked-in
[`_tsc.js`](tsrs2/vendor/typescript-6.0.3/lib/_tsc.js) bundle, its library
files, and the matching conformance corpus. The vendored compiler is both the
source-level specification for porting and the executable oracle used by the
test harness.

The supported compatibility target covers:

- batch syntactic and semantic diagnostics;
- emit-free suggestion diagnostics;
- grammar and unused diagnostic families;
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
- deep JSDoc-driven checking of JavaScript files;
- compatibility with TypeScript versions newer than 6.0.3.

These boundaries are diagnostic-identity based: excluded oracle rows remain
visible in the all-corpus results and may not be hidden by fixture, code, or
glob exclusions. See the normative
[definition of done](docs/design/greenfield/definition-of-done.md) for the
full contract.

## Status

Milestones M0–M6 are complete: the harness and code generators, scanner and
parser, binder, type and relation foundations, checker skeleton, flow
narrowing, inference, and overload resolution are in place. The phase-9
2XXX sweep is also complete: its supported-scope T0 false-negative residue is
zero while the all-corpus false-positive gate remains zero.

Work is underway in M7, which covers grammar, unused, suggestion,
program/options, and remaining non-2XXX diagnostics. Its first
checker-grammar owner slices now cover modifier/decorator grammar and
object-literal grammar, followed by the signature parameter-list grammar
owner, declaration-file top-level source grammar, and derived-constructor
`this`/`super` ordering. The Node CommonJS top-level-`await` work is split
by its two actual emitters; both `checkAwaitGrammar` and the separate
`for await` grammar producer are complete. The regex work is likewise split
at its real boundary: target-aware scanner/parser plumbing (including ES5
identifier tables, recovery rescan, and unterminated-literal state) is
complete as an accepted-set-neutral prerequisite, and the complete UTF-16
regex validator now follows it with generated Unicode-property data, exact
target gates, and primary/related diagnostic grouping. The fresh residual
survey selected the module-format sequence next. Its A10 prerequisite now
keeps implied Node format tri-state, distinguishes explicit package
`"commonjs"` from a missing package type, and preserves decisive extension
evidence for emit. B16 now completes the `resolveExternalModule`
Node16/Node18 synchronous-import owner, including import-equals,
type-only resolution-mode attributes, nested conversion details, and the
diagnostic-only package `exports`/`imports`/self-name target projection.
The separate A11 `checkExportAssignment` producer is now complete as
well: checked-JavaScript `export =` uses the decisive emit format, and
the exact verbatim/isolated type-only export rules are live. The TS1340
`getTypeFromImportTypeNode` owner is now complete too, using a
diagnostic-only package-module meaning projection without publishing
package symbols or members generally. The module-format sequence now
also includes the complete TS1361/TS1362 type-only alias value-use
worker, with exact import/export related information and checked-JavaScript
publication. The first 8.1f producer is complete as well: object-literal
private-name placement now closes all 31 TS18016 rows and brings the
checker-grammar canaries to 4/4. Exact position review corrected the
remaining TS18028 split: both checked-JavaScript rows belong to
`checkGrammarAccessor`, and both are now complete. The following
`checkJSDocTypeIsInJsFile` slice closed all 12 nullable/non-nullable
TypeScript-syntax rows (TS17019/TS17020); JSDoc-only M8 diagnostics
remain closed. The final 8.1f producer then closed the 12 residual
TS18010 accessibility rows at their exact JSDoc tag spans without
opening the general JSDoc checking surface. Together the four 8.1f
producer slices added all 57 planned identities. The fresh 8.1g
residual survey then selected the already-ported `checkESModuleMarker`
owner: its caller now uses the Node package file's per-file emit format
and closes all eight TS1216 rows across TypeScript and checked
JavaScript. The following direct `checkImportDeclaration` slice closes
the six package-`exports` TS1543 rows as well: it reuses the bounded
diagnostic-only package-target projection to inspect JSON target file
names while ordinary package resolution remains suppressed. The next
direct `checkImportMetaProperty` slice publishes its four already-exact
CommonJS-format TS1470 rows in checked JavaScript, completing the
TypeScript/JavaScript matrix without broadening JSDoc checking. The
following `checkImportDeclaration` slice now selects the JavaScript-
specific TS1473 top-level-context diagnostic while retaining TS1232
for the TypeScript sibling. Its separate `checkExportDeclaration`
counterpart now does the same for TS1474 versus TS1233. The next
`checkAliasSymbol` producer slice ports the live isolated/verbatim
type-only import and re-export rules (TS1205, TS1288, TS1448,
TS1484, and TS1485), including exact TS1377 related origins. It also
uses the exact extension-sensitive CommonJS message helper for the
three TS1295 alias rows owned by this producer; the export-assignment
and dynamic-import rows stayed with their separate producers. The
following `checkExportAssignment` slice then publishes the three
CommonJS export-default TS1295 rows through that shared helper. The
final TS1295 slice closes the dynamic-import row at its direct
`checkGrammarImportCallExpression` owner, preserving tsc's
highest-priority CommonJS/verbatim grammar branch and whole-call span.
The next `checkModuleDeclarationDiagnostics` slice closes the final
TS1287 namespace row after confirming that tsc deliberately excludes
module declarations from the generic modifier producer: only an
instantiated top-level CommonJS namespace is diagnosed, while its
type-only sibling remains clean.
M7 reuses the approach that made the 2XXX sweep effective: measure
exact oracle rows first, group them into `(diagnostic code, pass)`
owner families, trace each family through the emitting `tsc` function
and its Rust implementation boundary, then port one bounded producer
slice at a time. See the
[M7 band and owner strategy](docs/design/greenfield/m7-band-and-owner-strategy.md).

The stage marker remains `M6` while M7 is active and advances only when the
milestone closes; this keeps M7-owned escape deadlines live during its
producer slices.

<!-- STATUS:BEGIN — generated by `cargo xtask readme-status`; do not edit by hand -->
Accepted conformance state at stage marker `M6` — the checked-in
`tsrs2/ratchet.toml` summaries, verified against the accepted-set
artifacts by every `cargo xtask ci` run:

| View | Exact diagnostic match (T0) |
| --- | --- |
| All bands | **96.5568%** (47,336 / 49,024) |
| 2xxx band | **97.4063%** (20,505 / 21,051) |
| Syntactic | **99.8219%** (2,242 / 2,246) |

False positives are a hard gate: 0 on every merge. Escape
ceilings: untagged 0, recovery 117. Non-2XXX family
map: frozen, 15 families / 433 rows.

M8 readiness (report-only until M7 close): 2/10 gates ready.
Ready: shadow-tiers, emitter-inventory. Pending: m7-gate, scope-frozen, rust-function-dispositions, emitter-dependency-closure, runtime-coverage, differential-fuzzer, performance-baseline, m7-family-rollup.
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

The active corpus-wide merge gate is T0 plus absolute all-corpus `FP = 0`.
T1–T3 are already measured as shadow evidence and are completed vertically
for every family touched by a slice; T4 activates after the exact scope is
globally frozen. Determinism, idempotence, job independence, ledger
freshness, and escape inventories are separate mandatory gates.

## Roadmap

| Phase | State | Focus |
| --- | --- | --- |
| M0–M6 | Complete | Harness, syntax, binding, types/relations, core checking, flow, inference, and overloads |
| Phase-9 2XXX | Complete | Supported-scope 2XXX T0 closure using emitter ownership and exact-row mining |
| M7 | Current | Grammar, unused, suggestion, program/options, family ownership, T1 activation, and structural diagnostics |
| M8 | Next | Readiness-gated long-tail mining, supported-scope T2/T3 and T4 closure, escape retirement, and performance/runtime evidence |
| M9 | Final hardening | Differential-fuzzer steady state and closure of every known divergence class |

M7 is more heterogeneous than the 2XXX band even if no single remaining
family is as broad. To retain a useful denominator and a clear owner, the
machine-readable A5 family map acts as a set of **virtual bands**. The first
checker-grammar sweep is split by producer—modifier/decorator, object
literal, declaration/function/accessor/heritage,
statement/expression/target, module/import/export/format, and
strict/private/JSDoc/ES-target—before a terminal residue pass.

## Getting Started

The corpus, oracle, library files, and goldens are checked in; no bootstrap or
package-install step is required.

Requirements:

- [Rust](https://www.rust-lang.org/tools/install) via `rustup`. The repository
  pins the toolchain and required `rustfmt`/`clippy` components in
  [`rust-toolchain.toml`](rust-toolchain.toml).
- Node.js for oracle probes, oracle-driver tests, and golden refreshes. The
  accepted version is pinned in [`tsrs2/.node-version`](tsrs2/.node-version).

Build and run the primary checks from the active workspace:

```sh
git clone https://github.com/kazhiramatsu/tsc-rs.git
cd tsc-rs/tsrs2

cargo build --workspace
cargo test --workspace
cargo xtask conformance --band 2xxx
cargo xtask ci
```

`cargo xtask ci` is the full local merge gate and includes formatting,
Clippy, build/tests, generated artifacts, accepted-state and scope/family
audits, all/2XXX/syntactic conformance, invariants, ledgers, escapes, and
README status freshness. See [setup and verification](docs/setup.md) for
focused commands and environment details.

## Repository Layout

| Path | Responsibility |
| --- | --- |
| `tsrs2/crates/syntax` | Scanner, parser, generated AST schema, traversal, and source-file representation |
| `tsrs2/crates/binder` | Symbols, scopes, declarations, assignments, and flow graph construction |
| `tsrs2/crates/types` | Shared compiler options, symbols, types, signatures, and relation data |
| `tsrs2/crates/checker` | Program assembly and semantic diagnostic checking |
| `tsrs2/crates/diags` | Generated diagnostic messages, diagnostic structures, line maps, sorting, and deduplication |
| `tsrs2/crates/harness` | TypeScript fixture expansion and program-input construction |
| `tsrs2/crates/oracle` | Node-based access to the vendored `tsc` oracle |
| `tsrs2/crates/conformance` | Differential comparison, exact identities, ratchets, scope, and family reports |
| `tsrs2/crates/fuzz` | Differential-fuzzing foundations for M8/M9 |
| `tsrs2/crates/xtask` | Code generation, audits, conformance commands, and the complete CI gate |
| `tsrs2/ts-tests` | Checked-in TypeScript conformance corpus |
| `tsrs2/vendor/typescript-6.0.3` | Pinned compiler bundle and standard library files |
| `docs/design/greenfield` | Authoritative architecture, execution plan, and completion contracts |

The original v1 implementation is no longer in the working tree and is
preserved at the `v1-final` tag.

## Contributing

Porting is evidence-led: read the matching vendored `tsc` function, probe the
oracle when behavior is ambiguous, capture immutable before evidence, and
then implement the smallest producer-owned slice. Expected diagnostics and
messages come from the oracle rather than memory.

Start with:

- [repository workflow and verification rules](CLAUDE.md);
- [greenfield execution guide](docs/design/greenfield/README.md);
- [completion convergence plan](docs/design/greenfield/completion-convergence-plan.md);
- [M7 band and owner strategy](docs/design/greenfield/m7-band-and-owner-strategy.md).

The status block in this README is generated. After changing accepted state or
readiness evidence, run `cargo xtask readme-status` from `tsrs2/` instead of
editing the block by hand.

## License

Licensed under the [MIT License](LICENSE).
