# TypeScript 7.1 の非 compiler suite への追随（P4）

状態：**計画（2026-10-07）**。利用者の指示（2026-10-02）「compiler の一致が完了したら、ひとまず compiler 以外の
test も完全一致を実装していきたい。LSP 関連と content mapper は大きいのでそれ以外をまず形にする」に、
P3-6g（command line）完了後の選択（2026-10-07「tsgo の他 suite への追随」）で着手する。
[post-emitter roadmap](../../post-emitter-roadmap.md) が後続作業の owner で、この packet はその
test suite 追随の部分を持つ。実装は suite ごとの slice（branch → PR → merge commit → `main` の docs commit）で、
記録はこの README に `## P4-n` として追記する。

## 固定参照

- commit `19dadef8888ba5b27d8b9f622480745cf623e020`（profile `7.1.0-dev-19dadef8`）。Go source は
  `target/typescript7/upstream/tsc`、参照 binary は `scripts/typescript7.py build`（tsgo、`--singleThreaded`）。
- 調査（read-only、2026-10-07）は上流の `internal/testrunner`、`internal/testutil/{baseline,harnessutil,tsbaseline,
  fsbaselineutil}`、`internal/tsoptions/*_test.go`、`internal/execute/tsctests`、`internal/api` を source で確かめた。
  以下の行番号はその commit のもの。

## 共通の性質（internal/testutil）

- `baseline.Run`（baseline/baseline.go:23-79）は `testdata/baselines/reference/<sub>/<name>` との **byte 一致**。正規化は
  全て writer 側。`<no content>` の実測に reference が無ければ pass、あれば `.delete` を書いて fail しない。
- `removeTestPathPrefixes`（tsbaseline/util.go:23-49）：`/.ts/`・`/.lib/`・`/.src/`・`bundled:///libs/` を空に、
  `file:///./{ts,lib,src}/` を `file:///` に（単純な部分文字列置換、text 全体に適用）。
- harness の program は single-threaded・checker 1 つ（testutil.go:37-49、harnessutil.go:970-974）。読み込みは
  BOM を除き UTF-16 を復号（vfs/internal/internal.go:131-173）。lib は `bundled:///libs`（113 file）。
- test case の directive 構文・unit 分割・configuration 展開（vary-by、名前 `k=v,...`、上限 25）は
  [conformance-ts71](../conformance-ts71/README.md) の runner が既に移植している。

## Inventory（tsc/testdata/baselines/reference）

| family | 件数 | 大きさ | producer（Go） | 形式の要点 | port の前提 | vendored |
| --- | --- | --- | --- | --- | --- | --- |
| compiler＋conformance `.errors.txt`／`.js`／`.js.map` | 13,422 configuration | — | testrunner/compiler_runner.go | — | 済 | 済 |
| `.types` | 12,779（compiler 6,691＋conformance 6,088） | 39 MB | compiler_runner.go:502-528 → tsbaseline/type_symbol_baseline.go | CRLF、`//// [tests/cases/…] ////`、file ごと `=== name ===`、`>text : type` | checker の node query＋node builder＋printer | 未 |
| `.symbols` | 12,779 | 42 MB | 同上 | `>text : Symbol(name, Decl(file, line, col), …)` | 同上 | 未 |
| `.sourcemap.txt` | 157（137＋20） | 3 MB | compiler_runner.go:485-500 → harnessutil/sourcemap_recorder.go | CRLF、`JsFile:`/`mapUrl:`/`sources:` header、`>>>` JS 行、`^` marker、`Emitted(l, c) Source(l, c)` | source map recorder（h2 の bridge あり） | 済（比較なし） |
| `.trace.json` | 148（72＋76） | <1 MB | compiler_runner.go:530-543 → tsbaseline/module_resolution_baseline.go | LF、JSON ではなく trace 文の列、`sanitizeTrace`（version → `FakeTSVersion`、package.json cache 文言の正規化） | resolver の trace 出力（port 未実装） | 未 |
| transpile | 25 case／41 baseline（19 `.js`＋22 `.d.ts`） | 220 KB | testrunner/transpile_runner.go | 入力 section ＋ 出力 section ＋ `//// [Diagnostics reported]`、vary-by は declarationMap／sourceMap／inlineSourceMap のみ | `internal/transpile` 相当（port は 6.0.3 期の prototype） | 未 |
| tsoptions（commandLineParsing） | 80（parseCommandLine 53＋parseBuildOptions 27） | 320 KB | tsoptions/commandlineparser_test.go の Go 埋め込み表 | LF、`Args::`／`CompilerOptions::`（raw の順序付き JSON）／`FileNames::`／`Errors::` | command line parser（P3-6g で済） | 未 |
| config／tsconfigParsing | 87 | 350 KB | tsoptions/tsconfigparsing_test.go の Go 埋め込み表 | `Input::`／`Config::`／`Errors::`（pretty＋ANSI）、`Fs::`／`configFileName::`／`CompilerOptions::`／`FileNames::` | config parser（済）、pretty 診断 | 未 |
| config／matchFiles | 142 | 570 KB | **producer なし**（Go 側に参照が無い orphan、Strada 形式） | — | 対象外として記録 | 未 |
| tsc | 224（edit 付き 74） | 3.1 MB | execute/tsctests（tsc_test.go 等の `tscInput` 表） | LF、`currentDirectory::`／`Input::`／`tsgo <args>`／`ExitStatus::`／`Output::`／FS diff／`SemanticDiagnostics::`／`Signatures::`／`Edit [i]::` | CLI 全般（help／init／showConfig／locale は未対応）、仮想 FS＋時計の harness、readable buildinfo | 未 |
| tsbuild | 192（edit 付き 125） | 5.5 MB | 同上（tscbuild_test.go） | 同上 | `-b`（済）、incremental（済） | 未 |
| tscWatch／tsbuildWatch | 42／65 | 2.9 MB | 同上（watch） | 同上＋`Watch Registrations::` | watch mode（未実装） | 未 |
| api | 2 | 8 KB | api/encoder/encoder_test.go | AST encoder の dump | API server（未実装） | 未 |
| 除外 | fourslash 1,749、lsp 3、project 2、astnav 7、contentmapper 15 | | | | LSP／content mapper | — |

## suite ごとの仕様の要点

### `.types`／`.symbols`（tsbaseline/type_symbol_baseline.go）

- 生成条件：`skippedTests`（42 名）と `SkipUnsupportedCompilerOptions`（UMD／System、node10／classic、
  `esModuleInterop=false`、`allowSyntheticDefaultImports=false`、baseUrl、ES5、`alwaysStrict=false` は skip、AMD／outFile
  は fatal）を通った configuration で、`@noTypesAndSymbols` が無ければ **常に両方**を書く（416 case が
  noTypesAndSymbols）。file は toBeCompiled＋otherFiles のうち program にあるもの（tsconfig unit は含めず、JSON unit
  は program にあれば含む）。空なら `<no content>`。診断の有無は出力を抑えず、`any` の表示だけを変える。
- 配置：header `//// [tests/cases/compiler/x.ts] ////` ＋ 空行、file ごと `=== unit ===`、code 行は content を
  `[\r\n]|\r?\n` で分けたもの、結果行は `>` ＋ node の text（改行除去）＋ ` : ` ＋ 型/symbol。結果の無い行の
  前に、直前の行が `^\s*[{|}]\s*$`（ASCII `\s`）でも空白のみでもなければ空行を入れる。全て CRLF。chunk ごとに
  `removeTestPathPrefixes`。
