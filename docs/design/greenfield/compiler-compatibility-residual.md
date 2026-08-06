# TypeScript 6.0.3 compiler compatibility residual

Status: audited design input, 2026-08-06, with the L0.0 evidence freeze
complete. This page records the current Rust implementation boundary, the work
required to finish bounded H1 JavaScript emit, and the remaining work after H1
for broader TypeScript 6.0.3 compiler and tooling compatibility. It is not an
implementation-complete claim and it does not authorize a broader H1 profile.

The persistent-source audit found one dependency-order correction: the L0
ownership/identity foundation and L1 incremental-parser proof are required
before H1 runtime implementation, although incremental behavior remains
outside H1's product claim. Full old-Program/resolution reuse, Language
Service, tsserver, and LSP remain later products.

Source authority: the current Rust workspace; vendored `_tsc.js` for the
command-line compiler, Program, builder, solution-builder, and watch
implementation; vendored `typescript.js` for the public compiler API,
Language Service, Project Service, and server implementation; and vendored
`typescript.d.ts` for their public observable shape. Upstream runner sources
and test inputs are pinned to source commit
`050880ce59e30b356b686bd3144efe24f875ebc8`. Vendored line numbers below are
navigation anchors. Each implementation track must replace planning anchors
with exact declaration spans and body hashes before porting them.

The checked-in vendor directory is not a complete TypeScript source or npm
package checkout. In particular, a local bundle being present does not pin
its original source declarations, runner code, reference baselines, locale
catalogs, or executable package layout. A track that consumes one of those
surfaces must add it through a reviewed, content-addressed pin transition;
fetching an unpinned current upstream file during qualification is forbidden.

The active execution contract remains [H1 emit](h1-emit.md). The existing
[definition of done](definition-of-done.md) remains normative for the
batch-diagnostics project and currently has only M9 steady-state evidence
pending. “Complete compiler” on this page means a broader follow-on target;
it must not be used to rewrite that historical denominator or to claim that
H1 includes build, watch, LSP, or public API compatibility.

## Audit closure record

The final review crossed the residual against these independent roots:

- `_tsc.js` `executeCommandLine`/`executeCommandLineWorker`,
  `performCompilation`, `performBuild`, Program emit, builder, solution
  builder, watch, and host construction;
- `typescript.d.ts` `Program`, `CompilerHost`, both builder-program variants,
  watch/solution hosts, `System`, `LanguageServiceHost`, `LanguageService`,
  `DocumentRegistry`, and server protocol/project/session declarations;
- `typescript.js` Language Service, document registry, module-specifier and
  package-json caches, Project Service, and server Session owners;
- the local option declarations, H0 prepared-program/config projections,
  M8 scope state, H0 qualification records, and upstream-suite expansion; and
- the pinned upstream compiler, projects, transpile, and FourSlash runner
  entry points and their reference-baseline roles.

That review fixes the current evidence boundary as follows:

| Evidence | Current fact | What it does not prove |
| --- | --- | --- |
| M8 diagnostic scope | `m8-scope.json` has **0 live exclusions** and 600 resolved tombstones | Emit/declaration/build/API paths that were not part of the frozen batch entry |
| H0 host closure | 241/241 historical host-resolution rows are closed | Emitted outputs, project references, watch, or public host callbacks |
| Compiler input expansion | 7,276/7,276 plans structurally load and run through H0 | Upstream diagnostic, trace, JS/d.ts/map, or type/symbol baselines |
| Project expansion | 82/632 H0-compatible plans are qualified; the other 550 are explicitly classified H0 non-scope | The upstream project emit/build baselines |
| Expansion manifest | All 7,908 compiler/project cases retain initial state `not-run` | Any upstream runner pass rate |

There is therefore no known conformance-diagnostic exclusion backlog to
silently fold into H1. New execution paths can still change which diagnostics
run, their order, suppression, rendering, write interaction, or exit status.
Each follow-on track must qualify those observations on its own runner instead
of reopening the historical M8 denominator or treating the H0 structural
7,276-plan sweep as baseline parity.

The public surface also needs a generated converse inventory. Function names
alone are not a completeness key: every admitted public signature needs its
`typescript.d.ts` signature identity, its implementation declaration identity
in `_tsc.js` or `typescript.js`, a Rust disposition, and tests for overloads,
optional callbacks, mutation/identity, cancellation, and error behavior.

## 1. Finish lines must not be conflated

There is no single honest “100%” number spanning these surfaces:

| Finish line | Current state | What remains |
| --- | --- | --- |
| Frozen batch-diagnostics implementation | M0-M8 complete | M9.1c-M9.7 confidence production, burn-in, freeze, and qualification |
| Frozen filesystem `--noEmit` compiler | H0 complete | Preserve behavior and cost; do not route it through emitter setup |
| Bounded one-shot JavaScript emit | H1 design started | Every H1 blocking package in section 4 |
| Broad one-shot `tsc` compilation | Not designed as one approved milestone | Full JS transform matrix, declarations, maps, output/config/CLI matrix, and complete emit suites |
| Build/watch/project references | Preliminary seams only | Builder state, `.tsbuildinfo`, graph reuse, solution orchestration, watchers, and their suites |
| Compiler API/custom transforms | Not exposed | Stable AST/factory/printer/Program/TypeChecker contracts and callback lifetimes |
| Persistent source + incremental parser | L0.3 owned bind state complete; L0.4 next | L0 one-shot/registry reuse proof and L1 fresh-equivalent, performance-qualified update parsing |
| Language Service | Audited engine prerequisites only | Full document registry/program and resolution reuse, query/cache APIs, cancellation, and FourSlash qualification |
| tsserver | Not implemented | Session protocol, Project Service, open-file overlays, watches, plugins, type acquisition, and server suites |
| LSP adapter | Not implemented and not an upstream tsc surface | Explicit protocol mapping, synchronization, capabilities, concurrency, and LSP tests |
| TypeScript versions after 6.0.3 | Unsupported | A separate re-vendor, codegen, ledger, oracle, and compatibility transition per version |

H1 closes only the third row. The persistent-source/incremental-parser row is
an implementation prerequisite but remains a separate compatibility claim;
the broader rows reuse the resulting L0/L1 and H1 seams and require their own
profile, oracle, performance policy, and definition of done.

## 2. Audited Rust entry state

### 2.1 Reusable implementation

The following work is real input to an emitter and should not be rebuilt:

- `crates/syntax` has a broad generated AST arena, parent links, source text,
  byte-domain node and array ranges, exact literal values/raw template text,
  numeric literal flags, scanner-collected comment directives, reference
  directives, external module indication, and a per-file UTF-16 line map.
- `crates/binder` and `crates/checker` already compute the complete frozen
  diagnostics surface and many semantic facts later needed by emit, including
  enum-member evaluation and a substantial `NodeCheckFlags` word.
- `PreparedSourceFile` already retains source text/path identity,
  `may_be_emitted`, raw implied node format, effective implied node format for
  emit, redirects, aliases, and package scope.
- `ConfigOptionBag` and `COMPILER_OPTION_DECLARATIONS` recognize and convert
  the complete TypeScript 6.0.3 compiler-option name catalog in tsc order.
- `CompilerOptions` already carries the checker-visible subset of target,
  module, module detection, JSX, class-field, decorator, helper, module
  resolution, interop, const-enum, isolated-module, verbatim-module, and
  relative-extension rewrite choices.
- H0 already owns deterministic source discovery, libraries, package/module
  resolution, config diagnostics, output-independent CLI diagnostics, and
  exact source eligibility facts.
