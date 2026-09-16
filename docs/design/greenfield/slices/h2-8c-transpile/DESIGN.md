# H2.8c — noCheck / transpile 専用パイプライン：依存設計・source oracle・隔離 prototype

作成日：2026-09-16。状態：段階 A（inventory / 設計）＋ 段階 B（隔離 prototype）。
依頼：[h2-8c-transpile-claude-handoff.md](../h2-8c-transpile-claude-handoff.md)、
共通手順：[claude-high-difficulty-handoffs.md](../claude-high-difficulty-handoffs.md)。
本書は runtime activation を許可する ready packet ではない。段階 C の引き継ぎ項目は §9。

## 1. 開始点と保存物

| 項目 | 値 |
| --- | --- |
| worktree / branch | `~/dev/tsc-rs-transpile` / `draft/h2-8c-transpile` |
| 開始 SHA | `526c2b37ad9d0ff7e932e1d76e658b8cf139807f`（origin/main、PR #527 マージ後。最初に切った `938dec454` には commit なし、`reset --hard` で付け替え） |
| toolchain | rustc 1.93.0、cargo 1.93.0、node v25.2.1 |
| `_tsc.js` / `typescript.js` SHA-256 | `1c59e77a…ddd3e3` / `56917765…12be39`（共通手順の pin と一致、observer が起動時に検証） |
| 開始 manifest | `target/h2-8c/start.txt` |
| 入力 manifest | `crates/compiler/tests/fixtures/h2_8c_transpile/inputs.v1.json`（287 case = 初版 260 ＋ run2 の checked control 6 ＋ 段階 C producer の 21。各版とも native 実行前に固定し期待値を 2 回採取） |
| 期待値 | `crates/compiler/tests/fixtures/h2_8c_transpile/expected.v1.json`（2 回採取して byte 一致を確認） |
| observer | `scripts/observe-transpile-routes.mjs` |
| native contract | `crates/compiler/tests/transpile_routes_contract.rs` |
| known-open 台帳 | `crates/compiler/tests/fixtures/h2_8c_transpile/known-open.v1.json` |
| native 証拠 | `target/h2-8c/native-<route>.json`（test が書く） |

## 2. Source の 3 経路：呼出 graph

### 2.1 共通の事実（噛み合わせの鍵）

pinned 6.0.3 では 3 経路が **同じ `emitFiles` worker** に到達し、違いは
(a) Program の構成（host / root / lib）、(b) option の強制、(c) 戻り値の形、(d) 診断の選択、の 4 点に限られる。

`transpileWorker`（typescript.js:146022-146133）が強制する option は
`transpileOptionValueCompilerOptions`（_tsc.js:37935 = `transpileOptionValue` を持つ宣言）:

| 強制値 | option |
| --- | --- |
| `true` | `noCheck`、`isolatedModules`（`verbatimModuleSyntax` 指定時は据え置き）、`noLib`（JS route のみ。declaration route は `false`）、`noResolve` |
| `undefined` | `incremental`、`declaration`、`emitDeclarationOnly`、`noEmit`、`lib`、`outFile`、`composite`、`tsBuildInfoFile`、`paths`、`rootDirs`、`types`、`allowImportingTsExtensions`、`out`、`noEmitOnError`、`declarationDir` |
| 追加 | `suppressOutputPathCheck=true`、`allowNonTsExtensions=true`；declaration route: `declaration=true`、`emitDeclarationOnly=true`、`isolatedDeclarations=true`；JS route: `declaration=false`、`declarationMap=false` |
| default（穴埋めのみ） | `target=12 (LatestStandard)`、`jsx=1 (Preserve)`（`getDefaultCompilerOptions`, typescript.js:153190） |

**したがって transpile 2 経路は「単一 file Program の noCheck emit」であり、Program noCheck 経路の特殊化である。**
これが本設計の中心的な統一で、Rust 側も 1 つの typed route plan（§5）で 3 経路を表す。

### 2.2 `noCheck` の全 consumer（_tsc.js）

