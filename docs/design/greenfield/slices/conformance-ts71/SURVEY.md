# TypeScript 7.1 (Go) compiler-test harness survey, for a tsc-rs conformance-runner rebuild

Status: research record for the [TypeScript 7.1 conformance packet](README.md), 2026-09-29.
It was produced by a read-only survey of the pinned upstream checkout and of tsc-rs.
The scratch artifacts named below (emulation scripts, tree listings, blob indexes)
lived in a session scratchpad and are **not** kept in the repository. Every number
here is a one-off emulation result; the P1 Rust runner re-measures them and its
records supersede this file.

No repository file was modified, and no cargo/Go/test command was run.

## 0. Sources and method

| Item | Identity |
| --- | --- |
| Upstream | microsoft/TypeScript `main` @ `19dadef8888ba5b27d8b9f622480745cf623e020` (2026-09-29, "Restore idempotency to `resolveObjectTypeMembers` (#64372)"). No 7.1 tag. The Go port lives under `tsc/`, and the TS 6.x JS sources are no longer on `main`. Checkout: `…/scratchpad/ts71/repo` |
| tsc-rs (read only) | `~/dev/tsc-rs-perf-r10`, HEAD `0ef1ac10e` on `refactor/remove-staging-scaffolding`. It has uncommitted changes in `crates/checker`, `crates/harness/{Cargo.toml,tests}`, `crates/xtask` and `pins/`. None of the files cited below from `crates/conformance`, `crates/harness/src` or `crates/oracle` were modified. |
| 6.0.3 corpus in tsc-rs | `ts-tests/tests/cases/{compiler,conformance,transpile}`, vendored byte for byte from TS commit `050880ce59e30b356b686bd3144efe24f875ebc8` (`crates/harness/src/upstream_suites.rs:27`) |

How the evidence was gathered:

- **Case deltas.** 6.0.3 and 7.1 cases were compared by git blob id. The 7.1 ids come from `git ls-tree`. The 6.0.3 ids come from `git hash-object --no-filters` over the vendored files, so no blobs had to be downloaded.
- **Harness model.** Go's `GetFileBasedTestConfigurations` and `SkipUnsupportedCompilerOptions` were emulated in Python and checked against the file names in the 7.1 baseline tree. On the 12,054 unchanged compiler and conformance cases, every predicted configuration matched, apart from the explained no-output configurations.
- **Fetched blobs, about 29 small files.** Sparse-added: `diagnosticwriter.go`, `declscompiler.go`, `enummaps.go`, `commandlineoption.go`, `compiler/program.go`, `core/compileroptions.go`, `ast/diagnostic.go`, `CHANGES.md` and `diagnosticMessages.json`. Fetched with `cat-file`:
  - 8 changed case files
  - 4 `.errors.txt` baselines
  - 5 bundled libs
  - `Herebyfile.mjs`
  - `.github/skills/compiler-and-fourslash-tests/SKILL.md`
  - TS 6.0.3 `src/harness/harnessIO.ts`, fetched by the blob sha that tsc-rs pins (`a06bde1c…`) and saved at `scratchpad/ts71/fetched/harnessIO.603.ts`

---

## 1. Layout at the pinned commit (Q1)

### 1.1 Test cases: `tsc/testdata/tests/`

| Directory | Files | Notes |
| --- | ---: | --- |
| `cases/compiler` | 6,839 | 6,701 `.ts`, 137 `.tsx`, 1 `.js`. The runner regex `\.tsx?$` (`compiler_runner.go:28`) excludes the `.js`, so 6,838 tests run. |
| `cases/conformance` | 5,911 | 5,699 `.ts`, 211 `.tsx`, 1 `.js`. 5,910 tests run. The same 52 top-level sub-directories as 6.0.3, plus a new `es2026/` (11 tests). |
| `cases/transpile` | 25 | Run by `TranspileBaselineRunner` (`transpile_runner.go`). |
| `lib/` | 4 | `react.d.ts`, `react16.d.ts`, `react18/{global,react18}.d.ts`, mounted at `/.lib`. There is no `lib.d.ts`. |

- There is no `cases/fourslash`. Fourslash tests are now Go files: 4,368 files under `tsc/internal/fourslash`.
- There is no `cases/project` or `cases/projects`. tsc-rs still vendors these 6.0.3 suites.

### 1.2 Baselines: `tsc/testdata/baselines/reference/`

48,169 files in total.

| Subtree | Files |
| --- | ---: |
| `compiler/` | 23,354 |
| `conformance/` | 22,179 |
| `fourslash/` | 1,749 |
| `config` | 229 |
| `tsc` | 224 |
| `tsbuild` | 192 |
| `tsoptions` | 80 |
| `tsbuildWatch` | 65 |
| `tscWatch` | 42 |
| `transpile` | 41 |
| `astnav` | 7 |
| `lsp` | 3 |
| `project` | 2 |
| `api` | 2 |

The `compiler/` and `conformance/` baselines are **flat**: there are no sub-directories, and `conformance/es6/x/foo.ts` has baselines at `conformance/foo(...)`. Strada used a single flat `tests/baselines/reference/` for both suites.

| Kind | compiler | conformance |
| --- | ---: | ---: |
| `.symbols` / `.types` | 6,691 / 6,691 | 6,088 / 6,088 |
| `.js` (includes `.d.ts` output and a `//// [DtsFileErrors]` section) | 6,167 | 6,018 |
| `.errors.txt` | **3,451** | **3,869** |
| `.sourcemap.txt` / `.js.map` | 137 / 130 | 20 / 20 |
| `.trace.json` | 72 | 76 |
| `.contentmapper.txt` (Go-only) | 15 | 0 |
| Configurations with any baseline | 6,840, over 6,653 cases | 6,582, over 5,809 cases |