- `ConfigRootPlan` already retains the primary raw `references`, normalized
  project-reference entries, wildcard watcher roots, and effective
  `watchOptions`, `typeAcquisition`, and `compileOnSave` observations. These
  are valuable inputs, but retention is not project orchestration, watching,
  automatic type acquisition, or compile-on-save execution.
- The diagnostic formatter already has byte-to-UTF-16 conversion. L0 moves
  it behind the snapshot `PositionIndex`, which H1 reuses; neither track needs
  UTF-16 positions on every persistent AST node.

### 2.2 Missing production boundaries

The audit found no production JavaScript emitter:

| Boundary | Current implementation | Missing boundary |
| --- | --- | --- |
| Workspace crate | No `crates/emitter` member | Emitter protocols, factory, transforms, writer/printer, output plan, and memory sink |
| Program execution | `ProgramSession::run(self)` only | Separate consuming emit entry; zero changes to the H0 call graph |
| Checker lifetime | Parsed sources, binders, and `CheckerState` are local to a checker function and collapse into `CheckResult` | Scoped checker execution that keeps the live state borrowed through resolver, transform, and print |
| Semantic emit API | No `EmitResolver` | Consumer-owned internal resolver implemented by the live checker |
| AST transform gates | No node or node-array `TransformFlags` storage | Exact emitter-session side tables plus synthetic-node aggregation |
| Transform metadata | No `emitNode`, original-node, helper, substitution, source-map-range, or generated-name state | Session-owned equivalent; no mutation of the parsed arena |
| Synthetic tree | No general `NodeFactory`/transform arena | Reachability-generated create/update/clone/replace surface and parenthesizer rules |
| Transformation | No `transformNodes` or built-in transformer | Context, lexical/block scopes, helpers, substitutions, notifications, diagnostics, and disposal |
| Printing | Only checker-private bounded display-clone printers | The real generic tsc printer, text writer, comments/trivia, precedence, literals, and map hook phases |
| Output | No output paths, artifacts, sink, or write diagnostics | Typed planning, collision gates, callback order, partial failure, memory/filesystem sinks |
| Emitting config/CLI | H0 loaders require `noEmit == true`; explicit files also force no-emit | Separate emitting projection, loader validation, CLI dispatch, and exact exit behavior |

The checker-private `display_clone*.rs` modules are deliberately not an
emitter. Their own module contract excludes source copying, general comments,
source maps, substitutions, declaration state, and the real writer. They can
provide focused algorithm comparisons, but H1 must not promote them into the
production printer or use their strings as oracle output.

### 2.3 The central lifetime refactor

`check_program_with_prebound_libs_at_observed` currently owns, in one stack
frame, `program_sources`, per-file binders, binder references, and a mutable
`CheckerState`. It checks files, copies diagnostics into `CheckResult`, and
drops all semantic state. Returning those objects as one ordinary Rust struct
would create self-references because binders borrow sources and
`CheckerState` owns a `ProgramBinder` borrowing binders. That current shape is
also unable to cache an unchanged parsed/bound file across Program versions.

L0 resolves the durable half before H1: `ParsedDocument` and `BoundDocument`
are owned immutable records, `BinderWorker<'a>` is temporary, and a
`ProgramSnapshot` owns `Arc` handles plus per-Program indexes. H1 then needs
only a scoped/callback execution owned by the checker crate:

```text
ProgramSnapshot owns parsed/bound documents
  -> checker creates and completes a fresh CheckerSession
  -> checker lends an internal EmitResolver + source view to one callback
  -> callback transforms, prints, and returns owned artifacts/observations
  -> checker assembles the ordinary diagnostic result
  -> checker/emit state drops; the snapshot follows its caller's lifetime
```

The existing `check_program... -> CheckResult` functions remain adapters over
that worker for H0 and harness callers. H0 uses an ephemeral document store
and retains no cache after invocation. Production emit must not use the
process-lifetime harness library cache and must not parse, bind, or check a
second time after diagnostics.

## 3. Load-bearing findings from the vendored emit graph

### 3.1 The bootstrap transform list has three owners

For explicit `target: ESNext`, explicit `module: Preserve`, absent/true
`useDefineForClassFields`, no JSX, and no decorators,
`getScriptTransformers` at `_tsc.js:115903` selects, in order:

1. `transformTypeScript` (`94036`);
2. `transformClassFields` (`95852`); and
3. `transformECMAScriptModule` (`113369`).

`transformClassFields` is always inserted. Under the frozen options its
ordinary rewrite gates are false, but its context hook installation and
source-file pass still exist. These need a ported lifecycle plus explicit
inactive-branch proofs.

`module: ESNext` is not an equivalent simplification. It selects
`transformImpliedNodeFormatDependentModule` (`113730`), which constructs both
the ESM and CommonJS module transformers and dispatches per file. The H1
bootstrap therefore fixes `module: Preserve` (200), not a vague
“ES-module-like” value.

### 3.2 Transform flags are computed facts, not an optional optimization

Every built-in transformer uses `node.transformFlags` as a semantic
reachability gate. The Rust AST has no such field. A naive OR of descendant
flags is wrong: tsc computes flags in every `NodeFactory` create/update
function and masks propagated child flags through
`getTransformFlagsSubtreeExclusions` (`25125`) for types, functions, classes,
parameters, properties, binding patterns, outer expressions, and other
containers. Node arrays also own aggregated flags.

H1 must generate or port two equivalent paths:

- parsed nodes and arrays receive exact flags in emitter-local side tables by
  a post-order pass using the same per-kind formulas and exclusions; and
- synthetic nodes and arrays compute the same flags when the H1 factory
  creates or updates them.

Direct comparison against a TypeScript AST/transform-flag oracle is required
for every admitted syntax kind. Adding a persistent word to every parsed node
is not the default because it would charge memory and cache traffic to
`--noEmit`; such a change requires the no-emit evidence first.

### 3.3 Option recognition is not option availability

The full config declaration table prevents misspelling and conversion drift,
but the effective `CompilerOptions` snapshot still contains only the subset
used by H0. In particular, output paths and formatting choices are currently
discarded or retained only transiently for source discovery. The existence of
an entry in `COMPILER_OPTION_DECLARATIONS` must never be treated as proof that
the checker, emitter host, or CLI can observe its effective value.

### 3.4 Generated names require both syntax and semantics

The printer's `isFileLevelUniqueName` (`12907`) rejects a candidate when the
current `SourceFile.identifiers` contains it or the resolver's
`hasGlobalName` reports it. Rust exposes only an identifier count, not the
tsc identifier map, and has no `hasGlobalName` emit query. H1 may build the
source identifier set once in the emitting session from the parsed tree, but
must prove it equals tsc's parser-interned set, including escaped identifiers,
private identifiers, and JS/JSDoc cases admitted later. Synthetic generated
names remain a separate session set. H1 must not guess uniqueness from binder
locals alone.

## 4. H1 blocking packages in dependency order

These are implementation packages, not permission to combine them into one
large slice. Each package closes only after exact owner, oracle, adjacent
control, and no-emit evidence exist.

Before package 4.2 changes runtime types, complete L0/L1 in
[the persistent Program design](lsp-and-incremental.md). Package 4.1 inventory
work may proceed in parallel. The post-L0/L1 H0 route is the baseline used by
the remaining packages.

### 4.1 Inventory, profile, and evidence freeze

- Generate the complete reachable declaration graph from `Program.emit`,
  `emitFilesAndReportErrors`, `getTransformers`, the three active transform
  owners, `transformNodes`, `createPrinter`, and output planning.
- Freeze exact compiler options, file extensions, syntax constructs,
  diagnostic gates, output locations, and unsupported results in a machine
  profile. “Erasable TypeScript” is not a sufficient manifest value.
