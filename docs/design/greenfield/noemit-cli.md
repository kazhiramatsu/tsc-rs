# H0: filesystem-hosted `--noEmit` execution contract

Status: active follow-on track after M8 batch-diagnostics completion. M9 is
paused after the merged M9.1b true-replay foundation while H0 executes.

Compatibility target: the vendored TypeScript 6.0.3 compiler only.

## 1. Purpose and completion boundary

H0 turns the completed in-memory batch checker into a filesystem-hosted,
single-project `--noEmit` compiler.

M8 already proves diagnostic compatibility when the ordered files, libraries,
compiler options, current directory, and host facts have been materialized.
H0 owns the missing boundary that turns command-line, config, and filesystem
input into that prepared program:

```text
command line
  -> config and option parsing
  -> CompilerHost-backed program discovery
  -> authoritative module/type-reference resolution
  -> PreparedProgram
  -> one-shot ProgramSession
  -> noEmit diagnostic driver
  -> rendered diagnostics and exit status
```

H0 does not reopen or redefine the frozen M8 denominator. Its host acceptance
registry is separate from the historical M8 supported-scope registry. M9
fuzz qualification resumes only through its own execution contract.

## 2. Audited entry state

The M8 checker has no remaining known relation, inference, flow, JSDoc, or
diagnostic-producer backlog on its frozen prepared-program contract.

At H0 entry, the remaining all-corpus exclusions were exactly 241
`host-resolution` rows across 30 fixtures:

| Code | Rows |
| ---: | ---: |
| 2307 | 214 |
| 2877 | 6 |
| 2792 | 6 |
| 2807 | 4 |
| 2339 | 3 |
| 2748 | 2 |
| 2322 | 2 |
| 2882, 2688, 2665, 2305 | 1 each |

The largest owner cluster is package `exports` pattern handling: 144 rows.
These rows are H0's initial host-owner inventory, not unfinished M8 checker
work.

At that entry baseline the implementation also had these driver-level gaps:

- no production compiler binary;
- no filesystem `CompilerHost`;
- no tsconfig/JSONC parser or root discovery;
- no general filesystem-backed `node_modules`, package-map, `paths`,
  `typeRoots`, or reference-types program construction;
- no `getOptionsDiagnostics` batch boundary;
- no exact command-line diagnostic gate or exit-status API;
- only the conformance per-file getter aggregate, which includes suggestion
  diagnostics that `tsc --noEmit` does not print.

Sections H0.0--H0.4 below are the current status authority: the 241-row
registry is now closed, the owned five-bucket `ProgramSession` and production
`FsCompilerHost` have landed, and the remaining config, CLI, and general
program-construction work is listed explicitly there.

## 3. Scope

H0 includes:

- one TypeScript 6.0.3 project per invocation;
- explicit source-file invocation and `-p` or automatic tsconfig discovery;
- JSONC config parsing, `extends`, `files`, `include`, and `exclude`;
- the declared H0 compiler-option allowlist with exact value and
  combination validation;
- default and explicit vendored library selection;
- relative, `paths`/`baseUrl`, `node_modules`, package `exports`/`imports`,
  conditions, `types`, `typesVersions`, `@types`, `typeRoots`, and supported
  reference-directive resolution;
- tsc-compatible source-file discovery and program ordering;
- syntactic, options, global, and semantic diagnostics under `noEmit`;
- exact plain and pretty diagnostic rendering, command-line ordering, and
  exit status; and
- case-sensitivity, canonical-path, BOM/encoding, and package-scope host
  behavior on the declared platform profiles.

`--noEmit` is mandatory in H0. An omitted or false value must not fall
through to an emitter.

## 4. Explicit non-scope

H0 does not include:

- JavaScript, source-map, declaration, or build-info emission;
- declaration-emit diagnostics selected by `declaration`, `composite`, or
  emit-only operation;
- `--build`, project-reference orchestration, or solution builds;
- watch or incremental operation;
- LSP or language-service queries;
- a public `TypeChecker` API;
- plugins or custom transformers; or
- compatibility with TypeScript versions other than 6.0.3.

An invocation requiring one of these features fails closed. It must not
silently ignore the option, check only part of the project, or report
success.

## 5. API and ownership boundaries

### 5.1 `CompilerHost`

Filesystem access is isolated behind a read-only host interface. The checker
must not call `std::fs` directly.

```rust
trait CompilerHost {
    fn current_directory(&self) -> Result<PathBuf, HostError>;
    fn use_case_sensitive_file_names(&self) -> bool;
    fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, HostError>;
    fn file_exists(&self, path: &Path) -> Result<bool, HostError>;
    fn directory_exists(&self, path: &Path) -> Result<bool, HostError>;
    fn read_directory(&self, path: &Path) -> Result<Vec<PathBuf>, HostError>;
    fn realpath(&self, path: &Path) -> Result<Option<PathBuf>, HostError>;
}
```

`MemoryCompilerHost` is the oracle and conformance adapter.
`FsCompilerHost` is the production adapter. Both feed the same resolver and
program loader.

Byte decoding is centralized and matches the vendored Node host for UTF-8,
UTF-8 BOM, UTF-16LE, UTF-16BE, and invalid UTF-8 replacement. A no-emit
host exposes no write operation. The program boundary currently owns Rust
`String` source text and JSON values, so either a raw UTF-16 payload or a JSON
escape whose value contains an unpaired surrogate cannot preserve Node's
JavaScript-string identity. They are typed decode or parse failures rather than
silent U+FFFD substitutions. Lossless support remains coupled to the existing
WTF-8 source-representation debt.

