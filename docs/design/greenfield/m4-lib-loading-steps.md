# M4 insert: lib loading — steps

Parent: m4-checker-skeleton-steps.md "LIB-LOADING DECISION POINT"
(raised by 5.4's FP burn-down; this doc IS that decision, executed).
Prerequisite: 5.4 gate green (@1d32c0a). Scope: program plumbing only —
no checker-semantics changes; every rate movement comes from names and
members RESOLVING that previously unwound Unsupported or hit the
lib_globals gate.

## 1. The oracle contract (pinned facts)

Everything below is what the goldens were ALREADY generated under —
lib loading makes our side match a contract that has been in force
since M1. No golden refresh is needed or wanted.

- The harness computes `ProgramJson.libs` per program:
  `resolve_program_libs` (crates/harness/src/lib.rs) takes the explicit
  `@lib` list or the target's default lib file
  (`default_lib_file_name`; note tsc's naming quirk: target es2015 →
  `lib.es6.d.ts`), expands `/// <reference lib>` transitively
  (`expand_lib_files`), and sorts by the SAME priority function as
  tsc's `getDefaultLibFilePriority` (_tsc.js 123124-123138:
  lib.d.ts/lib.es6.d.ts first, then `libs`-array index order).
- The oracle host (crates/oracle/program-host.mjs) sets
  **`noLib: true`** (`compilerOptionsFromProgram` tail) and passes the
  expanded libs as ORDINARY ROOT NAMES ahead of the fixture files
  (`createProgramFromJsonPath` rootNames = libs ++ files).
  Consequences, each verified in _tsc.js:
  - `processLibReferenceDirectives` is gated on `!options.noLib`
    (124408-124410) — **`<reference lib>` directives are inert
    program-wide**, in lib files and fixture files alike. The
    default-lib bucket + `compareDefaultLibFiles` sort (122874,
    123121) therefore never receives a file; every file is an
    ordinary root (`isSourceFileDefaultLibrary` false for all).
  - `processReferencedFiles` (`/// <reference path>`) stays live
    (gated on `!options.noResolve`, unset) — pre-existing fixture
    behavior, unaffected by this work.
  - grep shows NO `options.noLib` reads in the checker region of
    _tsc.js (46000-91000) — noLib is program-layer only; the checker
    behaves identically given the same file set.
- **Empirical order pin** (scratchpad files-dump.mjs over the oracle's
  own program construction, three lib sets: target=es2015 default,
  es5+dom explicit, no-target default): `program.getSourceFiles()`
  order == `ProgramJson.libs` order ++ fixture files, exactly. The
  engine consumes `libs` as given and never re-derives lib order.
