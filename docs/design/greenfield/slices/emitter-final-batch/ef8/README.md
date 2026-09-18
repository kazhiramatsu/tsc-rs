# EF8 — output-axis coverage audit and A-CLOSE candidate skeleton

Evidence owner deliverable for child slice EF8 of
[H2.8a-A-RES-EMITTER-FINAL](../README.md#ef8--出力経路の仕上げとa-close候補).
Source/evidence audit only: no Rust, Node or TypeScript was executed, and no
count below is a Rust result. Start SHA `c35e00ccb` (worktree
`draft/h2-8a-emitter-final`); hosted mapping read from
[witness-coverage inventory v23](../../witness-coverage/inventory.v23.json)
(`source_commit f35766ce6`) plus a grep of `.github/ci/replay.py` and
`scripts/witness.py` at the start SHA.

Files in this directory:

| File | Role |
| --- | --- |
| [axis-matrix.py](axis-matrix.py) | stdlib generator: `--write`, `--check` (byte-exact), `--summary`; ~5 s niced |
| [axis-coverage.v1.json](axis-coverage.v1.json) | the matrix: per-source hosting, per-axis and pairwise cells, the requirement rows, the two checkpoints, the API-owned known ids |
| [api-boundary.md](api-boundary.md) | evidence that the disposed-print known 2 and the shared-node SUPER known 4 are API-only, and the counterexample search |

```sh
python3 docs/design/greenfield/slices/emitter-final-batch/ef8/axis-matrix.py --check
```

## 1. Inputs and what a "case" means

| Input class | Records | Route | Notes |
| --- | --- | --- | --- |
| `crates/compiler/tests/fixtures/*.json[.zst]` (207 files) + 5 emitter fixtures loaded by compiler suites | 21,197 | `complete-command` 17,905 (case has `typescript_observation.writes`, or bundle-sink `call.writes`); `inputs-only` 3,232 (`*-inputs.json`); `direct-or-api` 124; `unclassified` 60 (manifests, census) | options = explicit `options` merged over `config.compilerOptions` (options win, as in tsc) |
| `ratchets/h2-*-qualification.v1.json` (29 profiles) | 13,272 | `complete-command` 12,094 (5g/5h/6a/6b/6c/7b/7c carry writes); `settings-only` 1,142 (1a–5f, 7de) | settings = `input.settings` (`@option` list, case-insensitive names) + `option_facets` when present |
| `ratchets/h2-8a-candidates.v1.json` (769 output-only rows of 809) | 769 | `candidate-settings-only` | this artifact holds no `option`/`settings` field (README §3 assumed one); settings were joined by case id from the vendored suite expansions: 551 compiler/conformance `@option` rows, 30 virtual-config rows, 188 project rows (module variant only — other descriptor options are not joined) |

Cell notation below is `complete-command cases / of which hosted`. A case is
"hosted" when its source fixture reaches a PR-gate job: an unfiltered witness
target, a filtered target whose named filter covers the loading module, an
acceptance driver module, or a qualification profile in the acceptance
early/wide/late groups. Hosted via the `retained` selection means the job runs
that module's selection of the fixture (530 rows), not necessarily every case.

Value encodings verified against bytes: fixture `newLine: 0` materializes CRLF
and `newLine: 1` LF (decoded `output-directories.json` writes), so most compiler
control fixtures are CRLF-observed; `absent` is the observing host default (LF
on the POSIX oracle host). `module` 102 is `node20`.

## 2. Pairwise matrices

### module × target

| | es5 | es2015 | es2016 | es2017 | es2018 | es2019 | es2020 | es2021 | es2022 | es2023 | es2024 | esnext | absent |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `none` | 27/21 | 21/15 | - | - | - | - | - | - | 1/1 | - | - | - | - |
| `commonjs` | 884/235 | 2349/1474 | - | - | - | - | - | - | 170/118 | - | 1/1 | 356/56 | - |
| `amd` | 139/29 | 423/296 | - | - | - | - | - | - | 47/15 | - | - | 27/27 | - |
| `umd` | 68/10 | 109/39 | - | - | - | - | - | - | 5/1 | - | - | 13/13 | - |
| `system` | 126/12 | 231/137 | - | - | - | - | - | - | 27/17 | - | - | 22/22 | - |
| `es2015` | 33/19 | 75/61 | - | - | - | - | - | - | 1/1 | - | - | - | - |
| `es2020` | 8/2 | 13/7 | - | - | - | - | 2/2 | - | 1/1 | - | - | - | - |
| `es2022` | 6/0 | 8/2 | - | - | - | - | - | - | 3/3 | - | - | - | - |
| `esnext` | 474/168 | 1453/865 | 2/2 | - | - | - | 48/48 | - | 974/815 | - | - | 1108/696 | 1/1 |
| `node16` | 6/0 | 21/1 | - | - | - | - | - | - | 22/22 | - | - | - | - |
| `node18` | 6/0 | 21/1 | - | - | - | - | - | - | 22/22 | - | - | - | - |
| `node20` | 6/0 | 21/1 | - | - | - | - | - | - | 22/22 | - | - | - | - |
| `nodenext` | 7/1 | 41/21 | - | - | - | - | - | - | 42/22 | - | - | 110/14 | - |
| `preserve` | 6/0 | 16/10 | - | - | - | - | - | - | 1/1 | - | - | 11/11 | - |
| `absent` | 877/869 | 8666/8658 | - | 1/1 | - | 2/2 | 10/10 | - | 1/1 | - | - | 46/46 | 759/759 |

Reading: every module kind has complete-command observations at es5, es2015,
es2022 and (except `none`/`es2015`/`es2020`/`es2022`/node*) esnext. The
intermediate targets es2016–es2021 and es2023/es2024 are observed only in the
corpus profiles at `module=absent` (es2016 2, es2017 1, es2019 2, es2020 60,
es2024 1) and never in a module-explicit control; es2018, es2021 and es2023
have **no complete-command observation at all** (they exist only as settings
rows: h2-5d 24 / h2-5a 20 / candidates 2+1).

### layout × product

| | js | d.ts | js.map | d.ts.map | mjs | cjs | jsx | inline-source-map |
|---|---|---|---|---|---|---|---|---|
| `neither` | 12133/12012 | 2105/1986 | 905/905 | 32/32 | 43/43 | 38/38 | 203/203 | 6/6 |
| `outDir` | 6714/2934 | 6160/2810 | 6164/2889 | 4281/2768 | 71/0 | 78/0 | - | - |
| `outFile` | 249/222 | 219/215 | 184/179 | 148/144 | - | - | - | 4/4 |
| `outFile+outDir` | - | - | - | - | - | - | - | - |

`outFile+outDir` together is one candidate row (settings only, no
observation); `mjs`/`cjs` under `outDir` is only the unhosted
`output-root-format.json` (50 rows, module `nodenext`).

### emitBOM × newLine

| | absent (host LF) | crlf | lf |
|---|---|---|---|
| `absent` | 12617/12506 | 7206/3077 | 137/137 |
| `false` | - | 2/0 | 2/0 |
| `true` | 2/2 | 29/3 | 4/2 |

`emitBOM=true` sources: `output-filesystem.json` 24 (unhosted), `output-directories.json` 4
(unhosted), bundle/declaration-map fixtures 4 (hosted), `h2-5h-parameter-temporaries.json` 2 (hosted),
one h2-6c and one h2-7b profile row (hosted acceptance late). Explicit `newLine=lf` with
BOM: 4 rows (bundle-declarations, bundle-maps, declaration-maps, declaration-comment-ranges
= hosted 2). BOM materialization itself (`write_byte_order_mark=true` on the callback) is
observed on 35 complete commands, 7 hosted.

### removeComments × declaration

| | absent | false | true |
|---|---|---|---|
| `absent` | 11676/11004 | 62/55 | 5117/2237 |
| `false` | 38/33 | 66/64 | 2876/2297 |
| `true` | 51/18 | - | 110/16 |

`removeComments=true` with declarations: 110 complete commands, 16 hosted
(bundle-metadata-t1 18 hosted; the rest are `array-comment-publication.json`,
`output-directories.json`, `class-field-initializer-comments.json` — unhosted).

### layout × maps and host × layout

| layout | none | sourceMap | declarationMap | sourceMap+declarationMap | inlineSourceMap | inlineSourceMap+inlineSources | sourceMap+inlineSources |
|---|---|---|---|---|---|---|---|
| `neither` | 11761/11554 | 910/910 | 26/26 | 6/6 | 13/13 | 4/4 | 14/14 |
| `outDir` | 771/45 | 1855/66 | 16/0 | 4347/2840 | 1/1 | - | - |
| `outFile` | 60/38 | 24/23 | 6/6 | 167/163 | - | 2/2 | 2/2 |

| host | neither | outDir | outFile |
|---|---|---|---|
| `case-insensitive` | 4/3 | 1/1 | 1/1 |
| `case-sensitive` | 2/2 | 5/5 | 37/37 |
| `unspecified` | 12736/12530 | 6986/2948 | 227/200 |

Case-insensitive complete commands: h2-5g 1, h2-6c 2, h2-7b 2 (hosted
acceptance), `package-output-inputs.json` 1 (unhosted); plus 2 candidate rows.

## 3. Single-axis coverage and hosting

| Axis / value | complete / hosted | Sources (suite → job) |
| --- | --- | --- |
| product js / d.ts / js.map / d.ts.map | 19087/13320 · 8477/3165 · 7243/2124 · 4461/1105 | corpus profiles (acceptance), all compiler controls |
| product inline-source-map (payload observed) | 10/10 | h2-6a/6c profiles, `declaration-maps.json` (`declaration-maps` job) |
| `newLine=crlf` / `lf` | 7219/1217 · 59/55 | crlf: nearly every compiler control; lf: bundle-* (controls), `system-generated-names`/`module-alias-underscores` (`module-identities`), `output-directories` 4 (unhosted) |
| `emitBOM=true` | 35/7 | see §2 |
| `removeComments=true` | 161/34 | `array-comment-publication` 4 (unhosted), `bundle-metadata-t1` 18 (hosted), corpus rows |
| `declaration=true` / `declarationMap=true` | 8103/4550 · 4394/2870 | 7b/7c/7de acceptance late; declaration-* controls |
| `sourceMap=true` | 7031/3015 | 5g/6a/6b/6c/7b acceptance; `h2-6a-map-option-projection` (`map-option-projection`); `source_map_*_witness_contract` (ratchets/h2-6a/6b-witnesses — **no hosted filter**) |
| `inlineSourceMap` / `inlineSources` | 27/27 · 27/27 | h2-6a/6c profiles, declaration-maps fixtures |
| `mapRoot` / `sourceRoot` set | 45/45 · 44/44 | h2-6c profile (acceptance late), `h2-6a-map-option-projection` |
| `outFile` / `outDir` / `rootDir` / `declarationDir` set | 246/178 · 6692/969 · 640/94 · 190/37 | outDir/rootDir/declarationDir bulk = compiler controls (**unhosted**: `output-directories`, `output-roots`, `package-output-inputs`, `output-root-format`, `declaration-dir`) |
| roots: multiple / reversed order | 1610/1261 · 3/3 | reversed: `bundle-declarations.json` 4 rows (hosted `bundle-program`/`bundle-declarations`), library-order/post-t1 inputs |
| unicode: non-ASCII path / non-BMP text / `\u` escapes | 11/3 · 32/14 · 508/274 | non-ASCII path: `output-directories` 8 (unhosted), declaration-maps 2 (hosted), 6a/6c 2; non-BMP: utf16 targets (hosted controls) |
| host case-insensitive | 6/5 | see §2 |
| outcome: emit-refused / emit-skipped(noEmitOnError) | 280/147 · 327/194 | 6c/7b/5g profiles, `output-directories`, `output-roots` |
| outcome: fs faults first-write / create-directory / all-writes | 6/0 each | `output-filesystem.json` only (**unhosted**) |
| outcome: sink on-error / throw / skip-unchanged | 1/1 · 4/4 · 4/4 | `bundle-sinks.json` (`controls/bundle-sinks`); `declaration-map-apis.json` 54 API-route rows carry the same rules but are API-owned |
| write metadata: callback sourceFiles / data metadata / BOM materialized | 19700/15525 · 9836/5661 · 35/7 | every complete-command fixture compares the callback tuple |
| listEmittedFiles status lines | 3,215 status-writes cells (see JSON `outcome.status-writes`) | bundle fixtures, `output-directories`, `system-dynamic-imports`, 7b/7c profiles |
| generated-name collisions (curated suites) | 11 suites | `import-helpers` (**unhosted**), `alias-conflict-display` (unhosted), `transformed-class-assigned-names` (unhosted), `system-generated-names`/`module-alias-underscores`/`bundle-module-identities` (`module-identities`), decorator-binding (hosted), auto-accessor storage names (unhosted) |

### Hosted-job mapping of the output-axis suites

| Module / target | Fixtures | Job (inventory v23) |
| --- | --- | --- |
| `contracts::h2_8a_output_directories` | output-directories, module-export-identifiers, system-dynamic-imports | **none** (no filter in witness.py; not an acceptance driver) |
| `contracts::h2_8a_output_filesystem` | output-filesystem | **none** |
| `contracts::h2_8a_output_roots` | output-roots | **none** |
| `contracts::h2_8a_package_output_inputs` | package-output-inputs, output-root-format | **none** |
| `contracts::emit_session_contract`, `cli_contract`, `source_map_*` | inline programs / ratchets/h2-6a,6b-witnesses | **none** (the `declaration-map-cli` job runs `h2_7e_original_corpus`, not `cli_contract`) |
| `h2_7d_bundle_sinks` | bundle-sinks | controls/bundle-sinks |
| `h2_6a_map_option_projection` | h2-6a-map-option-projection | controls/map-option-projection |
| `h2_7d_*`, `h2_7e_*` | bundle-*, declaration-map* | controls/bundle-*, declaration-maps/* |
| `h2_8b_*` (config/library) | h2-8b-* | controls/config-library (24 filters) |
| `h2_8a_retained_accessor_owners` | class-field-alias-map-positions et al. (selection) | retained/retained |
| qualification profiles 1a–5f / 5g / 5h,6a,6b,6c,7b,7c,7de | — | acceptance early / wide / late |
| `h2_8a_original_corpus` (769 global), `emitter_final_batch` (class 40 / global 14), `emitter_final_rows` | ratchets | **none** — integrator-hosted full replay |

The compiler `contracts` target reaches hosted CI only through the 24
`h2_8b_*` config-library filters and the one retained filter; the 70 other
`h2_8a_*` integration modules (all the unhosted fixtures listed by
`--summary`) run only in a local unfiltered `cargo test -p tsc-rs-compiler --test contracts`.

## 4. Not covered by any current control (precise)

| # | Axis value | Product | Observation missing | Evidence |
| --- | --- | --- | --- | --- |
| U1 | `target=es2018`, `es2021`, `es2023` (any module) | js | no complete-command tuple; only `@target` settings rows (h2-5d 24, h2-5a 20, candidates 3) whose profiles record no writes | `requirements[]` status `uncovered` |
| U2 | `target` ∈ es2016–es2021, es2023, es2024 with an explicit `module` | js (+ d.ts) | module-explicit controls exist only at es5/es2015/es2022/esnext; the corpus rows are `module=absent` | module × target matrix |
| U3 | `emitBOM=true` × `d.ts.map` and × `js.map` with `newLine=crlf` | js.map, d.ts.map | BOM+CRLF observed only on js/d.ts (`output-*` fixtures have no maps); BOM+maps observed only at `newLine=lf` (bundle-maps 1, declaration-maps 1) | `emitBOM_x_newLine`, `layout_x_product` |
| U4 | `removeComments=true` × `outFile` × declarations | d.ts, d.ts.map | 110 removeComments+declaration rows are all `outDir`/`neither` except bundle-metadata-t1's ES2015 outFile rows (JS + d.ts, no d.ts.map with removeComments) | `removeComments_x_declaration` sources |
| U5 | `outFile+outDir` both set | all | 1 candidate settings row, no observation (TS reports TS5053-style option conflict; the refusal path is unobserved) | `layout` axis |
| U6 | reversed root order × `outDir` (multi-file, non-bundle) | js, d.ts | the 3 observed reversed-order rows are all `outFile` bundles (`bundle-declarations.json`) | `roots` axis sources |
| U7 | non-BMP / `\u{…}` escaped **export names** flowing into `d.ts` and `d.ts.map` under `outDir` | d.ts, d.ts.map | non-BMP text rows (32) are JS-only utf16 targets; declaration products with non-BMP identifiers appear only in `hoisted-declaration-export-ranges.json` (escaped class/export names, JS+d.ts, no map) | `unicode` × `product` |
| U8 | case-insensitive host × `declarationDir` collision (d.ts overwrite of an input on a case-folding host) | d.ts | the 6 case-insensitive complete rows are single-product or bundle; no declaration-collision tuple | `host_x_layout`, `output_plan_contract::declaration_collision_preflight_*` is an emitter unit control, not a complete command |
| U9 | fs faults (first-write / create-directory / all-writes) on **d.ts / js.map / d.ts.map** products | d.ts, maps | `output-filesystem.json` faults are JS-only (`emitBOM`, no declaration/map); the sink-rule bundle controls cover onError/throw/skip for all four products but only for the `outFile` bundle | `outcome` axis |
| U10 | `noEmitOnError=true` with `outFile` + maps (emitSkipped with partial bundle products) | js.map, d.ts.map | emit-skipped rows (327) are `outDir`/`neither` | `outcome` × `layout` (JSON) |

Covered but **unhosted** (needs a job, not an input): fs faults (U9's JS half),
`importHelpers=true` (98 rows, `import-helpers.json`), every `output-*` /
`package-output-inputs` / `output-root-format` row, `emit_session_contract`
(44 tests incl. the 5 per-module `filesystem_failure_at_each_write_index`
controls and `h2_3d_json_text_paths_bom_newlines`), `cli_contract` (29 tests
incl. BOM/noEmitOnError/TS5033 continuation), `source_map_*_witness_contract`.

## 5. Bounded proposal: minimal additional inputs

No cross product. Each row is one small fixture family with two fresh TS
observations per row, following the `output-directories.json` shape
(`options`, `files`, `roots`, `typescript_observation.writes[]` with
`write_byte_order_mark`, `status_writes`, `emit_result`, `exit_code`).

| ID | Inputs (rows) | Closes | Why this and not more |
| --- | --- | --- | --- |
| P1 | one 2-file program × target {es2018, es2021, es2023} × module {commonjs, esnext} = 6 rows, JS + d.ts | U1, U2 | the three targets with zero tuples plus the two module families that select different transformer stacks; es2016/2017/2019/2020/2024 already have corpus tuples |
| P2 | `output-directories` clone with `sourceMap+declaration+declarationMap` × `emitBOM=true` × newLine {crlf, lf} = 4 rows | U3, U9 (BOM half) | BOM on map products is a printer/writer question independent of directory layout; two newline values × one BOM value suffice |
| P3 | `bundle-metadata-t1`-style outFile program × `removeComments=true` × `declarationMap=true` = 2 rows (es2015, es2022) | U4 | declaration-map segments are the only product where comment removal changes mappings; outFile is the one layout not yet paired |
| P4 | 1 row `outFile`+`outDir` both set, 1 row `noEmitOnError` + outFile + maps with a type error | U5, U10 | refusal/skip paths are single observations each |
| P5 | `output-roots` clone: 3 files, `roots` reversed, `outDir` + `declaration` = 2 rows (with/without `rootDir`) | U6 | root order affects `program_source_order`, callback order and common-source-directory inference only under outDir |
| P6 | `hoisted-declaration-export-ranges` subset re-observed with `declarationMap=true`, `outDir`, 4 rows (escaped class/export names, one non-BMP) | U7 | reuses existing escaped-name inputs; adds the missing product |
| P7 | `package-output-inputs` clone with `use_case_sensitive_file_names=false` and a `Types/`-vs-`types/` declarationDir collision, 2 rows | U8 | one positive (collision → TS5055/TS5056) and one negative (distinct on a case-sensitive host) |
| P8 | `output-filesystem` clone with `declaration+sourceMap`, faults {first-write, create-directory} = 4 rows (2 directories) | U9 | keeps the 24-row family's operation-trace comparator; adds the multi-product write sequence |

Total: 25 rows. Hosting: register one `output-matrix` controls entry running
`--test contracts h2_8a_output_ h2_8a_package_output_inputs h2_8a_import_helpers`
plus the new family (budget: the four modules are 236 cases ≈ 212 s after
build per [h2-8a-filesystem.md](../../h2-8a-filesystem.md)), and add
`emit_session_contract`/`cli_contract`/`source_map_*` to the same entry or to
`declaration-map-cli` (the CLI wrapper already builds the compiler).

## 6. A-CLOSE candidate table (skeleton)

Target rows are the two historical denominators; every row carries
`case ID + input hash + options/profile + observed products + version + owner`.

| Section | Denominator / rows | Source of truth | Status field |
| --- | --- | --- | --- |
| A. Existing positive protection | global **769** (A6-37: 755 exact twice, 188 project exact) + class **1228** (A6-34: 1100 exact twice; bands: alias-map 384, hoisted-export 168, helper-accessor 144, promoted-export 144, one-sided 128, assigned-names 108, header-token 88, initializer-comments 64) + compiler controls (17,905 complete commands, 12,094 profile commands) | `ratchets/h2-8a-global-after-a6-37.v1.json`, `ratchets/h2-8a-class-convergence-after-a6-34.v1.json`, `axis-coverage.v1.json` `sources` | exact-twice at final HEAD / regressed / not re-run |
| B. New repairs (EF1–EF7) | EF1 5 commands (retired known), EF2 12 IDs, EF3 8 IDs, EF4/5 40 commands (128 class failures at A6-34 → re-measured), EF6 14 IDs (global failures list in JSON `checkpoints.global.failure_case_ids`), EF7 ledger additions | `../DESIGN.md` §4–§9, `../records/` | before → after, per-cause commit |
| C. Unresolved (ordinary emit) | rows still unequal after B, plus this audit's U1–U10 once observed | REPORT §7 + P1–P8 observations | producer named / owner named / runner missing |
| D. Other-product boundary | API re-emit: disposed print typed known 2 (`decorator-binding-direct/lifecycle/dispose/{numbered,file-level}#direct#after_dispose`); custom-transformer shared-node SUPER known 4 (`decorator-super-direct/{es2015,es2022}/{set,define}/shared-node-used-twice`); adjacent API routes: `declaration-map-apis.json` 54, `h2_8c_transpile` known-native/known-open, printer direct targets | [api-boundary.md](api-boundary.md) | API owner + evidence that no Program command reaches the shape |
| E. Upstream exceptions | the 2 upstream exceptions retained by the primary witness job (witness-testing.md "primary 670 exact × 2 plus two upstream exceptions"); `target=es2025`-style invalid settings rows (1) | witness job receipts, h2-5g profile | upstream ref + frozen expected |
| F. Hosted plan | per-suite job mapping (§3) + the missing `output-matrix` entry (§5) + integrator full replays: global 769 (`h2_8a_original_corpus original_output_matrix_candidates`, ~30 min at A6-37 = 1781 s) and class 1228 (A6-34 run 1485 s for 4 targets) — two jobs ≤ 45 min each, 2 workers | witness-testing.md budgets | job / measured seconds |

Rules carried from the handoff: B never re-implements PR #555-integrated
repairs; C keeps every unresolved ordinary-emit row visible (no "later"
erasure); D is admitted only with the boundary evidence; A's counts are not
updated by focused runs (`h2-8.md` §"global769 and class1228 remain frozen").

## 7. Findings for the integrator (doc-level)

1. `emitter-architecture.md` §4 still labels `E-MAPS` and `E-OUTPUT-FUTURE`
   `dormant` (audit 2026-08-14) although H2.6a/7d/7e activated maps and
   multi-product write ordering (bundle-sinks callback JS map → JS → DTS map →
   DTS is a frozen control). A-CLOSE should carry the requalification rows.
2. `h2-8a-candidates.v1.json` has no per-case settings; option axes for the
   769 come from the vendored expansions (this script's join) and, for the 188
   project rows, only the module variant. The full-replay comparator already
   consumes the pinned project mount, so this is a reporting gap, not a
   replay gap.
3. Unreferenced fixtures (never loaded by a test at the start SHA) are listed
   in the JSON `summary.compiler_fixtures_unreferenced_by_any_test`
   (`decorator-lexical-prologue-readers.json` v1 and the `*-inputs.json`
   companions read by observers only).