### 5.2 `PreparedProgram`

`PreparedProgram` is the only checker input owned by H0. It contains:

- display and canonical current directories;
- ordered roots, libraries, and discovered source files;
- checker-consumed `CompilerOptions`;
- program and host options such as `noLib`, `typeRoots`, `paths`, and
  `rootDirs`;
- canonical-path and package metadata;
- an authoritative resolution table keyed by source, specifier, and
  resolution mode; and
- config, option, and program-construction diagnostics.

CLI-only options such as `project`, `ignoreConfig`, and `pretty` do not enter
`CompilerOptions`.

### 5.3 `ProgramSession`

`ProgramSession` owns one `PreparedProgram` and is consumed by execution:

```rust
fn run(self) -> Result<NoEmitOutcome, DriverError>;
```

Parser, binder, and checker borrows remain inside `run`. H0 does not expose
a self-referential session, retained checker, or public query API. A future
LSP session creates a fresh checker per program version under its own
design.

### 5.4 Batch diagnostic boundary

`NoEmitOutcome` preserves separate config, syntactic, options, global, and
semantic collections. The command-line driver follows the vendored tsc
order:

1. config diagnostics;
2. syntactic diagnostics;
3. options diagnostics only when the preceding gate permits;
4. global diagnostics; and
5. semantic diagnostics only when the preceding gate permits.

Suggestion diagnostics remain available to existing per-file consumers but
are not part of `tsc --noEmit` output.

## 6. Path, ordering, and cache rules

Display file names and canonical lookup paths are distinct values. Case
folding follows the host profile; realpath and lexical normalization may not
be substituted for one another. Immediate filesystem entries use the display
basename and JavaScript's lexicographic UTF-16 code-unit order, independently
of the host's case-sensitivity profile; canonical keys remain identity-only.

Program file order follows vendored `createProgram` discovery, not parse
request order. For example, a root `a.ts` importing `b.ts` is observed as
`lib.d.ts`, `b.ts`, `a.ts`.

`NodeId` and `NodeArrayId` owner routing therefore must not assume semantic
file order equals allocation-base order. `ProgramBinder` maintains a
separately sorted interval index for node and array ownership. `SymbolId`s
remain contiguous in final bind order.

No H0 production path may rely on `Box::leak`. A one-shot session owns and
drops its sources and bound state. Any reusable lib cache is injected,
bounded, and evictable.

A cache hash is an index accelerator only. Cache reuse requires exact
ordered file-name, file-text, and binder-option equality. Forced hash
collisions must select distinct entries. Setting a cache-disable flag must
not leak one new bundle per invocation.

## 7. Owner inventory

`ratchets/host-resolution.v1.json` is the machine authority for H0 owner
closure. It is seeded from the 241 frozen M8 host exclusions and records for
each exact row:

- fixture, matrix, pass, diagnostic identity, and occurrence;
- host feature and effective `ModuleResolutionKind`;
- the exact vendored resolution-request chain, including canonical source,
  specifier, request mode, anchor kind and offset, and synthetic-request
  status;
- exact vendored tsc primary/dependency/diagnostic owner declarations, spans,
  and hashes;
- Rust resolver, loader, or consumer boundary;
- an emitting canary and a reviewed typed control, classified as
  exact-feature/same-mode, closest-available/same-mode, or the explicitly
  allowed Classic-to-Bundler alternate-mode contrast; and
- open, closed, or lapsed status, evidence, and closing commit.

The initial owner families are:

1. package `exports` patterns and blocked subpaths;
2. package `imports`, self-name references, and condition selection;
3. `node_modules` traversal, package main/types, and `typesVersions`;
4. `@types`, `typeRoots`, and type-reference directives;
5. alternate-resolver and Classic/Node/Bundler message selection;
6. external-helper and untyped-package semantic consumers;
7. program discovery, canonical paths, case collisions, and reference paths;
   and
8. config, option, batch-driver, renderer, and exit-status owners.

Rows close only through their exact owner family. A same-code or same-file
improvement cannot close an adjacent family.

An open row is exactly a live A2 host exclusion. A closed row must have left
the live set, match a non-lapsed A2 tombstone at the same full closing commit,
name an authoritative Rust producer-to-consumer route, and retain historical
All-view accepted-set evidence at T0--T4 from that commit. The historical
artifact is revalidated with its oracle-input and vendored-TypeScript pins,
and the authoritative symbols must exist both in the current tree and at the
closing commit. A `seam-only` row cannot close.

`lapsed` is distinct from closure: it records an A2 oracle-correction
tombstone. A row that had already closed retains its immutable historical
T0--T4 evidence and authoritative route when it lapses; a row corrected while
still open lapses without fabricated closure evidence.

## 8. Execution order

### H0.0 — contract and frozen inventory

- land this design;
- materialize and validate `host-resolution.v1.json`;
- pin all 241 entry identities and owner families;
- add exact vendored request chains and owner hashes plus positive canaries
  and reviewed typed controls; and
- record bounded pre-H0 CPU, wall-time, and RSS reference profiles.

H0.0 changes no checker behavior.

H0.0 is complete. `ratchets/host-resolution.v1.json` freezes all 241 exact
identities imported from the schema-2 A2 scope under the eight-family
inventory. It pins the effective module-resolution kind, exact vendored
request chain, D2 primary/dependency/diagnostic declaration spans and hashes,
an emitting canary, a reviewed typed control, and bounded pre-H0 local and
GitHub-hosted CPU/wall/RSS observations. These observations are reference
baselines, not the final resolver/CLI profiles or budgets frozen by H0.6.
`cargo xtask host-resolution check` reconciles open, closed, and lapsed rows
with live A2 exclusions and tombstones, and rejects owner/control drift or a
closed row without an authoritative Rust route and historical exact T0--T4
evidence against the trusted baseline. Every initial Rust target is
`seam-only`; those anchors identify the intended route but do not claim H0
resolution authority until the authoritative table route lands.