- Capture callback-level oracle observations: path, text bytes before BOM,
  BOM decision, source-file provenance, callback metadata, callback order,
  diagnostics, `emitSkipped`, optional `emittedFiles`, and exit status.
- Freeze H0 startup/project/scale no-emit baselines and constructor/write-zero
  canaries before any emitter code enters the workspace.
- Pin the compiler, conformance, project, transpile, and FourSlash inventory
  inputs described by H1; classify rather than silently omit every
  out-of-profile row.

### 4.2 Protocol crate and option ownership

- Add the acyclic emitter protocol owner described by H1: artifact kinds,
  output-path slots, sink disposition, errors, outcomes, emit host, resolver,
  transform roots, and dormant map/declaration/bundle/build-info axes.
- Freeze where effective emit options live. The current
  `PreparedProgram` has no `outDir`, `rootDir`, formatting, map, declaration,
  or build-info snapshot. Re-reading raw config text inside the emitter is
  forbidden.
- Prefer a typed optional emit projection or emitting prepared wrapper so H0
  does not allocate emitter-only paths and lists. Scalar options that affect
  diagnostics still belong at their normal option-validation boundary.
- Keep `CompilerHost` read-only and keep the filesystem behind
  `OutputSink`.

### 4.3 Separate emitting loader and preflight

- Add config and explicit-root loading entries that admit the frozen emit
  profile without weakening `validate_admitted_options`, which currently
  requires `noEmit == true`.
- Preserve `outDir`/`rootDir`/config-path facts through preparation and port
  the exact common-source-directory and output-extension rules.
- Run all reached `verifyCompilerOptions` and `verifyEmitFilePath` checks,
  including input overwrite and case-aware duplicate output, before the
  first sink call.
- Reject maps, declarations, bundling, build info, unsupported module/target,
  unsupported extensions, and unsupported syntax through typed preflight.
- Keep the existing H0 load and CLI route unchanged.

### 4.4 Scoped checker execution and resolver adapter

- Build a fresh `CheckerSession` borrowing the L0 `ProgramSnapshot`, and run
  one internal callback after required file checks while links, merged
  symbols, and types are live. Parsed/bound documents remain owned by the
  snapshot rather than the callback stack.
- Preserve the existing owned `CheckResult` adapter and diagnostic bucket
  behavior for every no-emit caller.
- Implement only generated in-profile `EmitResolver` methods. A missing
  method is a typed unsupported result; it never returns a convenient
  `false`, `None`, empty list, or error type.
- Restore emit-only checker producers only behind reached checking work and
  prove that H0 does not collect new per-node lists or flags unnecessarily.

### 4.5 Parsed-tree facts and synthetic factory

- Generate exact parsed-node and node-array transform flags with subtree
  exclusions.
- Add an emitter-local original/synthetic identity model, text/source ranges,
  emit flags, comment metadata, helper metadata, assigned/generated names,
  constant values, and source-map/token ranges.
- Port the reachable `createNodeFactory` and parenthesizer/converter closure,
  including create/update identity rules and array metadata.
- Keep parsed trees immutable and keep all synthetic/session IDs out of
  artifacts and global caches.
- Inventory every literal/token field read by active transforms and printer;
  derive source-backed raw spelling from validated byte ranges only where tsc
  does so.

### 4.6 Transformation runtime

- Port the reached `transformNodes` lifecycle: initialization state,
  lexical/block scopes, hoists, initialization statements, helpers,
  substitutions, emit notifications, feature gates, diagnostics, disposal,
  and one-result assertions.
- Port the reachable emit-helper factory methods and exact helper ordering,
  scoping, deduplication, `importHelpers`, and `noEmitHelpers` controls; the
  first profile may prove most helpers unreachable but cannot delete the
  request protocol.
- Preserve the before/built-in/after/declaration/afterDeclarations order slots
  without exposing a public custom-transformer ABI in H1.

### 4.7 Writer and generic printer

- Port `createTextWriter` with exact text bytes, indentation, line state, and
  independent generated UTF-16 position tracking.
- Port printer pipelines for substitution, notifications, comments, source
  maps, hints, parenthesization, names, tokens, lists, literals, line endings,
  prologues, shebangs, directives, helpers, and source-file context.
- Keep node/list/SourceFile/Bundle root shapes internally typed even though
  H1 reaches only whole `SourceFile` output.
- Execute disabled source-map hooks at their real before/after/token phases;
  do not create a second map-less printer.
- Qualify astral characters, combining marks, escaped names/literals, all tsc
  line breaks, NEL as a non-line-break control, comment trivia, and generated
  names directly against the oracle.

### 4.8 Active transforms and semantic producers

- Port the exact three-transform list in section 3.1, including context hook
  composition and disposal.
- Admit one generated syntax set at a time: type erasure first, then only
  dependency-closed runtime constructs. Runtime enum, namespace, const-enum,
  parameter-property, decorator, JSX, module-downlevel, and helper behavior
  remain rejected until their owner slice lands.
- Close every resolver call reachable from each admitted construct and retain
  inactive branch canaries for the rest.
- Compare the transformed tree as a structural debug observation as well as
  the final bytes; byte equality alone can hide compensating transform/printer
  errors.

### 4.9 Output orchestration and sinks

- Port `getSourceFilesToEmit`, output paths, file-family extensions,
  `forEachEmittedFile`, `emitFiles`, write/BOM behavior, and emitted-file-list
  order.
- Make `MemoryOutputSink` the acceptance authority and inject a failure at
  every write index to pin diagnostic continuation and partial output.
- Add `FsOutputSink` with exact parent-directory creation/retry and stable
  TS5033 text. The emitter itself never calls `std::fs`.
- Keep callback order distinct from `emittedFiles` order and keep optional
  lists absent when the corresponding tsc option is inactive.

### 4.10 CLI, qualification, and release

- Dispatch emitting invocations to the separate emit entry; continue to
  dispatch `--noEmit` directly to `ProgramSession::run`.
- Match config/syntactic/options/global/semantic/emit diagnostic gates and
  `noEmitOnError`/exit status for the admitted profile.
- Run the full compatible upstream inventory, deterministic repeat/job
  checks, memory/filesystem sink equivalence, failure injection, and resource
  qualification.
- Publish H1 only with zero open in-profile owner row and zero H0 behavioral
  or performance regression.

## 5. Resolver inventory and current checker reuse

### 5.1 H1 bootstrap consumers

Static direct-call scanning gives the following upper bound. H1.0 still has
to reduce it by exact option and syntax reachability; a text grep is not the
final dependency graph.

| Consumer | Direct resolver surface | Bootstrap disposition |
| --- | --- | --- |
| JavaScript printer | `hasGlobalName` | Required as soon as generated-name collision checks are reachable |
| `transformTypeScript` | `getConstantValue`, `getEnumMemberValue`, `getReferencedExportContainer`, `isReferencedAliasDeclaration`, `isTopLevelValueImportEqualsWithEntityName`, `isValueAliasDeclaration` | Alias/reference subset is expected in the first type-erasure profile; constant/namespace rows wait for admitted constructs |
| `transformClassFields` | `getReferencedValueDeclaration`, `getTypeReferenceSerializationKind`, `hasNodeCheckFlag` | Rewrite branches are inactive under the bootstrap options; lifecycle remains active and calls need reachability canaries |
| `transformECMAScriptModule` | No direct resolver calls | Still consumes emit host, helper factory, original nodes, substitutions, names, and module syntax |
| `emitFiles` fast paths | `markLinkedReferences`, and declaration-only `collectLinkedAliases` | `noCheck`, declaration, and force-dts routes are rejected in H1 |