Baseline names are `<basename-without-ext>(<cfg>).<kind>`. `<cfg>` is the sorted list of lower-cased `key=value` pairs of the **varying** options, for example `foo(module=commonjs,target=es2015).errors.txt`.

### 1.3 The submodule split is gone

Evidence:

- No `submodule` directory and no `.diff` files exist in the baseline tree.
- There is no `.gitmodules` file and no `_submodules/` directory.
- The only compiler test entry point is `TestLocal` (`compiler_runner_test.go:12`), which runs both suites. It asserts that no two test files, across compiler and conformance, share a basename (`:23-30`).
- `SKILL.md` §1.3 documents the same.

TS-inherited cases were "promoted" into the same directories as the Go-only cases. `tsc/testdata/promotedTestCollisions.txt` (18 lines) records how clashes were handled:

- 2 identical duplicates were removed.
- 13 are `renamed-promoted X.ts -> X_promoted.ts`. The Go-local test keeps the original name, and the Strada test becomes `_promoted`. 11 of those are byte-identical to the 6.0.3 file.
- 2 are `renamed-promoted-by-basename` (`conformance/node/nodeModulesPackageImportsRootWildcard{,Node16}.ts`), renamed because Go-local `compiler/` tests have the same basenames.

`submoduleAccepted.txt` (1,539 lines, 1,169 entries) and `submoduleTriaged.txt` (2 entries: `compiler/augmentExportEquals2`, `exportAssignmentMembersVisibleInAugmentation`, both `.errors.txt`) are now **historical ledgers**. Each entry names an old `<suite>/<baseline>.diff`, a Corsa-versus-Strada difference that was accepted, with a category header. Examples of headers:

- "Error spans for non-BMP characters … measured by code points"
- "Error message for deprecated 6.0 options has changed"
- JS `declare` emit changes
- the `@enum` / `@class` removals

Accepted entries by kind: 448 `errors.txt` (240 compiler, 208 conformance), 387 `js`, 245 `types`, 50 `symbols`, 39 fourslash. 440 of the 448 `errors.txt` entries still correspond to an existing 7.1 baseline.

Nothing reads these files: there are no references in `tsc/internal/testrunner` or `testutil`, or in `Herebyfile.mjs`. Baselines are compared only against `reference/` (`baseline.go:23-80`).

### 1.4 Delta between the 6.0.3 corpus (`050880ce`) and 7.1, by blob id

Full lists are in `scratchpad/ts71/cases_{changed,removed,added}.txt`.

| Status | Total | compiler | conformance | transpile |
| --- | ---: | ---: | ---: | ---: |
| Identical | 12,076 | 6,235 | 5,819 | 22 |
| Changed in place | 252 | 198 | 54 | 0 |
| Removed | 139 | 104 | 35 | 0 |
| Added | 447 | 406 | 38 | 3 |

- **Added** covers Go-local tests, 11 `conformance/es2026/*` tests, the 15 `_promoted` renames and 3 transpile tests.
- **Changed and removed cases almost all involve 6.0-deprecated options.** Only 23 changed and 10 removed cases do not, and several of those have them in `tsconfig`, in AMD directives, or are promotion renames. Details are in §3.3.

---

## 2. Harness semantics: `tsc/internal/testrunner` and `testutil/harnessutil` (Q2)

**Enumeration.** Every `\.tsx?$` file under `testdata/tests/cases/{compiler,conformance}` is a test (`compiler_runner.go:66-76`, `harnessutil.go:990-1024`). A skip list by basename is applied first (§2.7).

**Settings.** `extractCompilerSettings` (`test_case_parser.go:281-289`) uses the regex `(?m)^//\s*@(\w+)\s*:\s*([^\r\n]*)` over the **whole file**, not just the header.

- Keys are lower-cased and the last occurrence wins.
- Values are trimmed and a trailing `;` is removed. `@declaration: true;` therefore becomes `true` in Go; in Strada the same text meant `false`.
- The file is read through the vfs, which decodes a UTF-8 BOM and UTF-16 (`instanceofOperator.ts` is UTF-16LE).

**Units.** `ParseTestFilesAndSymlinksWithOptions` (`test_case_parser.go:138-279`):

- The content is split on `\r?\n` and re-joined with `\n`, so CRLF becomes LF.
- Every `// @x: v` line is removed from unit text (`:170-197`), wherever it appears.
- `// @filename:` starts a new unit.
- Leading blank lines of a unit are dropped (`:255-258`).
- Non-comment content before the first `@filename` panics.
- `// @link: A -> B` makes `B` a symlink to `A` (`:291-298`, `harnessutil.go:207-211`).
- `// @symlink: p1,p2` inside a unit makes each `pN` a symlink to that unit (`:178-184`).
- `@currentDirectory` is honoured.
- These unit semantics are the same as Strada's `makeUnitsFromTest` (`harnessIO.603.ts:1260+`).

**tsconfig.** A `tsconfig.json` or `jsconfig.json` unit is parsed as config. It is removed from the units, and its `FileNames` become the roots (`test_case_parser.go:79-110`, `compiler_runner.go:291-305`).

**Roots without a tsconfig** (`compiler_runner.go:306-327`). If `@noImplicitReferences` is set, or the last unit contains `require(` or `reference path`, only the last unit is a root; the other units exist on disk only. Otherwise all units are roots. `baseUrl` is made absolute. The current directory defaults to `/.src`: `GetNormalizedAbsolutePath(@currentDirectory, "/.src")` (`:280`).

**Variations** (`harnessutil.go:1038-1226`):