### H0.1 — ownership, path, and resolution seam

- add the host, program, and compiler crate boundaries;
- add `PreparedProgram`, `ProgramSession`, and typed resolution outcomes;
- separate display names from canonical paths;
- make `ProgramBinder` node/array owner routing independent of file order;
- add an owned no-emit execution path; and
- make the existing lib cache collision-safe.

This is a prerequisite-only slice: every existing accepted diagnostic and
rendered result remains byte-identical. H0.1 is complete: the legacy harness
cache now verifies exact ordered names, texts, and binder options within each
fingerprint bucket, while its cache-off path owns and drops fresh parse/bind
state locally. The process-lifetime cache remains legacy harness
infrastructure; `ProgramSession` does not use it, and any future reusable H0
production cache remains subject to the injected, bounded, and evictable rule
in section 6.

### H0.2 — authoritative `MemoryCompilerHost` module resolution

Port the vendored resolution spine behind `MemoryCompilerHost`:

- Classic, Node, Node16/NodeNext, and Bundler selection;
- `baseUrl`/`paths` and extension probing;
- `node_modules` and `@types` traversal;
- package main/types/`typesVersions`; and
- package `exports`/`imports`, patterns, conditions, and self-reference.

The checker consumes the authoritative table. The production path may not
fall back to the current “suppressed because host machinery is unknown”
verdict.

Close the TS2307 owner families first, starting with the 144-row
`exports`-pattern cluster.

Implementation status: H0.2 is partial. The reviewed in-memory route now
plans static imports, export-from declarations, external import-equals, and
literal dynamic imports with authoritative request modes;
resolves the bounded package-exports pattern, blocked-subpath, conditional,
direct TypeScript/JavaScript target, relative-substitution, and `typesVersions`
surfaces; resolves bounded package-imports exact/pattern/condition/array/null
targets, bare/self re-entry, and raw-target extension facts; and feeds loaded
or deliberately unloaded results through the exact `PreparedProgram` table.
The package-exports-patterns-and-blocked-subpaths family is closed at 179/179
rows and package-imports-self-references-and-conditions is closed at 36/36,
both with exact T0--T4 artifact evidence and all-corpus FP=0. The reviewed
legacy package-fields slice additionally covers manifestless `node_modules`,
`typings`/`types`/`main`, `typesVersions` package-root and back-reference
handling, Node ESM directory rejection, and source emit eligibility for
TS2877. It closes 6/6 rows. The reviewed types slice additionally preserves
exact triple-slash type-reference spelling and mode, probes configured and
default type roots case-sensitively, follows package metadata and real paths,
and selects import/require conditional exports for JSDoc `@import`. It closes
the 3/3 types-type-roots-and-reference-directives rows. The reviewed H0.3
consumer slices resolve ambient const-enum import and re-export aliases through
that authoritative module table, and publish the synthetic `tslib` request
needed to validate private get/set helper shapes in the source file's static
resolution mode. They close the 2/2 const-enum-module-binding rows and 4/4
external-helper-consumer rows. The reviewed alternate-resolution slice closes
the 7/7 resolution-mode-and-message-selection rows: Classic mode owns the 6
TS2792 diagnostics across import-type and type-only import matrices, while
Node10 preserves the alternate package location needed for its single TS2307
diagnostic. The reviewed untyped-package consumer slice closes the remaining
3 TS2339 rows by planning checked-JavaScript literal `require` requests and
loading their resolved module symbols, and closes the remaining TS2665 row by
planning an external-module augmentation whose JavaScript target remains
authoritatively untyped. Exact Bundler ambient and unloaded-package controls
remain non-emitting. This brings the registry to 241/241 closed with 0 rows
open. The general optional-settings slice additionally applies ordered
`paths` exact and longest-prefix wildcard selection, ordered substitutions,
and cwd- or `baseUrl`-relative candidates before the ordinary lookup in the
vendored Classic, Node10, Node16/NodeNext, and Bundler extension-pass order.
A matched mapping miss suppresses only `baseUrl` and still permits package
fallback. The general legacy type-reference slice additionally admits Classic
and Node10 on the same node-style primary/secondary lookup used by the modern
profiles. It preserves custom/default root order, nearest
`node_modules`/`@types` fallback, package identity, lexical-to-realpath
transitions, local direct-hit manifest isolation, and the explicit-mode
secondary exports boundary. The general file-probe slice also ports the full
vendored written-extension replacement groups for the TS/JS, MTS/MJS, and
CTS/CJS families. Outside Node ESM, a replacement miss proceeds to the
separate implicit-addition stage over the full written candidate; that stage
never claims `resolvedUsingTsExtension`. Package `exports` and `imports`
targets remain replacement-only, including the exact-only fast path for
targets which already carry an admitted TypeScript implementation or
declaration extension. JSON declaration twins retain the upstream
`.d.json.ts` arbitrary-extension identity. The recursive loader admits
`.d.*.ts` twins when `allowArbitraryExtensions` is enabled or the containing
source is itself a declaration file; otherwise it preserves an unloaded
authoritative row and the checker reports TS6263. These slices do not claim
the remaining general H0.2 resolution surface or H0 completion.

### H0.3 — residual host consumers

The reviewed residual consumers are complete:

