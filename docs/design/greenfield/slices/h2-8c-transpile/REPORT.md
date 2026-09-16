# H2.8c 隔離 prototype：段階 B ＋ 段階 C producer の結果報告

受領時の報告。統合担当の再現・修正・最終検証は [INTEGRATION.md](INTEGRATION.md) を参照。

作成日：2026-09-16（同日、段階 C producer まで続行）。base `526c2b37ad9d0ff7e932e1d76e658b8cf139807f`（origin/main、PR #527 含む）。
設計は [DESIGN.md](DESIGN.md)。本報告は H2.8c の runtime activation を報告するものではない（§5）。

## 1. 提出物

| 種別 | path |
| --- | --- |
| source oracle | `scripts/observe-transpile-routes.mjs`（`--write-inputs` で manifest 固定、`--inputs/--out` で採取） |
| 入力 manifest / 期待値 / 台帳 | `crates/compiler/tests/fixtures/h2_8c_transpile/{inputs,expected,known-open}.v1.json`（287 case、期待値は 2 回採取して byte 一致） |
| native contract | `crates/compiler/tests/transpile_routes_contract.rs`（5 tests） |
| emitter | `crates/emitter/src/route.rs`（新）、`execute.rs`（route-aware admission、markLinkedReferences hook、allowNonTsExtensions の source family）、`host.rs`（`emit_route`）、`plan.rs`（suppressOutputPathCheck、`getOutputExtension` の最終 `.js` arm）、`resolver.rs`（`mark_linked_references`）、`external_module_names.rs`（`try_rename_external_module`）、`builtins.rs` / `builtins/system.rs`（rename の接続 3 箇所） |
| checker | `modules.rs`（`mark_linked_references_unspecified`、`mark_async_function_alias_referenced`）、`check.rs`（`calculate_node_check_flag_worker`、`checked_source_files`）、`emit.rs`（resolver 実装、lazy `has_node_check_flag`）、`links.rs`（`calculated_flags`）、`lib.rs`（`InputFile` の API facts、parse 時の適用、allowNonTsExtensions の loop 条件）、可視性調整 |
| syntax | `SourceFile.renamed_dependencies`（新 field） |
| program / types | `CompilerOptions.allow_non_ts_extensions`（internal）、loader の root / extensionless admission、`PreparedProgramBuilder` の extensionless 完全一致、`compiler_option_named_choices` |
| compiler | `src/transpile.rs`（`transpile_module` / `transpile_declaration` / `fixup_compiler_options`）、`lib.rs`（`ProgramSession::with_emit_route` / `with_source_api_facts`、`SourceApiFacts`、forced-declaration command、`checked_source_files` 公開） |
| hosted / local 入口 | `scripts/witness.py`（suite `transpile-routes`）、`.github/ci/replay.py`（controls job に登録、所有 path 規則）、`.github/ci/test_replay.py`、`docs/witness-testing.md` |
| 実行記録 | `target/h2-8c/native-run{1..7}.log`、`target/h2-8c/native-<route>.json`（行ごとの diff・evidence）、`target/h2-8c/expected.run2.json`（2 回目採取） |

focused 入口（0 tests 不可、5 tests / 287 case）:

```sh
taskpolicy -b nice -n 15 python3 scripts/witness.py transpile-routes --all
# 同等: taskpolicy -b nice -n 15 env CARGO_BUILD_JOBS=2 cargo test -p tsc-rs-compiler --test transpile_routes_contract -- --test-threads=1
```

## 2. before / after

| route | case | run1（prototype 接続直後） | run4（段階 B 最終） | run7（段階 C producer 後、最終 bytes） | known-open |
| --- | --- | --- | --- | --- | --- |
| transpile-js | 135 → 150 | 114 exact / 21 open | 115 exact / 20 known-open | **138 exact** | 12 |
| transpile-dts | 80 → 86 | 79 / 1 | 79 / 1 | **86 exact** | 0 |
| program-no-check | 45 → 51 | 40 / 5 | 41 / 10 | **41 exact** | 10（checked control 4 含む） |
| 合計 | 287 | | | **265 exact** | 22 |

段階 C で閉じた行（run4 の known-open から retire、全て exact）：`module-name/*` 3、`renamed/*` 2、`jsdoc/parse-none`、`filename/nonstandard-extension` ×2（js/dts）、`options/jsx-react-jsxdev-import-source`。
段階 C で追加した 21 行（extensionless、`.txt` の source map、moduleName の UMD / import 付き / 空文字 / pragma 上書き、renamed の AMD / System / UMD / dynamic import / export-from / type-only、jsDocParsingMode の JS d.ts ParseNone vs ParseAll 等）は全て exact。

「exact」は公開観測の完全一致（transpile: outputText の UTF-16 単位数・UTF-8 bytes・診断全構造・sourceMapText の有無と値・例外の有無。program: writes/reported/status/exit/emit result）。例外行（TS の `Debug.fail`）は例外の有無で比較し、message は記録のみ。