- **VaryBy set.** Every non-command-line-only boolean or enum option with any `Affects*` flag, plus `noEmit` and `isolatedModules` (`compiler_runner.go:150-177`). That is 72 options, including `target`, `module`, `moduleResolution`, `jsx`, `strict*`, `alwaysStrict`, `esModuleInterop`, `downlevelIteration` and `deduplicatePackages`.
- **Value lists.** Values are comma lists. `*` means every enum key, or `true,false`. `-x` or `!x` excludes a value, and an unknown excluded value such as `-es3` is ignored (`:1134-1142`). Values are de-duplicated by normalized value, keeping the first spelling (so `es6` and `es2015` collapse). An empty result panics. More than 25 combinations is fatal.
- A dimension exists only when it has 2 or more values. `@lib: a,b` is a list option, not a variation.
- **Configuration name.** Sorted lower-case keys, `k=lower(v)` joined with `,` (`:1026-1036`). The baseline name is `stem(name)ext` (`compiler_runner.go:262-268`). A test with no varying options uses its plain basename.
- **Differences from Strada.** Strada used a fixed, ordered 77-name `varyBy` list, which tsc-rs reproduces at `upstream_suites/compiler.rs:11-89`. It matched names case-sensitively and included the removed options. Go builds the set from the option declarations and lower-cases names. No case in the corpus is affected by the case difference; the `;` difference affects 3 compiler tests.

**Recognized directives** (`SetOptionsFromTestConfig`, `harnessutil.go:291-316`):

- **Compiler options:** all 7.1 `OptionsDeclarations` (125 names), plus `allowNonTsExtensions`, `noErrorTruncation`, `suppressOutputPathCheck` and `noCheck`.
- **Harness options:** `useCaseSensitiveFileNames`, `baselineFile`, `includeBuiltFile` (a no-op), `fileName`, `libFiles`, `noImplicitReferences`, `currentDirectory`, `symlink`, `link`, `noTypesAndSymbols`, `fullEmitPaths`, `reportDiagnostics` and `captureSuggestions`.
- **Ignored:** `typescriptVersion`. In Strada it was passed to `createProgram` and changed deprecation handling.
- **Fatal:** any other name ("Unknown compiler option"), any enum value that is not recognized, and object-typed options such as `paths` given as a directive.
- **Option set, 6.0.3 compared with 7.1.** Present in 6.0.3 but gone in 7.1: `charset`, `importsNotUsedAsValues`, `keyofStringsOnly`, `noImplicitUseStrict`, `noStrictGenericChecks`, `out`, `preserveValueImports`, `suppressExcessPropertyErrors`, `suppressImplicitAnyIndexErrors`. New in 7.1: `checkers`, `deduplicatePackages`, `pprofDir`, `quiet`, `runExternalCode`, `singleThreaded`.
- **Enum maps** (`enummaps.go:153-195`):
  - `target`: `es5`…`es2026`, `esnext`. `es3` is gone and `es2026` is new.
  - `module`: `commonjs`, `amd`, `system`, `umd`, `es6`/`es2015`, `es2020`, `es2022`, `esnext`, `node16`, `node18`, `node20`, `nodenext`, `preserve`. `none` is gone.
  - `moduleResolution`: `node16`, `nodenext`, `bundler`, `classic`, `node`, `node10`.
  - The deprecated keys (`es5`; `none`/`amd`/`system`/`umd`; `node`/`classic`/`node10`) are hidden from TS6046 value lists (`commandlineoption.go:197-201`).

**Harness defaults** (`harnessutil.go:97-105`). These are identical to TS 6.0.3 `harnessIO.compileFiles` (`harnessIO.603.ts:345-350`), and neither injects `ignoreDeprecations`.

- `newLine` is CRLF if unset.
- `skipDefaultLibCheck` is true if unset.
- `noErrorTruncation` is true, but can be overridden by a directive.
- `useCaseSensitiveFileNames` is true.
- `outDir`, `project`, `rootDir`, `tsBuildInfoFile`, `baseUrl`, `declarationDir`, `rootDirs` and `typeRoots` are made absolute (`:163-186`).

**Language defaults in 7.1** (`core/compileroptions.go:199-303`):

- Default target is `ES2026` ("LatestStandard", `:531`). In 6.0.3 it is `ES2025` (`typescript.js:21979-22000`).
- `module` is derived from `target`.
- `moduleResolution`: `classic` and `node10` are treated as unset, then resolve to `bundler`, `node16` or `nodenext`.
- `strict` defaults to true, and `esModuleInterop`, `allowSyntheticDefaultImports` and `alwaysStrict` are effectively always true (the fields are marked `deprecated:"true"`, `:125-136`).
- Union ordering is always the stable order (Corsa `compareTypes`). This is TS 6.0's opt-in `stableTypeOrdering: true`.
- In practice the default target rarely matters: in the unchanged corpus, every conformance case and all but 12 compiler cases set `@target` explicitly.

**Libraries.**

- The host serves the 113 embedded libs at `bundled:///libs/` (`harnessutil.go:217-218,253`).
- The default lib comes from `targetToLibMap`: `esnext` → `lib.esnext.full.d.ts`, …, `ES2015` → `lib.es6.d.ts`, anything else → `lib.d.ts` (`enummaps.go:216-242`). `@lib`, `/// <reference lib>` and `@noLib` behave as usual.
- `@libFiles` adds `/.lib/<f>`, except `lib.d.ts` unless `noLib` is set (`:146-156`). `/.lib` is mounted when any input contains `/.lib/`. 163 cases reference `/.lib/react*.d.ts`, 105 of them in conformance.
- **Compared with the 6.0.3 libs vendored in tsc-rs:** 96 identical, 8 different, 9 new in 7.1, 4 gone.
  - Different: `es5`, `es2015.core`, `es2015.collection`, `es2015.symbol`, `es2017.string`, `es2018.intl`, `es2020.intl`, `esnext`. The ones inspected differ mostly in JSDoc, but `es2017.string` renames the parameters of `padStart`/`padEnd` (`targetLength`, `padString`), and `lib.esnext.d.ts` now references `es2026`.
  - New in 7.1: `lib.es2026{,.full,.array,.collection,.error,.iterator,.json,.math,.typedarrays}.d.ts`.
  - Gone: `lib.esnext.{array,collection,error,typedarrays}.d.ts`. Those names still resolve through `LibMap` aliases to the `es2026.*` files (`enummaps.go:108-122`).