| 場所 | 内容 | 影響 |
| --- | --- | --- |
| 18896 `skipTypeCheckingWorker` | `!ignoreNoCheck && options.noCheck` → skip | `checkSourceFileWorker`（87003）は冒頭で return：**型検査本体は走らない** |
| 123993 `getBindAndCheckDiagnosticsForFileNoCache` | `skipTypeChecking` → `emptyArray` | semantic getter は空（bind 診断も落ちる） |
| 123978 `getProgramDiagnostics` | 同 skip → 空 | file 所有の program 診断（未解決 import 等）も落ちる |
| 124038 `getGlobalDiagnostics` | checker を生成し `initializeTypeChecker` を走らせる | `noLib` などの **global 診断は noCheck でも報告される**（案 `no-lib/global-diagnostics` で観測、TS2318×10、exit 2） |
| 116597 `emitJsFileOrBundle` | `noCheck || !canIncludeBindAndCheckDiagnostics(file)` → `markLinkedReferences(file)` | 未検査 file の alias 参照を emit 前に遅延計算（import elision の `referenced` 事実） |
| 116650 `emitDeclarationFileOrBundle` | 同条件 + `emitResolverSkipsTypeChecking` → `collectLinkedAliases(file)` | declaration 側の alias 可視性 |
| 88132 `calculateNodeCheckFlagWorker` | `noCheck || !canInclude…` のときのみ `hasNodeCheckFlag` が flag group を遅延計算 | ES5 loop capture、`arguments`、`super`、constructor reference、private-name scope |
| 126439/126902/127656-127709 | builder program の `checkPending` | incremental/build 専用、本依頼の対象外 |

`getEmitResolver(file, ct, skipDiagnostics)`（47561）は `!skipDiagnostics` なら `getDiagnostics(file)` → `checkSourceFile` を呼ぶが、noCheck では上記 skip により何もしない。
declaration route（`forceDtsEmit && emitOnly`）は `emitResolverSkipsTypeChecking` により最初から skip する。

### 2.3 経路別 graph

```
transpileModule(input, opts)        transpileDeclaration(input, opts)
        └── transpileWorker(input, opts, declaration=false|true)
              ├── fixupCompilerOptions(opts.compilerOptions, diagnostics)   // enum 文字列→数値、TS6046
              ├── defaults(target=12, jsx=1) → forced（§2.1）
              ├── compilerHost（in-memory：getSourceFile は input と lib.d.ts のみ、readFile→""、
              │                 directoryExists→true、useCaseSensitiveFileNames→false、cwd→""）
              ├── createSourceFile(inputFileName, input, {target, impliedNodeFormat, setExternalModuleIndicator, jsDocParsingMode})
              │     inputFileName = opts.fileName || (raw compilerOptions.jsx ? "module.tsx" : "module.ts")
              ├── sourceFile.moduleName / renamedDependencies（任意）
              ├── createProgram([inputFileName], options, host)
              ├── reportDiagnostics: getSyntacticDiagnostics(sourceFile) → getOptionsDiagnostics()
              ├── program.emit(undefined, undefined, undefined, emitOnlyDtsFiles=declaration, transformers, forceDtsEmit=declaration)
              │     └── emitWorker: handleNoEmitOptions（forceDtsEmit 時は skip）→ getEmitResolver(file, ct, skip=declaration) → emitFiles
              ├── diagnostics += emit.diagnostics
              └── outputText===undefined → Debug.fail("Output generation failed")
                  return { outputText, diagnostics, sourceMapText }

Program noCheck（tsc CLI 相当）
  createProgram(roots, {noCheck:true,...}, host)
  emitFilesAndReportErrorsAndGetExitStatus(program, …)   // _tsc.js:129412-129481
     config → syntactic → [options → global → (semantic: skip → 空)] → (noEmit&&declaration: getDeclarationDiagnostics)
     program.emit() → emitWorker → getEmitResolver(undefined, ct, false) → emitFiles
       JS unit: markLinkedReferences(file) → transformNodes(scriptTransformers) → print
       d.ts unit: collectLinkedAliases(file) → transformDeclarations → print
     exit: emitSkipped&&diags→1、diags→2、else 0
```

## 3. 観測結果（source oracle）

`scripts/observe-transpile-routes.mjs --write-inputs` が manifest を固定し、`--inputs … --out …` が 2 回一致する期待値を採取した（287 case、exception 3、replica mismatch 0）。
transpile 経路は「公開 API の観測」と、同じ host/option/Program を再構成した **replica**（getEmitResolver 呼出、host request trace、noCheck 下の semantic/global getter 値）を分けて記録し、replica の公開結果が実 API と一致することを全 case で確認した（`replica_public_result_matches`）。
Program 経路の内部 checker は公開面から差し替えられないため host trace のみ記録する。

主要な観測（設計を決めた事実）:

| 観測 | 意味 |
| --- | --- |
| transpile 全 case で `getEmitResolver(undefined, skip=false)`（JS）/ `(undefined, skip=true)`（dts）が 1 回、`checker.getDiagnostics` は 0 回 | noCheck では resolver 取得が check を起動しない |
| `program.getSemanticDiagnostics()` は transpile/noCheck 全 case で空、`getGlobalDiagnostics()` は JS route で TS2318（noLib）×5〜10 | 「全診断を空にする」処理ではなく、skip の帰結として semantic が空。global は残る |
| `filename/declaration-input-throws`、`filename/json-input`（js）、`filename/declaration-input-throws`（dts） | `Debug.fail("Output generation failed")`：出力の無い入力は例外 |
| `declaration/declaration-error-blocks-dts`（noCheck） | 通常 check なら TS4025 になる `export const v = new Private()` が、noCheck では `declare class Private {}` を **可視化して d.ts に出力**し、診断 0・exit 0。`collectLinkedAliases` の効果 |
| `isolated-declarations/error`（noCheck） | Program 経路では 9007 を出さず推論型を出力（isolatedDeclarations の 90xx は checker の `checkSourceFile` 側で、noCheck では走らない）。一方 `transpileDeclaration` は declaration transformer 側で 9007/9010/9038 を出す |
| `no-emit-on-error/semantic`（noCheck） | semantic が空なので emit される（exit 0）。`no-emit-on-error/syntax` は skip（exit 1） |
| `syntax-error/still-emits` | parse error でも emit、exit 2 |
| `diag/option-invalid-enum-*` | TS6046（file なし）が reportDiagnostics の有無に関係なく戻る（fixup は常時） |
| `options/no-check-false-forced-true` 等 | caller の `noCheck:false`/`noEmit:true`/`declaration:true`/`outFile` は強制値で上書き |
| `sink/on-error-*` | writeFile の onError → TS5033 が emit 診断＋reported に入り exit 2、後続 write は継続 |

## 4. Rust の現行実装との照合（gap 表）

| 領域 | 現行（開始 SHA） | 差 | 本 prototype の処置 |
| --- | --- | --- | --- |
| `skipTypeChecking` | `should_skip_type_checking_file` が `no_check` を既に含む（checker/lib.rs） | なし | 再実装しない |
| semantic getter の skip | `get_program_semantic_diagnostics` が skip file を除外 | なし | — |
| emit 側 option admission | `validate_emit_options` が `noCheck`（Files）、`isolatedModules`、`verbatimModuleSyntax` を unsupported として拒否 | **経路依存の判断が無い** | typed route（§5）で研究経路のみ admit。Program route は不変 |
| `markLinkedReferences(file)`（116597） | emitter に呼出なし。checker には hint 別 marker（identifier/property/export/jsx/import-equals/export-specifier/decorator）が個別に存在、Unspecified hint の front door が無い | **未実装** | checker `mark_linked_references_unspecified`（71662-71731 の port）＋ resolver method `mark_linked_references(source)`＋ emitter の script loop hook |
| `collectLinkedAliases`（116650） | declaration orchestration に既存（`collect_linked_aliases_for_declaration`、条件は `canInclude…` のみ） | `noCheck ||` 条件は `skip_type_checking_file` を経由して既に成立 | 変更なし（観測で確認） |
| `calculateNodeCheckFlagWorker`（88132） | `has_node_check_flag` は links を読むだけ | **未実装**（未検査 file では flag が計算されない） | `NodeLinks.calculated_flags` を追加し、`calculate_node_check_flag_worker` を port。`checkSingleSuperExpression` / `checkSingleIdentifier` / block-scope binding の 3 group |
| transpile API | 無し（H1 inventory は 22 source を `classified-not-run` で保持） | **未実装** | `crates/compiler/src/transpile.rs` |
| `sourceFile.fileName`（caller spelling） | Program の display は host root で絶対化（`/module.ts`）。相対 root も空 cwd も path model が受けない（probe 済み） | jsxDEV の `_jsxFileName`、診断 file 名が絶対 path になる | `SourceApiFacts::file_name`：checker-edge の `InputFile` 名と metadata 名を caller spelling にし、checked host は同名で syntax を引く。Program identity / 出力 path は prepared spelling のまま |
| `fixupCompilerOptions` | tsconfig 変換（config.rs）に同種の enum 変換と TS6046 生成あり | API 面が無い | `fixup_compiler_options` を新設し、選択肢文字列は `compiler_option_named_choices`（config.rs を pub 化）を共有 |
| in-memory 単一 file Program | `load_emitting_program` + `MemoryCompilerHost` | `allowNonTsExtensions` / `suppressOutputPathCheck` に typed field が無い | `CompilerOptions.allow_non_ts_extensions`（internal）を追加：loader の root admission（124176）と extensionless 完全一致（124200-124205）、checker の program-file loop、emitter の source-family admission と `getOutputExtension` の最終 `.js` arm を port。`suppressOutputPathCheck` は route（`EmitRouteKind::suppresses_output_path_check`）|
| barebones lib | `LibraryCatalog` は 6.0.3 固定名 | `lib.d.ts` は catalog 名に含まれる | memory host の `/lib/lib.d.ts` に barebones 本文を置き `default_library_file_name("lib.d.ts")` |
| `sourceFile.moduleName`（API 指定） | parser の `/// <amd-module name>` からのみ | API からの注入経路なし | `InputFile::with_module_name` → parse 直後に `SourceFile.module_name` を上書き（146099-146101）。AMD/UMD/System の `define("name")` は既存 `try_get_module_name_from_file` が消費 |
| `renamedDependencies` | 無し | 未実装 | `SourceFile.renamed_dependencies`（新 field）＋ `try_rename_external_module`（27720）を CJS/AMD/UMD の `getExternalModuleNameLiteral`、System の dependency group、dynamic import の 3 箇所に接続（resolved-file 名 → rename → clone の順） |
| `jsDocParsingMode` | parser の `ParseOptions.js_doc_parsing_mode` は存在、Program は常に ParseAll | API から per-file に渡す経路なし | `InputFile::with_js_doc_parsing_mode` → その file の `ParseOptions` に反映（146097）。JS 入力の d.ts で ParseNone/ParseAll の差（`a: 1` vs `a: number`）まで一致 |
| forced d.ts emit | `emit_forced_declarations`（DeclarationSession）は診断 bucket を返さない | reportDiagnostics 用の syntactic/options が取れない | `emit_forced_declarations_command_for_transpile`：通常 command と同じ checked session で `emit_forced_declarations_with_activity` を実行 |
| 内部 counter | `NoEmitWorkCounters`（parse/bind/copy） | 「check が走ったか」の counter が無い | `CheckerState.checked_source_files`（`checkSourceFileWorker` 本体の実行回数）を追加し `EmitCommandOutcome::checked_source_files` / `TranspileEvidence` で公開 |