- TS2339 untyped-package member behavior (3/3 rows); and
- TS2665 untyped-module augmentation (1/1 row).

The source planner publishes checked-JavaScript literal `require` requests in
their exact mode and external-module augmentation literals in the file's
static mode. The authoritative provider keeps loaded JavaScript targets as
real module symbols while unloaded JavaScript targets remain untyped, so the
checker selects the underlying member and augmentation diagnostics without a
diagnostic-specific shortcut. The exact Bundler controls freeze the negative
boundary.

No diagnostic-specific shortcut may replace the underlying resolution fact.

### H0.4 — program construction and filesystem host

- port recursive source discovery and tsc file ordering;
- load default or explicit libs and supported reference directives;
- add package-scope and canonical-path discovery;
- implement `FsCompilerHost`;
- prove MemoryHost/FsHost equivalence for the same logical tree; and
- add platform-specific case, separator, symlink, and encoding canaries.

Filesystem discovery must use the same resolver closed in H0.2 and H0.3.

Implementation status: H0.4 is active and partial. The production
`FsCompilerHost` primitive preserves raw bytes, distinguishes absence from
typed I/O failure, follows filesystem realpaths, and exposes deterministically
ordered immediate entries in JavaScript UTF-16 display-name order plus a
directory-only `CompilerHost::get_directories`
projection under an explicit or detected case profile. The
`tsc_program::CompilerConfigHost` adapter now owns the shared config-side
recursive enumeration, include/exclude filtering, UTF-16 ordering, and host
text decoding for both `FsCompilerHost` and `MemoryCompilerHost`; the CLI no
longer carries a second copy of those rules. The
shared program-layer decoder consumes those bytes with the vendored Node
host's BOM, endian, odd-byte, and invalid-UTF-8 rules, and package metadata
uses that same decoded text. Package consumers then apply the vendored
`readJson` boundary: strict JSON first, the shared JSONC AST conversion on a
syntax failure, duplicate-key last-wins semantics, and an empty field view for
empty, invalid, or non-object manifests. A present manifest remains the
nearest package scope and retains its exact decoded text even with that empty
view; host and text-decoding failures still propagate as typed failures. The
same semantic converter, but not the resolver's I/O cache, filters automatic
type packages with `typings: null`. The syntax parser owns the single leading-pragma
observation for path, type, and lib references, and the source request plan
projects those references together with module keys and the exact
resolution-only versus source-loading distinction.

The bounded recursive loader is complete through both `load_no_lib_program`
and the catalog-enabled `load_program`. Both require `noEmit=true` and accept
explicit TypeScript-family roots. With `allowJs`, explicit `.js`, `.jsx`,
`.mjs`, and `.cjs` roots, local JavaScript module dependencies, and supported
JavaScript path references join ordinary source membership. JavaScript targets
found while searching `node_modules` advance a separate external-library depth
on every external resolution edge and join membership through the inclusive
`maxNodeModuleJsDepth` boundary. Deeper targets retain authoritative unloaded
resolution rows. Config projection retains fractional and infinite JavaScript
numbers, applies the `jsconfig.json` default of two, and lets an own null/invalid
value mask that default back to createProgram's zero. The canonical compiler
option value additionally preserves programmatic NaN. Every unloaded row
carries its reason across the compiler
seam, so an `allowJs` program cannot silently accept an unexplained local
unloaded target. A `.jsx` module target without an active JSX mode is retained
without reading its bytes and produces TS6142; an already-owned `.jsx` source still
produces TS6142 without losing its module symbol. ReactJSX/ReactJSXDev,
`jsxImportSource`, and leading `@jsxImportSource`/`@jsxRuntime` pragmas now
publish TypeScript's synthetic runtime request (`react/jsx-runtime`,
`react/jsx-dev-runtime`, or the configured package) immediately after the
synthetic `tslib` request, preserving the checker-visible source order.
`@jsxRuntime classic` suppresses that synthetic request. Effective
`resolveJsonModule` admits explicit JSON roots as well as explicit JSON
requests. `noDtsResolution` applies TypeScript's implementation-file mask,
removes `types`/`types@...` package conditions, and suppresses declaration
fallbacks. The no-lib
wrapper requires explicit `noLib=true`. Ordered `rootDirs` participate in
relative module resolution and recursive source membership. Their normalized
display paths select the strict longest prefix, probe the original candidate
first, and visit the remaining roots in declaration order. Classic and Node10
preserve their outer TypeScript/declaration then JavaScript/JSON passes, while
Node16/NodeNext/Bundler finish all admitted extensions per root candidate. The
catalog-enabled route also admits absent or false `noLib`, retains lowercased
raw `compilerOptions.lib` keys, treats an explicit empty list as suppressing
the default library, and lets H0.5 publish TS5053 at both option names for the
`noLib` plus `lib` combination while the lower loader suppresses all library
host work as `createProgram` does. Ordered `paths` mappings and `baseUrl`
participate in recursive source discovery through the shared resolver. Roots
are normalized and visited one at a time, preserving input order,
multiplicity, and observable failure precedence. An extensionless root retains that requested
path in `PreparedRoot` while its source identity records the first existing
candidate from `.ts`, `.tsx`, and `.d.ts`; `allowJs` appends only `.js` and
`.jsx`. The modern and JSON extensions remain outside this first probe group,
and a complete miss produces fileless TS6231 with the full supported-extension
display list. Canonical source identities are loaded once, cycles and diamonds
are staged without duplicate files, and `SourceFileId`s are assigned only
after discovery.