**Diagnostic collection** (`compileFilesWithHost`, `harnessutil.go:626-719`). A pre-emit program (with `traceResolution` off) and a post-emit program are both built. Each collects:

1. config-file parsing diagnostics
2. program (options) diagnostics
3. syntactic diagnostics
4. semantic diagnostics
5. global diagnostics
6. declaration diagnostics, only if declaration emit is enabled
7. suggestions, only with `@captureSuggestions`

Each set is sorted and de-duplicated with `compiler.SortAndDeduplicateDiagnostics` (`program.go:1651-1690`, which merges related info). If the pre- and post-emit counts differ, a synthetic code −1 diagnostic is added and the test fails, so reference baselines never contain it.

### 2.7 Skipping

1. **By basename:** `skippedTests` (`compiler_runner.go:78-124`) has 42 entries: 10 `APISample*`/`APILibCheck` plus 32 tests using removed options (`preserveValueImports*`, `importsNotUsedAsValues_error`, `noImplicitUseStrict_*`, `keyofStringsOnly` users, `moduleNone*`, `requireOfJsonFileWithModule*EmitNone`, …).
2. **Per configuration, after compiling** (`harnessutil.go:1236-1273`). The effective program options include tsconfig options.
   - `t.Skip` when: `module` is UMD or System; `moduleResolution` is node10 or classic; `esModuleInterop=false`; `allowSyntheticDefaultImports=false`; `baseUrl` is set; `target` is ES5; `alwaysStrict=false`.
   - `t.Fatal` when `module=AMD` or `outFile` is set, so no 7.1 test may still contain them.
3. `skippedEmitTests` (8 names) skips only the `.js` subtest (`compiler_runner.go:429-438`).
4. The whole run is skipped if the libs are not embedded.

Skipped configurations produce **no baseline files at all**. Hereby's unused-baseline tracking (`TSGO_BASELINE_TRACKING_DIR`, `testutil/baseline/testmain.go:25`; `Herebyfile.mjs:858-897`) keeps `reference/` exactly equal to what the tests produce.

---

## 3. Options deprecated in 6.0 or removed in 7.x (Q3)

**The 7.1 compiler** (`program.go:959-1032`, "Removed in TS7") unconditionally reports:

- TS5102 "Option '{0}' has been removed. Please remove it from your configuration." for `baseUrl` (chained with TS5106 "Use '\"paths\": {\"*\": [...]}' instead." when a tsconfig exists), `outFile` and `downlevelIteration`.
- TS5108 "Option '{0}={1}' has been removed…" for `target=ES5`, `module=AMD`/`System`/`UMD`, `moduleResolution=Classic`/`node10`, `alwaysStrict=false`, `esModuleInterop=false` and `allowSyntheticDefaultImports=false`.

The options then have no semantic effect. `ignoreDeprecations` is still declared (`declscompiler.go:1205`) but is never read or validated. Values that can no longer be parsed (`target: es3`, `module: none`) give TS6046 in a tsconfig and are fatal as directives. The removed 5.x options give TS5023 "Unknown compiler option" in a tsconfig.

**TS 6.0.3** (`typescript.js:129936-130124`):

- `checkDeprecations("6.0","7.0")` reports TS5107 (with a value) or TS5101 (without) for exactly the same list, plus `module=None`, unless `"ignoreDeprecations": "6.0"` is set.
- `checkDeprecations("5.0","5.5")` reports TS5102 or TS5108 ("removed") for `target=ES3`, `charset`, `out`, `keyofStringsOnly`, etc.
- The 6.0.3 harness does not silence these. It very likely printed TS5101/5107 in 6.0.3 `errors.txt` files (inferred; the 6.0.3 baselines were not inspected).
- tsc-rs already emits TS5101/5107 in its Program config path (`crates/program/src/config.rs:1167-1168,3357+`).

**What the 7.1 harness does:** it skips ES5, UMD, System, classic, node10, baseUrl, `esModuleInterop=false`, `allowSyntheticDefaultImports=false` and `alwaysStrict=false` configurations. It is fatal on AMD and `outFile`, so all such tests were rewritten or removed. `downlevelIteration` is **not** skipped: those configurations run, and their baselines gain TS5102.

**Corpus treatment.** 6.0.3 cases, by status in 7.1:

| 6.0.3 option | same | changed | removed |
| --- | ---: | ---: | ---: |
| `target=es5` | 1,104 (mostly `es5, es2015` pairs; only the es2015 config runs) | 17 | 37 |
| `module=amd` | 0 | 183 | 81 |
| `outFile` | 0 | 50 | 62 |
| `module=system` | 109 | 19 | 5 |
| `module=umd` | 26 | 6 | 6 |
| `moduleResolution=classic` / `node10` | 52 / 4 | 3 / 0 | 0 |
| `alwaysStrict=false` | 52 | 0 | 0 |
| `esModuleInterop=false` | 6 | 6 | 0 |
| `allowSyntheticDefaultImports=false` | 3 | 0 | 0 |
| `baseUrl` | 4 | 1 | 2 |
| `downlevelIteration` | 24 | 0 | 0 |
| `@ignoreDeprecations: 6.0` | 54 | 0 | 0 |