The checker already has reusable enum evaluation, many type/symbol queries,
and many `NodeCheckFlags` producers. It does not expose the resolver methods
above, and several underlying side effects are explicitly marked in code as
emit-only omissions. Examples include computed-property loop-capture lists
and some collision/capture producers. Existing primitives reduce port size;
they do not make the resolver complete.

### 5.2 Full resolver surface after H1

The vendored `createResolver` (`88545`) publishes these groups:

- alias/reference classification: referenced export/import containers,
  colliding declarations, value aliases, referenced aliases, linked aliases,
  referenced value declaration(s), and symbol-to-declaration conversion;
- emit/check facts: global-name lookup, node check flags, top-level import
  equals classification, optional parameters, captured bindings, local
  `arguments`, external-module files, and implicit-undefined requirements;
- constants and serialization: constant/enum values, literal-const
  declarations/values, type-reference serialization, and late-bound names;
- JSX: factory and fragment factory entities;
- declaration visibility/building: declaration visibility, overload
  implementations, expando functions/properties, type-of/return-type nodes,
  symbol/entity accessibility, augmentation imports, declaration statements,
  global `Symbol` classification, and late-bound index signatures.

Full declaration emission reaches almost all of the last two groups. Full
downlevel JavaScript reaches more of the first three. They must be scheduled
by consuming transformer, not implemented as an untested bulk trait fill.

## 6. Syntax and source-file fact gaps

| Fact | Current Rust state | Required treatment |
| --- | --- | --- |
| Source text representation | Files are retained as Rust UTF-8 `String` values | L0 shares an `Arc<str>` snapshot without projection copies while preserving H0 decoding; broader UTF-16-file inputs and JavaScript API/snapshots must audit lone-surrogate behavior and use a lossless code-unit model or declare a narrower profile |
| Parsed ranges | Node/array byte offsets; reference directive spans are already UTF-16 observations | Keep typed domains; convert only at diagnostics/map/API boundaries |
| Line conversion | Per-source `LineMap` with byte-to-UTF-16 support | L0 hides it behind a static/versioned `PositionIndex`; H1 maps and future LSP reuse checked accessors rather than public vectors |
| Transform flags | Absent on nodes and arrays | Exact session side tables for parsed trees; exact fields for synthetic trees |
| Emit/original state | Absent | Session tables for `EmitFlags`, original nodes, comments, ranges, helpers, names, substitutions, assigned names, and constants |
| Source identifiers | Count only | Build and oracle-check the exact emitted-session identifier set; combine with resolver global names |
| Source pragmas | Reference directives and JSX import-source/runtime observations exist; checker can rescan `@jsx`/`@jsxFrag` | H1 rejects JSX; full JSX needs one exact pragma authority rather than divergent rescans |
| Script kind | Inferred during parse but not published as a `SourceFile` field | Freeze extension/script-kind projection for JS/JSX/TSX/JSON and public API cases |
| Literal raw data | Template raw text, regex termination, string escape fact, and numeric flags mostly exist | Audit printer/transform reads; add missing template/token flags for downlevel tagged-template and recovery cases |
| Comments/trivia/shebang | Exact source text and ranges exist | Port tsc rescanning and emit flags; do not normalize or attach comments heuristically |
| Triple-slash directives | Parsed source facts exist | Preserve exact declaration/reference re-emission rules and AMD ordering |
| AMD/module metadata | No `moduleName` or `amdDependencies` source facts | Required for AMD/outFile/bundle profiles, not H1 |
| Implied module format | Prepared source has raw/effective values | Project into `EmitHost` without re-reading package metadata |
| Source eligibility | Prepared source has `may_be_emitted` | Verify against target-source, redirect, external-library, JSON, and bundle rules |

The H1.0 AST audit must be generated from every property read in the reached
factory/transform/printer graph. A hand-maintained list is useful for review
but cannot be the completeness authority.

## 7. Emit option, config, and CLI residual

### 7.1 Already projected into `CompilerOptions`

Useful existing fields include `target`, `module`, `moduleDetection`, `jsx`,
`experimentalDecorators`, `useDefineForClassFields`, `importHelpers`,
`downlevelIteration`, `esModuleInterop`, `preserveConstEnums`,
`isolatedModules`, `verbatimModuleSyntax`, `allowImportingTsExtensions`,
`rewriteRelativeImportExtensions`, JSX factory/import-source options, and
`noEmit`.

### 7.2 Recognized but not available to the emitter

Direct reads in the vendored transform/emitter/program spine expose at least
the following missing effective values:

- products and gates: `declaration`, `declarationMap`,
  `emitDeclarationOnly`, `sourceMap`, `inlineSourceMap`, `noCheck`,
  `noEmitOnError`, `incremental`, `composite`, and `tsBuildInfoFile`;
- paths/topology: `outFile`, `outDir`, `rootDir`, and `declarationDir`;
- text/maps: `removeComments`, `emitBOM`, `newLine`, `sourceRoot`, `mapRoot`,
  `inlineSources`, `noEmitHelpers`, and `extendedDiagnostics`;
- transform/declaration policy: `emitDecoratorMetadata`,
  `isolatedDeclarations`, `erasableSyntaxOnly`, `stripInternal`, and legacy
  deprecated-option compatibility;
- library/program policy: `libReplacement`, project-reference redirect
  switches, and the remaining option-default/implication rules that affect
  source identity or output eligibility; and
- reporting: `listEmittedFiles`, `listFiles`, `explainFiles`, diagnostics/
  tracing switches, and internal output-path suppression used by tsc APIs.

Some names are already converted by `ConfigOptionBag`, and `outDir` plus
`declarationDir` are temporarily used to exclude generated directories from
config discovery. They still do not survive as an effective emit snapshot on
`PreparedProgram`.

### 7.3 H1 policy

H1 supports only the exact frozen subset and rejects every other output-active
choice before writes. It must nevertheless preserve the dormant typed product
and path slots. Default-valued choices such as newline, BOM, comments, and
helper policy are explicit in the machine profile rather than assumed by the
printer.

The command parser is currently a small H0 parser: explicit files require
`--noEmit`, config loading requires effective `noEmit`, and only a narrow
selection surface is accepted. Emitting CLI support therefore requires a
separate argument/config dispatch; removing those H0 guards in place is a
stop condition.

### 7.4 Broader drop-in CLI work after H1

A broad `tsc` command also needs exact behavior for help/version/init,
`showConfig`, list/explain modes, pretty/color and locale selection,
diagnostics/extended diagnostics, trace/profile outputs, response and config
selection errors, project-plus-file conflicts, watch/build selection, and all
exit-status variants. Recognizing their spellings is not implementation.

### 7.5 Config fields outside `compilerOptions`

Full config compatibility is wider than the compiler-option declaration
catalog. `ParsedCommandLine` also publishes `fileNames`, raw config,
`projectReferences`, `watchOptions`, `typeAcquisition`, wildcard directories,
`compileOnSave`, and ordered parse diagnostics. The current H0 plan preserves
several of those fields specifically so they are not lost, while deliberately
rejecting the behaviors that would consume them.

The later owners are distinct:

- `references` feeds Program redirects, solution graph construction, and
  build ordering;
- `watchOptions` controls watcher selection, recursion, fallback polling,
  synchronous directory callbacks, and exclusions;
- `typeAcquisition` belongs to Project Service and the typings installer, not
  ordinary one-shot `tsc` emit;
- `compileOnSave` is a project-service/editor command path, not a synonym for
  watch mode; and
- `plugins` and extra file extensions require Language Service/server loading
  and security policy even though their config syntax can already be parsed.