- 走査 `forEachASTNode`（:319-344）：反復の pre-order DFS（`ForEachChild` の子を逆順に push）。JSDoc は訪れない。
  `NodeFlagsReparsed` の node は subtree ごと飛ばす（As／Satisfies とその `Expression()` の例外あり）。候補は
  `IsExpressionNode`、Identifier、`IsDeclarationName`、heritage clause の型参照名の QualifiedName（symbol 走査か、
  親も QualifiedName）。
- 型（:356-415）：`IsPartOfTypeNode`、reparsed な型の As／Satisfies、値の意味を持たない親の Identifier（type alias
  名は例外）、OmittedExpression は出さない。class の extends の `ExpressionWithTypeArguments` は親の型を使う。
  error baseline が無く any-flagged で、BindingElement／PropertyAccess／QualifiedName／label／`declare global`／
  MetaProperty／import・export 名／intrinsic JSX tag の下でなければ `IntrinsicName()`（any／error／unresolved／
  intrinsic）。他は `NewNodeBuilder(...).TypeToTypeNode(t, node.Parent, (NoTruncation|AllowUniqueESSymbolType|
  GenerateNamesForShadowedTypeParams) & NodeBuilderFlagsMask | IgnoreErrors, InternalFlagsAllowUnresolvedNames)` を
  `printer.NewPrinter({RemoveComments: true})` で `NewTextWriter("", 0)`（改行 ""、indent 4）に書く。type alias 名の
  結果が同じ identifier なら `InTypeAlias` を足して再生成。`checker.TypeToString` とは flag が違う。
- symbol（:417-456）：`GetSymbolAtLocation` が nil なら出さない。`Symbol(` ＋ `EscapeAllInternalSymbolNames(
  SymbolToStringEx(sym, node.Parent, SymbolFlagsNone, SymbolFormatFlagsAllowAnyNodeKind))` ＋ 宣言 5 件まで
  `, Decl(basename, line0, utf16col0)`（`declaration.Pos()`、lib.*.d.ts は `--, --`）、6 件目以降は
  ` ... and N more` ＋ `)`。
- 名前は `\.tsx?$` を置換（`x.d.ts` → `x.d.types`）。union の順序は `checker.CompareTypes` で決定的
  （compiler_runner.go:552-575 が検証）。

### `.sourcemap.txt`（harnessutil/sourcemap_recorder.go、harnessutil.go:915-951）

- sourceMap／inlineSourceMap／declarationMap のいずれかが true で、noEmitOnError＋診断でなく、record が空でなければ
  書く（150 case）。名前は `ChangeExtension(name, ".sourcemap.txt")`（`.d.ts` を丸ごと落とす）。
- `EmitResult.SourceMaps` の順（file の emit 順、JS → d.ts）に区切りなしで連結。header は `=` 67 個の行、
  `JsFile:`／`mapUrl:`／`sourceRoot:`／`sources:`／任意の `sourcesContent:`、再び `=` 行。source file が変わると
  `-` 67 個の行、`emittedFile:`、`sourceFile:`、`-` 行（Strada 互換の bug 条件で `emittedFile` に ` (l, c)` が付く）。
  生成行ごとに `>>>` ＋ JS 行、span の marker 行（`N >` ＋ `^`、継続は `1->`）、source の text、
  `<id>Emitted(gl, gc) Source(sl, sc) + SourceIndex(i)[ name (N)]`、`---`。全体に `removeTestPathPrefixes`。
- port には h2 期の bridge `ProgramSession::print_units_with_source_map_recording_for_harness`
  （`SourceMapRecordingInputs`）があり、recorder の描画はそこから作り直す。

### `.trace.json`（tsbaseline/module_resolution_baseline.go、harnessutil.go:552-605）

- `@traceResolution: true` の case（157 file、baseline 148）。pre-emit program では trace を強制 off、post-emit
  program の trace だけ。filesparser.go:440-445 が parse task ごとに trace を溜め、決定的な DFS 順で再生する。
- 1 行 1 文（英語の診断文言、`Fprintln`、LF、末尾 `\n`）。`/.src/` は落とさない。`sanitizeTrace`：最初の
  `'<version>'` を `'FakeTSVersion'` に（最初の 1 回で return）、package.json の cache 文言は path ごとに初回を非 cache
  形、以後を「according to earlier cached lookups」形に正規化。
- port の resolver（`crates/program/src/{module_resolution,resolution}.rs`）に trace は無い（`resolution_cache.rs`
  の `trace()` は C05 の観測行で別物）。tsgo の trace 文（diagnostics の `Resolving_module_0_from_1` 等）を
  resolver に足し、CLI の `--traceResolution`（P3-6g の残差）も同じ出力で閉じる。

### transpile（testrunner/transpile_runner.go、internal/transpile/transpile.go:118-258）

- `tests/cases/transpile`（25 file、`\.[cm]?[tj]sx?$`）。vary-by は declarationMap／sourceMap／inlineSourceMap だけ、
  名前は camelCase（`(declarationMap=true)`）。option は空から（CRLF や noErrorTruncation の harness 既定なし、
  skip 規則なし）。unit ごとに `TranspileModule`／`TranspileDeclaration`：incremental／declaration／emitDeclarationOnly／
  noEmit／lib／outFile／composite／tsBuildInfoFile／paths／rootDirs／types／allowImportingTsExtensions／noEmitOnError／
  declarationDir を消し、isolatedModules（verbatimModuleSyntax でなければ）／noCheck／noResolve／
  suppressOutputPathCheck／allowNonTsExtensions を立てる。JS は noLib、declaration は isolatedDeclarations＋最小 lib。
  診断は `@reportDiagnostics` のときだけ syntactic／config／program、emit 診断は常に。
- 出力：`//// [unit] ////` ＋ 入力（CRLF）、`[unit.js]`／`[unit.d.ts]`、任意の `[out.map]`、診断があれば
  `//// [Diagnostics reported]` ＋ `GetErrorBaseline`。emit の text は LF のまま。header も path 除去も無い。
- port の `crates/compiler/src/transpile.rs`（651 行、`transpile_module`／`transpile_declaration` @6.0.3）を tsgo の
  `internal/transpile` に pin し直す。

### tsoptions／config（internal/tsoptions の `_test.go`）

- 入力は Go の埋め込み表（parseCommandLine 33＋20 case、parseBuildOptions 22＋5、tsconfigParsing 7＋32×2＋8×2）。
  Strada の fixture（`testdata/fixtures/typescript/.../config/commandLineParsing`）は fileNames／options の照合に
  使われ、Errors は比較しない。port は同じ表を Rust の test 表に写し（Go の行を pin）、baseline を vendoring して
  byte 比較する。
- 形式：parseCommandLine は `Args::`／`CompilerOptions::`（raw OrderedMap の順序の compact JSON）／`FileNames::`／
  `Errors::`（非 pretty、`\n`）。parseBuildOptions は `buildOptions::`／`compilerOptions::`（struct の field 順）／
  `Projects::`／`Errors::`。jsonParse は `Input::`／`Config::`（2 space）／`Errors::`（pretty＋ANSI）。json api／
  jsonSourceFile api は `Fs::`（`//// [path]` ＋ CRLF）、`configFileName::`、任意の `CompilerOptions::`／
  `TypeAcquisition::`、`FileNames::`、`Errors::`（pretty、`\r\n`、cwd = basePath）。JSON は encoding/json/v2（HTML
  escape なし）。
- matchFiles（142）は producer が無い（vfsmatch の test は baseline を使わない）。対象外として記録する。