The emulated run over the unchanged cases gives 14,330 configurations:

- 1,630 are skipped by option: es5 1,233; UMD/System 229; classic/node10 80; `alwaysStrict=false` 52; `baseUrl` 30; `esModuleInterop=false` 6.
- 216 cases are skipped entirely: UMD/System 113; classic/node10 69; `baseUrl` 29; `esModuleInterop=false` 4; es5 1.
- 40 cases are in the skip list.

The rewrite pattern was checked on fetched blobs:

- `@module: amd` → `@module: commonjs` (for example `ambientExternalModuleWithInternalImportDeclaration.ts`, `commentsExternalModules.ts`).
- `@outFile: foo.js` → `@outDir: out` (`blockScopedClassDeclarationAcrossFiles.ts`).
- `@module: *` → `*, -amd` (`emitHelpersWithLocalCollisions.ts`, whose baselines include `(module=es6)`, `(module=node20)`, etc., but no system or umd).
- `amd` dropped from lists (`importHelpersWithExportStarAs.ts`).
- `"module": "amd"` → `"commonjs"` inside a tsconfig (`deprecatedCompilerOptions2.ts`).

**Concrete examples.** All files use CRLF (`^M$` in `cat -ve`).

- **`downlevelIteration` still runs.** `compiler/blockScopedBindingsInDownlevelGenerator.ts` has `@target: es5, es2015` and `@downlevelIteration: true`. The only baseline is `…(target=es2015).errors.txt`:
  ```
  error TS5102: Option 'downlevelIteration' has been removed. Please remove it from your configuration.


  !!! error TS5102: Option 'downlevelIteration' has been removed. Please remove it from your configuration.
  ==== blockScopedBindingsInDownlevelGenerator.ts (0 errors) ====
  ```
  `conformance/destructuringArrayBindingPatternAndAssignment1ES5iterable.errors.txt` has the same TS5102 line first, followed by the ordinary TS2493 errors.
- **`ignoreDeprecations` is silent.** `compiler/ambientWithStatements.ts` has `@ignoreDeprecations: 6.0` and `@alwaysStrict: true, false`. Only `ambientWithStatements(alwaysstrict=true).*` exists, and it has no deprecation diagnostics.
- **es5 pairs.** `conformance/ES5SymbolProperty1.ts` has `//@target: ES5, ES2015` and only `ES5SymbolProperty1(target=es2015).{js,symbols,types}`. `conformance/asyncArrowFunction11_es5.ts` has only `(target=es2015)`.
- **Rewritten test.** `compiler/deprecatedCompilerOptions2.errors.txt` (tsconfig with `"target": "ES3"` and removed options):
  ```
  /foo/tsconfig.json(4,19): error TS6046: Argument for '--target' option must be: 'es6', 'es2015', …, 'es2026', 'esnext'.
  /foo/tsconfig.json(5,9): error TS5023: Unknown compiler option 'noImplicitUseStrict'.
  ```
  Note that `es5` is hidden from the TS6046 list.
- **Removed tests:** `amdDependencyComment2.ts`, `outModuleConcatAmd.ts`, `importCallExpressionInAMD1-4.ts`, `outFileIsDeprecated.ts`, `systemModule12.ts`, and others. No 7.1 baseline should contain TS5108, because every TS5108 trigger is either skipped or fatal. This is inferred from the harness rules; the baseline contents were not searched.

---

## 4. The `.errors.txt` format (Q4): `tsbaseline/error_baseline.go`

Written only when there is at least one diagnostic. Otherwise the in-memory `<no content>` means the file is absent (`error_baseline.go:35-49`, `baseline.go:21,62-70`). The comparison is byte-exact against `reference/` (`baseline.go:62`).

**Structure.** The newline is always `\r\n`. There is no trailing newline at end of file. The first emitted line has no leading newline.

1. **Summary.** `WriteFormatDiagnostics` over all sorted diagnostics (`diagnosticwriter.go:565-582`), one entry per diagnostic:
   - Format: `[<file>(<line>,<col>): ]<category> TS<code>: <flattened message>\r\n`
   - `line` is 1-based. `col` is the **UTF-16** column + 1 (`GetECMALineAndUTF16CharacterOfPosition`).
   - Message chains are flattened with `\r\n` plus two spaces per level (`:348-376`).
   - File paths are passed through `removeTestPathPrefixes`, which strips `/.src/`, `/.lib/`, `/.ts/` and `bundled:///libs/` (`util.go:23-49`). Other absolute paths, such as `/foo/a.ts`, stay absolute.
   - Lib locations become `lib.x.d.ts(--,--)`.
   - The block ends with two further `\r\n` (`error_baseline.go:143-147`).
2. **Global diagnostics** (no file). For each non-empty line of the flattened message: `!!! <category> TS<code>: <line>`. Then, for each related diagnostic: `!!! related TS<code>[ <file>:<line>:<col>]: <flattened>`, where lib locations become `lib.x.d.ts:--:--` (`:100-128`).
3. **One block per input file**, in the order: tsconfig unit, roots, then other units (all units, including those with 0 errors):
   - Header: `==== <unitName> (<N> errors) ====`. N counts every category.
   - Each source line is printed as `    <line>`, split on `\r?\n`, with a trailing `\r` removed.
   - For each diagnostic that starts or continues on a line, a squiggle line is printed: `    ` + `line[:start]` with every non-whitespace **rune** replaced by a space, then `~` × (number of **runes** in `[start, min(start+len, EOL))`) (`:202-219`).
   - On the diagnostic's last line (or at end of file), its `!!!` lines follow.
   - The diagnostic count is asserted.

