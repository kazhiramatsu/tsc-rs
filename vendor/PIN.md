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

The reviewed H1 owner projection of that exact `_tsc.js` is pinned at
`ratchets/h1-owner-inventory.v1.json` (SHA-256
`6148160678bf0b34a8310551eac8c9ab3f2afb1cd9260fa8eaa59efadc71abb5`)
under `.github/ci/contracts/h1-owner-inventory.schema.json` (SHA-256
`433809f8489f12ec34c3de10dbecc8fad019bf0d7f777302298b10cfaf707cf2`).
Schema 2 retains 6,193 active-root declarations and 24,054 ownership edges,
records 5,202 reviewed call-site dispositions (711 exact-symbol edge sites and
4,491 classified non-edges) plus 442 structural-property candidate sets, and
requires zero unresolved or undispositioned calls.

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

The runner-derived companion inventory is pinned at
`vendor/typescript-6.0.3/transpile-suite-inventory.v1.json` (SHA-256
`5762b9fbf86c0437ff3bfc2f1e1deab48e35effc8d9117299035d23d6c45f949`).
It reconstructs the exact 22-fixture, 25-configuration `TranspileRunner`
matrix as 42 fixture units, 37 cases, and 79 per-unit operations. All 37 cases
retain execution state `not-run`: 14 are JavaScript transform/printer controls,
two are deferred source-map controls, 20 are deferred declaration controls,
and one is a deferred declaration-map control. No case is admitted to the H1
bootstrap profile. The manifest pins expected reference-baseline paths but
does not vendor or compare their contents. Regeneration and freshness checking
use only the pinned vendor and corpus:

```text
node crates/oracle/h1-transpile-inventory.mjs --write
node crates/oracle/h1-transpile-inventory.mjs --check
```

This establishes exact runner expansion and classification only, not a
transpile execution result, baseline parity, or equivalence between the
component API and H1 whole-Program emit.

The next additive H1 source transition is pinned at
`vendor/typescript-6.0.3/test-suites-pin.v3.json`. It binds the complete v2
pin by path and SHA-256, preserves all four suite entries and the
`transpileRunner` source identity exactly, and has SHA-256
`5f7aee7d434066017c5cd115fb2195ff4959e5203eddc7ed9dafaf705cb38b34`.
The v3 pin records the complete 6,568-file FourSlash source-tree identity but
vendors only the 38 fixtures whose DSL body directly calls one of the four
emit-output verification operations. Those exact 31,051 bytes are described
by `vendor/typescript-6.0.3/fourslash-emit-projection.v1.json` (SHA-256
`d652d0e0ad1a6195cb3d74e97cb241f3da6a55b6811bd4770fb1ec56a2843c46`),
along with 38 operation lines, 49 ordered `emitThisFile` directives, two
false-positive controls, the full-tree identity, and the projection tree and
blob inventories. The v3 pin also records the Git blobs for
`src/harness/fourslashImpl.ts` and
`src/harness/fourslashInterfaceImpl.ts`.

The projection producer can re-scan a pinned upstream checkout and always
checks the checked-in projection offline:

```text
node crates/oracle/fourslash-emit-projection.mjs --check
node crates/oracle/fourslash-emit-projection.mjs --check --source-root /path/to/TypeScript/tests/cases/fourslash
```

This is an inventory-only `not-run` transition. It adds zero expansion,
execution, or passing rows; it claims neither a FourSlash pass rate nor
Language Service or whole-Program emit equivalence. The complete FourSlash
tree and runner are not vendored or executed.

The projection's exact H1 disposition is pinned separately at
`vendor/typescript-6.0.3/fourslash-whole-program-equivalence.v1.json`
(SHA-256
`3960e1beefe159ff9d609d7ff66d979b0f30198edcb0bf7f82efdfc1e9b0ff4b`).
It consumes the v3 pin, projection, frozen H1 profile, and vendored
`typescript.js` byte-for-byte. The artifact pins the two FourSlash harness
source blobs plus the exact bundle declarations proving that each selected
operation calls `LanguageService.getEmitOutput(fileName)`, then
`getFileEmitOutput`, then targeted `Program.emit(sourceFile)`. Its 36
`emitThisFile=true` selections and two active-file selections yield 47
targeted calls. All 38 cases use `target=ES2025`; module is absent for 30,
CommonJS for seven, and AMD for one. None matches H1's target-free
whole-Program request or required `ESNext`/`Preserve` profile, so promotions
are zero and every row remains deferred, unpromoted, and `not-run`. Reference
baseline comparisons are zero. Regeneration and freshness checks are fixed
and unfiltered:

```text
node crates/oracle/h1-fourslash-equivalence.mjs --write
node crates/oracle/h1-fourslash-equivalence.mjs --check
```

This is route/profile classification evidence only. It claims neither
FourSlash execution nor Language Service or whole-Program emit parity.

The additive H1 source-universe pin v4 is
`vendor/typescript-6.0.3/test-suites-pin.v4.json` (SHA-256
`9cd0b499d22c8936b78d1bd30d5ab7faa295b23903e838953fddaaffc48d52d4`).
It binds v3 byte-for-byte, preserves all four existing full suites, all three
implementation-source identities, and the FourSlash projection exactly, then
appends the complete `tests/cases/conformance` tree: 5,908 files, 3,825,804
bytes, 5,862 unique blobs, Git tree
`9d28e54f5b0c7695ca2de6b1a15508dc35b0db98`, and no executable paths. The
Rust suite contract reconstructs that Git tree and its blob inventory from the
checked-in files. This transition adds no case expansion, execution, baseline
comparison, or passing result; the existing diagnostic harness evidence is not
promoted to an upstream emit-suite result.