### tsc／tsbuild／tscWatch／tsbuildWatch（internal/execute/tsctests）

- 宣言：`tscInput{subScenario, commandLineArgs, files, cwd（既定 /home/src/workspaces/project）, edits, env,
  outputIsTTY（既定 true＝色付き）, ignoreCase, windowsStyleRoot}`、`tscEdit{caption, commandLineArgs, edit, expectedDiff}`
  （runner.go:18-43）。baseline は `reference/<tsc|tsbuild>[Watch]/<scenario>/<subScenario>.js`（folder は引数の
  `-b`／`-w` の有無で決まり、実行は `--build` が先頭のときだけ build）。
- 仮想 FS（`vfstest.FromMapWithClock`、`testFs`）：lib は `/home/src/tslibs/TS/Lib`（最初の読み込みで一度 `*Lib*` と
  して diff に出る）、`.tsbuildinfo` の書き込みは version を `FakeTSVersion` に直して `.readable.baseline.txt` を併せて
  書く、file version は `xxh3-128 hex + "-" + 全文`。時計は呼ぶたび +1 s。edit ごとに FS diff（`*new*`／`*modified*`／
  `*deleted*`／`*rewrite with same content*`／`*mTime changed*`／`-> target`）と、全 edit を適用した clean build との
  差（`Diff::`、8 file）。
- 出力の正規化（sys.go:438-555）：`'7.1.0-dev'` → `'FakeTSVersion'`、symbol id → `<symbolId>`、status 行の時刻 →
  `HH:MM:SS AM`、statistics block は削除、listFiles／trace block は marker 行を除いて保持、ANSI は保持、幅は
  `TS_TEST_TERMINAL_WIDTH`。`ExitStatus::` は `Success`／`DiagnosticsPresent_OutputsSkipped`／
  `DiagnosticsPresent_OutputsGenerated`／`InvalidProject_OutputsSkipped`／`ProjectReferenceCycle_OutputsSkipped`／
  `NotImplemented`。
- one-shot は tsc 224＋tsbuild 192 = 416、watch は 107（watcher、`MockWatchBackend`、`DoCycle` が要る）。

### api

- `api/encoder/encoder_test.go` の 2 baseline（AST encoder の dump）。API server と一緒に後回し。

## port 側の受け皿と不足

| 必要なもの | 既存 | 不足 |
| --- | --- | --- |
| node ごとの型／symbol の query | checker 内部の `type_to_string*`（check.rs:5522-）、`symbol_to_string_via_node_builder` | harness 向け公開 query（`get_type_at_location`／`get_symbol_at_location`／`symbol_to_string_ex`）、walker 用の flag 組（NoTruncation／AllowUniqueESSymbolType／GenerateNamesForShadowedTypeParams／IgnoreErrors／AllowUnresolvedNames／InTypeAlias）での node builder＋printer 経路（`RemoveComments`、text writer） |
| checked program を閉包へ渡す入口 | `ProgramSession::with_checked_emit_resolver_for_harness` | 同じ形の `with_checker_for_harness`（no-emit の診断 session の後、checker を保ったまま walker を動かす） |
| source map recorder | `print_units_with_source_map_recording_for_harness`、`SourceMapRecordingInputs` | tsgo 形式の描画（sourcemap_recorder.go）、d.ts map、`removeTestPathPrefixes` |
| resolver の trace | なし | tsgo の trace 文（module／type reference／automatic type directive、package.json cache 文言）、parse task ごとの buffer と DFS 順の再生、CLI `--traceResolution` |
| transpile | `transpile.rs`（6.0.3 の pin） | `internal/transpile` の option 整形・最小 lib・FS、runner の形式 |
| command line／config parser | P3-6g の parser、config parser | baseline 形式の描画（raw option の順序付き JSON、pretty＋ANSI 診断の `\r\n` 版）、Go 表の Rust 化 |
| tsc harness | `MemoryCompilerHost`、CLI | 仮想 FS＋時計の `TestSys`、FS differ、出力 sanitizer、readable buildinfo、`ExitStatus` 名、Go 表の Rust 化（可能なら script で生成）、help／init／showConfig／locale の採用判断 |
| watch | なし | watcher・backend・cycle（後回し） |

## 順序と slice（案）

既存機能を再利用する比較を先に、機能実装を後に（利用者の 2026-10-02 指示）。各 slice の終了条件は「tsgo の
baseline と byte 一致した件数を ratchet に固定し、0 regressions」で、conformance-ts71 と同じ運用（1 回の全体実行は
最終 bytes で `--workers 2`、heavy な replay は hosted）。

| slice | 対象 | 主な作業 | 前提 | 大きさの目安 |
| --- | --- | --- | --- | --- |
| P4-1 | `.types`／`.symbols`（12,779×2） | vendoring（+25,558 file、81 MB、integrator）、checker の harness query、walker／writer、runner の family（report 列 `types`／`symbols`、ratchet 列、`--dump`）、初回値の計測、差分 class の整理 | 既存 conformance runner | 最大。初回は多数の差分 class（型表示の忠実度）が出る前提で、計測 slice と修正 slice（P4-1a…）に分ける |
| P4-2 | `.sourcemap.txt`（157） | recorder の描画の移植、runner の family | h2 の bridge | 小〜中 |
| P4-3 | `.trace.json`（148）＋ CLI `--traceResolution` | resolver の trace、buffer／再生、`sanitizeTrace`、CLI 出力 | P3-6f の parse task | 中 |
| P4-4 | transpile（41） | `internal/transpile` への pin 直し、runner、vendoring（cases＋baselines） | emitter | 中 |
| P4-5 | tsoptions（80）＋ config/tsconfigParsing（87） | Go 表の Rust 化、形式の描画、vendoring | P3-6g の parser、config parser | 中 |
| P4-6 | tsc（224）＋ tsbuild（192） | `TestSys`（仮想 FS・時計・差分・sanitizer・readable buildinfo）、Go 表の Rust 化、`ExitStatus`、help／init／showConfig／locale の採用、vendoring | P3-6a〜g | 大 |
| P4-7 | tscWatch／tsbuildWatch（107）、api（2） | watch mode、API server | 別計画（roadmap の P6／P5） | 後回し |

## runner・report・ratchet・CI

- compiler／conformance 由来の family（types／symbols／sourcemap／trace）は既存 `conformance-ts71` に列を足す
  （report.json の `results[]` に `types`／`symbols`／`sourcemap`／`trace` の agreement、ratchet TSV に列を足す。
  既存列の意味は変えない）。実行時間は walker の query 分だけ増えるので hosted job の予算を初回値で確かめる。
- transpile／tsoptions／config／tsc／tsbuild は入力が別なので新しい binary（`crates/conformance` 内の
  `suites-ts71`）と `scripts/suites_ts71.py`、`ratchets/ts71/suites-*.tsv`、hosted job を足す（path 選択は既存の
  planner に従う）。
- vendoring は `scripts/vendor_typescript_native.py` の sets に family を足し（`BASELINE_SUFFIXES` に `.types`／
  `.symbols`／`.trace.json`、`tests/cases/transpile`、`baselines/reference/{transpile,tsoptions,config/tsconfigParsing,
  tsc,tsbuild}`）、manifest と `baseline-names.txt` を同じ規則で更新する。integrator 所有、各 slice の記録に残す。

## 未決事項

- `config/matchFiles`（142）は producer が無いので対象外（この記録で閉じる）。
- tsc suite の help／init／showConfig／locale（cs など）は、実装して一致させるか ratchet の known に留めるかを
  P4-6 の初回値で決める。