A complete config claim must compare the full public parsed result, extends
provenance, wildcard metadata, errors, and relevant host calls. It must not
count a value merely because `ConfigRootPlan` can retain or reject it.

### 7.6 Host, `System`, and library resolution

The production H0 host is intentionally narrower than the public and
long-lived hosts. Broader compatibility still needs exact contracts for:

- `CompilerHost` source-file creation/recreation, `getSourceFileByPath`,
  cancellation, default-lib location, write callback, current/canonical path,
  newline, and custom module/type-reference resolution hooks, including their
  legacy and literal-aware overloads;
- resolution-cache publication, environment variables, invalidated
  resolution reporting, hashes, parsed referenced configs, and the internal
  library-resolution hook used by Program/builder reuse;
- `System` terminal/TTY width, directories, modified times, deletion,
  SHA-256/hash, memory reporting, timers, screen clearing, base64, watcher,
  profiler, and process-exit capabilities; and
- TypeScript 6.0 `libReplacement`: resolving `@typescript/lib-*` packages,
  package identity and invalidation, fallback to the bundled lib catalog, and
  exact diagnostics/traces. Embedding the stock 6.0.3 libraries proves only
  the fallback branch.

Host callback presence is observable. A missing optional capability can
select a diagnostic or fallback, so replacing every optional callback with a
single always-capable Rust filesystem object is not automatically compatible.

## 8. JavaScript transformer expansion after H1

The following is the complete built-in script-transformer owner list selected
by `getScriptTransformers` in TypeScript 6.0.3. “Direct resolver calls” does
not include helper/host/factory dependencies or indirect calls.

| Owner | Vendored line | Activation | Direct resolver calls |
| --- | ---: | --- | --- |
| `transformTypeScript` | 94036 | Always | constant/enum values, export containers, alias reference/value classification |
| `transformLegacyDecorators` | 98430 | `experimentalDecorators` | referenced value declaration, node check flags |
| `transformJsx` | 103845 | JSX transform enabled | JSX factory and fragment queries occur through the context resolver |
| `transformESNext` | 103278 | target below ESNext | None direct |
| `transformESDecorators` | 98946 | standard decorator lowering condition | None direct |
| `transformClassFields` | 95852 | Always inserted | referenced value declaration, type-reference serialization, node check flags |
| `transformES2021` | 103205 | target below ES2021 | None direct |
| `transformES2020` | 102943 | target below ES2020 | None direct |
| `transformES2019` | 102907 | target below ES2019 | None direct |
| `transformES2018` | 101680 | target below ES2018 | node check flags |
| `transformES2017` | 100810 | target below ES2017 | type-reference serialization, node check flags, local `arguments` binding |
| `transformES2016` | 104646 | target below ES2016 | None direct |
| `transformES2015` | 104740 | target below ES2015 | colliding declaration/name, node check flags, local `arguments`, captured binding |
| `transformGenerators` | 108119 | target below ES2015 | referenced value declaration |
| `transformECMAScriptModule` | 113369 | module Preserve | None direct |
| `transformImpliedNodeFormatDependentModule` | 113730 | ES/CommonJS/Node module families | None direct; constructs and dispatches both ESM/CJS closures |
| `transformModule` | 110090 | CommonJS/AMD/UMD/default branch directly or through implied format | referenced export container, import declaration, and value declarations |
| `transformSystemModule` | 112050 | System | referenced export container, import declaration, and value declaration(s) |

Completing JavaScript emit means more than porting the functions in this
table. It also includes the exact feature flags, helper text and dependencies,
substitution/notification composition, module prologues, extension rewrites,
JSON copying, JS/JSX/TSX behavior, MTS/CTS/MJS/CJS extension matrix,
decorator metadata, JSX classic/automatic runtimes, all targets/modules, and
every option combination that makes a branch reachable.

Each expansion slice must add one transformer plus its factory, helper, host,
resolver, printer, option, and test closure. A transformer whose top-level
visitor compiles is not complete if one of those dependencies still returns a
placeholder.

## 9. Non-JavaScript output tracks after H1

### 9.1 JavaScript source maps

The H1 printer retains disabled hook phases and typed source/generated
positions. A source-map track still must port:

- `createSourceMapGenerator` (`92365`), source switching, node/token mapping
  ranges, generated-name mappings, and source ordering;
- generator `appendSourceMap` composition, decoded mapping iteration,
  generated start/end clipping, source/name index remapping, `sourceRoot`
  rebasing, and carried `sourcesContent`;
- UTF-16 source and generated line/column behavior, including synthetic and
  original ranges;
- VLQ encoding, JSON serialization, path normalization, `sourceRoot`,
  `mapRoot`, `inlineSources`, and `sourcesContent`;
- external versus inline maps, base64 encoding, `sourceMappingURL`, map URL
  position metadata, and exact callback/emitted-file ordering; and
- JS/JSX/JSON/bundle exclusions plus every Unicode/newline edge case.

Source maps can be implemented after the H1 printer foundation without
waiting for declaration emit. Declaration maps require both tracks.

### 9.2 Declaration and declaration-map emit

Declaration output is a separate semantic compiler pipeline, not another
printer mode. It requires:

- `transformDeclarations` (`114265`) and the declaration transformer list;
- full declaration-side `EmitResolver` visibility/accessibility/type-builder
  queries and linked-alias/reference collection;
- complete NodeBuilder behavior for inferred public types, symbol trackers,
  truncation, inaccessible/private names, unique symbols, overloads,
  expandos, late-bound names, and error locations;
- declaration diagnostics and their exact `noEmitOnError` position;
- TypeScript and checked-JavaScript/JSDoc declarations, global/module
  augmentations, reference directives, `stripInternal`,
  `isolatedDeclarations`, and declaration-only/no-check modes;
- `.d.ts`/`.d.mts`/`.d.cts` path selection, `declarationDir`, bundles,
  partial/suppressed writes, and builder signature output; and
- declaration maps after the source-map generator is complete.

The current checker has diagnostic-oriented node-builder/display slices and
some declaration semantic primitives. Those are inputs to the closure, not a
complete declaration emitter.

### 9.3 Bundle and `outFile`

Bundle support activates `SourceFile | Bundle` throughout transformer,
printer, source-map, helper/prologue, AMD dependency, directive, output path,
and write ordering. It also changes source eligibility and common-source
directory behavior. H1 keeps the root discriminant but intentionally rejects
this route.

### 9.4 Build info and builder signatures

`.tsbuildinfo` uses the artifact/sink boundary but not the JavaScript printer.
It requires versioned serialization, file/version/signature tables, option
identity, affected-file computation, unchanged-output suppression, declaration
signature emit, project-reference state, and tsc's special behavior where
build info can be written under option combinations that otherwise suppress
ordinary output.

## 10. Compiler and tooling residual outside ordinary emit

### 10.1 `noCheck`, transpilation, and targeted emit

Full tsc supports emit paths that do not perform ordinary semantic checking.
`emitFiles` then uses linked-reference marking, and `transpileModule`/
`transpileDeclaration` construct still smaller single-file pipelines with
their own diagnostics and custom-transformer behavior. H1 rejects these
routes. They require explicit performance contracts; forcing them through the
full checker would be behaviorally and operationally wrong.

Program `emit(targetSourceFile, writeFile, cancellationToken, emitOnly,
customTransformers, forceDtsEmit)` also exposes targeted, emit-only, builder
signature, declaration-only, and forced-declaration axes. H1 reserves typed
internal discriminants but exposes none publicly.

### 10.2 Project references and solution build