**Ordering.** `ast.CompareDiagnostics` (`ast/diagnostic.go:482+`) orders by:

1. file name
2. pos
3. end
4. code
5. category
6. source
7. message **key** (not the rendered text)
8. message args
9. chain size, then chain content
10. related information

6.0.3 `compareDiagnostics` (`typescript.js:21817`) orders by file, start, length, code, **rendered text**, chain, related.

**Differences from the 6.0.3 Strada format** (`harnessIO.603.ts:484-640`). The layout is the same. The differences are:

- Squiggle start and width use runes in 7.1, UTF-16 units in 6.0.3 (`:619`). The two differ only on astral characters.
- Tie ordering (above).
- Corsa message and span differences (the ledger in §1.3).
- Union order (stable in 7.1).

**What an exact Rust renderer must reproduce:**

- the official unit texts, which are echoed verbatim
- the root and unit set and order
- the full diagnostic set: config, options, global, declaration diagnostics, and suggestions only with `captureSuggestions`
- Go ordering
- CRLF newlines, path-prefix stripping (also inside messages) and lib masking
- UTF-16 columns in the summary and rune-based squiggles
- a different "pretty" layout for the 14 `@pretty: true` cases

**Other baselines per configuration.** All are exact-compare, and absent when empty (the "Absent when" column):

| Kind | Absent when |
| --- | --- |
| `.js` (JS, `.d.ts` and `DtsFileErrors`) | the test has only `.d.ts` inputs |
| `.types` / `.symbols` | `@noTypesAndSymbols` |
| `.js.map` + `.sourcemap.txt` | no source maps |
| `.trace.json` | `traceResolution` is off, or the trace is empty |
| `.contentmapper.txt` | not a content-mapper test (Go-only) |

Two further checks produce no baseline: union ordering and parent pointers.

---

## 5. Gap analysis and a proposed runner (Q5)

### 5.1 How the current tsc-rs runner works

**Corpus scope.** Only `ts-tests/tests/cases/conformance` (`crates/conformance/src/lib.rs:3231-3233`): 5,908 fixtures and 7,691 cases. `.js` and `.jsx` count as fixtures (`:3271`). The compiler suite is covered elsewhere, by the H1/H2 lanes.

**Expansion.** A custom adapter, not the official harness: `crates/harness/src/lib.rs:669-1040`.

- **Directives.** Only a header block is read (`parse_fixture`, `:745`). Directives after the first `@filename` stay in the file text (`split_fixture_files`, `:795`). Lines keep their original CRLF/LF endings.
- **Matrix.** A matrix is built only for `target`/`module`/`moduleResolution`/`jsx` and boolean lists, and enum aliases are not de-duplicated.
- **Roots and cwd.** `noImplicitReferences` is ignored (`:1340`), all files are roots, and the cwd is `/`, not `/.src`.
- **Libs.** Libs are resolved by `resolve_program_libs` (`:1016`) from 6.0.3 data.
- The harness already contains a **faithful** Strada expander and loader for the compiler suite: `upstream_suites/compiler.rs` (varyBy `:11-89`, units `:269`, configurations `:386`), and `execution.rs` (`load_compiler_no_emit` `:61`, harness defaults `:572-573`, root rule `:2494`, `/.src`, symlinks).

**Expected results.** Schema-3 JSON goldens in `goldens/conformance/**.json.zst`, produced by `crates/oracle/driver.mjs` with the vendored 6.0.3 `typescript.js`.

- `program-host.mjs:43-68` adds `noLib: true` and passes the lib list as roots. As a result `/// <reference lib>` is **not** honoured. Example: the `libReferenceDeclarationEmit.ts` golden has TS2304 `Cannot find name 'HTMLElement'`, while both official harnesses report no error.
- `driver.mjs:10-31` collects only per-root-file syntactic, semantic and **suggestion** diagnostics: no options, global, config, declaration or lib-file diagnostics. No deprecation code (5101/5102/5107/5108) appears in any of the 5,908 goldens.
- There is no `noErrorTruncation`, `skipDefaultLibCheck` or `newLine` default.

**Comparison** (`ratchet.rs:108-128,736+`; `lib.rs:1159-1210`):

- **T0:** a key set of (file, code, line, UTF-16 col).
- **Multiplicity:** equal counts at each key.
- **T1:** plus category.
- **T2:** plus start, length and top text.
- **T3:** plus full chain and related info.
- **T4:** a byte-exact hash of the 6.0.3 pretty CLI render.
- The views are `all`, `2xxx` and `syntactic`.
- The set is enforced append-only by `ratchets/oracle-inputs.v1.json.zst` and `conformance-matches.v1.json.zst` (with lineage anchors).
- `ratchet.toml` records T0–T3 49,024/49,024 and T4 7,691/7,691.

**Execution.**

- The default path is the in-memory checker, `check_program_with{,_prepared_harness}_libs_at` (`lib.rs:3031-3046`).
- **51** hard-coded fixtures (`h0_memory.rs:20-72`) instead go through `MemoryCompilerHost` + `ModuleResolver` + `PreparedProgram` (`h0_memory::run`, `:117+`).
- All positions are UTF-16 (`lib.rs:3091-3151`).

### 5.2 Gaps between this runner and the 7.1 harness