- watch と api は roadmap の P5／P6 と同じ計画で扱い、この packet では順序だけ置く。
- `.types` の `any` 表示は error baseline の有無で変わる（`hasErrorBaseline` は content mapper の診断も数える）。
  runner は既存の診断 session の結果をそのまま使う。

## P4-1 `.types`／`.symbols` baseline の比較（2026-10-08）

tsgo testutil/tsbaseline/type_symbol_baseline.go の `DoTypeAndSymbolBaseline` を conformance runner に足し、compiler／conformance の
全 configuration で `.types`／`.symbols` を byte 比較する。この slice は計測（runner・初回値・差分 class の整理）で、差分の修正は
P4-1a 以降。

- **vendoring**：`scripts/vendor_typescript_native.py` の `BASELINE_SUFFIXES` に `.types`／`.symbols` を足し、profile
  `7.1.0-dev-19dadef8` に 12,779＋12,779 file（81 MB）を取り込んだ（58,238 file、`--check` と harness の
  `native_vendored_inputs_match_the_manifest`（件数 58,238 に再 pin）が一致）。
- **checker**（`crates/checker/src/location.rs`、新規）：tsgo の `GetTypeAtLocation`（`getTypeOfNode`、checker.go:32418-32516）と
  `GetSymbolAtLocation`（`getSymbolAtLocation`／`getSymbolOfNameOrPropertyAccessExpression`、:32069-32417）を移植
  （`CheckerState::get_type_at_location`／`get_symbol_at_location`）。港の JSDoc reparse は木の外（side table）なので
  `GetReparsedNodeForNode` に当たるものは無い。`checkMetaPropertyKeyword` は tsgo でも stub（error type）。未移植：
  `getApplicableIndexSymbol`（index signature で解決した property access の `__index` symbol。IndexInfo に slot が無い）、
  `import.meta` の `meta` の transient symbol。
- **checker**（`crates/checker/src/type_writer.rs`、新規、`#[doc(hidden)]`）：walker の移植（`forEachASTNode` の反復 pre-order、
  候補の filter、型の skip 規則、`any` の intrinsic 名の規則（error baseline の有無は runner が決めるので両方の文を返す）、
  node builder（`NoTruncation|AllowUniqueESSymbolType|GenerateNamesForShadowedTypeParams|IgnoreErrors`、internal
  `AllowUnresolvedNames`、alias 名の `InTypeAlias` 再生成）＋ printer（RemoveComments；港の printer は declaration-syntax gate
  の下で型 node を印字するので `with_declaration_syntax(true)`）、symbol は `SymbolToStringEx(sym, parent, None,
  AllowAnyNodeKind)` ＋ `Decl(file, line, utf16 col)` ×5）。`GetMeaningFromDeclaration`、`IsLabelName`、
  `isImportStatementName`／`isExportStatementName`／`isIntrinsicJsxTag` も移植。
- **session／compiler**：`CheckerSession::with_state_for_harness`（checked state を閉包に貸す）、`ProgramSession::
  run_for_native_harness_with_walk`／`emit_then_run_for_native_harness_with_walk`（`HarnessWalk` を診断の後、checker を保ったまま
  呼ぶ。emit-first の branch では declaration getter の後、one-checker の branch では getter と同じ callback で）。
- **runner**（`crates/conformance/src/ts71.rs`、`ts71/type_symbol_baseline.rs`）：walk は第 2 program（emit → 診断）の checker、
  emit しない／`noEmitOnError` の configuration は第 1 program の checker で行い（tsgo は第 2 program）、`--checkers 1` のときだけ。
  `@noTypesAndSymbols` と control run は NotAssessed。layout（`generateBaseline`／`iterateBaseline`：CRLF、bracket 行／空行の
  規則、`removeTestPathPrefixes`）を移植し unit test で固定。`Outcome::Compared` に `types`／`types_detail`／`symbols`／
  `symbols_detail`、`--dump` は差分の `.types`／`.symbols` も書く。`scripts/conformance_ts71.py`：summary に
  `types_full`／`types_mismatch`／`types_not_assessed`（symbols も）、ratchet TSV に第 4・5 列（`none`／`full`；旧行は `none`）。
- 計測の経過：初回（run 1）は types 11,821／symbols 12,549 一致、error tier に 36 regression（walk が display arena を入れ子で
  取って panic → harness error）。原因は、型の表示中に node builder が到達不能な symbol のエラー名を作るため
  `symbol_to_string_via_node_builder` を入れ子で呼び、共有 display arena を二重に取ること。`CheckerState::emit_display_taken`
  を足し、入れ子の symbol 表示は一時 arena で build・印字するようにした（walk の panic も `catch_unwind` で walk の失敗に
  閉じ込め、walk は declaration getter の後に動かす）。run 2 で regression は深い再帰の 4 case（stack／memory）だけになり、
  これらは `SKIPPED_WALK_TESTS`（NotAssessed）。alias 名の再生成（`InTypeAlias`）は tsgo が「build した node が Identifier の
  とき」だけ行うのに、文字列比較で行っていたため alias が展開されていた（run 2 の types 差分の最大 class）→ 修正。
- run 3（最終 bytes `ee3c8f909` 相当、macOS、`nice -n 20`、2 worker）：12,748 case／462 s（walk 込み。P3-6g の 482 s と同程度）、error／emit は不変
  （full 13,451、emit_full 13,443、mismatch 0）、**types full 12,534／mismatch 233／not assessed 684、symbols full 12,595／
  mismatch 172／not assessed 684**（not assessed = `@noTypesAndSymbols` 680 configuration ＋ `SKIPPED_WALK_TESTS` 4）。ratchet は
  `--update` 相当（report から `update_ratchet`）で第 4・5 列を記録：types full 12,534／none 917、symbols full 12,595／none 856、
  0 regressions。
- 残る差分の class（run 2 の dump から。P4-1a 以降で扱う）：(1) `import.meta`：`meta` の型 `ImportMeta`（港は `any`）と
  `ImportMetaExpression.meta` の transient symbol；(2) JS の `require`：tsgo は `require : any`／`Symbol(require)`、港は
  error／symbol 無し；(3) `typeof import("./a.js")` の module specifier（港は `./a`）と module symbol の名前；(4) index
  signature で解決した access の `__index` symbol（`getApplicableIndexSymbol`）；(5) JS の `this.arguments : object`
  （港 `any`）、JS の property assignment の型（`number` vs `any`）、`any` vs `typeof A`（declFile*）；(6) package
  redirect の file（`content not parsed` の unit が program に無い）；(7) `any` vs `error`（JS の未解決名）。
- local（`nice -n 20`、2 job）：`cargo fmt --all -- --check`、checker／compiler／conformance／harness の
  `cargo clippy --all-targets -- -D warnings`、`cargo test --no-fail-fast`（conformance＋harness＋compiler 464 passed／0 failed、
  checker 1,797／0）。run 3 の後の変更は clippy の指摘（boolean 式の書き換え）だけで挙動は変えていない。fix commit `ee3c8f909`。

### P4-1のhostedの記録とmerge（2026-10-08）

- hosted：最終候補 `9aba21d02`（fix `ee3c8f909` ＋ packet の記録 `b76da68bf` ＋ Cargo.lock）の run 37658154634（`plan` 34s、
  `rust` 10m3s、`conformance (TypeScript 7.1)` 19m43s、`gates` 15s。全て成功）。merge →
  `3eb329bec`（merge commit）。