## 5. 設計：typed route plan

```rust
// crates/emitter/src/route.rs
pub enum EmitRouteKind { Program, ProgramNoCheck, TranspileJavaScript, TranspileDeclaration }
impl EmitRouteKind {
    fn admits_no_check(self) -> bool                 // Program 以外
    fn admits_isolated_module_options(self) -> bool  // transpile 2 経路
}
trait EmitHost { fn emit_route(&self) -> EmitRouteKind { Program } ... }   // 既定は不変
```

- option 判断は `validate_emit_options(options, operation, route)` の 1 箇所。pass 側に文字列分岐を置かない。
- `ProgramSession::with_emit_route(route)` が host に route を載せる。CLI / config / profile は `Program` 固定のまま（root の admission を変えない）。
- resolver capability の追加は `EmitResolver::mark_linked_references(source)` の 1 method。既存 `can_include_bind_and_check_diagnostics` と組で 116594-116598 を再現する。
  checker を持たない transform-only fixture resolver は `UnavailableForSource` を返し、従来通り「検査済み file」として扱う（挙動不変）。
- `hasNodeCheckFlag` は checker 側で `calculate_node_check_flag_worker` を先に走らせる（検査済み file では即 return、既存挙動不変）。
- transpile adapter（`crates/compiler/src/transpile.rs`）：
  `TranspileOptions{compiler_options: Vec<(name, TranspileOptionValue)>, file_name, report_diagnostics, module_name, renamed_dependencies, jsdoc_parsing_mode}` →
  `fixup_compiler_options` → defaults/forced（§2.1 と同順）→ memory host（cwd `/`、input は `/<fileName>`）→ `load_emitting_program`（JS: `ProgramOptions::with_no_lib(true)`、dts: barebones lib）→
  `ProgramSession::with_emit_route` → JS: `emit_for_cli`（handleNoEmitOptions を含む通常 command）/ dts: forced-declarations command →
  `TranspileOutput{output_text, diagnostics, source_map_text, evidence}`。
  診断の file 名は caller の spelling に戻す（host root `/module.ts` → `module.ts`）。
  例外（出力なし）は `TranspileError::OutputGenerationFailed`、Rust-only の拒否は別 variant（§7 の集計で区別）。

