# EF7 ledger — PLAN-BASE memberships × current evidence, per observation unit

2026-09-17. Evidence owner for child slice EF7 of [the emitter-final handoff](../README.md).
Start SHA `c35e00ccb006e3b4e3e2643491e8fe3207595097` (branch `draft/h2-8a-emitter-final`).
**This is a ledger, not a qualification: no native, oracle, Rust or Cargo execution was
performed. Every number below was read from a pinned record. Counts are not additive
across profiles and must never be summed into a "remaining bugs" figure.**

```sh
python3 docs/design/greenfield/slices/emitter-final-batch/ledger/build_ledger.py --check   # byte-compare, ~2 s
python3 docs/design/greenfield/slices/emitter-final-batch/ledger/build_ledger.py --write   # regenerate ledger.v1.json
```

## 1. Method

- **Unit.** One observation unit is `case ID + input identity + profile/options + observation
  kind + TypeScript 6.0.3 + owner`. A ledger row groups the kinds of one (case, profile) that
  share a state (`k` lists them); the summary counts both rows ("pairs") and kinds ("units").
  Kinds: `js`, `dts`, `map`, `dtsmap`, `buildinfo`, `other`, `direct`, `diag`, `result`, `write`,
  and `*` when the profile recorded no observation for that case.
- **Denominator.** All 6,045 PLAN-BASE IDs / 9,004 memberships
  ([plan-base/inventory.v1.json](../../plan-base/inventory.v1.json), base `f9ef828a5`) plus the
  handoff's 5 EF1 IDs (`origin: handoff-EF1`). Global IDs that are exact-only and were never
  seeded into PLAN-BASE (12,290 − overlap) are outside the ledger; one H2.7d/e exact-only ID is
  counted as `exact_ids_outside_plan_base_denominator`.
- **Inputs.** 67 blobs read with `git show c35e00ccb:<path>` and pinned by SHA-256 in
  `ledger.v1.json#inputs`: the 29 frozen `ratchets/h2-*-qualification.v1.json`, the three known
  manifests, `h2-8a-candidates/observations`, `h2-8a-global-after-a6-37`, `h2-8a-class-convergence-
  after-a6-34`, `h2-8a-convergence-causes`, `h2-7de-observations`, `h2-candidate-dispositions`
  (the 15,642 universe), the class/parameter/post-T1/direct fixtures, the `h2_8c_transpile` inputs,
  the PR-gate entry ledger `witness-coverage/inventory.v23.json`, and the POST-T1 hosted receipt
  with its 14 gzipped job logs (each gz hash verified against the receipt).
- **Current record.** PR #555's candidate `2883e3c79` (tested checkout `5ae9a7a72`, identical tree
  `2281920b`, merged as `ce39261ac`). The builder asserts both are ancestors of the start SHA and
  that `git diff --name-only 2883e3c79 c35e00ccb` contains documentation paths only (34 files), so
  every non-docs source at the start SHA is what that hosted run executed. All 14 checks succeeded.
- **Reconciliation rules.**
  1. Profiles H2.1a–H2.7b: the hosted line `H2.x emit acceptance: candidates=.. exact=.. known_diverging=.. deferred=..`
     is asserted against the frozen artifact (`candidates == cases`, `exact == admitted − known`,
     `deferred == deferred rows`). Every admitted, non-known case is then **CE** for the kinds its
     `typescript_observation` records; this is a count-level proof (the runner fails otherwise), the
     same rule PLAN-BASE used, not a per-ID log line.
  2. H2.7c / H2.7d/e / retained / pipeline / controls / primary: per-ID `corpus PASS`, `EXACT x2`,
     `KNOWN x2`, `DIVERGENCE RECORDED x2`, `UPSTREAM EXCEPTION` lines are parsed and matched by ID.
  3. Known manifests: **RD** for the whole case (`kd` names the diverging kinds); `emit_refused`
     rows are **UP** with the refused option. For `sourceMapWithNonCaseSensitiveFileNames` the frozen
     manifest says `outFile` while the hosted `H2.6c refused_option totals` line names
     `useCaseSensitiveFileNames`; the row records both.
  4. Profile-deferred rows (`rust_expectation: typed-failure-before-first-sink-write`) are **UP**
     with the deferring owner; `x` lists profiles where the same ID is CE (informational only —
     different input identity, no credit transferred).
  5. Records without a runner at the current head (global 769 matrix, 40 class rows, later
     references) are **RP** when an older record exists (`prior` = exact | repaired | failed) or
     **RM** when no observation/runner exists at all. Rows with only a local Cargo entry carry
     `hosted_entry: false`.
  6. `transpile:` IDs, the SUPER direct shared-node rows and the two typed `InvalidLifecycle`
     dispose-print rows are **OB** with the schedule row (`boundary`: H2.8c / API1.2).