- 計測（conformance の run 3、crate test）は上の記録のとおり、merge前に行った。実project の比較は行っていない（この slice は
  conformance runner と checker の query／表示の追加で、command line・config・loader の経路は変えていない）。性能は計測していない
  （利用者の指示）。
- scratchpad：`p41/design.md`（tsgo 側の仕様の控え）、`p41/classify.py`（report と dump から差分 class を集計）、
  `p41/update_from_report.py`（既存 report から ratchet を更新）、`p41/run3.sh`／`local-checks.sh`（計測と検証の chain）。
- 次：P4-1a 以降で残る class（`import.meta`、JS の `require`、module specifier／module symbol の名前、`__index` symbol、JS の型、
  package redirect の unit、`SKIPPED_WALK_TESTS` の 4 case）を順に閉じる。

## P4-1a `.types`／`.symbols` の差分 class（第 1 回）（2026-10-08）

P4-1 の計測（types mismatch 233、symbols mismatch 172）の大きい class を tsgo に合わせる。

- **`import.meta`**（types 13、symbols 11）：tsgo の `IsRightSideOfQualifiedNameOrPropertyAccess`（ast/utilities.go:3735-3746）は
  MetaProperty の name も右辺に数える。港の `is_right_side_of_qualified_name_or_property_access` に MetaProperty の arm を足し、
  `getRegularTypeOfExpression(meta)` が `import.meta` 全体の型（`ImportMeta`）を返すようにした。`meta` の symbol は tsgo の
  `getGlobalImportMetaExpressionType().members["meta"]`（transient な `ImportMetaExpression.meta`、型は `ImportMeta`）：
  `CheckerState::get_import_meta_expression_meta_symbol`（初回に作る）。
- **JS の `require`**（types 35、symbols ～30）：tsgo binder/nameresolver.go:330-336 は JS file で require call の callee `require` を
  何も宣言していなければ synthetic な `requireSymbol`（型 `any`、checker.go:16903）に解決する。港は error を抑えるだけで symbol を
  返していなかった（M2 3.4c の残差）：`resolve_name_full` に fallback を足し、`get_type_of_symbol(require_symbol)` を `any` に、
  `is_common_js_require` で `require_symbol` を CommonJS require と認める（tsgo checker.go:16002。これで `require("./x")` の型が
  module の型（`typeof X`）になる）。
- **index signature の `__index` symbol**（symbols 8）：tsgo `getApplicableIndexSymbol`（checker.go:32571-32601）。港の `IndexInfo`
  には `indexSymbol` の slot が無いので `(object type, key type)` を key にした `index_symbols` 表で 1 回だけ作る（`__index`、
  declarations は info の declaration か applicable な info 全部、parent は型の symbol、型は value type）。
  `getSymbolOfNameOrPropertyAccessExpression` の property access の arm で、`resolvedSymbol` が無ければこれを返す。
- **`typeof import("./a.js")` の拡張子**（types 5、symbols 9）：module specifier の ending は importing file の既存の import から
  推定する（`inferPreference`）。港の `module_name_literals` は `import(...)` だけを集めて `import.defer(...)` を落としていた
  （tsgo `IsImportCall` は defer も含む）→ `is_import_call` で集める。
- 残る class（P4-1b 以降）：JS の型（`this.arguments : object`、property assignment の `number`、`any` vs `Outer`／`typeof A`、
  declFile* の `any`）、package redirect の unit（tsgo は redirect file の text と primary の木で walk する：`duplicatePackage`
  2 configuration）、`SKIPPED_WALK_TESTS` の 4 case、`T_1` vs `T`（shadowed type parameter の名前）。
- 計測（最終 bytes `aa7bb7399`、macOS、`nice -n 20`、2 worker）：12,748 case／468 s、error／emit 不変（full 13,451、emit_full 13,443、
  mismatch 0）、**types full 12,593／mismatch 174（P4-1 の 233 から）、symbols full 12,715／mismatch 52（172 から）**、
  not assessed 684、ratchet 0 regressions・128 configuration 上昇（report から `update_ratchet`）。
- local（`nice -n 20`、2 job）：`cargo fmt --all -- --check`、checker／compiler／conformance／harness の
  `cargo clippy --all-targets -- -D warnings`、`cargo test --no-fail-fast`（conformance＋harness＋compiler 464 passed／0 failed、
  checker 1,797／0）。fix commit `aa7bb7399`。

### P4-1aのhostedの記録とmerge（2026-10-08）

- hosted：最終候補 `c7cb15651`（fix `aa7bb7399` ＋ packet の記録）の run 37664836440（`plan` 32s、`rust` 11m30s、
  `conformance (TypeScript 7.1)` 19m59s、`gates` 16s。全て成功）。merge → `02dc706e4`（merge commit）。
- 計測（conformance の全体実行、crate test）は上の記録のとおり、merge前に最終bytes `aa7bb7399` で行った。実project の比較は
  行っていない（JS の `require` の解決と `__index` symbol は checker の挙動を変えるが、conformance の error／emit が不変で、
  実project（`--noEmit` の診断）に影響する経路は `require` の型（JS file の `require(...)` が module の型になる）だけ。次の
  checker 変更を含む slice で corpus を再比較する）。性能は計測していない（利用者の指示）。
- 次：P4-1b（JS の `@type` hosting：`this.y = 12` の assignment declaration、`@type` を非 assignment の expression statement に
  当てない；heritage clause の名前の `any`；shadowed type parameter の参照名；module symbol の specifier 名；
  package redirect の unit；`SKIPPED_WALK_TESTS`）。

## P4-1b `.types`／`.symbols` の差分 class（第 2 回：JavaScript の型）（2026-10-08）

- **constructor の this-property のアクセス**（types 11：`argumentsReferenceInConstructor*_Js`、`typeFromPropertyAssignment10`／`10_1`、
  `jsdocReadonlyDeclarations`…）：tsgo の `isThisPropertyAccessInConstructor`（TypeScript 7.1）は
  `isConstructorDeclaredThisProperty(prop)` が **Constructor** kind のとき（hosted JSDoc `@type` を含む型注釈が無いとき）だけ
  `autoType` を使う。港は tsc 6.0.3 の `isConstructorDeclaredProperty`（hosted の `@type` を見ない）で判定していたので
  `/** @type {number} */ this.y = 12` が auto 型（`any`）になっていた。`is_this_property_access_in_constructor` を
  `this_assignment_declaration_kind` で判定する形に移植（6.0.3 の述語は未使用のまま残す）。
- **unresolved な alias 型の property access**（types 8：`typeFromPropertyAssignment`、`2`、`3`、`40`、`14`、`15`、`16`、`24`）：
  tsgo `checkPropertyAccessExpressionOrQualifiedName` の any-like の arm は `isErrorType(apparentType)`（error type か alias 付きの
  any）なら素の error type を返す。港は `== intrinsics.error` だけを見て alias 付きの error 型（`@type {Outer}` の TS2749 の型）を
  そのまま返していたので `si.m : Outer` と出ていた（tsgo は `any`）。`tables.is_error_type` で判定。