The current program is one prepared project. Full `--build` needs config
reference parsing, graph ordering/cycle diagnostics, prepend/reference output
selection, redirect behavior, up-to-date checks, clean/dry/force/verbose
modes, status reporting, cross-project output collision checks, and solution
builder invalidation. It depends on deterministic declaration/build-info
output, not on incremental parsing.

The public solution-builder contract is larger than invoking `tsc -b` once.
It exposes `build`, `clean`, `buildReferences`, `cleanReferences`, and a pull
API through `getNextInvalidatedProject`. Invalidated work can either rebuild a
project or update output timestamps, and callers may supply cancellation,
write callbacks, and project-specific custom transformers. Completion must
therefore preserve pull ordering, `done` discipline, timestamp-only work,
partial graph state after errors/cancellation, and every `ExitStatus`, as well
as ordinary CLI bytes.

### 10.3 Incremental program and watch

Builder reuse and watch require:

- abstract, semantic-diagnostics, and emit-and-semantic builder programs,
  `createIncrementalProgram`, `readBuilderProgram`, `emitNextAffectedFile`,
  and affected results whose owner may be a `SourceFile` or whole `Program`;
- old-Program structure reuse, resolution-cache invalidation, source versions,
  changed package/config/library handling, and signature comparison;
- builder host hash/write callbacks and exact write-callback precedence;
- watcher registration, wildcard directory policy, event coalescing,
  polling/fallback strategies, timer scheduling, cancellation, and stable
  diagnostic/status output; and
- memory and long-running leak qualification, not only one-shot parity.

Watch additionally observes root-file updates, missing-file and failed-lookup
directory watches, type-root/config/package changes, `watchOptions`, delayed
compilation scheduling, `afterProgramCreate`, status messages, screen-clearing
policy, and `close`. Build-with-watch combines both state machines and needs a
separate qualification lane; passing one-shot incremental compilation does
not prove it.

Watch depends on builder/program reuse. It does not require the incremental
parser for every host, but editor-quality changed-file performance benefits
from it.

### 10.4 Incremental parser and Program reuse

The audited [LSP/incremental design](lsp-and-incremental.md) separates three
layers whose schedules differ:

- **L0, before H1 runtime:** shared text/position snapshots, collision-safe
  identity leases, owned parsed/bound documents, `ProgramSnapshot`, an
  ephemeral H0 adapter, and proof that an unchanged file parses/binds zero
  times across two snapshots;
- **L1, before H1 runtime:** port `updateSourceFile`, affected-range extension,
  syntax cursor and node-reuse eligibility, copy-on-reuse relocation, exact
  fresh-parse equivalence, and the large-file edit performance gate; and
- **L2, after H1 is allowed:** complete `DocumentRegistry`, old-Program
  structure states, resolution dependency tracking/invalidation, watchers,
  and service-owned eviction policy.

The current empty `SyntaxCursor`, Program-order-dependent node/symbol bases,
borrowed binder result, full-text copies, per-byte position table, consuming
`ProgramSession`, and per-run resolution cache are not sufficient for a
Language Service. L0/L1 are on H1's architectural critical path because a
failed copy-on-reuse benchmark may force an arena representation change. They
remain outside H1's observable compatibility scope, and the checker remains
fresh per Program version.

### 10.5 Public compiler API and custom transformers

The vendored `typescript.d.ts` exposes a much larger compatibility contract
than a CLI:

- stable `Node`, `SourceFile`, `NodeFactory`, `TransformationContext`,
  `Printer`, `Program`, `TypeChecker`, host, builder, watch, and language
  service interfaces;
- `before`, `after`, and `afterDeclarations` custom transformer callbacks;
- public node/list/file printing, parsing, config parsing, module resolution,
  transpilation, cancellation, and write callbacks; and
- JavaScript object identity/mutation/optional-property behavior that needs a
  deliberate Rust API mapping rather than accidental exposure of internal
  arenas.

The closure includes more than `Program.emit`. `Program` publishes diagnostic
buckets, source/library classification, counters, resolution-mode queries,
project-reference results, and a long-lived `TypeChecker`. The module also
publishes scanner/parser/update helpers, config and module-resolution helpers,
factory/visitor/transform/printer APIs, formatting and diagnostic utilities,
transpile APIs, builder/watch/solution constructors, and enums/flags whose
numeric values are observable. The current internal checker API and arena IDs
do not satisfy those contracts by existing under similar names.

H1 only stabilizes internal architecture. A public Rust API requires its own
semver, ownership, thread-safety, cancellation, and observable-error policy.
Custom transformers should be added only after the internal factory/context/
printer contracts and clone/original-node semantics are qualified; exposing
them earlier would freeze an incomplete ABI.

The future API profile must state whether the target is behavioral parity for
a Rust-native API or source/runtime compatibility for JavaScript consumers.
The latter additionally requires a JS binding/package layer with JavaScript
object identity, callbacks, maps/arrays, `undefined`, exceptions, and module
exports. A Rust facade alone cannot be called drop-in `typescript` API
compatibility.

### 10.6 Language Service, tsserver, and LSP

The Language Service requires `IScriptSnapshot` change ranges, script/project
versions, `createLanguageServiceSourceFile`/`updateLanguageServiceSourceFile`,
and `DocumentRegistry` keys, acquire/update/release reference discipline,
script kind, implied module format, and semantic/partial-semantic/syntactic
modes. Its query surface includes diagnostics/classifications, definitions,
references, completions and auto-imports, quick info, rename, navigation,
call hierarchy, inlay hints, formatting/indentation, outlining/brace/comment
operations, code fixes/refactors, organize imports, file-rename and paste
edits, signature help, and per-file emit. Package-json, module-specifier,
source-mapper, and auto-import-provider caches need independent invalidation
and long-running memory evidence.

tsserver is a separate TypeScript protocol product above that service. It
adds framed request/response/event ordering, a server `Session`, configured/
inferred/external project ownership, open-file overlays, background and
region-prioritized diagnostic events, cancellation by request ID, watch
scheduling, preferences, plugins,
logging/performance/telemetry, package installation, automatic type
acquisition, and the typings installer. FourSlash primarily qualifies
Language Service operations; tsserver protocol/project-system unit suites are
still required for this layer.

LSP is not the tsserver protocol and is not exported by the vendored
TypeScript compiler. An LSP product additionally needs its own adapter contract
for initialize/capabilities, URI/path and UTF-16 conversion, document
synchronization, cancellation, concurrent request scheduling, diagnostics
publication, workspace/configuration changes, progress, and latency/memory
budgets. It is complete only against LSP protocol tests plus mapped Language
Service witnesses, not by inheriting a tsserver or FourSlash pass rate.

The internal H1 resolver is not a public TypeChecker and the FourSlash emit
inventory is not a Language Service pass claim.

### 10.7 Platform, packaging, and version transitions

A broad drop-in claim also needs qualified Windows path/case/drive/UNC
behavior across emitting and build outputs, POSIX case profiles, permissions,
symlinks, timestamps, watcher backends, terminal capabilities, and filesystem
failure precedence. Packaging must cover the intended `tsc`, compiler-library,
and server entry points, the exact version, the stock lib catalog, license and
package metadata, and reproducible artifacts.

Locale selection is a real output surface: diagnostic catalogs, locale
validation/fallback, help/error rendering, and package layout must be pinned.
The current vendor snapshot has the English diagnostic catalog but no locale
directories, so localized CLI compatibility is unimplemented rather than an
available option spelling.

Supporting another TypeScript release requires a new source/lib/locale/package
pin, generated constants and diagnostics, refreshed declaration hashes,
oracle records, accepted sets, option/suite inventories, and an explicit
compatibility transition. It is not routine dependency updating.