| Gap | Size |
| --- | --- |
| **G1. Unit text.** Compared on all 5,908 conformance fixtures (an emulation of the Rust adapter). | Only 1,435 have identical unit texts. The differences: CRLF kept in 3,953; **mid-file `// @x:` lines kept in 194**; leading blank lines kept in 188; trailing-newline differences; unit names differ in 38 (a comment-only prefix becomes an extra default-named file). The mid-file-directive, leading-blank-line and unit-name groups shift line numbers against 7.1 baselines. 2 fixtures (`es2019/importMeta/importMeta.ts`, `destructuringArrayBindingPatternAndAssignment4.ts`) even expand to a different matrix. |
| **G2. Programs.** | cwd `/` instead of `/.src` (paths inside messages differ); root selection; tsconfig handling; `/.lib` react libs not vendored; lib-reference handling (noLib projection); defaults such as `noErrorTruncation`, which changes message text. |
| **G3. Diagnostic scope.** | Missing: options, global, config, declaration and lib diagnostics. Extra: suggestions, which official baselines contain only with `captureSuggestions`. |
| **G4. 7.1 semantics tsc-rs lacks.** | ES2026 target and libs: `ScriptTarget` stops at `ES2025` (`crates/types/src/flags.rs:2165-2173`) and the default target is ES2025 (`options.rs:436-442`). Stable union ordering: tsc-rs parses `stableTypeOrdering` but its emitter refuses `true` (`crates/emitter/src/execute.rs:~166`), and the checker comments describe only the off path. The intentional Corsa deltas from §1.3 and `tsc/CHANGES.md` (JS constructor functions, `@enum`/`@class`, expandos, UTF-8 positions with rune squiggles, message wording). |
| **G5. Case identity.** | The 7.1 baseline store is flat per suite. 252 cases were changed in place, 139 removed, 447 added, and 13+2 renamed. |

### 5.3 (a) Reading 7.1 cases and 7.1 `.errors.txt` files as expected results

1. **Vendor with pins.** Pin the commit and tree ids, as in `vendor/typescript-6.0.3/test-suites-pin.*`. Vendor:
   - `tsc/testdata/tests/cases/{compiler,conformance}` (12,750 files)
   - `tests/lib` (4 files)
   - `baselines/reference/{compiler,conformance}/*.errors.txt` (7,320 files)
   - a manifest of *all* baseline file names, used to tell "ran with no errors" apart from "skipped"
   - optionally, the 113 7.1 libs
2. **Port the Go expansion.**
   - Reuse `upstream_suites::compiler::make_units_from_test` for units; its semantics are the same as Go's. Add BOM/UTF-16 decoding and the `;` trim.
   - Implement `GetFileBasedTestConfigurations` with the 7.1 varyBy set and enum maps, and generate the exact baseline names.
   - Implement `skippedTests` plus `SkipUnsupported`/`failOn` over the effective options (tsconfig options overlaid by directives).
   - A Python prototype of this reproduced the baseline configuration set for 12,054 of 12,054 unchanged cases. The only exceptions were explained no-output configurations: `noTypesAndSymbols` + `noEmit` with no errors (29), and one UTF-16 file the prototype did not decode.
3. **Build programs the official way for both suites.** Generalize `load_compiler_no_emit`: `/.src` cwd, roots rule, tsconfig, links and symlinks, `/.lib`, harness defaults. Collect config + options + syntactic + semantic + global + declaration diagnostics, and suggestions only with `captureSuggestions`. Sort with a port of Go `CompareDiagnostics` if rendering.
4. **Compare.** Parse each `.errors.txt` into structured records:
   - From the summary: file, line, UTF-16 col, category, code and flattened text.
   - From the file blocks: chain lines, related entries (lib locations masked), and span lengths from the squiggles, converting runes to UTF-16 using the echoed line.
   - Grade them with the existing T0–T3 comparators.
   - As a top tier, render tsc-rs output into the exact Go text (about 300 lines to port) and compare bytes.
   - Record "config ran, no baseline" as an empty expected set.
5. **Keep the 7.1 lane on its own ratchet family**, for example `ratchets/ts71/…` plus new `ratchet.toml` sections. Do not rewrite the 6.0.3 goldens or their lineage.
6. **Add a divergence registry seeded from `submoduleAccepted.txt`.** Map `suite/<name>(<cfg>).errors.txt.diff` to a case and configuration. 203 of the 6,478 lane-A tsc-rs conformance cases (§5.4) already carry an accepted Corsa-only difference. Also decide per category whether tsc-rs adopts Corsa behaviour or keeps 6.0.3 behaviour.

### 5.4 (b) Deprecated-option cases: 6.0.3 oracle instead of the 7.1 baseline

**Mechanical predicate.** Evaluate it per configuration after variation expansion. Use the effective options: tsconfig `compilerOptions` overlaid by the directive values of that configuration.

```
lane_B(cfg) := target∈{es3,es5} ∨ module∈{none,amd,umd,system} ∨ moduleResolution∈{node,node10,classic}
  ∨ outFile≠"" ∨ out≠"" ∨ baseUrl set ∨ esModuleInterop=false ∨ allowSyntheticDefaultImports=false
  ∨ alwaysStrict=false ∨ downlevelIteration set
  ∨ any of {charset, importsNotUsedAsValues, keyofStringsOnly, noImplicitUseStrict,
            noStrictGenericChecks, preserveValueImports, suppressExcessPropertyErrors,
            suppressImplicitAnyIndexErrors} set
```

This is the union of 6.0.3 `checkDeprecations("6.0","7.0")` (`typescript.js:130044-130124`), `("5.0","5.5")` (`:130002`), and the values 7.1 no longer parses. It is the superset of Go's skip, fatal and skip-list rules plus `downlevelIteration`.

**Cross-checks from 7.1 data:**