- 残る class（P4-1c 以降）：heritage clause の名前の `any`（declFile*、`declarationEmitNameConflicts`、
  `genericTypeReferenceWithoutTypeArgument`：tsgo の経路の特定に instrumented tsgo が要る）、shadowed type parameter の参照名
  （`T_1`：再利用した型 node の中の参照、`controlFlowInstanceofWithSymbolHasInstance`、`unspecializedConstraints`、
  `declarationsWithRecursiveInternalTypesProduceUniqueTypeParams`、`unionAndIntersectionInference1`）、duplicate な class member
  の型（`duplicateClassElements`：accessor と property の merge で tsgo は `number`）、module symbol の specifier
  （`nodeModulesDeclarationEmitDynamicImportWithPackageExports`：`./index.mjs` vs `package/mjs`）、`Clod.x`／escape 名／
  declaration 列の差（各 1–2）、package redirect の unit、`SKIPPED_WALK_TESTS`。
- 計測（最終 bytes `c330f3265`、macOS、`nice -n 20`、2 worker）：12,748 case／454 s、error／emit 不変（full 13,451、emit_full 13,443、
  mismatch 0）、**types full 12,654／mismatch 113（P4-1a の 174 から）**、symbols full 12,715／mismatch 52（不変）、not assessed 684、
  ratchet 0 regressions・61 configuration 上昇（report から `update_ratchet`）。
- local（`nice -n 20`、2 job）：`cargo fmt --all -- --check`、checker／compiler／conformance／harness の
  `cargo clippy --all-targets -- -D warnings`、`cargo test --no-fail-fast`（conformance＋harness＋compiler 464 passed／0 failed、
  checker 1,797／0）。fix commit `c330f3265`。

### P4-1bのhostedの記録とmerge（2026-10-08）

- hosted：最終候補 `b0b850c53`（fix `c330f3265` ＋ packet の記録）の run 37670779626（`plan` 30s、`rust` 8m49s、
  `conformance (TypeScript 7.1)` 12m43s、`gates` 15s。全て成功）。merge → `d812a2778`（merge commit）。
- 計測（conformance の全体実行、crate test）は上の記録のとおり、merge前に最終bytes `c330f3265` で行った。実project の比較は
  行っていない（JS の checker の変更は conformance の error／emit が不変。P4-1a と合わせて、次に checker を変える slice で
  corpus を再比較する）。性能は計測していない（利用者の指示）。
- 次：P4-1c（`getTypeOfSymbol` の accessor 優先順（tsgo は Accessor を Variable|Property より先に見る）、再利用した型 node の中の
  shadowed type parameter 名、package 自己参照の module specifier、heritage clause 名の `any`（instrumented tsgo）、
  package redirect の unit、`SKIPPED_WALK_TESTS`）。

## P4-1c `.types`／`.symbols` の差分 class（第 3 回：型の表示）（2026-10-08）

- **accessor と property を merge した symbol の型**（types 1：`duplicateClassElements`）：tsgo の `getTypeOfSymbol`
  （TypeScript 7.1）は `Accessor` の arm を `Variable|Property` より先に見る（tsc 6.0.3 は逆順）。港は property の arm に
  入って `any` を返していた（tsgo は accessor の `number`）。arm の順を tsgo に合わせた。
- **別 file の node を再利用したときの shadowed type parameter の名前**（types 4：`controlFlowInstanceofWithSymbolHasInstance`、
  `unspecializedConstraints`、`declarationsWithRecursiveInternalTypesProduceUniqueTypeParams`、`unionAndIntersectionInference1`）：
  tsgo の `attachSymbolToLeftmostIdentifier`（nodecopy.go）は訪問中の node が元の木の leftmost identifier と同じ
  （`node == leftmost`）なら type parameter を `typeParameterToName` で改名する（`T_1`）。港の
  `attach_symbol_to_entity_name` は `node.node() == leftmost` で比べていたが、別 file（lib）の node は target source に
  clone されて id が変わるので常に偽になり、素の clone（`T`）を出していた。parse node（`require_parse_tree_resolver_node`）
  で比べる形に直した。
- **property と auto-accessor を merge した symbol の型**（`duplicatePropertyAndAccessor`：1 回目の full run で errors が
  full → none に下がった唯一の configuration）：上の arm の順の変更で `getTypeOfAccessors` に入るようになったが、港は
  auto-accessor を「最初の PropertyDeclaration が auto accessor なら」で探していた（tsc 6.0.3 の `tryCast(getDeclarationOfKind(...))`）。
  tsgo は `core.Find(symbol.Declarations, IsAutoAccessorPropertyDeclaration)` で宣言全体から探す。港の探し方では
  `y: number = 2; accessor y: number = 3;` の型が `any` になり TS2717 が余分に出た。宣言全体から探す形に直した。
- 残る class：package 自己参照の module specifier（`./index.mjs` vs `package/mjs`、symbols 3）、`Clod.x`／escape 名／
  declaration 列の差（各 1–2）、heritage clause 名の `any`（declFile*：instrumented tsgo が要る）、package redirect の
  unit、`SKIPPED_WALK_TESTS`。

## P4-2 `.sourcemap.txt`（source map の record）（2026-10-08）

tsgo の `DoSourcemapRecordBaseline`（tsbaseline/sourcemap_record_baseline.go）は、`sourceMap`／`inlineSourceMap`／
`declarationMap` のいずれかが真の configuration で `GetSourceMapRecord`（harnessutil.go:915-951）の record を書く
（`noEmitOnError` ＋診断あり、または record が空なら baseline なし）。record は `EmitResult.SourceMaps` の順に、map ごとに
`sourcemap_recorder.go` の writer が描く：`=` 67 個の header（`JsFile:`＝map の `file`、`mapUrl:`＝生成 file 末尾の
`//# sourceMappingURL=`、`sourceRoot:`、`sources:`、任意の `sourcesContent:`）、source file が変わるたびに `-` 67 個の区切りと
`emittedFile:`／`sourceFile:`、生成行ごとに `>>>` ＋行、span の marker（`N >` ＋ `^`、継続は `->`、次の行の先頭は `1->`）、
source の text、`<id>Emitted(l, c) Source(l, c) + SourceIndex(i)[ name (N)]`、`---`。driver は map の `mappings` を
decode して span を流し、writer はもう一つの decoder で同じ span を照合する（食い違えば `!!^^` の行）。

- **emitter**：`SourceMapObservation`（tsgo の `SourceMapEmitResult`）に `generated_file` を足した（JS と d.ts の
  producer の 2 か所）。`input_source_files`（generator の raw sources）と map の JSON は既にあった。
- **conformance crate**：`ts71/sourcemap_baseline.rs` に recorder を移植した：`MappingsDecoder`（sourcemap/decoder.go、
  error 文言も同じ）、`ComputeECMALineStarts`（`\r\n`／`\n`／`\r`／U+2028／U+2029）、
  `ComputePositionOfLineAndUTF16Character`（UTF-16 列 → byte 位置、`allowEdits`）、`TryGetSourceMappingURL`、
  `SpanWriter`（`sourceMapSpanWriter` ＋ `recordedSpanWriter`）。tsgo が Strada 互換のまま残している癖も写した：継続 marker の
  長さは**次の**生成行の byte 長から（`len(jsFileText)-1`）、`emittedFile:… (l, c)` の header は先頭 span の
  generated character と新 span の generated line が等しいとき（char == line）。文字列の位置は tsgo と同じく UTF-8 の
  byte、mapping の列は UTF-16。`sourcesContent:` は tsgo の `json.Marshal`（encoding/json/v2、HTML escape なし）と同じ
  serde_json の出力。全体に `removeTestPathPrefixes`。
- **runner**：第 2 Program の `EmitOutcome::source_maps()` から record の入力（生成 file 名、入力 file 名（絶対）、JSON）を
  取り、生成 file の text は `Emission` の js／dts から、source の text は Program の入力から引く。
  `Outcome::Compared` に `sourcemap`／`sourcemap_detail`、`--dump` で `<stem>.sourcemap.txt`。