Case-insensitive host aliases retain every alternate display spelling on the
owning `PreparedSourceFile`. The default/absent
`forceConsistentCasingInFileNames` value publishes TS1149 at the program
preprocessing boundary, including the `The file is in the program because:`
message chain with one root-file reason per root occurrence; an explicit
`false` keeps the collapsed source without that diagnostic. Config projection
and the harness option allowlist carry the same tri-state value.

`LibraryCatalog::typescript_6_0_3` injects static metadata for the exact 107
logical library names and 95 distinct mapped files. It performs no runtime
`_tsc.js` parse and obtains every byte through the same `CompilerHost` as user
sources. Absent-target/ES2025 selection starts at `lib.es2025.full.d.ts`, and
ES2015 preserves the `lib.es6.d.ts` compatibility root. A real-vendor contract
pins the transitive closure sizes at 82 files for ES2025, 19 for ES2015, and
15 for explicit `es5` plus `dom`.

For each source the loader follows the vendored construction phases: each
path reference performs its DFS before the next path reference; every unique
type-reference key is resolved before any resolved type target is visited;
each enabled lib reference performs its sequential DFS before the module
phase; and every module key is resolved before any source-loading module
target is visited. Under `noLib`, lib-reference occurrences remain counted
but perform no host work. After all requested roots finish, non-wildcard
`types` entries retain input order and multiplicity. An absent or empty list
performs no automatic discovery. A list containing `"*"` expands effective
`typeRoots` in declared order, or nearest-to-farthest ancestor
`node_modules/@types` directories from the config-file directory/current
directory. Expansion retains that JavaScript UTF-16 display-name order, probes
manifests before filtering dot directories, excludes exactly packages whose
decoded JSON or JSONC has `typings: null`, and performs case-sensitive stable
first-wins deduplication after flattening.

The `noResolve` branch follows the same boundary as
`findSourceFileWorker`: path and type-reference discovery is skipped, while
module requests are still resolved and retained as authoritative unloaded
rows without adding their targets to source membership. An explicitly rooted
target remains owned and is still published normally.

All automatic names are resolved under the normalized
`__inferred type names__.ts` synthetic origin and unspecified mode before the
first target is visited. Resolved declaration targets then run sequential DFS
at root depth zero as ordinary sources; external-library reachability follows
the resolver fact. Misses produce fileless TS2688 with the explicit or
implicit inclusion chain. Repeated explicit names retain raw diagnostic
occurrences while final diagnostic consumption sorts and deduplicates them.
An empty requested-root list suppresses this phase, whereas a requested but
missing root does not. A normalized `ProgramOptions::config_file_path` anchors
both automatic and source-owned default type-root lookup. H0.5 additionally
retains the root config source and the first matching UTF-16 string syntax by
option/value from the first root `compilerOptions` object, without carrying a
parser arena across the program boundary. A missing explicit automatic type
therefore publishes TS1419 at its `types` entry; wildcard discovery selects
the `"*"` entry. A missing target-selected default library publishes TS1426
at the exact `target` literal. Case-only values, inherited-only syntax, and an
absent target intentionally have no root-config location. The pinned API
oracle also preserves TypeScript 6.0.3's asymmetric explicit-`lib` behavior:
the mapped `lib.es5.d.ts` option value does not match the raw `"es5"` config
literal, so that missing-root TS6053 has no TS1423 related information.

Default or explicit library roots are selected only after the automatic type
phase. Publication then forms a stable catalog-priority default-library prefix
followed by ordinary dependency postorder without replaying host work.
TypeScript keeps distinct
`processingDefaultLibFiles` ordering and checker-visible `libFiles` sets when
a default library owns a path reference; the current `PreparedProgram` prefix
cannot encode that non-contiguous state, so this shape fails typed as
`default-library-path-references`. The pinned 6.0.3 library graph contains no
such path references. Unknown lib references produce located TS2726/TS2727,
known-but-missing reference targets produce located TS6053, and missing
selected roots produce fileless TS6053 with the default-target or
explicit-option inclusion chain. An ordinary/default-library canonical
collision remains a typed unsupported boundary because publication cannot
silently move an already classified source across the prefix.

The resulting `PreparedProgram` owns source text, roots, library membership,
package-scope and implied-format facts, program-construction diagnostics, and
authoritative type/module rows. Supported misses remain normal tsc
diagnostics or `NotFound` rows. JavaScript resolutions remain unloaded under
`allowJs=false`; with `allowJs`, local JavaScript joins source membership while
external JavaScript is admitted through `maxNodeModuleJsDepth` (default zero).
If an existing source is later reached outside `node_modules`, its path, type,
lib, and module references are reprocessed; a merely shallower revisit retries
only imports previously elided by the depth boundary. An
explicit `.json` request loads JSON only when `resolveJsonModule` is effective.
External-library reachability remains part of source emit eligibility.
Roots, path references, and local source discovery retain lexical identities
without a blanket `realpath` host call. An external resolver transition visits
its physical `resolvedFileName` directly and retains the lexical
`originalPath` on the authoritative row, whether the target is loaded or
unloaded. Module-extension classification remains attached to that lexical
spelling even when the physical target has a different suffix or none.
Publication preserves the resolver's display spelling while binding
loaded rows to a validated `SourceFileId`; the compiler projects that id to the
checker and keeps the physical path for unloaded-JavaScript diagnostics.
Same-tree Unix canaries, including `rootDirs`,
`paths`, `baseUrl`, and Classic/Node10 type-reference candidates, prove that
`MemoryCompilerHost` and `FsCompilerHost` produce the same prepared program;
the compiler-level canary separately proves the same five-bucket
`ProgramSession` diagnostic outcome.