## 3. known-open 22 行の内訳

| 種別 | 行数 | 内容 | owner |
| --- | --- | --- | --- |
| inherited-emitter | 14 | ES5 async `super` の `_super_1` accessor、ES2015 `arguments_1` の位置、ES5+commonjs+importHelpers の export rest destructuring、namespace 内 `export import` の source map。**同じ入力を通常 checked route（`control-checked/*`）で走らせて native bytes が noCheck route と一致することを確認**（H2.8c 由来ではない） | emitter |
| inherited-h2.9 | 7 | parse 診断付き emit の recovery（`ParseDiagnosticsDeferred`）。`text/unicode` は TS 自身が 9 件の構文診断を出す入力 | H2.9 |
| rust-unsupported | 1 | enum 範囲外 `target=1234`：TS6046 は同一に生成するが、TS は 1234 のまま emit し Rust の emit admission は typed refusal | emitter admission（fixture/比較式は変えない） |

Rust-only の typed error は互換性に加点していない（`kind` で区別、`native-*.json` の `evidence.kind` に `rust-*`）。

### 3.2 checked control（inherited 判定の根拠）

`control-checked/{es5-flags, es2015-flags, import-helpers, namespace-source-map}`（`noCheck:false`、通常 Program route）は TS 側では noCheck と同じ出力であり、Rust 側でも noCheck route と **byte 同一**の差分を出す（`checked_source_files` は 19〜20、noCheck 側は 0）。
したがってこれら 14 行は開始 SHA の emitter に既にある差で、専用 route の未完成ではない。

## 4. 専用 route の証拠（「全型検査を走らせて診断だけ捨てる」ではないこと）

`no_check_routes_run_no_source_checking` test が全行で検証:

| 観測 | transpile-js | transpile-dts | program-no-check |
| --- | --- | --- | --- |
| `checked_source_files == 0`（`checkSourceFileWorker` 本体が走った file 数） | 全成功行 | 86/86 | noCheck 41/41 |
| checked control の `checked_source_files` | — | — | 83/83/84/83/1/19/20/19/20（`control-checked/*`、`*-control-checked`、`ts-nocheck-directive`） |
| `parsed_documents` | 1（input のみ） | 2（input + barebones lib） | — |
| semantic evidence（getter、noCheck 下） | 全行空 | 全行空 | 全行空（control は非空） |
| global evidence | TS2318（noLib）— TS と同じく残る | 0 | `no-lib/*` で TS2318×10、TS と一致 |
| forced option | `noCheck=Some(true)`、`noResolve=Some(true)`、`isolatedModules=Some(true)`（verbatim 時除く）、`allowNonTsExtensions=Some(true)` | 同左＋`declaration/emitDeclarationOnly/isolatedDeclarations` | route admission のみ |

noCheck 下でも維持される型計算（source と同じ）：`markLinkedReferences` の alias 解決（`import-elision/*` 7 行 exact、`renamed/type-only-elided` exact）、`getConstantValue` の enum 評価（`enum/*` exact）、`getTypeReferenceSerializationKind`（`decorators/metadata-cross-file` exact）、`calculateNodeCheckFlagWorker` の lazy flag（`flags/es2017-static-block` exact、ES5/ES2015 行は inherited 差）、declaration の `collectLinkedAliases`（`declaration/declaration-error-blocks-dts` exact）、`getExternalModuleFileFromDeclaration`（`module-name/amd-with-imports` exact）。

性能は機能一致後に別記する（本報告では未計測。計測は同一入力・同一優先度で `checked_source_files`/work counters と wall を並記する）。

## 5. 未接続・未完了（H2.8c 完了とはしない）

1. Program `noCheck` の CLI/config 接続（runtime activation）：依頼資料の指示により未接続。`ProgramSession::with_emit_route` は adapter/harness からのみ。root の option admission・CLI・profile は変更していない。
2. builder/incremental の `checkPending` 系（対象外）。
3. inherited 14 行の emitter 修復と H2.9 recovery 7 行は別 owner。
4. `target` 範囲外値の emit admission（1 行、typed refusal のまま）。
5. hosted：`transpile-routes` suite を controls job に登録済み（§7）。**hosted 実行はまだ無い**（PR head の run URL は統合担当が記録する）。全 witness / emitter 全 suite の replay は hosted に委ねる。

## 6. 段階 C producer の設計要点