## 2. State vocabulary

| code | state | meaning in this ledger |
| --- | --- | --- |
| CE | current-exact | a hosted record at `2883e3c79` proves the unit exact (rule 1 or 2) |
| RP | remeasure-pending-same-observation | an older record (exact / repaired / failed) exists for the same observation; nothing runs it at the current head |
| RD | reproduced-divergence | the current hosted record reproduces a difference (known manifests, POST-T1 known 5, H2.1a diagnostic controls) |
| UP | unsupported-producer | Rust refuses by contract (typed refusal or profile deferral with `typed-failure-before-first-sink-write`) |
| RM | runner-missing | no entry runs the observation (count-only later references, universe rows never selected by any profile) |
| OB | other-product-boundary | transpile API, custom-transform-only direct rows, dispose-after print; cited schedule row |
| UX | upstream-exception | TypeScript itself throws on both passes; no native credit |

## 3. Per-profile summary (pairs = case×profile rows; units = observation kinds)

| profile | CE | RD | UP | RM | RP | OB | UX | notes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| H2.1a | – | 5 diag units (controls) | 49 (196u) | – | – | – | – | 246 exact hosted; only the 5 controls' `diag` kind is RD |
| H2.1b / 1c / 1d | – | – | 5 / 2 / 1 | – | – | – | – | all PLAN-BASE rows here are profile deferrals |
| H2.1e | 3 (12u) | – | 2 (8u) | – | – | – | – | |
| H2.2a / 2b | 6 / 15 | – | 5 / 3 | – | – | – | – | |
| H2.2c / 2d / 3c | 6 / 9 / 4 | – | – | – | – | – | – | |
| H2.3b / 4a | – | – | 4 / 1 | – | – | – | – | |
| H2.4b | 5 (20u) | – | 2 (8u) | – | – | – | – | |
| H2.5a / 5b | 31 / 5 | – | 5 / 4 | – | – | – | – | |
| H2.5c / 5d / 5e | – / 1 / 1 | – | 1 / 1 / 1 | – | – | – | – | |
| H2.5g | 153 (612u) | – | 516 (2,064u) | – | – | – | – | 510 → H2.9, 6 → H2.8a; 8,511 exact hosted |
| H2.5h | 149 (592u) | 12 (48u) | 44 (176u) | – | – | – | – | EF2 = the 12 RD rows |
| H2.6a | 173 (865u) | 1 (5u) | 2 (10u) | – | – | – | – | |
| H2.6b | 6 (24u) | – | – | – | – | – | – | |
| H2.6c | 630 (3,474u) | 6 (31u) | 4 (20u) | – | – | 2 (6u) | – | UP = 2 typed refusals + 2 map deferrals; EF3 = 6 RD + 2 UP |
| H2.7b | 1,506 (7,526u) | – | 36 (`*`) | – | – | – | – | 26 → H2.7c (all 26 CE in H2.7c), 10 → H2.9 |
| H2.7c | 32 (140u) | – | – | 10 (`*`) | – | – | – | the 10 retained holds; 2 are CE in H2.7de |
| H2.7d/e | 313 (1,558u) | – | – | 9 (37u) | – | 2 (`*`) | – | 314 hosted exact (1 outside the denominator); 11 later references |
| H2.8a-global (769+40) | – | – | – | 40 (172u) | 769 (3,780u) | – | – | prior: exact 755, repaired 7, failed 7; **no hosted entry** (v23) |
| fixture: class-field-alias-map-positions | 52 (364u) | – | – | – | 28 (196u) | – | – | EF4 = the 28 RP (C2 24, C3 4), `hosted_entry: false` |
| fixture: class-helper-accessor-producers | 36 (244u) | – | – | – | – | – | – | |
| fixture: class-header-token / hoisted-declaration-export-ranges | – | – | – | – | 8 / 4 | – | – | EF5 = 12 RP (C4 8, C5 4), local-only entry |
| fixture: h2-5h-parameter-temporaries | 5 (21u) | – | – | – | – | – | – | PLAN-BASE strict failures closed by A-PC1 (PR #538); unfiltered hosted target |
| fixture: post-t1-residuals (EF1, extra) | – | 5 (35u) | – | – | – | – | – | `KNOWN x2` in the pipeline job |
| direct: decorator-super-direct | – | – | – | – | – | 4 | – | API1.2 boundary; divergence recorded x2 |
| witness: primary / retained | – | – | – | – | – | – | 2 / 2 | |
| universe: H2.0a | – | – | – | 2,034 (`*`) | – | 33 (`*`) | – | 217 "no product record" + 1,817 PLAN-BASE IDs never observed by any profile + 33 transpile |

Totals for orientation only (not a bug count): 6,806 rows, 24,875 units; CE 15,764 / RP 4,040 /
UP 2,650 / RM 2,253 / RD 119 / OB 45 / UX 4 units. ID-level rollup (informational): 2,414 IDs have
every observed unit CE; 232 are CE in one profile and open elsewhere; 3,399 have no CE unit.
Profiles H2.3a, H2.3d, H2.5f contribute no PLAN-BASE rows.

## 4. Deferred sets reconciled by ID (`summary.deferred_sets`)

| set | state in that profile | same ID CE elsewhere | disposition |
| --- | --- | ---: | --- |
| H2.7c retained holds (10) | RM ×10 (no observation in 7c) | 2 (`declarationDir3` amd/commonjs → H2.7de) | 6 nodeNext self-name/declarationDir + 2 project declarationDir2 rows are RP in H2.8a-global (owner H2.8a/H2.8b); `declarationEmitToDeclarationDirWithCompositeOption` has no observation anywhere (H2.8b) |
| H2.7d/e later references (11) | RM ×9, OB ×2 | 0 | 9 have TypeScript observations, no Rust runner: outFile/out+noEmit ×2 and JS type assertions/index constraint (H2.9); importHelpers+outFile ×2, incrementalOut, composite JS (H2.8b; the two `buildinfo` writes touch BLD1.2); `sourceMapWithNonCaseSensitiveFileNames` (UP in H2.6c) |
| H2.5g deferred to H2.8a (6) | UP ×6 | 0 | 3 also in the H2.8a candidate band as RP/RM rows; jsx/parser real-world rows |
| H2.5g deferred to H2.9 (510) | UP ×510 | 0 | reason not itemized in any record beyond `owner_reachability` (`transform-es2016` etc.) — see proposal EF7-h |
| H2.5h deferred (44) | UP ×44 | 0 | same |
| H2.6a deferred (2) / H2.6c deferred (4) | UP ×2 / UP ×2 + OB ×2 | 0 | `unicodeEscapesInNames02` es5/es2015 maps (H2.9); the two transpile rows are H2.8c API rows |
| H2.6c typed refusals (2) | UP ×2 | 0 | isolatedModules (H2.8c-MOD1), useCaseSensitiveFileNames (H2.8b-SYS1) |
| H2.7b deferred (36) | UP ×36 | 26 | the 26 H2.7c-owned rows are CE in H2.7c; 10 → H2.9 |

## 5. `runner-missing` / `remeasure-pending` units by likely Rust owner (`summary.open_units_by_likely_rust_owner`)

| bucket | RM | RP | what the records say |
| --- | ---: | ---: | --- |
| (none — exact history) | – | 3,715 (H2.8a-global) | 755 IDs EXACT x2 at `d1c04df5c`; `h2_8a_original_corpus` is `no-direct-target-command` in v23 → OPS entry, not a producer |
| transforms | 924 (universe) + 33 (8a later refs) | 196 (class C2/C3) + 4 (global G8a) | noEmitHelpers/importHelpers/experimentalDecorators/jsx rows never selected; class field alias/this/nested computed name; JSX `UnknownNode` |
| options | 976 (universe) + 24 (7de) + 8 (7c) + 18 (8a) | 56 (global G2/G4/G5/G6/G7 rows; prior repaired 7 / failed 7 … see §6) | noEmit route rows (diag/result-only observations), allowJs, outFile/outDir/declarationDir/composite |
| printer | – | 40 (class-header-token C4) + 24 (hoisted C5) | export/default comment & token ownership; escaped class/export names and ranges |
| module | 110 (universe) | – | verbatimModuleSyntax / isolatedModules / export=,import= / module=ES2022,ES2015,ES2020 at target=ESNext never selected |
| host | 121 (8a later refs H2.8a,H2.9) + 13 (7de H2.9) | – | JS inputs, case profiles, project mounts |
| declaration | 24 (universe) + 2 (7c) | 5 (global G5d) | declaration/emitDeclarationOnly rejected rows; declarationDir3 project rows |
| transpile-api (OB) | 33 + 2 + 2 | – | all 37 `transpile:` IDs are listed in the H2.8c `transpile-routes` inputs (API observation, separate runner) |

## 6. EF-group cross reference

- **EF1** (5): RD in `fixture:post-t1-residuals`, owner E-COMMENT-SCOPE-H (printer).
- **EF2** (12): RD in H2.5h, `kd=writes:1`, owner `h2-5h-ca-2a-r4` (transforms).
- **EF3** (8, 1 shared with H2.6a): 6 RD (maps; `kd` per row) + 2 UP typed refusals.
- **EF4** (28) / **EF5** (12): RP `prior=failed` at `b6621f25a`, fixture bytes unchanged, `hosted_entry: false`;
  the 88 same-fixture repairs are CE via the retained job.
- **EF6** (14) in `H2.8a-global`: RP `prior=repaired` ×7 (G2 ×2, G4a, G4b, G5c, G5d, G7 — receipts are
  older heads; the hosted `require-rewrite`, `declaration-specifiers`, `declaration-comments`,
  `jsdoc-return` suites do not contain the original IDs and `require_rewrite_original_complete_commands`
  is not a selected filter) and RP `prior=failed` ×7 (G8a, G5b ×2, G5a ×2, G6, G8b with the recorded errors).
- **EF7 217**: all RM (`universe:H2.0a`). Blockers: `route:noEmit=true` 106 (expected observation
  = diag/result/empty write set only), `rejected-option:noEmitHelpers` 89, `allowJs` 15,
  `verbatimModuleSyntax` 5, `jsx` 3, `isolatedModules` 2, `export=/import=` 1, and 15 rows whose only
  blocker is `module=ES2022/ES2015/ES2020` (never selected by H2.1a). Buckets: options 102,
  transforms 92, module 23. Owners (last required slice): H2.9 106, H2.8b 89, H2.8c 7, H2.1a 15.
  Suites: conformance 156, compiler 61.
- **PLAN-BASE IDs never observed by any profile (1,850, `origin: plan-base-unobserved`)**: RM 1,817 +
  OB 33 (transpile). Blockers as above at scale (noEmit 737, noEmitHelpers ≈ 500, allowJs 421,
  importHelpers 35, experimentalDecorators 44, isolatedModules 31); owners H2.9 943 / H2.8b 778 / H2.8c 129.

## 7. Proposed additional child slices (bounded; from records only)

| id | unit | observation | suspected owner | suggested focused entry |
| --- | --- | --- | --- | --- |
| EF7-a global matrix hosted entry | 769 IDs of `h2-8a-observations` (755 RP-exact, 7 RP-repaired, 7 RP-failed) | js/dts/map/diag/result/write | integrator (OPS-COVER); Rust n/a | register `h2_8a_original_corpus::original_output_matrix_candidates_match_complete_production_commands` as its own witness job (historical wall 1,782 s > 45-min review margin → split by suite) |
| EF7-b class 40 selector | EF4 28 + EF5 12 | js/dts/map/dtsmap/diag/result/write | transforms (C2/C3), printer (C4/C5) | thin selector over the three `h2_8a_class_*` integration tests sharing the fixture comparator; hosted filter in `compiler/contracts` |
| EF7-c EF6 originals to hosted | 7 RP-repaired IDs | complete commands | require-rewrite / declaration / object-property owners | select `require_rewrite_original_complete_commands`; add the original IDs to the specifiers/comment-ranges/jsdoc-return/object-property fixtures |
| EF7-d global 7 failures | G8a, G5b ×2, G5a ×2, G6, G8b | js/dts writes; G8a `UnknownNode`, G8b `UnsupportedCompilerOption(allowImportingTsExtensions)` | transforms (jsx), declaration (JSDoc/anonymous export class), options | same entry as EF7-a, focused on the 7 IDs first |
| EF7-e later references | 7c 10 + 7de 9 + 8a 40 | outDir/declarationDir/composite/outFile+noEmit/importHelpers+outFile/JS inputs | options/host (H2.8a/H2.8b/H2.9); `buildinfo` writes → BLD1.2 | promote count-only references in `h2_7c_acceptance`/`h2_7de_acceptance` to compared rows once each producer or typed refusal exists |
| EF7-f typed refusals | isolatedModules, useCaseSensitiveFileNames | whole command | H2.8c-MOD1, H2.8b-SYS1 | EF3 |
| EF7-g universe rows | 217 + 1,817 | noEmit route: diag/result only; others: full command | options (noEmit, allowJs), transforms (noEmitHelpers/importHelpers/jsx/experimentalDecorators), module (verbatim/isolated/ES20xx module selection) | a no-emit command runner (diagnostics + exit) and per-option admission children; first a read-only census of the blockers per option |
| EF7-h 5g/5h H2.9 deferrals | 510 + 44 UP rows | js/diag/result/write | transforms (`owner_reachability`) | read-only census of the per-row refusal reason (parser depth / syntax boundary / option) before any implementation |

### 7.1 Status of the proposals at r9 (2026-09-18)

| id | status |
| --- | --- |
| EF7-a | integrator (hosted job); the local entry `h2_8a_original_corpus` is unchanged |
| EF7-b | done — `crates/compiler/tests/emitter_final_batch.rs` (class 40 selector, 40/40) |
| EF7-c | integrator (hosted selection); the 7 repaired originals replay exact in `emitter_final_batch` (global 14 = 14/14) |
| EF7-d | done — global 14 = 14/14 (EF6-UMD-FACTORY, EF6-JSDOC-LINK, EF6-IMPORT-TYPE-SELF) |
| EF7-e | open (options/host owners H2.8a/H2.8b/H2.9; `buildinfo` → BLD1.2); count-only references unchanged |
| EF7-f | done — isolatedModules and verbatimModuleSyntax refusals removed (EF3-ISOLATED, EF7-VERBATIM-GATE); useCaseSensitiveFileNames → 方針 1 (integrator re-mint) |
| EF7-g | **done** — the 217 rows and the 1,798 compiler/conformance PLAN-BASE rows are minted (`scripts/observe-emitter-final-universe.mjs`) and replayed (`emitter_final_universe`); at the r11 bytes 217 = 216 exact / 1 KNOWN (H2.9), 1,798 = 1,731 exact / 67 KNOWN (H2.9 35, resolution 4, H2.5h 1, checker 27; REPORT §9.8); the noEmit route is the CLI's `load_program` branch inside the replay; per-row states are in [DELTA-r9.md](DELTA-r9.md) (8 project rows and 11 build-info rows are recorded as skipped with reasons in `ef7/universe-plan-base.v1.json`) |
| EF7-h | open (read-only census still pending; the 5g/5h deferrals were not re-observed in this batch) |

## 8. Not reconciled / caveats

- Rule 1 is count-level for H2.1a–H2.7b (no per-ID exact lines exist in those logs).
- The `diag` RD for the five H2.1a diagnostic controls comes from the frozen `diagnostic_disposition`
  (H2.0b checker divergence), not from a fresh comparison.
- The 12,290 exact-only global IDs outside PLAN-BASE are not rows; H2.3a/3d/5f have no PLAN-BASE rows.
- `x` (same ID CE in another profile) never changes a state; inputs differ per profile.
- The handoff inventory's EF1..EF8 groupings are cited, not re-derived; T1/POST-T1 pipeline 767
  and the 2 dispose-print `InvalidLifecycle` rows are outside the PLAN-BASE denominator (gaps.md).
- Nothing here is a completion claim for the emitter, H2.8 admission or TS7 compatibility.
