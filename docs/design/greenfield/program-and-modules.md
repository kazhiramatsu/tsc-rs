# design: Program layer, module resolution, checker initialization

The parent design set covers scanner→parser→binder→checker but left
three architecture holes that every multi-file / lib-touching fixture
hits. This doc closes them: the Program/host layer, module
resolution, and the checker's global-environment initialization.
tsc anchors verified at the 6.0.3 pin.

## 1. Program construction (the host layer)

There is no compiler without an ordered file list. tsc's
`createProgram` does: host file loading → default-lib injection →
reference/import walking → SourceFile ordering. The greenfield
equivalent is deliberately smaller because the harness (m0 stage
0.5) already fixed the file set per program.json — no disk, no
discovery:

```rust
pub struct Program {
    /// ordering contract = tsc's: lib files FIRST (in libEntries
    /// layering order), then user files in program.json order.
    /// File INDEX order is diagnostic-sort order (compareDiagnostics
    /// keys on file) — observable, so it is a contract, not a detail.
    pub files: Vec<SourceFile>,
    pub options: CompilerOptions,
    /// resolved module map: (importing file, specifier) → target file
    /// or None (→ 2307 at check time). Filled ONCE before binding.
    pub resolved_modules: HashMap<(FileId, String), Option<FileId>>,
}
```

Pipeline order (drives phases 3+; the driver in impl-checker-2xxx
assumes it): parse all files → externalModuleIndicator post-step
(impl-nodes §4) → resolve all import/export specifiers (§2) → bind
all files in file order → checker init (§3) → check user files in
order (lib files are bound but only checked on demand through symbol
resolution — tsc behavior; checking libs eagerly is pure cost and
emits nothing the corpus compares).

`SourceFile.is_declaration_file` = name ends `.d.ts` — gates
statement checking rules (ambient-only contexts) and several grammar
checks.

## 2. Module resolution (scoped to what the corpus observes)

Fixtures import each other with relative specifiers
(`import x from "./a"`) and test ambient modules
(`declare module "foo"`). The design ports the OBSERVABLE subset of
`resolveModuleName` (`_tsc.js` 40649):

1. Relative specifiers (`./`, `../`): resolve against the importing
   file's directory over the IN-MEMORY file set with tsc's extension
   candidate order — exact name if it has a recognized extension,
   else `.ts`, `.tsx`, `.d.ts` (then `.js/.jsx` under allowJs —
   ledger-note, defer). Directory → `index.ts` family. Case
   sensitivity: exact match, with the 1149/1261 casing diagnostics
   at the program layer.