## 6. 観測 schema

- transpile 2 経路（公開）: `{exception, outputText{present, utf16_units, utf8_base64, utf8_sha256}, diagnostics_present, diagnostics[], sourceMapText{…}}`。
  absent（`undefined`）と空文字列は `present` で区別、UTF-16 の単位数と UTF-8 materialization を別記。
  内部（別記）: `input_file_name, implied_node_format, script_kind, external_module, fixup_diagnostics, forced_options, effective_options, syntactic/options 診断, semantic/global evidence, emit_result, source_files, trace{host, emit_resolver_requests, checker_get_diagnostics}`。
- Program noCheck（公開）: 既存 complete command tuple `{writes[]（path/kind/callback bytes/BOM/materialized/sourceFiles/data metadata + sink_action/on_error_messages）, reported_diagnostics, status_writes, exit_code, emit_result{emit_skipped, diagnostics, emitted_files, source_maps}, exception}`。
  内部: `semantic_diagnostics_evidence, global_diagnostics_evidence, trace.host`。Rust 側は `checked_source_files`。
- 診断: `{code, category, file, start, length, message（chain を 2 space indent で平坦化）, related_information}`（既存 h2_7d contract と同形）。

## 7. Manifest（287 case）と disposition

固定 inventory の **22 source file 全て**を transpile 2 経路に対応付けた（`inventory/*`：js 42 unit、dts 37 unit。`@filename` で分割、設定文字列は fixup に渡す）。追加 family:

| route | family（件数） |
| --- | --- |
| transpile-js (150) | inventory 42、diag 13、options 18、lang 22、modules 8、filename 9、renamed 9、module-name 7、maps 5、text 5、kinds 5、jsdoc 3、empty 2、repeat 2 |
| transpile-dts (86) | inventory 37、diag 7、lang 7、options 6、filename 5、text 4、jsdoc 3、maps 3、modules 3、kinds 3、basic 2、repeat 2、empty 1、globals 1、module-name 1、renamed 1 |
| program-no-check (51) | import-elision 7、flags 5、declaration 4、control-checked 4、no-emit-on-error 3、semantic-error 3（checked control 2 含む）、decorators/enum/isolated-declarations/js-declaration/jsx/maps/no-emit/no-check/no-lib/options/sink 各 2、multi/syntax-error/text 各 1 |

handoff の必須 family（空入力、TS/TSX/JS、fileName absent/explicit/非標準、moduleName、renamedDependencies、構文/option/semantic-only/declaration-only error、reportDiagnostics on/off、noCheck×noEmitOnError/noEmit/declaration/isolatedDeclarations/JS declaration、import/export/type-only/未解決、enum/namespace/decorator/JSX/class field ladder、sourceMap/inlineSourceMap/declarationMap、LF/CRLF/Unicode、不正 option、未対応拡張子、source throw、sink failure、反復呼出）は全て 1 件以上ある。

native の集計は `known-open.v1.json` の台帳で行う：exact / known-open（理由付き）/ open（新規差）/ known-open-now-exact（台帳の retire 要求）。
Rust-only typed error（`rust-*`）は互換性に加点せず、台帳の理由で区別する。

## 8. 段階 B の結果

→ [REPORT.md](REPORT.md)（before/after、集計、開かれた行、性能の別記）。

## 9. 段階 C へ渡す残項目（activation 前提）

段階 C の producer 4 件（`moduleName` / `renamedDependencies` / `jsDocParsingMode`、`allowNonTsExtensions`、
caller spelling の `fileName`）は 2026-09-16 の続行で本 worktree に実装済み（§4 の該当行、[REPORT.md](REPORT.md) §2）。残りは：

1. route admission の正式化：`ProgramNoCheck` を CLI/config の `noCheck` に接続する場合の profile/ratchet。依頼資料の「本依頼では activation しない」に従い未接続（`ProgramSession::with_emit_route` は adapter/harness からのみ）。
2. `calculateNodeCheckFlagWorker` の残 group の検証（private-name scope、computed property name）を witness で拡張。
3. Program 経路の `checkPending`（builder/incremental）は対象外のまま。
4. `target` 範囲外値（TS6046 後も emit する TS の挙動）は emitter admission の typed refusal のまま（fixture/比較式を変えない方針）。
5. 性能観測：`checked_source_files=0` と work counters を固定入力で before/after 別記（機能一致後、同環境）。
6. inherited 14 行（emitter）と H2.9 の parse recovery 7 行は他 owner。