Each invocation supplies independent ceilings for unique source files,
request occurrences before resolution-key deduplication, zero-based source
depth, raw bytes per source, and total raw source bytes. A separate structural
depth cap of 256 protects the recursive worker. Host, decode, resolution,
preparation, unsupported-scope, and ceiling failures retain typed operation
and path context. Source-byte ceilings begin after a host returns its owned
payload and do not include resolver-owned or wildcard-discovery `package.json`
bytes. The request ceiling applies to final automatic-name occurrences, not
raw directory candidates filtered during wildcard expansion. These limits do
not claim to bound a host's single-read/listing allocation or all resolver or
discovery I/O; wildcard enumeration is opt-in through an explicit `"*"`.
Before the recursive JSON syntax parser runs, package manifests receive a
non-recursive token preflight: unsupported expression tokens expose empty
package fields directly, and object/array nesting above 256 does the same. The
converter itself uses an explicit task stack.

An explicit `preserveSymlinks=true` now keeps external non-relative module and
type-reference results on their lexical link identities without publishing
`originalPath`; absent or false retains the physical-target identity and its
lexical `originalPath`. The policy is program-owned, and source publication
therefore deduplicates only the physical-policy result.

This is deliberately not general H0.4 program construction. The first H0.5
root-planning slice now parses the recorded projection for all 103 virtual
compiler configs (106 case expansions). The frozen TypeScript 6.0.3 oracle has
167 fixture-level roots (170 case-weighted) and the compiler runner's
original-unit stable partition is preserved. This fixed corpus has four
fixtures with `extends`, one with `files`, one with `include`, none with
`exclude`, no `jsconfig.json`, and no nonempty config diagnostics; it is not a
general proof of those semantics. General filesystem config discovery and
package redirects during program construction remain in later slices. The
focused case-only alias diagnostics now match the pinned TypeScript oracle;
the complete cross-platform case/separator/symlink/encoding matrix remains
open. Discovery stays sequential where vendored host calls and failure
precedence are observable; future pipeline parallelism must preserve that
contract.

The compiler-suite harness now exposes a bounded `load_compiler_no_emit`
adapter. It reconstructs the recorded compiler fixture VFS (including
document/global symlink identities), projects loader-relevant options and
root selection, and returns the same catalog-backed `PreparedProgram` used by
the Rust no-emit session. Representative default, type-reference,
case-sensitive, virtual-config, and preserve-symlink fixtures exercise this
boundary. The local audit now loads and executes all 7,276 recorded compiler
plans through `ProgramSession` with zero failures; that proves the Rust
no-emit session boundary, not upstream baseline equivalence. The adapter and
audit intentionally stop before emit, baseline comparison, and the remaining
explicitly unsupported resolver families (duplicate package identities,
conflicting `noLib`/`lib`, and unowned Windows/UNC path forms); those oracle
tiers remain `not-run` until their owners are ported. A
compiler-crate integration contract feeds representative prepared programs
through `ProgramSession::run`, so this source/config/loader seam is exercised
by the actual Rust no-emit session without adding the expensive corpus sweep
to GitHub Actions.

A focused project-runner bridge now owns the official `NodeModulesSearch`
config-to-loader path for its three descriptors under CommonJS and AMD. All
233 files in the pinned `projects` tree are verified once and exposed through
one immutable, case-sensitive, read-only mount shared by the 632 project case
plans. For the six focused variants the bridge reproduces project config
selection, the loader-facing existing-option projection, relative root
discovery, AMD request planning, the host-selected `lib.es5.d.ts`, exact root
and source publication order, inclusive external-JavaScript depth,
shallower/root reprocessing, and automatic `@types` membership. An official
frozen oracle records those facts and the 17 upstream pre-emit diagnostics.
The Rust boundary deliberately adds `noEmit=true`; it neither emits nor
compares project baselines, and all six manifest cases remain `not-run`. The
local compiler contract nevertheless runs the six prepared programs through
`ProgramSession` and compares their source/global diagnostic rows with the
frozen oracle (option-deprecation rows remain on `ConfigRootPlan`). This is
not general `ProjectConfig` or `DiscoverConfig` execution.
The mount removes repeated corpus-artifact decoding by Git blob identity; each
independent program still owns its source-text decode and parse. Reusing that
prepared text/parse work across project variants is a later performance slice
and must preserve option-sensitive source identity and publication order.

The project harness now also exposes a shared no-emit adapter for all three
descriptor root modes: explicit `inputFiles`, an explicit `project` config,
and `tsconfig.json` discovery at the project root. It applies the runner's
existing `module`, `moduleResolution`, `strict`, and no-error-truncation
defaults before forcing `noEmit=true`, and it delegates config validation to
the program-owned fail-closed gate. The adapter is qualified on representative
explicit and discovered projects, including the `NestedDeclare` ambient
import-equals boundary. Descriptor/config requests for declaration, source
maps, output paths, and other emit-only controls remain typed unsupported
outcomes; `noResolve` is projected into the same source-discovery boundary as
the production loader and is no longer rejected by the adapter.

### H0.5 — tsconfig and command-line driver

- port JSONC config parsing and config diagnostics;
- implement `extends`, `files`/`include`/`exclude`, and supported option
  merging;
- implement `-p`, automatic config discovery, explicit-file handling, and
  TypeScript 6.0's `--ignoreConfig` and TS5112 rule;
- port `getOptionsDiagnostics` for the declared option surface;
- add the exact no-emit diagnostic gate;
- add plain and pretty rendering plus error-summary behavior; and
- add exit status and a production binary.