- A lane-B configuration must have no 7.1 baseline, except `downlevelIteration` configurations, whose `errors.txt` contains TS5102.
- Any TS5102, TS5108, TS6046 or TS5023 line in a 7.1 `errors.txt` flags a case to review.
- Content alone cannot detect the skipped configurations, because they are simply absent.

**Content identity.** A changed case such as `@module: amd` → `commonjs` is two different cases. The 6.0.3 blob stays in lane B with its existing golden. The 7.1 blob is a new lane-A case. Removed cases stay lane B. Added 7.1 cases are lane A only.

**Numbers for the current 7,691 tsc-rs conformance cases:**

| Route | Cases |
| --- | ---: |
| Lane A, a 7.1 configuration exists | **6,478** (3,808 with an `errors.txt`, 2,670 with none) |
| Lane B | **1,211**: es5 833; UMD/System 123; changed in 7.1 106; classic/node10 51; removed 46; `alwaysStrict=false` 36; skip list 12; `esModuleInterop=false` 2; `baseUrl` 2 |
| Unmapped (G1 matrix drift) | 2 |

Error presence already disagrees on 266 of the 6,478 lane-A cases:

- 174 have a golden with 0 diagnostics but a 7.1 `errors.txt`. 12 of these are `downlevelIteration` TS5102 cases; the others come from JS/JSDoc Corsa changes and the missing program-level diagnostics.
- 84 have a golden with diagnostics but no 7.1 `errors.txt`. These come from the noLib projection, Corsa changes, and similar causes.

**Policy points to decide:**

- In 6.0.3, lane-B configurations emit **TS5101/TS5107 deprecation errors** unless `ignoreDeprecations: "6.0"` is set, and tsc-rs's Program path reproduces this. Choose one:
  - keep them (6.0.3-faithful)
  - run lane B with `ignoreDeprecations: "6.0"` injected
  - filter codes 5101/5107 on both sides
- For `downlevelIteration`, prefer lane B; the alternative is lane A with TS5102 filtered.
- The current lane-B goldens use the custom projection (G2/G3). Keep them as is at first, which needs no regeneration. For a single artifact type, later produce lane-B expected output with a Node port of `harnessIO.compileFiles` + `getErrorBaseline` over the vendored 6.0.3 `typescript.js`, in a new artifact family.

---

## 6. Other things a design must account for (Q6)

- **Promotion renames.** `promotedTestCollisions.txt`, §1.3. For 4 of them (`jsxFunctionTypeChildren`, `missingDiscriminants{,2}`, `subtypeReductionWithAnyFunctionType`), neither the original nor the `_promoted` blob equals the 6.0.3 file.
- **The imported TS tests do not match 6.0.3 exactly.** 23 changed cases have no deprecated option at all (for example `unicodeEscapesInNames01.ts` gained 2 extra files). Identify cases by blob, not by path.
- **Target and libs.** The default target is ES2026, with new `es2026` libs and target; 11 `es2026` conformance tests; esnext libs reorganized; 8 lib files changed. Nearly every case sets `@target` explicitly.
- **Stable union order.** Corsa always uses stable union order, which is 6.0's `stableTypeOrdering: true`. Expect many message-text diffs until tsc-rs implements that mode.
- **Positions.** Corsa node positions are UTF-8 offsets (`CHANGES.md` Scanner §1), baseline columns are UTF-16, and squiggles count runes.
- **No 5101/5107 in 7.1.** 7.1 never emits 5101/5107. `ignoreDeprecations` is accepted and ignored, and `typescriptVersion` is ignored.
- **Scale of intentional differences.** `CHANGES.md` and `submoduleAccepted.txt` are the authoritative lists of intentional differences, mostly in JS/JSDoc/Salsa areas and declaration emit. Both are historical snapshots against the submodule's Strada commit, not against 6.0.3.
- **Suites with no 7.1 case files.** `project`/`projects` (the tsc-rs 6.0.3 project runner), and fourslash (now Go code with baselines in `reference/fourslash/<cmd>/`). Transpile stays separate: 25 cases, `transpile_runner.go` varies only `declarationMap`/`sourceMap`/`inlineSourceMap` and does not apply the skip rules.
- **Two programs per case.** Every case builds a pre-emit and a post-emit program, and the reference baselines are exact and complete (unused-baseline tracking). The absence of a baseline is therefore meaningful.

## 7. Not verified

- The content of TS 6.0.3 reference baselines, which are not vendored. The claim that they contain TS5101/5107 is inferred from the harness and compiler code.
- Whether 7.1 `tests/lib/react*.d.ts` equals TS 6.0.3 `tests/lib`.
- The added and changed 7.1 cases were not read, apart from 8. Their configurations are inferred from baseline names only.
- The tsc-rs unit-splitting comparison is a Python emulation of `crates/harness/src/lib.rs`, not a tsc-rs run.
- The absence of an on-path for `stableTypeOrdering` in the tsc-rs checker is inferred from grep, and `crates/checker` has uncommitted edits in that worktree.
- 6.0.3's handling of `ignoreDeprecations: "5.0"`.

## 8. Scratch artifacts (`scratchpad/ts71/`)

| Files | Contents |
| --- | --- |
| `tree_tests.txt`, `tree_baselines.txt` | 7.1 tree listings |
| `cases603.tsv`, `cases71.tsv`, `cases_{changed,removed,added}.txt` | Blob comparison |
| `classify603.json` | Deprecated-option classification |
| `bindex.json` | 7.1 baseline index |
| `percase_same.json` | Emulated routing |
| `golden_keys.json`, `golden_cat.json` | tsc-rs golden case keys and counts |
| `emulate.py`, `emulate2.py`, `mapkeys.py`, `agree.py`, `unitsdiff.py`, `h0route.py` | Reproducible scripts |
| `fetched/` | Fetched blobs |