- **caller spelling**：`SourceApiFacts::file_name` が checker-edge の `InputFile` 名と metadata 名を caller の綴り（`module.tsx`、`src/deep/a.ts`）にする。parsed `SourceFile.file_name` がその綴りになるので jsxDEV の `_jsxFileName`、診断の file 名が TS と一致。Program identity・resolution row・出力 path は prepared spelling（`/module.tsx`）のまま。checked host は override 名で document を引く。
- **moduleName**：parse 直後に `SourceFile.module_name` を上書き（pragma より API が勝つ：`module-name/pragma-overridden` exact）。空文字は falsy として無視（`empty-string-ignored` exact）。
- **renamedDependencies**：`SourceFile.renamed_dependencies` を新設し、`tryRenameExternalModule` を resolved-file 名の後・clone の前に置く（CJS/AMD/UMD の `getExternalModuleNameLiteral`、System の dependency group、dynamic import）。ESM 出力と d.ts は TS と同じく rename しない（`renamed/esnext`、`renamed/dts-ignored` exact）。
- **jsDocParsingMode**：`InputFile::with_js_doc_parsing_mode` → その file の `ParseOptions`。JS 入力の d.ts で ParseNone（`a: 1` / `f(s: any): any`）と ParseAll（`a: number` / `f(s: string): string`）を区別して一致。
- **allowNonTsExtensions**：`CompilerOptions.allow_non_ts_extensions`（internal、tsconfig からは設定不可）。loader の root admission（124176）と extensionless 完全一致（124200-124205、probe なし）、`PreparedProgramBuilder` の root↔source 一致、checker の program-file loop、emitter の source-family admission と `getOutputExtension` の最終 `.js` arm（`module.txt` → `module.txt.js`、`module` → `module.js`）。通常 Program（option 未設定）の挙動は不変。

## 7. 隣接 regression の実行記録（最終 bytes、`taskpolicy -b nice -n 15`、`CARGO_BUILD_JOBS=2`）

| 集合 | 結果 | log |
| --- | --- | --- |
| `cargo test -p tsc-rs-compiler --test transpile_routes_contract -- --test-threads=1` | **5/5 green**（run7、最終 bytes、287 case） | `target/h2-8c/native-run7.log` |
| `cargo test -p tsc-rs-syntax --lib` | **175 green** | `target/h2-8c/adjacent-syntax.log` |
| `cargo test -p tsc-rs-program`（unit 47、contracts 481 + 5 ignored、他 6 target） | **全 green** | `target/h2-8c/adjacent-program.log` |
| `cargo test -p tsc-rs-emitter --test contracts --test emit_pipeline_phases_contract --test decorator_super_direct_contract` | **452 + 1 + 1 green** | `target/h2-8c/adjacent-emitter2.log` |
| `cargo test -p tsc-rs-checker --lib` | **1738 green** | `target/h2-8c/adjacent-checker.log` |
| `cargo test -p tsc-rs-compiler --test contracts` | 414 passed / **17 failed** / 16 ignored。17 件は段階 B 時点と同一集合で、開始 SHA `526c2b37a` の無改変 worktree でも 17/17 FAILED（新規 0、回復 0） | `target/h2-8c/adjacent-compiler2.log`, `adjacent-compiler-base-526c2b37a.log` |
| `cargo test -p tsc-rs-compiler --test h2_7d_bundle_program --test h2_7d_bundle_sinks --no-fail-fast` | bundle_program 3/4（`bundle_later_owner_references_remain_separate` は開始 SHA でも同じ行で FAILED：inherited）、bundle_sinks **2/2 green** | `target/h2-8c/adjacent-h27d.log`, `adjacent-h27d-base-526c2b37a.log` |
| `cargo clippy -p tsc-rs-{syntax,program,emitter,checker,compiler} --lib --test transpile_routes_contract` | exit 0。変更 hunk 内・新規 file に警告 0（既存警告は base 由来の `result_large_err` / `useless_conversion`） | `target/h2-8c/clippy3.log` |
| `cargo fmt --all -- --check`、`git diff --check`、`python3 .github/ci/test_replay.py` | clean / 18 tests OK | — |

注記：段階 B の報告で「h2_7d の 2 target green」と書いた点は誤りだった。`cargo test` は最初に失敗した target で止まるため、当時の chain では h2_7d 2 target は実行されていない。本節で `--no-fail-fast` により個別に実行し、上記のとおり記録した。
`h2_8a_original_corpus`（769 command 級の重い replay）はローカルでは実行していない（heavy replay は hosted の方針）。

17 件の inherited 失敗の内訳（base で同一に再現）：`emit_session_contract` 2（旧 outFile/outDir refusal 期待）、`h2_7a_m4_controls` 3（bundle-root refusal の文字列 pin）、H2.8a/8b の complete-command 8（`ParseDiagnosticsDeferred` 等）、`program_session_contract` 1、`source_map_emit_witness_contract` 2。
本 branch の変更による新規の退行は 0。

## 8. 提出状態

- commit は未作成（user が commit / PR / hosted acceptance を所有）。変更 ~30 file + 新規 8（`git status` 参照）。
- hosted：`transpile-routes` suite は controls job に登録済み（planner の所有 path 規則、`test_replay.py` の件数 287 を更新）。PR 作成後の witness run URL と結果は統合担当が記録する。