## 11. Test and evidence ownership

### 11.1 Upstream evidence map

| Upstream source | What it proves | H1 use | Later use |
| --- | --- | --- | --- |
| `tests/cases/compiler` | Compiler directives and the inputs consumed by `compilerRunner` | Classify all; run compatible JS rows | Complete all admitted product/option rows |
| `tests/cases/conformance` | Feature and transform matrices | Run compatible JS emit rows alongside existing diagnostics | Full target/module/JSX/decorator/declaration/map matrix |
| `tests/cases/project` and `projects` | Config roots, output topology, multi-file order, project behavior | Compatible single-project JS cases | Declarations, bundles, references, build/watch |
| `tests/cases/transpile` | `transpileModule`/transformer component behavior | Pin and use focused factory/transform/printer controls | Complete transpile and no-check APIs |
| `tests/cases/fourslash` | Language Service and per-file emit operations | Inventory/promote only batch-relevant emit witnesses | Full FourSlash runner and L-track qualification |
| `tests/baselines/reference` plus runner code | Exact expected diagnostics, traces, JS/d.ts/maps, source-map records, type/symbol views, project, service, and server observations | Pin only reached H1 products | Required; input inventory alone is insufficient |
| `src/testRunner/unittests` watch/build/incremental/public-API/tsserver owners | Long-lived, cache, callback, protocol, and builder behavior | Inventory only | Required by their corresponding tracks |
| `APISample_*` compiler cases | Public compiler, watch, linter, transform API examples | Classification controls | Public API compatibility |

The checked-in `ts-tests` snapshot currently contains compiler,
conformance, project, and projects inputs but no transpile or FourSlash tree
and no checked-in upstream output baselines. H1's reviewed suite-pin transition
must preserve exact source commit, paths, blob hashes, runner/extractor
identity, and unsupported classifications.

The local `test-suite-expansion.v1.json` contains 7,276 compiler and 632
project cases, and all 7,908 rows deliberately have initial execution state
`not-run`. H0 later proved structural load/session execution for all compiler
plans and 82 compatible project plans, but it did not mutate that manifest
into an upstream baseline result. The remaining 550 project rows are
classified as 452 descriptor emit controls, 70 descriptor declaration
requests, 10 `compileOnSave` configs, 16 config declaration/`outFile`
requests, and two descriptor `rootDir` emit controls.