- **script**：`scripts/conformance_ts71.py` の summary に `sourcemap_full`／`sourcemap_mismatch`／`sourcemap_not_assessed`、
  ratchet TSV に 6 列目（`sourcemap`：`full`／`none`）。既存の行は `none` から上がるだけ。
- dev binary で `.sourcemap.txt` の baseline を持つ 152 case（213 configuration）を先に回した：vendored の 157 baseline のうち
  **155 が byte 一致**、残り 2（`contentMapperDeclarationEmit`、`contentMapperDeclarationEmitFailure`）は harness option
  `runExternalCode` 未対応の既存の harness error（P4-1b の run でも同じ）。baseline の無い configuration で両側 None の
  「Full」は無い（compared の Full ＝ baseline のある 155 ちょうど）。
- 1 回目の full run（accessor の探し方を直す前、1 worker、maintenance clamp、4,306 s）：errors full 13,450／mismatch 1
  （上の `duplicatePropertyAndAccessor`）、emit 13,443（不変）、types full 12,675／mismatch 92（113 から：`T_1` の修正が
  `declarationEmitPromise`、`correlatedUnions`、`strictBindCallApply1`、`typedArrays` など 22 configuration を上げた）、
  symbols 不変、**sourcemap full 13,451／mismatch 0／not assessed 0**、harness error 15（不変）。
- 計測（最終 bytes、macOS、`taskpolicy -c maintenance nice -n 20`、1 worker）：12,748 case／4,625 s、errors full 13,451（mismatch 0）、emit full 13,443（mismatch 0、not assessed 8）、
  **types full 12,676／mismatch 91（P4-1b の 113 から）**、symbols full 12,715／mismatch 52（不変）、
  **sourcemap full 13,451／mismatch 0／not assessed 0**、harness error 15（不変）。ratchet 0 regressions：6 列目（sourcemap）を
  全 13,451 行に `full` で記録し、types の 22 行が none → full（下がった行は無い）。fix commit `a6b20716c`。
- local（maintenance clamp、1 job）：`cargo fmt --all -- --check`；clippy（`--all-targets -- -D warnings`）emitter／checker／compiler／conformance／harness；
  test emitter＋conformance＋harness＋compiler 1,106/0、checker 1,797/0（最終 bytes `a6b20716c`、`taskpolicy -c maintenance nice -n 20`、
  1 job）。

### P4-2のhostedの記録とmerge（2026-10-08）

- hosted：最終候補 `6c30897fc`（fix `a6b20716c` ＋ packet の記録）の run 37704574662（`plan` 30s、`rust` 11m11s、
  `conformance (TypeScript 7.1)` 20m05s、`gates` 15s。全て成功）。merge → `89ba87fca`（merge commit）。
- 計測（conformance の全体実行、crate test）は上の記録のとおり、merge前に最終bytes `a6b20716c` で行った。実project の比較は
  行っていない（checker の変更は accessor の型と別 file の再利用 node の名前で、conformance の error／emit は不変；次に
  checker を変える slice で corpus を再比較する）。性能は計測していない（利用者の指示）。
- 次：P4-3（`.trace.json` ＋ `--traceResolution`：tsgo resolver.go の tracer（trace 文 61 種）を港の resolver の同じ地点に足し、
  parse task ごとの buffer を決定的な順で再生、harness の `sanitizeTrace`）。

## P4-3 `.trace.json`（`--traceResolution` の trace）（2026-10-08）

tsgo の `DoModuleResolutionBaseline`（tsbaseline/module_resolution_baseline.go）は `@traceResolution: true` の configuration で、
Program 構築中に `Host.Trace` に渡った行を harness の `TracerForBaselining`（harnessutil.go:536-605）で整形して
`.trace.json` に書く（行が無ければ baseline なし）。行の出どころは `module/resolver.go` の `tracer`（`ResolveModuleName`／
`ResolveTypeReferenceDirective` が `(result, traces)` を返す）で、`compiler/fileloader.go` が task ごとに
`typeResolutionsTrace`／`resolutionsTrace` に溜め、`filesparser.go:440-445` が file の収集順（task ごとに type reference の
解決 → module の解決、import の出現順）に再生し、最後に lib replacement の解決（`pathForLibFileResolutions`、key 順）を流す。
harness の整形（`sanitizeTrace`）：最初の `'7.1.0-dev'` を `'FakeTSVersion'` に、`File '…' does not exist.`／
`Found 'package.json' at '…'.`／`… according to earlier cached lookups.` は path ごとに初出を素の形、2 回目以降を
「according to earlier cached lookups」の形に揃える（thread の順に依らない）。

- **vendoring**：`.trace.json` を `BASELINE_SUFFIXES` に足して再 vendoring（148 file、compiler 72 ＋ conformance 76；
  `@traceResolution` の case は 157）。harness の件数 pin は 58,238 → 58,386。
- **resolver（`crates/program/src/module_resolution.rs`）**：`trace!` macro ＋ `ResolutionTrace`（message ＋ 引数、
  `text()` で英語の行）。tsgo の 61 種の trace 文を同じ地点に置いた：request の先頭と結果、`moduleResolution` の種類、
  mode と conditions（`GetConditions`）、相対 path の file／folder の読み込み、親 directory・候補 directory の不在、
  `node_modules` の walk（preferred／fallback の pass、`@types`、scoped package の mangle）、file の probe
  （`tryFileLookup`、`moduleSuffixes` も）、package.json の probe（cache の文言も）、拡張子の剥がし、package.json の
  field（`typings`／`types`／`main`、`typesVersions` の全 trace）、`paths`／`rootDirs`、exports／imports の
  conditional exports（`Entering`／`Matched`／`Saw non-matching`／`Resolved under`／`Failed`／`Exiting`）、subpath と
  target、無効な target／`null`、imports の bare target の再解決、type reference directive の全段、realpath、
  peerDependencies（`getPackageId` の時点）、URI の skip、`import` 条件の retry。結果（`HostModuleResolution`）に
  `trace` を持たせ、type reference は `take_trace()` で取る。