2. Non-relative specifiers: NO node_modules walk (the harness never
   provides one). Lookup order: ambient module declarations
   (`declare module "name"` — a symbol under the global
   `__module`-quoted names), pattern ambient modules
   (`declare module "foo*"` — tsc's best-pattern-match rule), else
   unresolved.
3. Unresolved → `resolved_modules` None → checker emits 2307
   (`Cannot_find_module_0...`) at the specifier, and the import's
   symbols become error-typed (the ANTI-cascade rules around 2307
   follow tsc: an unresolved import does not spray 2304s).
4. `moduleResolution`/`module` option interactions beyond this
   (node16 ESM/CJS splits, package.json exports, extension rewrites
   5097-family): OUT of the 2XXX-first goal — ledger-note per
   emission-map rule; the corpus share is small and phase 9 mining
   will size it.

Import/export SEMANTICS (alias symbols, export=, re-exports,
import-equals) are binder/checker work already covered by
impl-binder §3 + impl-checker-2xxx §3/§6 — this section only fixes
WHERE specifier→file resolution happens (before bind, cached on
Program) so alias resolution is a pure symbol-table affair.

## 2b. Pragmas, reference directives, comment directives

Surfaced by the full-band accounting (impl-checker-2xxx §10): four
band codes are emitted by PROGRAM-layer preprocessing, and one
mechanism SUPPRESSES arbitrary diagnostics. All live here:

- **Pragma processing** (`processPragmasIntoFields` @36275): the
  leading-comment pragmas — `/// <reference path|types|lib|
  no-default-lib>`, `<amd-module>`, `<amd-dependency>`. Emits 2458
  (multiple AMD name assignments). `path` references add files to
  the program (harness supplies the closed file set — a path
  reference to an absent file is the 6053-family, non-band);
  `types` references resolve against type roots — absent in the
  harness, so failures emit 2688 symmetrically with the oracle
  (`processTypeReferenceDirectiveWorker` @124513); `lib` references
  resolve against the vendored lib table — failures emit 2726/2727
  (`filePreprocessingLibreferenceDiagnostic` @125846).
- **Comment directives — `@ts-expect-error` / `@ts-ignore`**: the
  scanner collects them per file (impl-scanner row 14); after
  bind+check, the program layer filters diagnostics through them
  (`getMergedBindAndCheckDiagnostics` @123752): a diagnostic whose
  line is preceded by a directive is DROPPED; an `@ts-expect-error`
  that dropped nothing becomes 2578. Two design consequences:
  (a) this filter sits BETWEEN the checker and the diagnostic sink —
  the pipeline order in §1 gains a filter stage before the final
  sort; (b) band parity on any directive-bearing fixture depends on
  it for EVERY code, so it lands in phase 5 with the driver, not as
  tail work.

## 3. Checker initialization — `initializeTypeChecker` (88732)

The missing piece between "binder produced per-file tables" and
"checker resolves names": the GLOBAL environment. Port the sequence:

1. **Globals merging**: for every non-module SourceFile, merge its
   locals into the checker's `globals` table via `mergeSymbolTable`
   (47818) — merge conflicts here re-run the declareSymbol-style
   duplicate reporting across FILES (`mergeSymbol` — this is where
   `lib.d.ts` globals + user script globals meet; 2403-family
   redeclare checks against lib declarations depend on it).
   Module files do NOT contribute (their locals stay file-scoped);
   their augmentations (`declare global`) merge afterward, in file
   order.
2. **globalThis**: the synthetic `globalThisSymbol` whose exports
   ARE `globals`.
3. **Intrinsic environment**: `unknownSymbol` ("unknown" identifier
   resolution target), `errorType`s, `argumentsSymbol`; then the
   lazy accessors for required globals — `globalObjectType`,
   `globalFunctionType`, `globalArrayType`, `globalStringType/
   NumberType/BooleanType`, `globalRegExpType`, iterable/iterator
   types, `globalTemplateStringsArrayType`, Promise types. Each is
   `getGlobalType(name, arity, reportErrors)` — a globals lookup
   with 2318 (`Cannot_find_global_type_0`) / 2317 arity errors.
   These lookups are LAZY and memoized; eager lookup changes
   diagnostic order (observable — keep tsc's laziness).
4. **Augmentation application order**: global augmentations from
   modules apply after the base merge, in program file order — the
   order is observable through duplicate-member diagnostics.

Consequence for the phase plan: this section lands at the OPENING of
phase 5 (before resolveName can work at all) — impl-checker-2xxx §3
row 0. With full vendored libs (m0), `getGlobalType` failures are
real config errors, not gaps: there is no curated-lib axis by design
(greenfield §7.1).

## 4. Lib layering (restating the contract in one place)

`target`+`lib` options → lib file list via the ported
`libEntries`/`libMap` (36426/36542) + `getDefaultLibFileName`
(11255); files injected FIRST in layering order (`lib.es5.d.ts` →
`lib.es2015.core.d.ts` → ...). Lib files are ordinary SourceFiles:
parsed, bound, globals-merged like any script — no special casing
anywhere downstream (the performance mitigation is the per-lib-set
bind/check snapshot cache of greenfield §9, which is invisible to
semantics by construction: assert snapshot-vs-fresh equality in the
invariant suite, `matrix-independence` covers it).

## 5. What this doc deliberately leaves out

- Emit: never needed (2xxx-first-order.md).
- Watch/incremental/language-service surfaces: out of the batch
  goal, but a PLANNED future consumer — the architecture facts, the
  keep-the-door-open rules binding on phases 0-9, and the future
  L-track are fixed in
  [lsp-and-incremental.md](lsp-and-incremental.md).
- `node_modules`/package.json resolution, path mapping (`paths`),
  project references: the harness never produces them; if phase-9
  mining finds corpus fixtures exercising them, size first.
- JS-file checking (`allowJs`/`checkJs`, CommonJS indicators):
  band-adjacent codes exist but the corpus share sits behind
  `@allowJs` fixtures — phase 9 decision with data.
