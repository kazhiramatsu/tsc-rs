# EF7 producer / option / runner gaps (one line each, with the record that names it)

Format: gap — evidence — state in ledger — likely owner. No line is a runtime result of this slice.

## Typed refusals and typed failures actually recorded
- `isolatedModules` typed refusal — `ratchets/h2-6c-known-divergences.v1.json` (`emit_refused`, `refused_option`), hosted `H2.6c refused_option totals: {"isolatedModules": 1, ...}` at 2883e3c79 — UP (H2.6c) — H2.8c-MOD1 / module.
- `useCaseSensitiveFileNames` typed refusal — same manifest row `sourceMapWithNonCaseSensitiveFileNames` (frozen: `outFile`), hosted totals name `useCaseSensitiveFileNames`; PLAN-BASE `current_refused_option` — UP (H2.6c), RM (H2.7de reference) — H2.8b-SYS1 / host.
- `rootDir` typed refusal — `ratchets/h2-7c-qualification.v1.json` (`rust_expected_unsupported_option`) — now CE (hosted `H2.7c corpus: 32 exact (1 H2.8a rootDir migration)`) — closed; keep as history.
- `UnsupportedCompilerOption { option: allowImportingTsExtensions }` — `ratchets/h2-8a-global-after-a6-37.v1.json` failures (G8b `bundlerImportTsExtensions`) — RP prior=failed (H2.8a-global) — options.
- `Emit(Transform(UnknownNode(...)))` on JSX — same checkpoint (G8a `reactImportDropped`) — RP prior=failed — transforms (jsx).
- Profile deferrals with `rust_expectation: typed-failure-before-first-sink-write` — 688 (case, profile) pairs in `ratchets/h2-*-qualification.v1.json` (510 H2.5g→H2.9, 44 H2.5h→H2.9, 36 H2.7b, 49 H2.1a, …) — UP — owner in `w`; reasons beyond `owner_reachability` are not itemized in any record.

## Runners missing at the current head (v23 entry ledger `witness-coverage/inventory.v23.json`)
- Global output matrix (769 IDs) — `tsc-rs-compiler/h2_8a_original_corpus` is `no-direct-target-command`; last record `d1c04df5c` (755 exact / 14 failed) — RP ×769 — OPS entry.
- Class fixtures outside the retained set — `tsc-rs-compiler/contracts` hosted filters are only `config-library` (24) + `retained` (1); `h2_8a_class_field_alias_map_positions` / `class_header_token` / `hoisted_declaration_export_ranges` run locally only — RP ×40 (`hosted_entry: false`) — transforms / printer.
- EF6 original commands — `require_rewrite_original_complete_commands` is not among the 4 selected filters (log: `10 filtered out`); the specifiers / comment-ranges / jsdoc-return / object-property hosted fixtures do not contain the 7 original IDs — RP prior=repaired ×7 — declaration / module / printer owners per receipt.
- H2.7c count-only later intersections — `ratchets/h2-7c-qualification.v1.json` (10 `deferred` rows without observation) — RM ×10 — H2.8a / H2.8b / H2.7d.
- H2.7d/e later references — `ratchets/h2-7de-qualification.v1.json` (9 observed `deferred` + 2 transpile) — RM ×9, OB ×2 — H2.8b / H2.9 / H2.8c.
- H2.8a later intersections — `ratchets/h2-8a-candidates.v1.json` (40: H2.9 29, H2.8c 8, H2.8b 3) — RM ×40 — host / transforms / options.
- 217 universe IDs — `ratchets/h2-candidate-dispositions.v1.json` `profile_blockers`; no profile artifact lists them — RM ×217.
- 1,817 further PLAN-BASE IDs whose memberships are all frozen-future-deferred / outside-band — same source — RM.

