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

The remaining all-corpus exclusions are exactly 241 `host-resolution` rows
across 30 fixtures:

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

The current implementation also has these driver-level gaps:

- no production compiler binary;
- no filesystem `CompilerHost`;
- no tsconfig/JSONC parser or root discovery;
- no general filesystem-backed `node_modules`, package-map, `paths`,
  `typeRoots`, or reference-types program construction;
- no `getOptionsDiagnostics` batch boundary;
- no exact command-line diagnostic gate or exit-status API;
- only the conformance per-file getter aggregate, which includes suggestion
  diagnostics that `tsc --noEmit` does not print.

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
host exposes no write operation.

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
be substituted for one another.

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
the 3/3 types-type-roots-and-reference-directives rows. The first reviewed
H0.3 consumer slice resolves ambient const-enum import and re-export aliases
through that authoritative module table, suppresses a duplicate imported
access diagnostic, and preserves the global ambient access diagnostic. It
closes the 2/2 const-enum-module-binding rows, bringing the registry to 226/241
closed with 15 rows open. This does not claim general H0.2 resolution beyond
the reviewed in-memory routes, filesystem hosting, or H0 completion.

### H0.3 — residual host consumers

Close the remaining host rows by exact owner:

- TS2792 and the remaining TS2307 alternate-resolution/message selection;
- TS2807 external-helper module shape;
- TS2339 untyped-package member behavior;
- TS2665 untyped-module augmentation.

No diagnostic-specific shortcut may replace the underlying resolution fact.

### H0.4 — program construction and filesystem host

- port recursive source discovery and tsc file ordering;
- load default or explicit libs and supported reference directives;
- add package-scope and canonical-path discovery;
- implement `FsCompilerHost`;
- prove MemoryHost/FsHost equivalence for the same logical tree; and
- add platform-specific case, separator, symlink, and encoding canaries.

Filesystem discovery must use the same resolver closed in H0.2 and H0.3.

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

TypeScript and config diagnostics are reported through the normal diagnostic
stream and exit one. H0 infrastructure or unsupported-scope failures are
reported separately and exit two. Success is exit zero.

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
5. run required hosted Linux Rust, semantic, and CLI lanes;
6. run macOS and Windows path smoke only when host, path, or config behavior
   changes; and
7. merge automatically with a merge commit after every required check
   passes.

The 241-row MemoryHost oracle artifact is content-addressed and regenerated
only when its resolver, host, option, vendored compiler, or fixture inputs
change. Full Node resolution sweeps must not run for unrelated Rust changes.

Documentation-only changes follow the repository Markdown-only exception:
review the rendered diff, links, anchors, and generated boundaries; run
`git diff --check`; do not run Cargo, Node, or full-corpus CI.

No H0 PR may update M8 completion status, M9 qualification state, or claim
host closure from a partial owner family.