At the pinned commit, upstream
[`compilerRunner.ts`](https://github.com/microsoft/TypeScript/blob/050880ce59e30b356b686bd3144efe24f875ebc8/src/testRunner/compilerRunner.ts)
independently verifies diagnostics, module-resolution traces, source-map
records, JavaScript/declaration output, source-map output, and type/symbol
baselines. A complete compiler-suite claim must publish a disposition/result
for every applicable observation. Matching only `.js` bytes, or merely
constructing every input Program, is not that runner pass.

Suite adoption follows four separate states: content-addressed input and
runner inventory; deterministic expansion; production execution with captured
observations; and exact comparison against pinned reference/oracle output.
Only the fourth state counts as parity. Every row not reaching it remains
`not-run`, `unsupported`, or `failed` with an explicit reason; there is no
implicit pass state.

### 11.2 Rust test owners

No new repository-root `tests/` tree is needed:

- `crates/emitter/tests/contracts.rs`: writer, positions, flags, factory,
  context, transforms, printer, helpers, output plan, artifacts, and memory
  sink;
- `crates/checker/tests/contracts.rs`: resolver answers and emit-only producer
  side effects with adjacent no-emit controls;
- `crates/compiler/tests/contracts.rs`: scoped session, diagnostic gates,
  CLI, filesystem sink, partial failures, and no-emit constructor/write-zero
  behavior;
- harness/oracle/conformance tests: upstream plan expansion, callback schema,
  exact comparison, pin validation, and qualification artifacts; and
- later builder/LSP crates: long-running invalidation, cancellation,
  fresh-versus-reused equality, protocol, and resource tests.

Every expected output byte comes from the pinned oracle. Structural debug
observations may help localize failures but do not replace callback/output
equality.

### 11.3 Required non-functional evidence

- H0 no-emit: cold/warm wall time, peak RSS, startup/binary effect, and
  emitter-constructor/write zero across explicit, project, and scale loads;
- emit: deterministic bytes across repeats/jobs, bounded peak memory, no
  process-global ID/state leakage, and memory/filesystem sink equivalence;
- build/watch/LSP: long-running memory, cache invalidation, cancellation
  latency, event/request ordering, and version transition behavior;
- public API/server/platform: callback absence/presence, object and cache
  lifetime, ABI/binding behavior, localization, filesystem faults, and
  package-entry smoke tests; and
- every track: exact source pin, owner hashes, supported/unsupported manifest,
  and zero silent fallback.

### 11.4 Cross-track CI and qualification topology

L0.0 expanded the GitHub workflow from the closed H0/M8 classifier/platform
canary to schema-bound fail-closed selection, a common non-documentation
format/locked-all-target lane, initial track-focused controls, the applicable
Windows host/program canaries, a stable aggregate, and deterministic scheduled
inputs. The unsplit local `cargo xtask ci` still owns semantic acceptance. The
strict receipt and bounded failure-artifact schemas are frozen and tested, but
the workflow does not yet mint an authenticated exact full-gate status, run
runtime stress, or qualify performance.

The complete shared topology is:

| Layer | Required role | Authority and boundary |
| --- | --- | --- |
| Required PR guardrail | Fail-closed path classification; formatting; locked, non-linking all-target workspace check; bounded owner-focused tests; applicable Windows/platform smoke | Fast regression feedback only; every selected job must report a known terminal state |
| Exact merge qualification | Unsplit full gate and track gates with a machine-readable result bound to exact HEAD/base, toolchain/Node pins, lockfile, vendor/suite/profile hashes, commands, and outputs | Semantic merge authority; a new commit, base update, merge-queue composition, or input change invalidates the result |
| Protected-main scheduled/soak | Randomized edits, open/close/cache reclamation, broad compatible emit corpus, deterministic repeats, bounded fuzz and long-running resource checks | Drift/stress authority; preserves bounded seeds, traces, diffs, counters, and reproducers, but never retroactively qualifies a PR |
| Approved performance/release | Alternating baseline/candidate measurements on a frozen runner followed by exact artifact/receipt verification and release gates | May mint performance ratchets and release claims; moving `*-latest` images may only act as functional canaries |

The serializable merge summary is separate from M8/M9's move-only,
same-process evidence token: it records that the authoritative command
completed for these immutable inputs, but it does not export, reconstruct, or
trust ephemeral producer evidence across jobs. Free-form PR-body text remains
a human summary and cannot satisfy the required check. Branch protection uses
one stable aggregate name and fails if classification is incomplete, a
selected lane is absent or unexpectedly skipped, or a receipt does not match
the candidate pair. A content hash alone is not authentication: an accepted
summary comes from an approved trusted runner with GitHub OIDC/artifact
attestation, or from an explicitly registered local signer whose verifier
posts the required status. Fork PRs receive no signing credential, and an
unsigned repository file, artifact, or PR comment is never a receipt.

Each track extends focused selection rather than replacing the common gate:

- L0/L1 select source/text/UTF-16 conversion, relocation, parse/bind ownership,
  fresh-versus-incremental, registry, reclamation, and H0 no-emit controls;
- H1 selects checker/emitter/compiler, transform/printer/output plan, memory
  and filesystem sinks, exact output ordering, and zero-constructor/write
  no-emit controls;
- build/watch select signature/build-info/reference, invalidation, watcher,
  partial-failure, and long-running event/resource tests;
- Language Service/tsserver select query cache, cancellation, project-service,
  protocol, plugin, type-acquisition, event-ordering, and memory tests; and
- an LSP product selects URI/path and UTF-16 protocol conversion,
  synchronization, capabilities, request cancellation, diagnostics, workspace
  changes, and multi-platform transport tests without borrowing tsserver
  results.

H1 output paths/sinks and later watch/server work expand the Windows selector
beyond the present host/program paths. Cargo commands use `--locked` where
supported, third-party Actions are pinned to reviewed full commit SHAs,
performance runners are explicitly approved, and failure artifacts are
bounded and content-addressed. L0.2 extends the trusted exact-result producer
with identity-owner focused selection, scheduled open/edit/close reclamation
stress, and chained approved-runner H0 comparison. H1 must reuse the common authenticated
authority while adding its implementation-specific focused selection, emit
stress, and approved-runner qualification before its first runtime change;
the aggregate hosted sentinel alone is not acceptance evidence.

The required workflow runs on `pull_request` and, when a merge queue is used,
`merge_group`; a protected-main `push` verifies the merged composition, while
scheduled and release entry points select only their declared authorities.
Jobs use least privilege, bounded timeouts, stale-PR concurrency cancellation,
and no writable cache for acceptance evidence. Classifier contract tests cover
renames, empty or unavailable base ranges, docs-only changes, the generated
README status block, every boolean output combination, and unknown/new paths;
ambiguity always selects more validation rather than silently skipping it.

## 12. Dependency order toward broader compatibility

The critical path is:

1. freeze the current H0 parse/bind/text-copy and resource evidence, define
   the shared CI lane/receipt/failure-artifact schemas, and continue the
   read-only H1 inventory/profile/oracle work;
2. land L0 shared text/position ownership, identity leases, owned bind state,
   `ProgramSnapshot`, ephemeral H0 adapter, and minimal registry reuse;
3. land L1 incremental parsing and its exactness, Unicode-edit, randomized-
   edit, memory, and large-file latency gates, then requalify H0;
4. freeze the post-L0/L1 H1 no-emit baseline;
5. land emitter protocols, emitting options/loader, and scoped checker
   lifetime;
6. land transform flags, factory/context, writer/printer, resolver, and the
   three-transform bootstrap;
7. close output planning, sinks, CLI, and H1 qualification;
8. expand JavaScript transforms/options/file kinds while closing one-shot
   config, host/System, library-replacement, CLI, and source-map behavior;
9. implement declaration emit, bundles/`outFile`, then declaration maps;
10. implement deterministic builder signatures/build info and project
    references, including solution-builder pull/clean behavior;
11. add L2 old-Program/resolution and builder-program reuse, affected-file
    queues, and solution build, then qualify ordinary watch and
    build-with-watch;
12. qualify the public compiler/custom-transformer API and any JavaScript
    binding/package profile under its own contract;
13. close Language Service query/cache suites, then tsserver Project Service,
    protocol, plugins, and type acquisition;
14. implement and qualify the separate LSP adapter if it remains a product
    goal;
15. close locales, platform matrices, package entry points, and reproducible
    release artifacts; and
16. start any post-6.0.3 version transition only after its new evidence
    contract is approved.

Some work can proceed in parallel without violating that order:

- H1 owner/profile/oracle inventory and later-surface inventories can proceed
  while L0/L1 land, but H1 runtime types and its performance baseline cannot;
- source-map generator and full JavaScript transformer expansion are largely
  independent;
- declaration resolver/NodeBuilder inventory can be designed while maps are
  implemented, but declaration maps wait for both;
- FourSlash, tsserver, and LSP inventories can continue without claiming or
  implementing their product layers; and
- CLI/config/System inventories, public signature inventories, suite pins,
  locale/package inventories, and platform probes can be prepared while
  transformer slices land.

The M9 producer fingerprint must freeze only after H1's shared checker
producer changes are stable. M9.1c-M9.7 are not prerequisites for H1
functionality, but freezing or qualifying M9 first would make later
emit-resolver side-effect corrections reset its evidence. H1 earns no M9
window credit.

## 13. Completion gates for the three practical targets

### 13.1 H1 bounded JavaScript emit

H1 is complete only under
[its own definition of done](h1-emit.md#15-definition-of-done): one frozen
profile, exact outputs and failure behavior, closed in-profile owners, and an
unchanged zero-cost H0 route. Deferred rows remain explicit.

### 13.2 Broad one-shot `tsc` compiler

A later broad CLI claim additionally requires:

- every one-shot compiler/config/command mode classified and every admitted
  combination exact, including help/init/show/list, diagnostics/traces/
  profiles, locale, library replacement, and host-capability fallbacks;
- full JavaScript targets/modules/file kinds/helpers, declarations,
  declaration maps, source maps, bundles, no-check/transpile, and targeted
  Program emit;
- complete compatible compiler/conformance/project/transpile suites at every
  applicable upstream runner observation, not only output-file bytes;
- exact diagnostics, output bytes/paths/order, callback metadata, partial
  failures, exit modes, and preservation of the existing zero-live-exclusion
  diagnostic state; and
- no unsupported operation hidden behind a successful exit or partial file.

Build/watch, Language Service, public API, and later TypeScript versions still
remain separate claims unless explicitly included by that future contract.

### 13.3 Full TypeScript tooling surface

Only the union of the broad compiler, build/watch/project-reference,
incremental parser/program, public compiler API/custom transformer, Language
Service, tsserver, any separately claimed LSP adapter, localization/platform,
package, and version-specific contracts could support a “complete TypeScript
tooling” statement. Each layer needs its own upstream runner/protocol result
and long-running evidence. No current milestone makes that claim.

## 14. Stop conditions

Stop and amend the relevant design if:

- H1 needs a transformer outside the frozen three-owner list without a
  profile transition;
- a resolver answer is stubbed instead of ported or rejected;
- the checker is collapsed and rebuilt/reparsed for emit;
- transform flags are approximated without subtree-exclusion parity;
- the display-clone printer is promoted as the JavaScript printer;
- raw config is reparsed inside the emitter because an effective option was
  discarded;
- an output-active option is accepted because its name is recognized even
  though its value has no consumer;
- H0 constructs any factory, resolver, transform session, printer, output
  plan, artifact, or sink write;
- H1 runtime implementation begins before the L0/L1 arena/ownership proof or
  uses a private source/binder representation that bypasses it;
- declarations, maps, bundles, build info, targeted emit, custom transforms,
  incremental state, or LSP behavior are counted as implemented merely
  because H1 reserved a typed slot; or
- retained `references`, `watchOptions`, `typeAcquisition`, or
  `compileOnSave` config values are counted as their runtime behavior;
- the 7,276 structural compiler-plan sweep or the 7,908-row input manifest is
  reported as an upstream baseline pass;
- `_tsc.js` alone is treated as the implementation authority for public
  Language Service or server work that lives in `typescript.js`;
- FourSlash, tsserver protocol/project-system, and LSP results are substituted
  for one another;
- localized output or `libReplacement` is claimed from option recognition
  while its catalogs/packages and resolution behavior are absent;
- internal arenas/checker methods are exposed as a public API without a
  declared Rust-native versus JavaScript-compatible product contract;
- an L0/L1, H1, builder, service, server, or LSP runtime change is accepted
  from classifier/platform success alone, a stale or free-form gate claim, or
  a receipt not bound to the exact candidate/base and immutable inputs; or
- an H1, M9, builder, L-track, public-API, or version claim borrows evidence
  from a different finish line in section 1.