Unknown options and valid-but-out-of-scope options are distinct failures and
are never ignored.

Implementation status: H0.5 is active and partial. `tsc_program` owns an
immutable, shareable `ConfigRootPlan` for the recorded valid root-planning
projection. JSONC values use the iterative syntax-AST converter. A separate
51-fixture TypeScript 6.0.3 oracle fixes recoverable primary/extended
syntax, unknown/type/enum option errors, missing/read-error/circular `extends`,
invalid specs, empty/no-input diagnostics, UTF-16 locations, host-call order,
readable extended source text, identity-only `extendedSourceFiles`, and the
`absent`/`undefined`/value option states. All seven compiler-option list schemas
are converted: the shared 107-entry `libMap`, path-list normalization and
root-level `${configDir}` substitution, falsy filtering, plugin object/array
elements, and `moduleSuffixes`' preserved JavaScript `undefined` slots. The
sole object compiler option, `paths`, accepts JavaScript object-like arrays,
retains recursive own-key order and `undefined` identity, carries its stored
`pathsBasePath` through inheritance, and substitutes direct string elements of
array-valued mappings at the final consuming root (including TypeScript's
changed-array-to-object copy shape). Typed maps are shared across the extends
graph and allocate an ordinary JSON projection only at observation boundaries.
File-path scalars share that final substitution pass. The official compiler
`pathsValidation1` through `pathsValidation5` sources are mechanically
reconstructed from the pinned expansion manifest and byte/blob identities.
Their TS5061/5062/5063/5064/5066/5090 option diagnostics match code, message,
final sorted order, and UTF-16 root-config location after `${configDir}`
substitution, including duplicate syntax, compacted-array indices, and
inherited fallback locations. They remain separate from parsed-config errors
as in `getOptionsDiagnostics`. `ConfigModuleResolutionOptions` projects the
currently modeled resolver-facing option surface, including the
`forceConsistentCasingInFileNames` casing-diagnostic switch. Effective `paths` and its
declaring `pathsBasePath` share one immutable allocation; `ModuleResolver` selects
`baseUrl` then `pathsBasePath` then cwd for substitutions without treating the
latter as a baseUrl fallback. Exact keys and valid single-star offsets are
compiled once, mappings are shared between resolver instances, and per-request
substitution-vector clones are eliminated; structural validation is cached
with that immutable table instead of rescanned by every resolver.
The public `ConfigRootPlan` also retains the effective `files`, `include`,
and `exclude` spec lists after extends rebasing, preserving absent versus
explicit-empty states alongside the discovered `fileNames` projection.
The public plan also retains the primary-only raw `references` value and the
effective inherited `watchOptions`, `typeAcquisition`, and `compileOnSave`
values. These fields are observable for ParsedCommandLine parity, while the
no-emit program gate rejects truthy unsupported scopes before loading sources.
It additionally projects normalized `projectReferences` entries and the
stable `wildcardDirectories` watcher roots (including recursive flags) without
enabling project-reference orchestration or watch mode.
`moduleSuffixes` is projected without normalizing case, whitespace, empty
entries, or recoverable JavaScript `undefined` slots. Every resolver file
candidate uses TypeScript's extension-major/suffix-minor probe order, including
arbitrary and declaration-like extensions, while package manifests remain
unsuffixed and package-field exact targets retain TypeScript's predicate-only
publication quirk. Selected spellings stay observable to the host while their
program/cache identities use normalized paths, so separator and dot aliases
deduplicate without weakening case-only collision checks. A focused frozen
oracle covers the 16 TypeScript 6.0.3 compiler fixtures, their 18 resolution
requests, and all 78 ordered `fileExists` probes. Failed-lookup and the other
host-probe streams are retained as oracle evidence but remain outside the
current Rust result contract; trace publication and the full compiler
baselines therefore remain `not-run`.
Diagnostic-class invalid mappings recover without turning option errors into
infrastructure failures. After TS5063/TS5064 are retained, non-array mappings
own an empty miss and non-string array elements are omitted instead of
replaying JavaScript's context-dependent coercions or runtime `TypeError`.
Root syntax locations are indexed lazily only when an option diagnostic needs
them, so valid large maps do not retain a second per-element location table.
Large invalid lists and maps use explicit phase/order indices rather than
inversion-based or repeated-search repair. Fatal errors are reserved for host,
path, resource, and explicitly unsupported conversion boundaries. The harness
uses a case-insensitive virtual adapter specialized for the fixed compiler
fixture units, parses each fixture once before matrix variants, and compares
raw config values, ordered `fileNames`, extended-source identities and
contents, four discovery-option values, the exact `ParseConfigHost` operation
trace for all 103 config-bearing fixtures (106 matrix cases), and original-unit
partitions with the official oracle. Include patterns are compiled once and reused through an
iterative, linear-scratch UTF-16 matcher; candidate matching does not recurse,
build a quadratic memo table, or require a regex dependency. The production
`CompilerConfigHost` now applies the same matcher to real and in-memory
recursive trees, prunes impossible directories, honors explicit package-folder
includes, and suppresses symlink cycles through host realpaths. The fixed
compiler-corpus `ParseConfigHost`/`matchFiles` trace qualification is now
green; general real-filesystem profiles and the remaining root fields are
still open.
Remaining root fields,
remaining resolver/loader options, full `ParsedCommandLine`, and the complete
project baseline/emit runner remain open. The validated projection now exposes
the merged checker/program options and has a fail-closed `load_config_program`
bridge into the general catalog-backed loader: config and fatal option
diagnostics stop source loading first, while TypeScript 6.0 deprecation rows
(5101/5107) remain reportable without blocking a no-emit program. An omitted
or false `noEmit` is rejected before any source host work. A command-line
override applies `noEmit=true` without mutating the remaining config options.
The program gate also maintains an
explicit allowlist for options projected into `CompilerOptions`,
`ProgramOptions`, or root discovery; a recognized option outside that set
fails as a typed unsupported scope instead of being silently ignored. The
same gate retains truthy `watchOptions`, `typeAcquisition`, and
`compileOnSave` root scopes across the `extends` graph and rejects them before
source loading, rather than dropping an inherited root setting. The
compiler crate now ships a bounded
`tsc-rs` binary that discovers a config from the current directory or `-p`,
accepts explicit files only with `--noEmit`, adapts `FsCompilerHost` to the
config parser's filtered directory contract, and renders contextual or plain
diagnostics to stdout with the contextual error summary; usage, host, config,
loader, and unsupported
option failures are stderr/exit 2. The initial CLI surface intentionally
rejects watch/build/emit options, project-plus-file mixes, and other
unimplemented flags instead of silently ignoring them. Focused
filesystem binary contracts cover include discovery, command-line no-emit
precedence, diagnostic output, no-output writes, and version/unsupported-option
behavior. Local ignored oracle contracts compare plain and explicit `--pretty`
`-p`/`--noEmit` results byte-for-byte with vendored `_tsc.js`, including the
status-2 no-emit diagnostic result, the ANSI contextual renderer, current-
directory config discovery, encodings, symlinks, `rootDirs`, and Node16/18/20/
NodeNext package modes. This bridge remains independent of CLI selection so
the same immutable plan can feed MemoryHost and FsHost differentials.