The conformance runner's subsequent inventory-only expansion is pinned at
`vendor/typescript-6.0.3/conformance-suite-expansion.v1.json`. It reproduces
the TypeScript 6.0.3 `CompilerBaselineRunner` `/\.tsx?$/` enumeration and
dynamic 77-option variation contract: 5,907 enumerated fixtures, one pinned
`.js` not-enumerated control, 7,697 cases, six runner observations per case,
and 46,182 case-observations. Every case and observation starts `not-run`;
execution-result and compared-reference-baseline counts are zero. The Rust
producer and independent Node reconstruction are fixed, unfilterable checks:

```text
cargo xtask h1-conformance manifest --check
node crates/oracle/h1-conformance-expansion.mjs --check
```

Regeneration uses `cargo xtask h1-conformance manifest --write`. The command
accepts no suite, filter, limit, or output-path selector. This expansion does
not classify a case into the H1 bootstrap profile and does not claim Program
construction, JavaScript emit, baseline parity, or an upstream pass rate.

The separate effective-option classification is pinned at
`vendor/typescript-6.0.3/conformance-profile-classification.v1.json`
(SHA-256
`d54b51d44da91836ad1b7be3be4b7de19c6892e6cb1fe9605fda06cdda5a67eb`).
It consumes the expansion byte-for-byte and reproduces virtual `tsconfig`
parsing, compiler-runner defaults, and harness/matrix override precedence for
all 7,697 cases. The artifact records 27 virtual configs, two config-diagnostic
fixtures, and 7,655 applicable JavaScript observations. Only three cases match
both `target=ESNext` and `module=Preserve`; all three retain another effective
option blocker, so every case is explicitly deferred and the bootstrap
admission count is zero. Every execution state remains `not-run`, no reference
baseline is compared, and the option-level zero-admission proof makes no
source-reachability or syntax-support claim. Regeneration and freshness checks
are fixed and unfiltered:

```text
node crates/oracle/h1-conformance-classification.mjs --write
node crates/oracle/h1-conformance-classification.mjs --check
```

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

The H1 compiler-runner classification is pinned separately at
`vendor/typescript-6.0.3/compiler-profile-classification.v1.json` (SHA-256
`3c3cbcb3c29a5254c145dc2665ca21683a1bd94f5271841320f061e82b614603`).
It consumes the expansion, config-plan artifact, frozen H1 profile, and
vendored TypeScript bundle byte-for-byte. It verifies all 103 virtual configs
and 106 config variants against the config-plan oracle before reproducing
compile defaults and harness/matrix override order for all 7,276 compiler
rows. Seven rows match `target=ESNext` plus `module=Preserve`; five retain an
effective-option blocker. The remaining two receive a vendored TypeScript
Program reachability analysis: reachable `export =`/`import =` defers
`modulePreserve1.ts#default`, leaving
`esmNoSynthesizedDefault.ts#module%3Dpreserve` as the sole bootstrap
candidate. The final 7,273 deferred-profile, two H0 `noEmit`, and one
candidate rows all remain `not-run`, and zero reference baselines are
compared. Regeneration and freshness checking are fixed and unfiltered:

```text
node crates/oracle/h1-compiler-classification.mjs --write
node crates/oracle/h1-compiler-classification.mjs --check
```

This is classification and admission evidence only. It claims neither Rust
Program/emit execution nor compiler-runner baseline parity or a passing
upstream test result.

The H1 project-runner classification is pinned separately at
`vendor/typescript-6.0.3/project-profile-classification.v1.json` (SHA-256
`4e984ab1847a551ecbe9f8736237aee74a8c42134403e7cb56a624a2fef78d87`).
It consumes the unchanged expansion, frozen H1 profile, vendored TypeScript
bundle, and focused six-case project oracle byte-for-byte. It reproduces the
pinned `projectsRunner.ts` defaults,
descriptor/compiler-option conversion, config precedence, and root-selection
order for all 316 descriptors and both CommonJS/AMD variants. The 632 rows
contain 570 explicit-input selections and 62 config-backed selections, with
six deliberately missing explicit roots, 74 config roots, and zero config
diagnostics. JavaScript observation is applicable to 572 rows. Every row has
both a required target and module blocker, so admissions are zero. Every row
remains `not-run`, and zero reference baselines are compared. Regeneration and
freshness checking are fixed and unfiltered:

```text
node crates/oracle/h1-project-classification.mjs --write
node crates/oracle/h1-project-classification.mjs --check
```

This is root/option classification and admission evidence only. It claims
neither Rust Program/emit execution nor project-runner baseline parity or a
passing upstream test result.

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
claim. The local oracle gate executes the pinned producer freshness check;
ordinary GitHub CI does not execute or syntax-check this producer. These 16
manifest cases remain
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
The local oracle gate executes the freshness check; ordinary GitHub CI does
not execute or syntax-check the producer.

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