## Option / route paths with no current record (counts from `profile_blockers`; 217 → universe-wide PLAN-BASE-unobserved)
- `route:noEmit=true` (expected observation: diagnostics + exit + empty write set) — 106 → 737 more — H2.9 (no no-emit command runner exists in the emit profiles).
- `rejected-option:noEmitHelpers` — 89 → ≈500 more — H2.8b / transforms (helper emission).
- `rejected-option:allowJs` — 15 → 421 more — H2.3a-adjacent JS source admission / options.
- `rejected-option:importHelpers` — 0 → 35 — H2.8b / transforms.
- `rejected-option:experimentalDecorators` — 0 → 44 — transforms (legacy decorators at CommonJS).
- `rejected-option:jsx` — 3 (+ `modulePreserve3`) — transforms (jsx).
- `rejected-option:verbatimModuleSyntax` — 5 — H2.8c / module.
- `rejected-option:isolatedModules` — 2 → 31 more — H2.8c-MOD1 / module.
- `rejected-feature:export-equals` / `import-equals` (`modulePreserve1`) — 1 — H2.2d / module.
- `required-option:module=ES2022(7) | ES2015(5) | ES2020(6)` at target=ESNext with no other blocker — 15 — H2.1a selection never admitted these module values (never observed).
- `outFile`/`out` + `noEmit` (`compilerOptionsOutAndNoEmit`, `compilerOptionsOutFileAndNoEmit`) — 7de references — H2.9.
- `importHelpers` + `outFile` (`importHelpersOutFile` es5/es2015) — 7de references — H2.8b.
- `composite` / `incremental` `build-info` writes (`incrementalOut`, `jsFileCompilationWithEnabledCompositeOption`) — 7de references; `buildinfo` kind crosses BLD1.2 (`post-h1-completion-slices.md` §5.2) while js/dts stay ordinary emit — H2.8b + BLD1.
- `declarationDir` / `outDir` / `composite` / nodeNext package self-name (`nodeNextPackageSelfNameWithOutDirDeclDir*`, `declarationEmitToDeclarationDirWithCompositeOption`, project `declarationDir2`) — 7c holds — H2.8a / H2.8b.
- JS-input rows (`checkIndexConstraintOfJavascriptClassExpression`, `jsFileCompilationTypeAssertions`) — 7de references — H2.9 / host.
- `unicodeEscapesInNames02` es5/es2015 source maps — H2.6a/6c deferred rows — H2.9 / maps.

## Reproduced divergences (current hosted record)
- ES5 lowering writes (12) — `ratchets/h2-5h-known-divergences.v1.json`, hosted `known_diverging=12` — RD — EF2 / transforms.
- Map / diagnostic / emit-result rows (6 unique + 1 shared) — `h2-6a/6c-known-divergences.v1.json`, hosted `known_diverging=1/8` — RD — EF3 / maps.
- Bound decorator target trailing comment + map (5) — pipeline job `post t1 residuals KNOWN x2` — RD — EF1 / E-COMMENT-SCOPE-H.
- H2.1a diagnostic controls (5): checker inference divergence vs the H2.0b base — `ratchets/h2-1a-qualification.v1.json` `diagnostic_disposition` — RD (`diag` only) — checker / H2.9.

## Explicit other-product boundaries
- `transpile:` IDs (37 in PLAN-BASE) — H2.8c row (`post-h1-completion-slices.md` §4.5); all 37 are listed as `inventory_case` in `crates/compiler/tests/fixtures/h2_8c_transpile/inputs.v1.json` (`transpile-routes`, controls job; `jsWithSourceMapBasic` / `jsWithInlineSourceMapBasic` also appear in `known-open`/`known-native`) — OB.
- SUPER direct shared-node rows (4) — controls job `DIVERGENCE RECORDED x2 ... (memoized required-value lowering; no credit)` — API1.2 — OB.
- Dispose-after-print typed `InvalidLifecycle` (2) — printer job `decorator binding direct SUMMARY exact=202 compared=204 mismatching=2` (outside the PLAN-BASE denominator) — API1.2.

## Upstream exceptions (no native credit)
- `decorator-super/esnext/set/handoff/static-{private,public}-accessor` — primary job — UX.
- `decorator-static-accessor-handoff/esnext/esnext/set/{class,nested-class}-static-accessor` — retained job — UX.
- `decorator-binding/computed/esnext/set/static-accessor-decorated` — pipeline job (outside the PLAN-BASE denominator).