### H0.6 — qualification and release

- close all 241 registry rows at exact diagnostic and rendered tiers;
- run the full host, config, filesystem, and CLI suites;
- freeze the H0 option, host, platform, and resource profiles;
- verify packaging contains the exact 6.0.3 library catalog;
- update README and setup documentation without changing M8 historical
  claims; and
- publish the binary only after the release gate is green.

## 9. Fail-closed policy

The following are hard nonzero outcomes:

- unsupported or ambiguous command-line/config option;
- project references, build, watch, incremental, emit, or plugin requests;
- unsupported resolution mode or package-map construct;
- undecodable or non-representable path under the declared platform
  profile;
- host read, metadata, or canonicalization failure;
- duplicate canonical path with incompatible content;
- missing source text required for rendering;
- cache-key equality failure or inconsistent cached bundle;
- configured file, byte, depth, or resource ceiling exceeded; and
- panic, malformed internal result, or incomplete program discovery.

A supported resolution miss becomes the corresponding tsc diagnostic. An
unsupported resolver branch becomes a typed H0 failure; it must not be
converted into `Missed`, `Suppressed`, an empty diagnostic list, or exit
zero.

TypeScript no-emit program and config diagnostics are reported through the
normal diagnostic stream and exit two, matching the vendored driver's
`DiagnosticsPresent_OutputsGenerated` result at its no-emit emit boundary.
Command-line selection diagnostics such as TS5112 retain exit one. H0
infrastructure or unsupported-scope failures are reported separately and
also exit two. Success is exit zero.

## 10. Definition of done

H0 is complete only when all of the following are true:

- all 241 frozen host identities match at T0–T4 on the H0 scope;
- every pre-H0 accepted identity remains accepted and full-corpus FP remains
  zero;
- no H0-scope resolution ends as suppressed, unknown, or untriaged;
- MemoryHost and FsHost produce identical `PreparedProgram`s and diagnostics
  for equivalent trees;
- CLI stdout, stderr, ordering, rendering, and exit status match vendored
  `_tsc.js --noEmit` on the frozen command/config suite;
- suggestion diagnostics never enter no-emit CLI output;
- no H0 invocation writes JavaScript, declarations, maps, build info, or
  other output;
- explicit/config root ordering, package modes, case behavior, and encodings
  pass their oracle canaries;
- cache-on and cache-off results are byte-identical and forced collisions
  are safe;
- no production H0 path uses `Box::leak` or an unbounded process-global
  cache;
- determinism, repeated-run, thread-count, current-directory, and
  platform-profile invariants are green;
- cold and warm wall time plus peak RSS remain inside the frozen H0 budgets;
  and
- the installable binary carries the exact vendored library catalog and
  requires no Node runtime.

M9 fuzz qualification, emit, LSP, project builds, and the public
`TypeChecker` API remain separate goals.

## 11. CI and merge policy

During implementation, use focused owner tests and targeted MemoryHost
differentials. Do not run full corpus or multi-platform probes in the edit
loop.

For each semantic H0 candidate:

1. run focused Rust and Node oracle tests;
2. run the affected owner-family gate;
3. run cache-on/cache-off and forced-collision tests when cache or ownership
   changes;
4. run `CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=2 cargo xtask ci --baseline
   origin/main` exactly once on the committed final candidate;
5. run the Windows path smoke only when host, path, config, or toolchain
   behavior changes (the required local gate already covers macOS); and
6. merge automatically with a merge commit after every required check
   passes.

The 241-row MemoryHost oracle artifact is content-addressed and regenerated
only when its resolver, host, option, vendored compiler, or fixture inputs
change. Full Node resolution sweeps must not run for unrelated Rust changes.

Documentation-only changes follow the repository Markdown-only exception:
review the rendered diff, links, anchors, and generated boundaries; run
`git diff --check`; do not run Cargo, Node, or full-corpus CI.

No H0 PR may update M8 completion status, M9 qualification state, or claim
host closure from a partial owner family.