- The oracle driver (driver.mjs) collects
  syntactic/semantic/suggestion diagnostics for `programJson.files`
  ONLY. tsc checks files lazily per `getDiagnostics(file)` call, so in
  the oracle's world **lib files are never checked** and diagnostics
  FILED under a lib file (the lib-side span of a duplicate-identifier
  pair, for example) never surface. Two engine-side exclusion rules
  follow: skip lib files in the check driver loop, and drop
  lib-file-anchored diagnostics at assembly (the same contract shape
  as 5.4's file-less exclusion, one comment site next to it).
- With libs present the init-band missing-global 2318s vanish
  entirely; the 5.4 file-less exclusion stays as the guard for
  genuinely-libless programs (unit probes, CLI without libs).

## 2. Decisions

- D1 API: `check_program_with_libs(libs: &[InputFile], files:
  &[InputFile], options) -> CheckResult`, with `check_program(files,
  options)` delegating with empty libs (no caller breakage;
  core-interfaces §9 gains the variant). Libs are ordinary files
  prepended to the program: same parse pass (`.d.ts` rides the
  existing TS path), same bind pass, same globals merge in program
  order — fixture declarations MERGE with lib declarations (interface
  augmentation, var conflicts) through the existing M2/5.0 machinery.
  The driver loop and BOTH diagnostic sinks cover fixture files only.
- D2 Ownership: `ProgramBinder` moves from `Vec<Binder<'a>>` to
  borrowed `Vec<&'a Binder<'a>>`. Binders are read-only after
  `bind_source_file` (M2 design), so sharing is sound; call sites
  (lib.rs, test_support, relpin) build owned binders locally and pass
  refs; cached lib binders arrive as `&'static`.
- D3 Cache: a process-lifetime bundle per lib set —
  `LibBundle { sources: Vec<SourceFile>, binders: Vec<Binder<'static>> }`
  built once and `Box::leak`ed (self-reference resolved by leaking
  sources first; no unsafe, no self-referential crate). EXACTNESS is
  architectural, not approximate: libs are the program PREFIX, so for
  a fixed lib list every lib file's NodeId/NodeArrayId/SymbolId bases
  are identical across programs — the cached arenas ARE the arenas an
  uncached run would build (L3 gate proves bytes-equal output).
  Key: ordered lib names + text hashes + the CompilerOptions fields
  the BINDER reads (start with the whole `CompilerOptions` — it is
  small and `Eq`; narrow to the proven-read subset only if the matrix
  multiplies cache entries measurably). Fixture files keep per-call
  parse/bind with bases continuing from the cached prefix.
- D4 Retirements and keepers among 5.4's FP gates:
  - RETIRE `lib_globals.rs` whole (names resolve now) and the
    "suggested-lib branch currently dead" note —
    `getSuggestedLibForNonExistentName` goes LIVE for real: under a
    restricted `@lib` set, a later-lib name misses resolution and the
    2583/2584 args come from the static feature table, matching tsc.
  - KEEP the all-meanings re-probe gate (alias resolution still
    unported, 5.8) and the declare-global gate (augmentation binding
    still unported).
- D5 Non-goals: skipLibCheck/skipDefaultLibCheck modeling (irrelevant
  under the per-fixture-file collection contract), `<reference lib>`
  processing (inert by contract), tsc's default-lib bucket ordering
  (unreachable under noLib), golden refresh (oracle unchanged).

## 3. Stage L1: lib corpus gate [M]

Prove parse+bind exactness over the vendor lib files BEFORE any
conformance wiring — dom.d.ts (~28k lines) is new coverage; M1/M2
gates ran the fixture corpus only.

- `cargo xtask lib-gate` (new arm): for every `lib.*.d.ts` under
  vendor/typescript-6.0.3/lib — parse diagnostics EMPTY, `ast-diff`
  zero against the oracle dump; then for every DISTINCT lib set in
  the conformance corpus (collect via harness expansion over all
  fixtures) — `symbol-diff` zero over a libs-plus-empty-fixture
  program, and the files-dump order probe asserting getSourceFiles ==
  libs ++ files.
- Fix any parser/binder divergence found, each with its own pin/test
  (expect JSDoc-free ambient .d.ts to be well-trodden; the risk is
  scanner/trivia edge cases at dom scale).

Gate: lib-gate green over all lib files + all corpus lib sets.
Commit: `m4 lib-loading L1: lib corpus parse/bind gate`.

## 4. Stage L2: plumbing, no cache [M]

- D1 API + program assembly in checker/src/lib.rs: libs parse+bind as
  leading files; driver loop and diagnostic assembly skip lib files
  (name-set based, alongside the file-less exclusion comment); the
  lib_globals gate stays in place THIS stage (retired at L4 after
  measurement).
- Conformance wiring: `current_tsrs_diagnostics` and the
  prefix-conformance loop read lib texts from `vendor_lib_dir` per
  `ProgramJson.libs` (the parameter current_tsrs_diagnostics already
  receives and ignores) and call `check_program_with_libs`. The
  truncation suite truncates fixture files only, libs pass whole.
- Unit tests (real `lib.es5.d.ts` as the lib input): a lib global
  resolves (`Date`), a fixture-declared `interface Array<T>` MERGES
  with the lib's (member from both sides visible), `@lib`-restricted
  2583 fires with the tsc arg (`Map` under es5-only → `'es2015'`),
  and the 5.4 driver tests stay green libless.

Gate: workspace tests green; `cargo xtask conformance --limit 200`
FP=0 (expect new true matches; the full run waits for L3 — the
no-cache path re-parses libs per case and is deliberately not run
over 7691 cases).
Commit: `m4 lib-loading L2: program plumbing + lib-backed resolution`.

## 5. Stage L3: the per-lib-set cache [M]

- D2 borrow refactor + D3 leaked bundle cache behind
  `check_program_with_libs`.
- Exactness A/B gate: over a sample (≥200 fixtures), cached vs
  uncached outputs BYTE-IDENTICAL (CheckResult debug serialization or
  the conformance JSON rows).
- Measure and record in the commit body: full-run wall time vs the
  5.4 baseline, resident memory, distinct-lib-set count. Fallbacks if
  budget blows (list, do not build speculatively): per-(file, base)
  source sharing across sets; post-lib globals-merge snapshot per lib
  set (the per-program merge of ~2-3k lib globals is the next cost
  tier).

Gate: A/B bytes equal; full `cargo xtask conformance` completes
within budget (target ≤3× the 5.4 wall time).
Commit: `m4 lib-loading L3: per-lib-set parse/bind cache`.

## 6. Stage L4: full measurement + gate retirement [M]

- MERGED-SYMBOL CHASE AUDIT (from the L2 find): tsc's
  getSymbolOfDeclaration (49936) is getMergedSymbol(node.symbol) — two
  of our ports read the raw binder symbol and broke the moment lib
  interfaces merged (appendTypeParameters minted `Promise<T, T>`).
  Audit every `node_symbol(` consumer in the checker crate for
  declaration-identity reads that must chase the merge, fixing each
  with a pin.
- Full conformance across all three bands; burn FPs to zero. Expected
  classes, in likely order: merge-band duplicates (fixture top-level
  names vs lib `declare var`s — tsc emits these too, so the work is
  span/code parity, not suppression), display-slice vocabulary (2344
  args now render lib types — ADD the named-object arm
  (interface/class symbol name) to type_to_string_slice; anything
  further stays Err→FN), ordering ties surfaced by richer member
  tables.
- RETIRE lib_globals.rs + its gate + tests; update the 5.4 ledger
  notes that reference it; the 2583 suggested-lib branch note flips
  to live.
- Ratchet bump to the measured rates (expect a MATERIAL jump: the
  2xxx band's name/member resolution stops FN-ing on lib types);
  ledger check; invariants idempotence; relpin unchanged (probes stay
  libless by design).
- Docs: as-landed notes here; memory update; NOTES entry for 5.5
  ("expression checking now lands against real lib types — the
  `"x".length` chain resolves end-to-end").

Gate: FP=0 all bands, ratchets bumped, tests/relpin/ledger/invariants
green.
Commit: `m4 lib-loading L4: measurement + lib_globals retirement`.

AS-LANDED NOTES (2026-07-12, L1-L4):
- L1 @b4813c3: lib-gate green FIRST RUN — 108 lib files, parse and
  bind byte-exact (dom.d.ts included), 39 distinct corpus lib sets,
  order contract holds for all. M1/M2 needed zero fixes.
- L2 @e43073c: one REAL bug — tsc's getSymbolOfDeclaration is
  getMergedSymbol(node.symbol) (49936); raw node_symbol reads broke
  merged lib interfaces (appendTypeParameters minted `Promise<T, T>`
  → spurious 2314 on every Promise reference).
- L3 @418f3f5: the borrowed-binder tripwire (symbol_mut refusing
  file-owned ids) caught BOTH remaining post-bind mutations at
  compile/test time: recordMergedSymbol's merged_into stamp on SOURCE
  symbols (moved to a per-checker mergedSymbols map — tsc's own shape;
  the binder Symbol field was the deviation and is gone) and relpin's
  probe shim (now clones into a transient twin). A/B cache-off vs on:
  identical. Full run 260s / 3.59GB RSS.
- L4: the merged-chase audit found TEN raw-read sites; the eight
  where tsc chases (resolveNameHelper's module-exports 19586 / enum
  19609 / interface-members 19636 / container 19660 / grandparent
  19679, getOuterTypeParameters' mapped-tp + thisType arms, getThisType
  parent, alias host 62913, enum members 57448) now chase; the raw
  ones (fn/class-expression self-names 19652/19710, infer-arm
  typeParameter.symbol, signature param.symbol) stay raw like tsc.
  type_to_string_slice gained the named-object arm (lib types in 2344
  args). lib_globals.rs retired; the libless failure band now emits
  the 2583/2584 family exactly like tsc-under-noLib (test flipped to
  live pins). The predicted merge-band duplicate FP wave DID NOT
  MATERIALIZE — FP=0 held through the whole insert; rates moved only
  modestly at the 5.4 emission surface (all 6.2483→6.2566%, 2xxx
  2.1083→2.1276%) because the surface is still type-parameter-band —
  the payoff lands with 5.5's expression forcing.

## Expected failure modes

| Symptom | Diagnosis | Fix |
|---|---|---|
| dom.d.ts ast-diff noise at L1 | scanner/trivia edge at scale | fix in syntax crate with a pin, NOT a lib-side patch |
| duplicate-identifier FPs at L4 | merge parity gap fixture-vs-lib | compare against oracle spans; the M2 merge machinery owns it |
| cache A/B divergence at L3 | option-dependent bind escaped the key | widen key to full CompilerOptions (the documented conservative default) |
| memory blow-up at L3 | too many distinct lib sets | per-(file, base) sharing fallback |
| rate DROPS anywhere | lib types flowing into arms that assumed emptyObjectType fallbacks | those arms' Unsupported escapes fire (FN, honest); investigate only FPs |