- **tsgo に合わせた挙動の修正（trace が露わにした差）**：(1) typeRoots から解決した module の `isExternalLibraryImport` は
  path が `node_modules` を含むときだけ（港は常に true にして realpath を取っていた）；(2) directory を package.json 経由で
  解決した結果に package id を付けない（tsgo `loadNodeModuleFromDirectory`；港は strada の `withPackageId` のままで、
  `paths` の末尾 `/` を含む候補では submodule 名が 1 文字ずれていた）；(3) lib replacement の解決は Program の resolver で
  CommonJS mode（tsgo `resolveLibrary`；港は Node10 の別 resolver だった）；(4) harness の `customConditions` は要素を
  trim しない（tsgo `ParseListTypeOption`；`' browser'` が条件になる）；(5) `typesVersions` の照合に使う compiler の
  version を `7.1.0-dev`（prerelease）に（港は 6.0.3 のままだった）；(6) non-relative の worker の結果は retry の前に
  完成させる（tsgo `createResolvedModuleHandlingSymlink`：`isExternalLibraryImport` は path が `node_modules` を含むか
  ——self-name／imports の結果も——、external なら realpath；retry の alternate も同様に realpath）；(7) `exports` の
  target が `null` なら lookup 全体を打ち切る（tsgo `unresolved()`；strada は `@types` と fallback pass に続いた）、
  `"."` の main export は値を問わず target loader に渡す（`".": null` は null の trace で打ち切り）、
  `Export specifier … does not exist` の trace は tsgo が関数末尾に達する経路だけ；(8) `typesVersions` の pattern が
  一致して全 substitution が失敗したら探索を続ける（tsgo `tryLoadModuleUsingPaths` は `continueSearching`、strada は
  `{value: undefined}` で打ち切り）；(9) type reference directive の node_modules 探索は realpath を lookup 全体の後
  （package id の peerDependencies の trace の後）に取り、null mapping の打ち切りも module と同じ；(10) `typesVersions` の
  substitution は strada の `onlyRecordFailures` の latch 無しに probe する（tsgo に latch は無い：package field の親
  directory や package root が無くても substitution file は解決する——tsgo 実機で確認、probe A／C）；(11) `typesVersions` を
  tsgo の `GetVersionPaths`／`GetPaths` に揃えた：entry は package.json の順（ordered map；JS の整数 key 優先ではない、
  probe E）、値が配列の key だけが mapping、文字列でない要素は空文字、一致した entry の値が object でなければ version
  paths 無し（probe G／H）。strada の generic `forEach` の模倣（array-like object、`[object Object]`、`Infinity`、JSONC
  `__proto__`）とその helper 群は削除；(12) `paths`／`typesVersions` の substitution と `exports`／`imports` の target の
  `*` は literal に置換（tsgo `strings.Replace`／`ReplaceAll`；strada の `$&` 等の置換 token は無効）。`js_string_ops`
  の予算付き token 置換は削除し、checker が module specifier 名に使う `replace_*_value` は token 意味論のまま残した（残課題）。
  (1)〜(3) と (6)〜(12) は解決結果にも影響し得る：conformance の errors／emit は下の full run で不変（errors 13,451／mismatch 0、emit 13,443）を確かめた。実 project
  （DT／azure／material-ui）の再比較はこの slice では行っていない（resolver の変更は file の包含や package の重複解消に
  効き得るので、次に corpus を比較するときに確かめる）。
- **test**：strada の挙動を pin していた contract test 26 件（`module_resolution_contract.rs`；うち 13 件は名前も tsgo の
  挙動に改名）を tsgo の挙動に書き換え、JS の置換 token 展開の予算を pin していた 1 件と `js_string_ops` の token 置換の
  unit test は置換と一緒に削除した。harness の `customConditions`（trim しない）と vendored manifest（`.trace.json`、
  pin 58,386）の test、`tsc_types` の version test も追随。判断は tsgo の source により、実 FS で表せる layout は tsgo 実機
  （7.1.0-dev-19dadef8、scratchpad `p43/probe*`）で確かめた：A＝一致した `typesVersions` mapping の全 substitution が
  失敗しても `types` → index に進む（`index.d.ts`）；B＝`"exports": {".": null}` は `@types/dep` があっても未解決；
  C＝package field の親 directory が無くても substitution（`types/good.ts`）に解決；D／F＝`imports` の bare target は
  primary の realpath → retry → alternate（F は TS7016 の「There are types at …/dep/legacy.d.ts」）；E＝key `"*"` と
  `"7"` では `*` が勝つ（package.json の順）；G／H＝配列でない値は mapping でない、配列の文字列でない要素は空文字、配列形の
  `typesVersions` は「Expected type … got 'array'」で paths 無し、`$&` は literal；I＝`paths` の exact key でも
  substitution の最初の `*` を置換（`value**` → `value*`）、空 capture は `*` を消す（`../literal/.ts`）。
- **loader（`loader.rs`）**：task の収集順（`enter_collected_task`）で、その file の type reference 解決の trace →
  module 解決の trace を tsgo の `moduleNames` の順（synthetic（tslib／jsx）→ import の出現順（static → dynamic）→
  文字列 literal の module augmentation；`SourceRequestPlan::module_request_order`、trace 時だけ作る）に再生。directory ごとの memo を共有した解決は先頭行の
  containing file をその file に差し替える。automatic types task はその解決の trace。lib replacement の trace は最後に
  key 順。`PreparedProgram::resolution_trace()` で公開。harness の option 表で `traceResolution` が「受理して無視」の群に
  入っていたので `CompilerOptions.trace_resolution` に写すよう直した。
- **runner／script／CLI**：`ts71/trace_baseline.rs`（`sanitizeTrace` の移植、3 test）、report の `trace`／`trace_detail`、
  `--dump` で `<stem>.trace.json`、ratchet の 7 列目（`trace`）、CLI は `--traceResolution` で trace 行を診断より前に出す
  （tsgo は Program 構築中に出す）。
- trace case（dev binary、`@traceResolution` の 157 case ＝ 186 configuration）：**149 configuration が byte 一致**、34 は
  native runner の既存 skip、3 は `bundlerDirectoryModule(module=node18|node20|nodenext,moduleresolution=bundler)` の
  条件行（tsgo `require`、港 `import`）——`GetImpliedNodeFormatForFile` が tsgo では全 TS／JS file に format を与えるのに
  対し港は bundler の file で `None`（checker も consume する）ため。errors／emit は全 configuration で不変。implied
  format の追随は別 slice に送る（制限として記録）。
- 計測（最終 bytes、macOS、`taskpolicy -c maintenance nice -n 20`、1 worker）：12,748 case／4,662 s、errors full 13,451（mismatch 0）、emit full 13,443（mismatch 0、not assessed 8）、
  types full 12,677／mismatch 90（P4-2 の 12,676／91 から：`customConditions(resolvepackagejsonexports=true)` が
  `customConditions` を trim しなくなって一致——tsgo の trace でも条件は `' browser'`）、symbols full 12,715／mismatch 52（不変）、
  sourcemap full 13,451／mismatch 0（不変）、**trace full 13,448／mismatch 3／not assessed 0**（baseline も trace 行も無い
  configuration は一致として数える；mismatch は上の `bundlerDirectoryModule` の 3 件）、harness error 15（不変）。
  ratchet 0 regressions：7 列目（trace）を全 13,451 行に記録（full 13,448、none 3）、types の 1 行が none → full、下がった行は無い。fix commit `a9465cc4f`。
- local（maintenance clamp、1 job）：`cargo fmt --all -- --check`；clippy（`--all-targets -- -D warnings`）program／types／
  emitter／checker／compiler／conformance／harness；test program＋types＋emitter＋conformance＋harness＋compiler 1,748/0、
  checker 1,797/0（最終 bytes `a9465cc4f`、`taskpolicy -c maintenance nice -n 20`、1 job）。

### P4-3のhostedの記録とmerge（2026-10-08）

- hosted：最終候補 `8ce4f25d2`（fix `a9465cc4f` ＋ packet の記録）の run 37734504815（`plan` 33s、`rust` 10m34s、
  `conformance (TypeScript 7.1)` 19m59s、`gates` 13s。全て成功）。merge → `f0daa6dce`（merge commit）。
- 計測（conformance の全体実行、crate test）は上の記録のとおり、merge前に最終bytes `a9465cc4f` で行った。実project の比較は
  行っていない（resolver の挙動の修正は file の包含や package の重複解消に効き得るので、次に corpus を比較するときに確かめる）。
  性能は計測していない（利用者の指示）。
- 次：P4-4（transpile（41）：`internal/transpile` への pin 直し、runner、cases と baseline の vendoring）。P4-3 の残り
  （bundler の implied node format、CLI の version 文字列、harness の他の list option の trim、checker の `replace_*_value`、
  exports の条件の列挙順）は別 slice。
