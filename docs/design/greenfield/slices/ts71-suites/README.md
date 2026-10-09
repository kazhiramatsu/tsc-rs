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
| P4-7 | tscWatch／tsbuildWatch（107）、api（2） | watch mode、api の encoder（API server は roadmap の P5） | 下の「P4-7 計画」 | 大。P4-7a〜d に分ける |

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
- tsc suite の help／init／showConfig／locale（cs など）は実装して一致させる（利用者、2026-10-08。P4-6 計画）。
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

## P4-4 transpile（`transpileModule`／`transpileDeclaration`）（2026-10-08）

tsgo の `TranspileBaselineRunner`（testrunner/transpile_runner.go）は `tests/cases/transpile` の case
（`\.[cm]?[tj]sx?$`、25 file）を compiler runner と同じ規則で unit に分け、`declarationMap`／`sourceMap`／
`inlineSourceMap` だけで configuration を展開し（名前は `declarationMap=true` の綴り）、空の option に directive を
`SetOptionsFromTestConfig` で載せる（compiler runner の harness 既定値も skip 規則も無い）。configuration ごとに、
`emitDeclarationOnly` でなければ `<name>.js`、`declaration` なら `<name>.d.ts` を書く。baseline は unit の入力
（`//// [unit] ////` ＋本文、本文が `\n` で終わらなければ `\r\n`）、unit ごとの `TranspileModule`／`TranspileDeclaration`
の出力（`.js`／`.d.ts`、別 file の source map があれば `.map`）、診断があれば `//// [Diagnostics reported]` ＋
`GetErrorBaseline`（並べ替えてから描き、診断の file 名 `/<unit>` を unit 名に置換）。

- **vendoring**：`scripts/vendor_typescript_native.py` の TREES に `tsc/testdata/tests/cases/transpile`（25 file）と
  `tsc/testdata/baselines/reference/transpile`（41 file：`.js` 19、`.d.ts` 22）を足し、同じ commit で再 vendoring した
  （追加のみで既存の file は不変、`--check` 一致、vendored 58,452 file、manifest に 2 set）。
- **compiler**：`crates/compiler/src/transpile.rs` を 6.0.3 の `transpileWorker`（raw option の fixup、
  `getDefaultCompilerOptions` の既定値、`moduleName`／`renamedDependencies`／`jsDocParsingMode`、caller の綴りへの
  file 名の写像、route の evidence）から tsgo の `transpileWorker`（transpile/transpile.go:118-258）に合わせ直した。API は
  `TranspileOptions { compiler_options: Option<CompilerOptions>, file_name, report_diagnostics }` →
  `TranspileOutput { output_text, diagnostics, source_map_text }`。option の消去と強制は `transpile_compiler_options`
  （incremental／declaration／emitDeclarationOnly／noEmit／lib／outFile／composite／tsBuildInfoFile／
  allowImportingTsExtensions／noEmitOnError／declarationDir を消し、isolatedModules（verbatimModuleSyntax でなければ）／
  noCheck／noResolve／suppressOutputPathCheck／allowNonTsExtensions を立て、declaration の有無で declaration／
  emitDeclarationOnly／declarationMap／isolatedDeclarations を決める。paths／rootDirs／types は港では Program option で、
  worker は空で作る）。入力名は `GetNormalizedAbsolutePath(name, "/")`（既定 `module.ts`、jsx が None でなければ
  `module.tsx`）、host は大文字小文字を区別する in-memory、declaration は target の既定 lib 名で `/lib` に barebones lib。
  診断は `reportDiagnostics` のとき syntactic＋config＋program、emit の診断は常に。診断の file 名は tsgo と同じ `/<unit>`。
- **emitter**（transpile の 2 baseline で見つかった source map の差。tsgo の `Clone` は node の位置を保つが、港の
  `clone_node` は Strada の `cloneNode` と同じ合成 node を作る）：
  - `using` の巻き上げ（using.go `hoistInitializedVariable`）：export された top-level の `using d = …` を
    `d = __addDisposableResource(…)` にするとき、代入先の clone に名前の位置を写した（名前の終端の mapping が出る）。
  - namespace の `export import inner = internal;`（runtimesyntax.go `visitImportEqualsDeclaration` →
    `createExportStatement`）：member 名の clone に名前の位置を写し（`GetNamespaceMemberName`）、代入に original と宣言全体の
    source map range を付けた（`createExportAssignment`：`inner` の mapping と、`;` の直前で宣言の終端への mapping）。
- **harness**：`upstream_suites/transpile.rs`（case の列挙、unit、`formatTranspileConfigurationName`、空の option への
  `apply_compiler_settings`、harness option の `reportDiagnostics`、`pretty`）。`native::configurations` は
  `configurations_varying_by`（`GetFileBasedTestConfigurations` に runner の vary-by を渡す形）の compiler runner 版にした。
- **runner・script・CI**：`crates/conformance/src/ts71/suites.rs` と binary `suites-ts71`（report
  `target/suites-ts71/<profile>/report.json`、`--filter`、`--dump`、case は大きな stack の thread で panic を捕まえる）、
  `scripts/suites_ts71.py`（`--check`／`--update`）、ratchet `ratchets/ts71/suites-7.1.0-dev-19dadef8.tsv`（`transpile/<baseline>`
  と `full`／`none`、41 行）。hosted は新しい job を足さず、既存の `conformance (TypeScript 7.1)` job で `conformance-ts71`
  と同じ cargo 呼び出しで `suites-ts71` も build し、lane A の後に `scripts/suites_ts71.py --check` を実行する（release
  build をもう 1 回しないため。上の「runner・report・ratchet・CI」の hosted job 案から変更）。CLAUDE.md の merge 条件・
  ratchet の規則・単一 writer の一覧と root README の「Run CI」に suites を足した。
- 結果：emitter を直す前は 39/41 が byte 一致（`jsWithSourceMapBasic(sourceMap=true).js`、
  `jsWithInlineSourceMapBasic(inlineSourceMap=true).js` が上の 2 箇所の mapping で不一致）、直した後は **41/41 が byte 一致**。
- 制限（41 baseline には現れない構造上の差）：tsgo の `SkipModuleResolution`（module／type reference／lib reference を
  解決せず、implied format を拡張子だけから決める）を港の Program は持たない。`noResolve` の下で import を解決し
  （in-memory host には入力と lib しか無いので、結果は未解決か入力自身への解決だけ）、declaration では `/// <reference lib>`
  の library file も探す（host に無く、`reportDiagnostics` でも何も報告されないことを probe で確かめた）。港の
  `clone_node` は合成 node を作るので、他の transform の clone でも source map／comment が tsgo と異なり得る
  （conformance の source map baseline には現れていない）。
- tests：compiler `transpile_contract`（6：option の消去と強制、何も解決しないこと、emit の診断は常に出ること、既定の
  file 名、namespace alias と `using` の source map。期待値は tsgo の baseline の出力）、harness `transpile_suite_expansion`
  （2：41 個の baseline 名の予測、configuration と option）と unit 2、conformance の unit 2、manifest の件数の pin を
  58,386 → 58,452。
- 計測（最終 bytes、macOS、`nice -n 20`、2 worker、利用者の作業中の負荷を抑えた設定）：transpile suite 41 baseline／full 41
  （`scripts/suites_ts71.py --check`：0 regressions、ratchet は `--update` で 41 行とも `full`）。conformance 12,748 case／481 s、
  errors full 13,451（mismatch 0）、emit full 13,443（mismatch 0、not assessed 8）、types full 12,677／mismatch 90、symbols full
  12,715／mismatch 52、sourcemap full 13,451／mismatch 0、trace full 13,448／mismatch 3、harness error 15（すべて P4-3 と同じ）。
  ratchet 0 regressions、上がった行は無い（emitter の 2 つの修正は conformance の baseline を変えなかった）。vendoring commit
  `8f314274e`、fix commit `609a2de2d`。
- local（`nice -n 20`、2 job）：`cargo fmt --all -- --check`；clippy（`--all-targets -- -D warnings`）emitter／checker／compiler／
  conformance／harness；test emitter 635、harness 31、conformance 37、compiler 418、checker 1,797（全て 0 失敗）、
  `.github/ci/test_replay.py` 6。workspace 全体の test と clippy は hosted の `rust` job に任せた。実 project の比較は行っていない
  （emitter の変更は上の 2 構文の source map range だけ）。parallel control は checker を変えていないので行っていない。
  性能は計測していない（利用者の指示）。

### P4-4のhostedの記録とmerge（2026-10-08）

- hosted：最終候補 `5842b6ded`（vendoring `8f314274e` ＋ fix `609a2de2d` ＋ packet の記録）の run 37756272860（`plan` 33s、
  `rust` 7m35s、`conformance (TypeScript 7.1)` 19m6s（lane A の後に `scripts/suites_ts71.py --check`）、`gates` 14s。全て成功）。
  merge → `139893d72`（merge commit）。
- 計測（conformance の全体実行、transpile suite、crate test）は上の記録のとおり、merge前に最終bytes `609a2de2d` で行った。
  実project の比較は行っていない。性能は計測していない（利用者の指示）。
- 次：P4-5（tsoptions（80：parseCommandLine 53＋parseBuildOptions 27）＋ config/tsconfigParsing（87）：Go 表の Rust 化、
  `Args::`／`CompilerOptions::`／`FileNames::`／`Errors::` 等の形式の描画、vendoring）。`suites-ts71` と
  `scripts/suites_ts71.py` に suite を足す。P4-4 の残り（`SkipModuleResolution`、clone の位置）は制限として上に記録。

## P4-5 tsoptions／config（command line と tsconfig の parse）（2026-10-08）

tsgo の `internal/tsoptions` の 2 つの test file は、Go の表を入力に baseline を書く。

- `commandlineparser_test.go`：`TestCommandLineParseResult`（33 scenario）と `TestParseCommandLineVerifyNull`（1＋
  5 種の option × null／非 null／後続 option／末尾 の 18 scenario、string／number の 2 種は test だけの tsconfig 専用
  宣言 `optionName` を足す）が `tsoptions/commandLineParsing/parseCommandLine/<name>.js`（53）を、
  `TestParseBuildCommandLine`（22＋5）が `…/parseBuildOptions/<name>.js`（27）を書く。parseCommandLine は
  `Args::`（引数を `"…"` で囲んで `, ` で連ねる。escape なし）、`CompilerOptions::`（parser の raw OrderedMap を
  最初の代入順・最後の値で compact JSON。enum は tsgo の数値、lib は library の file 名、`null` はそのまま）、
  `FileNames::`、`Errors::`（`WriteFormatDiagnostics`、`\n`）。parseBuildOptions は `buildOptions::`（`core.BuildOptions`
  の field 順）、`compilerOptions::`（build と共通の compiler option を `core.CompilerOptions` の field 順で。watch option は
  含めない）、`Projects::`（無ければ `.`）、`Errors::`。
- `tsconfigparsing_test.go`：`parseConfigFileTextToJsonTests`（7）が `config/tsconfigParsing/<title> jsonParse.js`
  （`Input::`／`Config::`（2 space の indent）／`Errors::`（pretty＋ANSI、`\n`、cwd `/`））を、
  `parseJsonConfigFileTests`（32）と `TestParseTypeAcquisition`（8）が `<title> with json api.js` と
  `<title> with jsonSourceFile api.js`（各 40）を書く：`Fs::`（`vfs.WalkDir` の名前順に `//// [path]\r\n内容\r\n\r\n`）、
  `configFileName::`、任意の `CompilerOptions::`／`TypeAcquisition::`（tsgo struct の field 順、`omitzero`、2 space）、
  `FileNames::`、`Errors::`（pretty＋ANSI、`\r\n`、cwd ＝ basePath）。

- **vendoring**：`scripts/vendor_typescript_native.py` の TREES に `tsc/testdata/baselines/reference/tsoptions`（80）と
  `tsc/testdata/baselines/reference/config/tsconfigParsing`（87）を足した（`config/matchFiles` の 142 は producer の無い
  orphan なので入れない）。追加のみ、`--check` 一致、vendored 58,619 file（harness の pin 58,452 → 58,619）。
- **入力の表**：Go の表を `crates/conformance/src/ts71/suites/tables.rs` に写した（使い捨ての変換 script で Go の literal を
  読み、Rust の literal に出力。表ごとに Go の行を記す）。baseline は引数・file・text をそのまま含むので、写し間違いは
  不一致として現れる。`TestParseCommandLineVerifyNull` の loop と `TestParseTypeAcquisition` の入力（`/apath`、`a.ts`／
  `b.ts`）は runner が Go と同じ手順で組む。
- **runner**：`ts71/suites/tsoptions.rs`（command line、`parse_command_line_with_declarations`／`parse_build_command_line`
  の結果を tsgo の値に写す）、`ts71/suites/tsconfig.rs`（in-memory host で `parse_config_root_plan`、`Fs::` の walk、
  CompilerOptions／TypeAcquisition の struct 順の JSON、診断の pretty 描画）、`ts71/suites/go_json.rs`（`encoding/json/v2`
  の compact／indent 出力、`core.CompilerOptions`（135 field）・`core.BuildOptions`・`core.TypeAcquisition` の field 順、
  tsgo の enum の数値：moduleDetection auto=1、newLine crlf=1、watch 系は Strada より 1 大きい）。`suites-ts71` と
  `scripts/suites_ts71.py` に suite `tsoptions` と `config` を足した（ratchet 行 `tsoptions/commandLineParsing/…`、
  `config/tsconfigParsing/…`）。
- **json api**：tsc-rs は config file を text から parse する（jsonSourceFile api と同じ）。tsgo の json api は同じ text を
  object に変換してから parse し、同じ診断を位置なしで報告する（vendored の 40 組を比べると、違いはその位置だけ）。runner は
  同じ parse の診断から位置を外し、root の text 自身の parse 診断を除いて描く（tsgo の json api は
  `ParseConfigFileTextToJson` の診断を捨てる）。tsc-rs に JSON object から parse する入口は無い（制限）。
- **港の変更（tsgo に合わせた）**：
  - `parse_command_line_with_declarations`（tsgo `ParseCommandLineTestWorker`：catalog の宣言の後に test の宣言を足す）と
    `CompilerOptionDeclaration::tsconfig_only`。unknown option の綴りの提案もその宣言を含める。
  - watch option の list 要素（`excludeDirectories`／`excludeFiles`）の file spec 検査を command line にも足した。tsgo の
    `validateJsonOptionValue` は spec の message を引数なしで作るので、文言は `'{0}'` のまま（tsconfig の watchOptions も同じ
    に直した。6.0.3 では spec が入っていた）。
  - `parse_config_file_text_to_json`（tsgo `ParseConfigFileTextToJson`：object（値の無い text は `{}`）と、text に parse
    診断があればその最初の 1 件、無ければ変換の診断）。root の object 化は `config_file_object` として config の parse と共有した。
  - jsconfig.json の既定 compiler option から `allowSyntheticDefaultImports` を外した（tsgo `getDefaultCompilerOptions`：
    allowJs／maxNodeModuleJsDepth 2／skipLibCheck／noEmit。7.1 では常に有効なので check には影響しない）。
  - type acquisition の既定値を tsgo の `getDefaultTypeAcquisition` にした：jsconfig.json は `enable: true` だけ、tsconfig.json
    は何も無し（6.0.3 は `enable`／`include: []`／`exclude: []`）。
- **6.0.3 の観測の更新**：`crates/program/tests/fixtures/h2-8b-config-{root-options,root-boundaries,reuse}.json`（6.0.3 の
  観測）を、7.1 で変わる field だけ記録し直した（P3-5b の diagnostics fixture と同じ扱い：version 2、typescript
  `7.1.0-dev-19dadef8`、source に根拠）。変更は script で種類を確かめてから書いた：acquisition probe の既定値（enable
  false → absent 64、include [] → absent 66、exclude [] → absent 68）、`type_acquisition` の既定値の削除（28×3）、TS5065 の
  文言（8）。他の field は 1 つも変わっていない。`config_option_catalog_contract` の jsconfig 既定値と
  `config_root_plan_contract` の type acquisition の 2 test も tsgo の値に直した。
- 結果：初回（描画を書いた直後）は tsoptions 80/80、config 51/87（36 件はすべて CompilerOptions／TypeAcquisition の節：
  type acquisition の既定値、jsconfig の `allowSyntheticDefaultImports`、`maxNodeModuleJsDepth: 1.0`（Go の数値の書き方））。
  直した後は **tsoptions 80/80、config 87/87、transpile 41/41（計 208）が byte 一致**。
- 制限（vendored の baseline には現れない）：tsc-rs は JSON object から config を parse する入口を持たない（json api は
  同じ text の parse で代える）。tsconfig の watchOptions の数値（`watch_options()` の JSON）はまだ Strada の番号
  （tsgo の baseline に watchOptions の出力は無い）。
- tests：program の単体 2（test の宣言、`'{0}'`）と contract 1（`parse_config_file_text_to_json`）、conformance の単体
  （go_json 2、tsoptions 1、tsconfig 2）。
- 計測（最終 bytes、macOS、`nice -n 20`、2 worker）：suites（`scripts/suites_ts71.py --update` → `--check`）**config 87／87、
  tsoptions 80／80、transpile 41／41**、ratchet は 41 → 208 行（全て `full`）、0 regressions。conformance 12,748 case／473 s、
  errors full 13,451（mismatch 0）、emit full 13,443（mismatch 0、not assessed 8）、types full 12,677／mismatch 90、symbols full
  12,715／mismatch 52、sourcemap full 13,451／mismatch 0、trace full 13,448／mismatch 3、harness error 15（すべて P4-4 と同じ）、
  ratchet 0 regressions、上がった行は無い。計測の後に `tables.rs` の 1 つの literal を同じ値の escape 形に直した（Go の text の
  「tab の前の空白」が git の whitespace 検査にかかるため）ので、suites の release binary を作り直して 208／208 を確かめ直した。
  vendoring commit `f81ba0ed1`、fix commit `38ea9b1e4`。
- local（`nice -n 20`、2 job、最終 bytes）：`cargo fmt --all -- --check`；clippy（`--all-targets -- -D warnings`）program／
  checker／compiler／conformance／emitter／harness／incremental；test program 599、harness 31、conformance 42、emitter 635、
  incremental 28、compiler 418、checker 1,797（全て 0 失敗）、`.github/ci/test_replay.py` 6。workspace 全体の test と clippy は
  hosted の `rust` job に任せた。実 project の比較は行っていない（config の既定値の変更は常に有効な `allowSyntheticDefaultImports`
  と、使われない type acquisition だけ）。parallel control は checker を変えていないので行っていない。性能は計測していない（利用者の指示）。

### P4-5のhostedの記録とmerge（2026-10-08）

- hosted：最終候補 `d6b39c40f`（vendoring `f81ba0ed1` ＋ fix `38ea9b1e4` ＋ packet の記録）の run 37765052524（`plan` 33s、
  `rust` 8m6s、`conformance (TypeScript 7.1)` 19m59s（lane A の後に suites 3 種の `--check`）、`gates` 13s。全て成功）。
  merge → `9cabc9a41`（merge commit）。
- 計測（conformance の全体実行、suites、crate test）は上の記録のとおり、merge前に最終bytes `38ea9b1e4` で行った。
  実project の比較は行っていない。性能は計測していない（利用者の指示）。
- 次：P4-6（tsc（224）＋ tsbuild（192）：`TestSys`（仮想 FS・時計・差分・sanitizer・readable buildinfo）、Go 表の Rust 化、
  `ExitStatus`、help／init／showConfig／locale の採用判断、vendoring）。大きいので計画を先に決める。

## P4-6 計画：tsc／tsbuild（2026-10-08）

利用者の指示（2026-10-08）：「testing/fstest.MapFS を Rust っぽい形で移植しましょう。そして残りの項目を全て実装して
ください。設計はおまかせします。Go をまんま移植すると Rust っぽさがなくなるので、そこだけは注意してください」、続けて
「この機能は wasm 化した時の目玉になる予感がします」。未決事項だった help／init／showConfig／locale は実装して一致
させる（known に留めない）。

- **System 層（tsgo `tsc.System`）**：command line は `tsc_compiler::system::System`（file system、current directory、
  標準 library の directory、時計、環境変数、terminal、出力）の上で動く。process は `NativeSystem`、test は in-memory、
  将来の WebAssembly embedding は自前の System を渡す。driver（CLI・`tsc -b`・emit）は `std::fs`、process の環境変数、
  wall clock を直接触らない。tsgo の test hook（`CommandLineTesting`）は同じ trait の optional な観測口。
- **file system 層（tsgo `vfs.FS`）**：`tsc_host::vfs`。`FileSystem` trait（`std::fs` に倣った名前：`read`／`metadata`／
  `read_dir`／`canonicalize`／`write`／`append`／`create_dir_all`／`remove`／`set_modified`、`io::Result`）、`OsFs`、
  `VfsCompilerHost`（任意の FileSystem 上の `CompilerHost`）、`MemFs`。`MemFs` は `testing/fstest.MapFS`＋tsgo `vfstest` の
  意味を **木構造** で持つ（Go の flat map＋canonical key の写しにしない）：directory ごとに canonical 名（case-insensitive なら
  fold）の `BTreeMap`、node は作成・最終書き込み時の綴りと mtime を持つ。symlink は読み書きで辿り、remove／set_modified／
  entry は辿らない（tsgo と同じ）。link 越しの write は与えた綴りを保つ。`create_dir_all` は tsgo `mkdirAll` の「link に
  当たった prefix から link 先の実 directory で walk をやり直す」を写す。時刻は `Clock` から読み、`SteppingClock` は読むたびに
  一定量進む（tsgo `TestClock`）。`from_entries` は tsgo `FromMapWithClock` の順（`comparePathsByParts` の順に全 entry を
  stamp、その後に親 directory を作る）。tsgo が「link の下の literal entry」を黙って持つ入力（link を通しては届かない）は
  error にする。
- **scenario の記録**：tsc_test.go／tscbuild_test.go の scenario は Go の値と edit の closure なので、Go の表を手で写さない。
  `scripts/tsctests_scenarios.py` が pinned checkout の tsctests package を `internal/execute/tsrsdump` に複製し、記録器
  （`scripts/tsctests_dump/tsrs_dump_test.go`）を足して one-shot の test（watch を除く）を走らせ、各 scenario の file・引数・
  環境・terminal と、各 edit の TestSys 操作（write／append／prepend／replace／replaceAll／remove／rename／touch）を順に
  `vendor/typescript-native/<profile>/tsctests-scenarios.json` に書く。runner.go と sys.go への差し込みは一意な文字列
  置換で、合わなければ script が止まる。test は記録中も baseline を reference と比べるので、記録は全 baseline を再現した
  run からだけ書かれる。incremental correctness の clean system での edit 操作が違う 2 件（`forIncrementalCorrectness`
  を見る edit）は `nonIncrementalOps` に別記。`--check` は記録し直して byte 比較。
- **runner**（`ts71/suites/tsc.rs`、`tsc/system.rs`、`tsc/build_info.rs`）：tsgo `TestSys` の移植（library file、
  build info の `FakeTSVersion` 化と readable 版、written／未読 library の追跡、FSDiffer、出力 sanitizer、edit の再生、
  incremental correctness の clean build、`OnEmittedFiles`、`OnProgram`）。suite は `tsc` と `tsbuild`。
- **slice**：P4-6a（土台＋初回値＋incremental の testing data＋見つかった修正）、P4-6b（help／init／showConfig／locale）、
  P4-6c（残りの class）。content mapper の baseline（tsc 5 種・tsbuild 2 種）は content mapper を後回しにする指示のまま
  対象外。watch（tscWatch／tsbuildWatch、記録には 68 scenario が入る）は P4-7。

## P4-6a tsc／tsbuild の土台と初回値（2026-10-08）

- **vendoring**：`scripts/vendor_typescript_native.py` の TREES に `reference/tsc`（224）と `reference/tsbuild`（192）。
  追加のみ、`--check` 一致、vendored 59,035 file（harness の pin 58,619 → 59,035）。記録 `tsctests-scenarios.json`
  （483 scenario：tsc 223・tsbuild 192・watch 68、890,517 byte）。tsc の 224 のうち 1 件は watch の test が書く。
- **System 層**：`run_cli` は契約そのまま（`NativeSystem` を作って出力を回収）、`execute_command_line(system, args,
  testing)` が公開入口。`NativeSystem` の compiler host は従来の cached `FsCompilerHost`＋埋め込み library
  （`NativeCompilerHost` として system.rs へ移動）。emit は `SystemEmitFileSystem`（System の FileSystem へ書く。並列書き込みは
  worker budget が 2 以上のとき）、`tsc -b` の時刻・mtime・削除・status の時刻と `TZ` も System 経由。診断の描画は
  `Format`（current directory・case profile・pretty）を受け、plain reporter が process の case profile を probe していた
  のを run の FileSystem のものに直した。
- **testing data**：testing session（`ProgramSession::with_testing`）では file version と signature に本文を付け
  （tsgo `hashWithText`）、emit した file を常に集める。`OnEmittedFiles` は emit の順に mtime を進め、`tsc -b` の mtime
  cache も更新する。snapshot は各 file の semantic 診断が「この run で計算」「旧 state から保持」「cache 無し」のどれかと、
  signature の更新種別（computed .d.ts／stored at emit／used version）を記録し、`OnProgram` で runner が
  `SemanticDiagnostics::`／`Signatures::` を書く。
- **初回値**（runner を書いた直後）：tsc 68／224、tsbuild 6／192。主な class：incremental の testing data が無い（190 前後）、
  version に本文が無い（build info 約 140）、`tsc -b` の pretty 出力が source text 無しで描画に失敗（約 50）、help／init／
  showConfig／locale、config program の `traceResolution`。
- **港の修正（tsgo に合わせた）**：
  1. `tsc -b` は pretty の error summary と project の config 診断を source text 無しで描いて失敗していた。orchestrator が
     parse した config と各 project の診断 file の text を保持する（実 project でも pretty の `tsc -b` に error があれば
     起きた）。
  2. `traceResolution` を config program（emit／no-emit）で受け付け、trace を run の出力の前に出す（explicit file と同じ。
     `tsc -b` は project の buffer に）。
  3. 参照 project の source を include する root は、その project の出力 d.ts を選ぶ（tsgo は program に出力を置く）。
     prepared program が「root が SourceFileId と合わない」で失敗していた。
  4. build の root reader が `resolvedRoot` の組を逆に読んでいた（tsgo は `[resolved, root]`）。参照 project の source を
     root に持つ project が毎回 rebuild になっていた。
  5. command line 自身の error（TS5023 など、`--project` と file の併用、tsconfig の不在）の後に error summary を出さない
     （tsgo `tscCompilation` は報告して止まる）。
- **結果**：**tsc 149／224、tsbuild 136／192**（ratchet に 416 行を足し 624 行、うち full 493）。transpile 41、tsoptions 80、
  config 87 は変わらず全て full。
- 残る class（131）：help／version 8（tsc 7・tsbuild 1）、`--init` 7、`--showConfig` 16、locale 2、`--generateTrace` 2、
  content mapper 8（対象外）、その他 88（tsc 36・tsbuild 52）。その他の主なもの：`target: es5` の emit 7（tsgo 7.1 は
  TS5108 を報告して ES2015 として出力、port は ES5 へ下げる。2026-09-29 の指示で ES5 の実装は残すので扱いを P4-6c で決める）、
  resolveJsonModule 9、module 解決（package.json scope・pnpm 風の layout・symlink・出力 d.ts からの解決）約 12、
  project references（参照先の設定・不正な field）6、incremental（大文字小文字を区別しない FS、global 診断、comment だけの
  edit 後の再計算）8、`--listFilesOnly`＋incremental、`forceConsistentCasingInFileNames` 2、`tsc -b` の status・時刻・
  rootDir 共有 build info の報告など。
- tests：host の vfs 18（vfstest_test.go の移植＋adapter）、incremental の resolved root 1、compiler の system contract 4
  （別 test process：command line は process 全体の checker mode を設定するため）、conformance の runner 単体 6。
- 計測（最終 bytes `e544587ec`、macOS、`nice -n 20`、2 worker）：conformance 12,748 case／491 s、errors full 13,451
  （mismatch 0）、emit full 13,443（not assessed 8）、types full 12,677／mismatch 90、symbols full 12,715／mismatch 52、
  sourcemap full 13,451、trace full 13,448／mismatch 3、harness error 15（P4-5 と同じ）、ratchet 0 regressions。suites
  （`scripts/suites_ts71.py --update` → `--check`）0 regressions。
- local（`nice -n 20`、2 job、最終 bytes）：`cargo fmt --all -- --check`；clippy（`--all-targets -- -D warnings`）host／
  incremental／compiler／conformance／program／harness；test host 46、incremental 29、compiler 422（contracts 401、
  system 4、unit 13、その他 4）、conformance 48、program 599、harness 31（全て 0 失敗）。workspace 全体の test と clippy は
  hosted の `rust` job に任せた。実 project の比較と parallel control は行っていない（checker を変えていない。build の
  root 読みと project 参照の root の修正は実 project にも効くので、P4-6 の終わりに DT／azure／material-ui で比べる）。
  性能は計測していない（利用者の指示）。

### P4-6aのhostedの記録とmerge（2026-10-08）

- hosted：最終候補 `a8c0f201b`（vendoring・System 層・vfs・runner・port の修正・ratchet・packet の記録）の run 37778876128
  （`plan` 32s、`rust` 11m0s、`conformance (TypeScript 7.1)` 15m47s（lane A の後に suites 5 種の `--check`）、`gates` 14s。
  全て成功）。merge → `8b8b43fb5`（merge commit）。
- 計測（conformance の全体実行、suites、crate test）は上の記録のとおり、merge 前に最終 bytes `e544587ec` で行った。
  実 project の比較は P4-6 の終わりに行う。性能は計測していない（利用者の指示）。
- 次：P4-6b（help／version／init／showConfig／locale）。branch `fix/ts71-tsc-cli-features`（base `a8c0f201b`）。

## P4-6b help／version／init／showConfig／locale（2026-10-08）

利用者の指示（「残りの項目を全て実装してください」）どおり、P4-6a で known に残した command line の機能を tsgo に合わせて
実装した（known に留めない）。

- **vendoring**：`scripts/vendor_typescript_native.py` の FILES に option 宣言の Go source 7 本（`tsc/internal/tsoptions/`
  `declscompiler.go`／`declsbuild.go`／`declswatch.go`／`commandlineoption.go`／`enummaps.go`、`tsc/internal/core/`
  `compileroptions.go`／`watchoptions.go`）と、tsgo が埋め込む翻訳 catalog 13 本（`tsc/internal/diagnostics/loc/<locale>.json.gz`）。
  追加のみ、`--check` 一致、vendored 59,055 file（harness の pin 59,035 → 59,055）。
- **option 宣言表**：`scripts/tsgo_option_declarations.py` が vendored Go source から `crates/compiler/src/options/gen.rs` を生成
  （名前・短縮名・種類・file path か・command line 専用か・簡易 help に出すか・説明・category・既定値の説明、enum map を宣言順で、
  help が省く deprecated key、list の要素宣言、`core.CompilerOptions` の field 順）。message は `diagnosticMessages.json` の
  key と code から port の `gen.rs` の名前へ写す。`--check` は生成し直して byte 比較。gen.rs は `#[rustfmt::skip]`。
- **help／version**（`crates/compiler/src/help.rs`、tsgo `execute/tsc/help.go`）：`PrintVersion`、簡易 help、`--all`、`-b --help`、
  `getHeader`（terminal 幅が足りれば右端に TS icon、右寄せは最大 120）、`createColors`（`defaultIsPretty` と `OS`／`WT_SESSION`／
  `TERM_PROGRAM`／`COLORTERM`／`TERM`）、`getPrettyOutput`（説明を幅で byte 単位に折る）、`formatDefaultValue`（enum の既定値は
  key、型の違う既定値（newLine の "lf"）は空）、`getPossibleValues`（同値の key を `/` で、deprecated key を除く）、category の
  初出順のまとまり。config が無く file も無いときは tsgo どおり version と help を出して exit 1（`--showConfig` なら TS5081）。
- **version の文字列**：`--version` と help の version は tsgo の `core.Version()`（`7.1.0-dev`）にした（従来は
  `7.1.0-dev-19dadef8`）。build info の `version` と typesVersions が使う版と同じ。commit は profile 名（vendor の directory）が持つ。
- **`--init`**（`crates/compiler/src/init.rs`、tsgo `execute/tsc/init.go`）：`generateTSConfig` を `ConfigWriter`（行と未出力の
  option を持つ）で写した。command line の値が既定値を置き換え、`Optional` の項目は command line にあれば comment を外し、残りの
  option は command line の順で末尾へ。enum は同値の最初の key（`--target es2015` は `"es6"`）、`lib` は要素の key。既に
  `tsconfig.json` があれば TS5054（exit 0）。書き込みは System の `FileSystem::write_creating_dirs`。
- **`--showConfig`**（`crates/compiler/src/show_config.rs`、tsgo `tsoptions/showconfig.go`）：`ConvertToTSConfig` の移植。
  option は `core.CompilerOptions` の field 順、category が Command-line Options／Output Formatting のものと command 自身の
  option を除き、enum は同値の最初の key、file path は config file からの相対、`lib` は要素の key。続けて `impliedOptions`（14 個、
  依存先が書かれていて値が既定と違うもの：`module`／`moduleResolution`／`moduleDetection`／`isolatedModules`／
  `preserveConstEnums`／`declaration`／`declarationMap`／`incremental`／`useDefineForClassFields`／
  `resolvePackageJsonExports`／`resolvePackageJsonImports`／`resolveJsonModule`／`allowJs`／`allowImportingTsExtensions`、
  getter は tsgo の `core.CompilerOptions` の写し）、references、files、include（validated、既定の `**/*` だけなら省く）、
  exclude（validated、`exclude` が無ければ outDir／declarationDir）、compileOnSave。JSON は Go の `jsontext` の 4 space indent、
  末尾改行なし。program crate の `ConfigRootPlan` が validated include／exclude spec を持つようにした。
- **`--locale`**（`crates/compiler/src/locale.rs`）：13 catalog を gzip のまま埋め込み、初回に展開（`miniz_oxide`）。tag は BCP 47
  の構文で読み、言語で翻訳を選ぶ（中国語は script／region で簡体／繁体）。`tsc_diagnostics::MessageCatalog` を通して、診断
  （pretty／plain、関連情報、error summary）、`tsc -b` の status、help、`--init` の見出しを翻訳する。
- **結果**：tsc 149 → **180**／224、tsbuild 136 → **138**／192（help／version 8、`--init` 7、`--showConfig` 16、locale 2）。
  transpile 41、tsoptions 80、config 87 は変わらず。
- **実 tsgo との比較**（`target/typescript7/bin/tsc-19dadef8`、一時 directory）：`--locale ja`、`--locale de --pretty false`、
  `--locale zh-TW --foo`、`--locale fr -b --verbose`、`-b --help`、`-b --help --locale es`、`--init --locale ko`、
  `--locale pt-br --all`、`--help`、`--all`、`--version`、`--locale it --version`、`--showConfig`（config のみ、
  `--module preserve --verbatimModuleSyntax`、`--strict false --lib es2015,dom --outDir out`）が stdout と exit status まで一致。
- **残る制限**：`--locale xx-YY` は tsgo が TS6048（x/text が未登録の subtag を拒否）、port は構文だけ見て英語。tsgo は CLDR の
  距離で近い言語／地域の翻訳も選ぶ（x/text の matcher）が、port は言語の一致だけ。x/text の registry と matcher の移植は後続の
  候補。`--generateTrace` 2 件は P4-6c で扱う（types file が tsgo の type id の生成順を要る）。watch の test が書く tsc baseline 1 件
  （`adds-color-when-FORCE_COLOR-is-set`）は P4-7。
- tests：compiler の unit（help 4：version 行と Czech、header の位置、help-all の候補と既定値、折り返し；init 3：tsgo baseline の
  全文、command line の順、同値の最初の key；show_config 3：implied、struct 順、相対 path と lib の key；locale 2）、
  CLI contract 1（version と help、`--all`、`--init` と TS5054、`--showConfig` の JSON、`--locale ja --version`）、
  version の contract を `7.1.0-dev` へ。

### P4-6bのhostedの記録とmerge（2026-10-08）

- hosted：最終候補 `70a485247`（vendoring `b488d6b80`・fix `5f592032b`・ratchet `27688ea5d`・packet の記録と main の取り込み）の
  run 37786150313（`plan` 26s、`rust` 11m21s、`conformance (TypeScript 7.1)` 19m30s（lane A の後に suites 5 種の `--check`）、
  `gates` 23s。全て成功）。merge → `28815d545`（merge commit）。
- 計測（conformance の全体実行、suites、crate test、実 tsgo との比較）は上の記録のとおり、merge 前に最終 bytes `5f592032b` で行った。
  実 project の比較は P4-6 の終わりに行う。性能は計測していない（利用者の指示）。
- 次：P4-6c（残りの class）。branch `fix/ts71-tsc-behaviors`（base `70a485247`）。

## P4-6c tsc／tsbuild の残りの class（2026-10-09）

利用者の指示（「残りの項目を全て実装してください」）どおり、P4-6a・P4-6b の後に残った class を tsgo に合わせた。

- **test harness**（`ts71/suites/tsc/`）：tsgo `CommandLineTesting` の file listing の marker（TSFILE 行・`--listFiles`・
  `--explainFiles` は `OnListFilesStart/End` の間）を incremental correctness の比較から外す。`FakeTSVersion` で書いた build
  info は compiler の version で読み直す。incremental correctness の diff は tsgo `baseline.DiffText`
  （`github.com/peter-evans/patience` v0.3.0 の patience diff と 3 行文脈の unified hunk）の移植（`patience.rs`、unit test は
  Go の library の出力で pin）。
- **config と command line**：references の検証（path が無い・文字列でない・`circular` が boolean でないと TS5024、空の path は
  TS18051）、空の references 配列は solution。command line の `null` は config の値を消す。include の理由は最初に当たった spec
  （JSON も）。node_modules の中の config は real path。drive root を持つ path。`--diagnostics`／`--extendedDiagnostics` は
  tsgo の統計表のうち port が測る行（file、行数、config・parse・check の時間、build は集計）。
- **extends の継承**（tsgo `applyExtendedConfig`）：書かれた key は値が無くても（`null`・値の欠落）継承を止める。継承するのは
  配列だけ（文字列・数値・object は継承しない）、文字列でない要素は書かれたまま残す（TS18003 の include は `[1]` と出る）。
  tsgo の JSON 変換は配列の `null` を落とし、`null` だけの配列は nil slice（spec の読み手には無いのと同じ）。tsc 6.0.3 の
  `__proto__`／truthy／文字列の文字ごとの継承の規則は捨てた。program の unit test をこれに合わせて書き直し（実 tsgo の
  `--showConfig` で確かめた）、tsc 6.0.3 の観測 fixture `h2-8b-config-extends` の 1 件（`own-null-files-inherits`）は
  tsgo で変わったので外して contract test に置き換えた。
- **参照 project の file の options**（tsgo `getCompilerOptionsForFile`／`getRedirectForResolution`）：program の file が
  参照 project の source か出力 d.ts なら、その project の options（emit module kind、module resolution、package.json の
  exports／imports、preserveConstEnums、common source directory、outDir）を `ReferencedProjectOptions` として持つ。checker は
  file ごとの resolution mode をそれで決め（`resolution_mode_for_usage`）、`canHaveSyntheticDefault` の project reference の
  分岐、出力 d.ts の const enum の TS2748 の例外、TS2878（rewrite する import が別 project へ解決し、出力の相対位置が source と
  違う）を tsgo どおりに行う。`authoritative Module resolution is missing` で止まっていた extends の rebuild もこれで直った。
- **参照 project の source**：program に入った参照 project の source（出力 d.ts が代わりにならない JSON など）は emit しない。
  別 project の source への `/// <reference path>` は出力 d.ts に redirect され（tsgo `getParseFileRedirect`）、checker の
  TS6053 の補完（loader を通らない program のためのもの）は参照 project の source を解決済みとする（provider に問う）。宣言 emit の `preserve` な参照は
  出力 d.ts を指す（`GetSourceFileForResolvedModule`）。module specifier は出力 d.ts を元の source の名で探し
  （`GetSourceOfProjectReferenceIfOutputIncluded`）、source の出力 d.ts も候補にする（`GetEachFileNameOfModule`）。monorepo の
  `import("package-c").MyType` が TS2883 になっていた。
- **`repopulateInfo`**（tsgo `RepopulateDiagnosticInfo`）：package.json の状態による chain（mode mismatch の
  TS1479 系の案内、module が見つからないときの案内）を `MessageChain::repopulate` に記録して build info に書き、cache の行を
  報告するときに今の program から作り直す（tsgo `toDiagnostic` と同じく遅延で、作り直した行を次の build info に書く）。
  詳細の計算は checker と共有（`mode_mismatch_details`／`module_not_found_details`）。
- **build info の `errors`**（tsgo `ensureHasErrorsForState` の `GetGlobalDiagnostics`）：planner が signature を作るときの
  file の無い診断（例：宣言 emit が先に型を解いて出る TS5114）と、check の global 診断（check していない JavaScript の
  filter で落ちるものも）を数える。
- **その他**：CommonJS の `export default` の文は comment range だけを持ち source map に載らない（tsgo
  `createExportStatement`）。unique symbol の property 名は tsgo の `"\xFE@<名前>@<id>"`（Rust の文字列では U+FFFD）。JSON の
  値の検証の診断は位置ごとに 1 つの規則を通らない（tsgo `validateJsonValue`）。
- **結果**：tsc 180 → **211**／224、tsbuild 138 → **182**／192。transpile 41、tsoptions 80、config 87 は変わらず全て full。
- **残る 23 件**：`target: es5` の 12 件（tsgo 7.1 は TS5108 を報告して ES2015 として出力、port は 2026-09-29 の指示どおり ES5 に
  下げる。TS5108 の診断は一致、違いは emit だけ）、content mapper 8 件（対象外）、`--generateTrace` 2 件（types file が tsgo の
  型の生成順と id を要る。利用者の判断待ち）、watch の test が書く 1 件（P4-7）。
- **後続の候補**：壊れた tsconfig の JSON の変換（tsgo は parser が tsconfig の値も検証して TS1328 を出し、変換の診断は前置
  trivia を含む位置に出る。TS5024 の型名は enum／Array。`1 + 2` のような JSON でない式を port は bounded grammar の外として
  止まる）。baseline には現れない。
- tests：compiler の statistics 2（表の揃え、行数）、conformance の patience 4（Go の library の出力で pin）、program の
  config contract（tsgo に合わせて書き直し 3：JSONC の `__proto__` は普通の key、値の無い include も継承を止める、継承は
  配列だけ；追加 2：`files: null` が継承を止める、TS18003 が文字列でない include をそのまま出す）、checker の unit 2
  （unique symbol の名前の prefix）。tsc 6.0.3 の観測 fixture `h2-8b-config-extends` は 28 件のうち 27 件を残し、
  `own-null-files-inherits` を tsgo の contract に置き換えた。
- 計測（code の最終 bytes `5777f9bfe`、macOS、`nice -n 20`、2 worker）：conformance 12,748 case／478 s、errors full 13,451
  （mismatch 0）、emit full 13,443（not assessed 8）、types full 12,677／mismatch 90、symbols full 12,715／mismatch 52、
  sourcemap full 13,451、trace full 13,448／mismatch 3、harness error 15（P4-6b と同じ）、ratchet 0 regressions。suites
  （`scripts/suites_ts71.py --update`：75 行 none → full、下げた行なし → `--check`）0 regressions。並列対照（`--checkers 4`、
  全件、458 s）：15,224 構成が一致、違う 4 構成は記録済みの「emit が先」の 4 構成（`mutuallyRecursiveInference`、
  `incorrectRecursiveMappedTypeConstraint`、`typeParameterWithInvalidConstraintType`、`recursiveMappedTypes`）。
- local（`nice -n 20`、2 job、最終 bytes）：`cargo fmt --all -- --check`；clippy（`--all-targets -- -D warnings`）diagnostics／
  types／syntax／program／incremental／emitter／checker／compiler／conformance；test diagnostics 51、types 43、syntax 246、
  program 601、incremental 29、emitter 635、checker 1,797、compiler 437、conformance 52（全て 0 失敗）。workspace 全体の test と
  clippy は hosted の `rust` job に任せた。実 project の比較（P4-6 の終わり）は下の hosted の記録に書く。性能は計測していない
  （利用者の指示）。

### P4-6cのhostedの記録とmerge（2026-10-09）

- hosted：最終候補 `41157148e`（fix `e89e48082`・`5777f9bfe`、ratchet `2cddf30a7`、main の取り込み `273f4994a`、packet の記録）の
  run 37808231058（`plan` 27s、`rust` 11m6s、`conformance (TypeScript 7.1)` 20m30s（lane A の後に suites 5 種の `--check`）、
  `gates` 17s。全て成功）。merge → `e23a8d471`（merge commit）。
- 実 project の比較（P4-6 の締め。最終 code `5777f9bfe` の release build を凍結して使用、`tsgo --singleThreaded` と
  `TSRS_CHECKERS=1`、`--noEmit`、`nice -n 20`、1 job）：material-ui 38／38、azure-sdk-for-js 2,756／2,756（505 s）、
  DefinitelyTyped 9,067／9,067（1,159 s）が診断と exit status まで一致。比較で clone に書かれた `.tsbuildinfo`（material-ui 5 件、azure 338 件）は
  消した。性能は計測していない（利用者の指示）。
- 次：P4-7（watch／api）。`--generateTrace` の 2 件は利用者の判断待ち（types file は tsgo の型の生成順と id を要る）。

## P4-7 計画：generateTrace／watch／api（2026-10-09）

利用者の指示（2026-10-09）：「`--generateTrace` については現在の実装でいいので機能としては用意してください。tsgo と同じで
なくても同じ機能を搭載して。P4-7（watch／api）にすすんでください」。`--generateTrace` は tsgo と同じ出力（trace・型・legend の
file）を持つ機能として入れ、型の id と生成順は port の checker のもの（tsgo と byte 一致を求めない）とする。

- **P4-7a `--generateTrace`**（tsgo `internal/tracing`、`execute/tsc.go` の `startTracingIfNeeded`）：`tsc` の compile（`-p`・
  config・explicit file。tsgo と同じく `-b` と watch では動かさない）で、指定 directory に `trace.json`（Chrome trace event の
  配列：metadata、`createProgram`・`createSourceFile`・`bindSourceFiles`／`bindSourceFile`・`checkSourceFiles`／
  `checkSourceFile`・`emit`／file ごとの emit・`emitBuildInfo` の B／E、checker の深さ上限の instant、10 ms の sampling 境界を
  跨いだ `checkExpression` などの X）、checker ごとの `types_<n>.json`（checker が作った全ての型の descriptor：id、intrinsic 名、
  symbol 名、recursion identity の token、union／intersection の構成、alias 引数、keyof・indexed access・conditional・
  substitution・reference・reverse mapped・evolving array の参照、宣言の位置、flag 名、anonymous／literal／union などの表示）と
  `legend.json` を書く。thread id は tsgo の規則（main 1、checker 2+n、file は `xxh3("file:" + path)`）。test（`CommandLineTesting`）
  では時刻を 1 ずつ進む counter にし、sampling event を出さない（tsgo の deterministic mode）。`--singleThreaded`（1 checker・
  serial worker）と `--checkers <n>` も tsgo どおり効かせる。2 件の baseline は型の file が違うので `none` のまま（理由を記録）。
- **P4-7b `tsc --watch`**（tsgo `execute/watcher.go`、`watchmanager`）：`Watcher`（初回 build、`DoCycle`：event の取り出し、
  tsconfig の mtime 検査、関係する変更の判定、incremental な再 build、`Found N errors. Watching for file changes.`）、watch status
  reporter（画面 clear・時刻）、`WatchManager`（望む directory watch の計算：wildcard directory、config の directory、program が
  触れた file の directory、存在しない directory の祖先への fallback、`CanWatchDirectory`）。backend は trait（test は tsgo の
  `MockWatchBackend` の移植、process は OS の通知、WebAssembly は embedding が渡す）。前回の program の状態は memory に持つ
  （build info を書かない project でも incremental に再利用）。runner は `-w` の scenario を `DoCycle` で進め、`Watch Registrations::`
  を書く。記録器を `TestWatch`（`tscwatch_test.go`、39 scenario）まで広げる。
- **P4-7c `tsc -b --watch`**（tsgo `build/orchestrator.go` の watch 部分）：project ごとの watch、`checkTasksForEventChanges`、
  package.json の lookup の監視、再 build の順序。tsbuildWatch 65。
- **P4-7d api の encoder**（tsgo `api/encoder`）：source file の binary 形式（header、string table、extended data、structured data、
  node 28 byte）を port の AST から書き、tsgo の `formatEncodedSourceFile` で 2 baseline を比べる。tsgo の `Kind` の番号と名前の表、
  `ForEachChild` 順の NodeList。decoder と unit test も移す。
  （decoder は P4-7d で API server と一緒に移すことに改めた。P4-7d の記録を参照。）
- **API server**（tsgo `api` の session／protocol、`cmd/tsc --api`、TypeScript 側 client `packages/typescript/src/api`）は
  roadmap の P5（公開 API）と同じ大きさの別作業として、P4-7d の後に計画を立てて着手する（Go の session test 17 file を Rust の
  test として移すのが一致の基準）。
- **orphan**：`tsc/commandLine/adds-color-when-FORCE_COLOR-is-set.js` は固定 commit のどの test も書かない（`tsc_test.go` の
  `colorTest` に無い名前。P4-6b の「watch の test が書く」は誤り）。`config/matchFiles` と同じく対象外として runner から外す。

## P4-7a `--generateTrace`（2026-10-09）

利用者の指示（「現在の実装でいいので機能としては用意してください。tsgo と同じでなくても同じ機能を搭載して」）どおり、tsgo の
`--generateTrace` と同じ file・event・descriptor を書く機能を入れた。型の id と生成順は port の checker のもの。

- **session**（`tsc_types::tracing`、tsgo `internal/tracing`）：`Tracing` は event を 1 つの lock の下の buffer に書き、`finish` で
  `trace.json`（Chrome trace event の配列）、checker ごとの `types_<n>.json`、`legend.json`（types file の path 順）を返す。command は
  それを System の FileSystem に書く（tsgo は 256 KiB ごとに追記するが、port は memory に持って最後に書く。失敗は tsgo と同じく
  `Warning: Failed to stop tracing: …` を出力に書く）。`Span`（B／E。終わりの event の引数を足せる）、`Sample`（10 ms の sampling
  境界を跨いだときだけ X）、`instant`（I、`s: "g"`）、`CheckerTracer`（`checkerId` を足し、legend に登録）。thread は tsgo の規則
  （main 1、checker 2+n、file は `xxh3("file:" + path) % 10⁹ + 10⁶`、衝突は +1、初出で thread_name の metadata）、JSON は tsgo の書式
  （field の宣言順、省略、引数の key は sort、Go の float 表記、HTML escape なし）。test harness（`CommandLineTesting`）の下では
  deterministic（時刻は 1 ずつ進む counter、sampled event なし）。
- **記録点**：command の compile（`-p`・config・explicit file。tsgo と同じく `-b` と watch では記録しない）。`createProgram`（B と E の間に、
  loader が parse した file の `createSourceFile` を開始順に書く。loader は `PreparsedSyntax` に parse の時刻を残す）、checker の driver の
  `bindSourceFiles`（library の bind の前から program の file の bind の後まで）と file ごとの `bindSourceFile`、checker が parse し直す
  file の `createSourceFile`、`checkSourceFiles`（serial と shard の check の周り）、checker の `checkSourceFile`（実際に check した file）、
  sampled の `checkExpression`／`checkVariableDeclaration`／`checkDeferredNode`／`checkTypeParameterDeferred`／`structuredTypeRelatedTo`、
  `getVariancesWorker`（終わりの event に variances の文字列）、深さ上限の instant 8 種（`checkTypeRelatedTo_DepthLimit`・
  `recursiveTypeRelatedTo_DepthLimit`・`typeRelatedToDiscriminatedType_DepthLimit`・`traceUnionsOrIntersectionsTooLarge_DepthLimit`・
  `getTypeAtFlowNode_DepthLimit`・`instantiateType_DepthLimit`・`removeSubtypes_DepthLimit`・`checkCrossProductUnion_DepthLimit`）、
  emit の `emit`（program。`--noEmit` でも空の span）と unit ごとの `emit`／`emitJsFileOrBundle`／`emitDeclarationFileOrBundle`、
  `emitBuildInfo`。checker は module provider（`AuthoritativeModuleProvider::tracing`）から session を受け、state の構築時に shard の
  index で `CheckerTracer` を持つ（incremental の planner の state は program の checker ではないので持たない）。emitter は
  `EmitHost::tracing`。
- **types file**（tsgo `buildTypeDescriptor`）：checker の state が終わるとき（`Drop`）、型の arena の全ての型を書く。id は `TypeId` + 1、
  intrinsic 名（tsgo の `TypeFlagsIntrinsic`）、alias か symbol の名前（late-bound の接頭辞は `__@`）、recursion identity の token、
  tuple、union／intersection の構成、alias の型引数、keyof・indexed access・conditional（解決していない枝は -1）・substitution・
  reference（target と解決済みの型引数、deferred node の位置）・reverse mapped・evolving array（final 型）、destructuring pattern、
  最初の宣言の位置（小文字の path、1 起点の行と UTF-16 文字）、flag 名、anonymous と literal／template literal／union／intersection の
  表示（失敗は表示なし）。trace を取る session は checker の state を leak せずに drop する。
- **`--singleThreaded`／`--checkers <n>`**：tsgo の `newCheckerPool` どおり（1 worker・1 checker／n checker、上限は MAX_CHECKERS）。
  command line の process option として読む（`-b` でも）。
- **結果**：generateTrace の 2 件は `legend.json` と `trace.json` の metadata・`createProgram`・`createSourceFile` が tsgo と byte 一致。
  違うのは bind と check の順序（port は library を program の file より先に bind し、program の file を library より先に check する）と
  types file（型の id と生成順が port のもの）で、利用者の判断どおり `none` のまま。実 CLI（wall clock）では sampled event、複数 checker
  の types file、emit の span が出ることを確かめた（JSON として読める）。
- **orphan**：`tsc/commandLine/adds-color-when-FORCE_COLOR-is-set.js`（固定 commit のどの test も書かない）を runner の比較から外した
  （`ORPHAN_REFERENCES`）。`scripts/suites_ts71.py --update` は、全体実行が報告しなくなった `none` の行を ratchet から落とす（`none`
  以外の行は残り、`--check` が失敗する）。
- tests：types の tracing 4（tsgo の thread id、deterministic な event 列、legend と types file、flag 名）、compiler の system contract 2
  （deterministic な session の file と event 列・型の descriptor、`-b` は記録しない・tsconfig の option も効く）。

- 計測（code の最終 bytes `d8903ccf3`、macOS、`nice -n 20`、2 worker）：conformance 12,748 case／468 s、errors full 13,451
  （mismatch 0）、emit full 13,443（not assessed 8）、types full 12,677／mismatch 90、symbols full 12,715／mismatch 52、sourcemap full
  13,451、trace full 13,448／mismatch 3、harness error 15（P4-6c と同じ）、ratchet 0 regressions。suites（`--update` で orphan の
  `none` 1 行を落として 623 行 → `--check`）0 regressions、tsc 211／223、tsbuild 182／192、transpile 41・tsoptions 80・config 87 は全て
  full。並列対照（`--checkers 4`、全件、464 s）：15,224 構成が一致、違う 4 構成は記録済みの「emit が先」の 4 構成
  （`mutuallyRecursiveInference`、`incorrectRecursiveMappedTypeConstraint`、`typeParameterWithInvalidConstraintType`、
  `recursiveMappedTypes`）。
- local（`nice -n 20`、2 job、最終 bytes）：`cargo fmt --all -- --check`；clippy（`--all-targets -- -D warnings`）types／syntax／binder／
  program／emitter／checker／incremental／compiler／conformance／harness；test types 47、syntax 246、binder 78、program 601、emitter 635、
  incremental 29、compiler 439、conformance 52、harness 31、checker 1,797（全て 0 失敗）。workspace 全体の test と clippy は hosted の
  `rust` job に任せた。実 project の比較は行っていない（trace を取らない run での変更は tracer の有無の分岐だけで、checker の意味は変えて
  いない）。性能は計測していない（利用者の指示）。

### P4-7aのhostedの記録とmerge（2026-10-09）

- hosted：最終候補 `386ad076e`（code `d8903ccf3`・ratchet `586ebc3b5`・packet の記録）の run 37860181232（`plan` 35s、`rust` 6m49s、
  `conformance (TypeScript 7.1)` 20m14s（lane A の後に suites 5 種の `--check`）、`gates` 17s。全て成功）。merge → `73a96eab6`
  （merge commit）。
- 計測（conformance の全体実行、suites、並列対照、crate test）は上の記録のとおり、merge 前に最終 bytes `d8903ccf3` で行った。実 project の
  比較と性能は計測していない（上の記録の理由と利用者の指示）。
- 次：P4-7b（`tsc --watch`）。branch `fix/ts71-watch`（`386ad076e` から）。

## P4-7b `tsc --watch`（2026-10-09）

tsgo の `execute/watcher.go`（`Watcher`）と `execute/watchmanager`（`WatchManager`・`DirWatchSet`・`CanWatchDirectory`）を移した。
`tsc-rs --watch`（`-w`）は compile の後、compile が読んだ・探した file と directory の directory を watch し、変更が来ると前回の状態の上で
compile し直す。

- **記録器と vendoring**：`scripts/tsctests_scenarios.py` の `RUN` に `TestWatch$`（`tscwatch_test.go`）を足し、`SKIP` を
  `TestBuildWatchStopsWhenContextIsCancelled`（watcher を直接動かし baseline を書かない）だけにした。記録は 522 scenario（tsc 223、
  tsbuild 192、tsbuildWatch 65、tscWatch 42。計画の「39」は誤りで 42）。`scripts/vendor_typescript_native.py` の TREES に
  `reference/tscWatch`・`reference/tsbuildWatch` を足し、107 file を vendoring した（manifest を更新、harness の件数の pin は 59,162）。
- **`Watcher`**（`crates/compiler/src/watch/mod.rs`、tsgo `Watcher`）：初回 build（`Starting compilation in watch mode...`）、
  `do_cycle`（event の取り出し、`recheckTsConfig`（config と extends の mtime を比べ、変わったか前回失敗していれば parse し直す。
  読めない config は診断を書いて cycle を止める）、`isRelevantChange`（build が触れた path、`PossiblyMatchesFileName`・
  `PossiblyMatchesDirectoryName`、watch 下の directory）、関係しなければ前回の program を `OnProgram` に渡すだけ）、build（include の
  pattern を毎回展開し直す。tsgo の full build の `ReloadFileNamesOfParsedCommandLine`。展開し直しても config の診断は最初の parse の
  もの、つまり root が全て消えても TS18003 を出さない）、`Found N errors. Watching for file changes.`。tsgo の single-file の fast path
  （`tryUpdateProgram`）は持たず、全ての build が tsgo の full build にあたる（出力は同じ。前回の状態を使うので check と emit は
  変わった file の分だけ）。status は tsgo の watch status reporter どおり（`Starting…`／`File change detected…` の前に画面の clear
  `\x1b[2J\x1b[3J\x1b[H`。`--preserveWatchOutput`・`--diagnostics`・`--extendedDiagnostics` では clear しない。pretty では
  `[時刻] `、そうでなければ `時刻 - `）。`--preserveWatchOutput` を option として受ける。
- **前回の状態**（`crates/compiler/src/incremental.rs`・`crates/incremental/src/snapshot.rs`）：build の後の snapshot を incremental な
  build info の形（`to_watch_state`）で options と一緒に memory に持ち（`WatchState`）、次の build は `OldState::from_build_info` で
  読む。build info を書かない project でも同じ（tsgo は watch 中 `incremental.Program` を memory に持つ）。`incremental`／`composite`
  の project は `.tsbuildinfo` も書く。
- **`WatchManager`**（`watch/manager.rs`）：build が触れた path（`TrackingHost`、tsgo `trackingvfs`。読んだ・有無を調べた・列挙した・
  realpath した path を、並列の読み込みと解決の分も記録する）と include の directory・config の file から、望む directory の集合を
  `resolve_desired_dirs`（存在しない directory は存在する最も近い祖先を非再帰で、watch できる祖先が無ければ watch しない）と
  `DirWatchSet`（再帰の directory はその下を覆う。大文字小文字を区別しない FS では正規化した名前で 1 つにまとめ、最初の綴りを残す。
  再帰を非再帰に戻さない）で作り、`reconcile` で backend の watch を足し・止め・再帰の変化を張り直す。`CanWatchDirectory`／
  `PerceivedOsRootLengthForWatching`（`/home/<user>`・`c:/Users/<user>`・`//server/c$` を root とみなし、その 1 つ下より深い
  directory だけ watch する）、`ShouldIgnoreWatchPath`（`.git`、`node_modules/.*`、`.#` の lock file）、debounce（50 ms 静かになるか、
  最初の event から 500 ms）。
- **backend**（`WatchBackend` trait）：process は OS の通知（`watch/native.rs`、`notify` 8.2 の推奨 backend：macOS FSEvents、Linux
  inotify、Windows ReadDirectoryChangesW、BSD kqueue。WebAssembly の target では compile しない）。event の path は OS が返す実 path
  （`/private/tmp/…`）を watch を頼んだ綴り（`/tmp/…`）に戻す。test の harness と WebAssembly の embedding は自分の backend を渡す
  （`CommandLineTesting::watch_backend`）。
- **command line**：`execute_command_line` は `CommandLineResult { status, watcher }` を返し、`run_cli` は watcher があれば
  `Watcher::run`（戻らない）に入る。config・`-p`・command line の file のどれでも watch する（file の場合は include の pattern が無い）。
  `-b --watch` は P4-7c まで `tsc-rs:` の usage error（exit 2）のまま。
- **runner**（`crates/conformance/src/ts71/suites/tsc/system.rs`）：`TestSystem` が tsgo の `MockWatchBackend` を移した backend を持つ
  （file の書き込み・削除で、watch 下の path とその親 directory の event を送る。`pathIsUnder`）。`-w` の scenario は edit ごとに
  `do_cycle` を呼び、`Watch Registrations::` を書く。suites に `tscWatch`・`tsbuildWatch` を足した。
- **tests**：compiler の unit test（`CanWatchDirectory` の深さの規則、存在しない directory の祖先への fallback、tsgo
  `watchmanager_test.go` の `DirWatchSet` の test 5 件、無視する path、directory の path）、system contract 1 件（memory の backend
  の上で初回 build・変更の無い cycle は何も書かない・file の変更で再 build・include に合う新しい file で再 build）。
- **結果**：tscWatch 42／42 が byte 一致（full）。tsbuildWatch 65 は P4-7c（`-b --watch`）までは `none`。他の suites は変わらない。
- **実 CLI と tsgo の比較**（tsgo は同じ commit の `tsc-19dadef8`、macOS、時刻を伏せて比べる）：2 file の project で型 error を入れて
  戻す scenario は出力（画面の clear・status・診断）と emit した file が tsgo と byte 一致した。edit ごとに 2 回 cycle が走るのも tsgo と
  同じ（2 回目は自分の出力 file の event。tsgo の `PossiblyMatchesDirectoryName` は再帰の include directory の下の全ての path を関係
  ありとする）。module が現れる・file を足して消す・tsconfig を変える・入れ子の directory を足して消す、を続ける scenario でも emit
  した file は一致した。違いは 2 つ：(1) cycle の回数。tsgo（`TS_WATCH_DEBUG=1` で確かめた）は FSEvents が watch を始める直前の自分の
  初回出力の event を遅れて受け取り、余分な cycle を 1 回走らせる。OS の event の時機によるもので、決まった振る舞いではない。(2) 構文が
  壊れた tsconfig（`{ broken` など）で、port は `tsc-rs: config failure: …` を出す（tsgo は TS1136・TS1005 などの診断と `Found N errors`）。
  これは watch に限らず `-p` でも同じ既存の差で（壊れた tsconfig 6 形のうち 4 形が違う：構文の回復の後の JSONC の変換、preflight が
  対応の取れない括弧を断る、変換の診断の位置、TS1328）、壊れた config からの回復として次の slice で直す。修正後の config で watch は
  回復する（確かめた）。
- 計測（code の最終 bytes `362eede39`（vendoring `8a4634d8d`、ratchet `0512e8555`）、macOS、`nice -n 20`、2 worker）：
  conformance 12,748 case／467 s、errors full 13,451（mismatch 0）、emit full 13,443（not assessed 8）、types full 12,677／
  mismatch 90、symbols full 12,715／mismatch 52、sourcemap full 13,451、trace full 13,448／mismatch 3、harness error 15（P4-7a と
  同じ）、ratchet 0 regressions。suites（`--update` で tscWatch の
  `full` 42 行と tsbuildWatch の `none` 65 行を足して 730 行 → `--check`）0 regressions、tscWatch 42／42・tsc 211／223・tsbuild 182／192・
  tsbuildWatch 0／65・transpile 41・tsoptions 80・config 87。checker は変えていないので並列対照は行っていない。
- local（`nice -n 20`、2 job）：最終 bytes で `cargo fmt --all -- --check`、clippy（`--all-targets -- -D warnings`）types／syntax／
  binder／program／emitter／checker／incremental／compiler／conformance／harness、test program 601。他の crate の test は
  `config.rs` の doc comment の位置を直す前の bytes（違いはその comment だけ）で、types 47、syntax 246、binder 78、emitter 635、
  incremental 29、compiler 449、conformance 52、harness 31、checker 1,797（全て 0 失敗）。workspace 全体の test と clippy は hosted の
  `rust` job に任せた。実 project の比較と性能は計測していない（watch でない compile の経路の変更は関数の切り出し
  （`old_state`・`run_explicit_files`）と build info の直列化の引数化だけで、checker と emitter は変えていない。conformance と suites の
  出力は変わらない。性能は利用者の指示）。
- 依存：`notify` 8.2（CC0-1.0）と、その推奨 backend の crate（`fsevent-sys`・`inotify`・`kqueue`・`mio`・`walkdir` と Windows の
  `windows-sys` 系）。WebAssembly の target には入らない。

### P4-7bのhostedの記録とmerge（2026-10-09）

- hosted：最終候補 `c88f19f0e`（vendoring `8a4634d8d`・code `362eede39`・ratchet `0512e8555`・packet の記録）の run 37864598584
  （`plan` 30s、`rust` 10m41s、`conformance (TypeScript 7.1)` 12m29s（lane A の後に suites 7 種の `--check`）、`gates` 13s。全て成功）。
  merge → `b61ffd02e`（merge commit）。
- 計測（conformance の全体実行、suites、crate test）は上の記録のとおり、merge 前に最終 bytes で行った。実 project の比較と性能は計測して
  いない（上の記録の理由と利用者の指示）。
- 次：壊れた・型の違う tsconfig の診断を tsgo に合わせる slice（P4-5b。実 CLI の比較で見つけた：構文の回復、変換の診断（位置・型名）、
  値の無い property、command line 専用の option、`watchOptions`、`paths` の検証）。その後に P4-7c（`tsc -b --watch`）。

## P4-5b tsconfig の診断（壊れた・型の違う tsconfig、`paths` の検証）（2026-10-09）

P4-7b の実 CLI の比較で、構文の壊れた tsconfig が `tsc-rs: config failure: …` になる（tsgo は TS1136・TS1005 などの診断）ことを
見つけた。tsgo の binary（vendoring の commit）と port に同じ tsconfig を渡す probe（`-p . --noEmit` と `-p . --outDir out`）で範囲を
確かめると、壊れた構文だけでなく、型の違う値・値の無い property・`watchOptions`・`paths` の検証にも差があった（6.0.3 の規則と、
H0／H1 の段階の制限が残っていた）。tsgo の `parseJSONText`・`convertToJson`・`onPropertySet`・`getFileNames`・`verifyCompilerOptions`
に合わせた。

- **parse**：config も値の検証（TS1327・TS1328・TS1136）込みで parse する（tsgo の parser は JSON source を常に検証する。
  `tsc_syntax::parse_json_source_text_from_snapshot`）。preflight（`config_parser_preflight`）は入れ子の深さ（object・array・括弧で
  256）だけを制限し、式の種類では断らない（tsgo は形を問わず parse する。parser は全ての source と同じく compiler の 1 GB の stack の
  上で動く）。閉じ括弧は同じ種類の最も近い開き括弧とその内側を閉じ、対応が無ければ飛ばす（tsgo の list の回復）。package.json の
  preflight は従来どおり。
- **変換**（`convert_jsonc_value_worker` の回復の mode、config だけが使う）：property assignment でない member は飛ばし、`?` 付きも
  読み、`["name"]`・`[1]` の名前は literal の文字列（tsgo `TryGetTextOfPropertyName`）、`-数値` 以外の前置の式は値なし。配列は `null`
  の要素を落とし、要素が一つも残らない配列は nil（`null`。空の配列 `[]` は空のまま）。
- **変換の診断**（`config_json_conversion_diagnostics_from_root` を書き直した。tsgo `convertPropertyValueToJson`）：node の全範囲
  （先頭の trivia を含む `Loc`）に、property assignment でない member は TS1136、`?` は TS8009、JSON に形の無い値は渡された option の
  TS5024（型名は tsgo の `getCompilerOptionValueTypeString`：`enum`・`Array`・`string or Array`）か、option が無ければ TS1328。option は
  root の表（`compilerOptions`・`typeAcquisition`・`extends`・`references`・`files`・`include`・`exclude`・`compileOnSave`。
  `watchOptions` は無い）と各 group の宣言から引き、配列の要素は配列自身の option で変換する（`"files": [tru]` は 'files' requires Array）。
  parser の診断を真似ていた TS1327 は parser に移った。
- **notifier**（tsgo `onPropertySet`）：nil の値（`null`、JSON に形の無い値）には option 名の検査をせず（TS5023・TS5025 なし）、TS5024 は
  変換に任せる（`extends` は `getExtendsConfigPathOrArray` がもう一度報告する）。command line 専用の option は TS6266 の後で値を設定
  しない（`"help": true` が program の構築を止めていた）。object の option（`compilerOptions`・`typeAcquisition`・`paths`・list の object
  要素）は配列を受けず TS5024、要素が全て落ちた配列（Go の nil の slice）も配列として型を検査する。`references` は配列でなければ
  TS5024 Array。enum の option と enum の要素の型名は `enum`、list の要素名は tsgo の表（`moduleSuffixes` は 'moduleSuffixes'）。
  `watchOptions` は診断しない（tsgo の root の option に無い）。
- **その他の config の規則**：TS6258（root に置いた compiler option）は `compilerOptions` の key があれば値が何でも出さない
  （`jsonObject.Has`）。TS18002（空の `files`）は `extends` の値が nil なら抑えず、`references` は配列でなければ無いものとみなす
  （`getPropFromRaw`）。TS18000 は引数を渡さない（文面に `{0}` が残る。tsgo と同じ）。parse が値を検証するので、単引用符などのある
  拡張元の config は parse の診断を出して適用しない（tsgo `getExtendedConfig`）。object の値（`paths` など）は source の順（tsgo の
  ordered map。数値の key を先に並べる JavaScript の順を止めた）。`moduleSuffixes` に変換できなかった要素を残さない（`undefined` の
  suffix で探さない）。
- **`--noEmit` の経路**：plan の option の診断を全て報告する（これまで 5 つの code だけで、`paths` の TS5061・TS5062・TS5063・TS5066・
  TS5090 などが落ちていた。emit する経路は自分で検証していた）。config の `noEmitOnError`・`outFile` を受ける（`--noEmit` では効かず、
  `outFile` は削除済みの TS5102）。
- **README**：command line で compiler option を受けないという記述と、`--noEmit` と emit の flag を併せられないという記述が P3-6g
  以来古かった（tsgo と同じ出力を確かめた）ので直し、壊れた config も tsgo どおり診断することを書いた。
- **残り**（記録。この slice では直さない）：削除済みの `moduleResolution: node10`／`classic` と `resolvePackageJsonExports` などを
  書いた config の TS5098（tsgo は module から決めた解決方式で判定する。port の実効値の helper は書かれた値を返し、解決そのものに
  関わるので別の slice）。`declaration` の無い `emitDeclarationOnly` の exit status（tsgo 2、port 1。診断は同じ）。
- **tests**：program の contract `config_tsgo_diagnostics_contract`（22 形。期待値は tsgo の binary の出力そのもの：code・UTF-16 の
  位置・文面）と `rejected_values_set_no_option`。keyword recovery の contract を書き直した（深い式は compiler の stack の上で parse
  され tsgo と同じ診断になる。入れ子の上限は残る）。期待値を tsgo の振る舞いに直した test：root plan の contract 13 件、`paths` の
  projection 1 件、option の catalog 1 件、UTF-16 の cycle 1 件（どれも入力を tsgo の binary で確かめた）。h2-8b の fixture 4 file の
  20 case・40 field を再記録（各 case の file で tsgo と port の CLI の出力が byte 一致することを確かめてから。各 file の
  `source.verification` に記録。6.0.3 の観測を P4-5 で一部だけ 7.1 にした fixture で、今回の差はすべて 6.0.3 の振る舞いだった）。
  compiler の CLI contract 2 件（`--noEmit` の option の診断、壊れた config）。
- **probe**（tsgo の binary `tsc-19dadef8` と port に同じ tsconfig と `a.ts` を渡し、出力と exit status を byte で比べた）：壊れた構文
  50 形・option と値 32 形・root の形と object の option 20 形・`null` の値 20 形の 122 形が、`-p . --noEmit` と `-p . --outDir out` の
  両方で一致（slice の前は `--noEmit` で 27・5・6・16 形）。option の関係の診断 24 形は残りの 2 形（上の「残り」）を除いて一致。他に
  keyword の式 10 形、nil の配列・`references`・`extends`・TS18002・TS6258・`paths` の順序などを 30 形ほど確かめた。h2-8b の fixture の
  失敗 20 case は全て tsgo と port の CLI の出力が一致した。実 CLI の watch（FSEvents）で tsconfig を壊して直す scenario は、壊れた間の
  診断と `Found 3 errors` が tsgo と一致し、直すと回復した（違いは P4-7b に記録した OS の event の時機の cycle だけ）。
- 計測（code の最終 bytes `dc1385033`、fixture `82762e5ae`、macOS、`nice -n 20`、2 worker）：conformance 12,748 case／479 s、
  errors full 13,451（mismatch 0）、emit full 13,443（not assessed 8）、types full 12,677／mismatch 90、symbols full 12,715／mismatch 52、
  sourcemap full 13,451、trace full 13,448／mismatch 3、harness error 15（P4-7b と同じ）、ratchet 0 regressions。suites の `--check`
  0 regressions（config 87・tsoptions 80・tsc 211／223・tsbuild 182／192・tscWatch 42 は変わらない。ratchet の変更なし）。checker は
  変えていないので並列対照は行っていない。
- local（`nice -n 20`、2 job、最終 bytes）：`cargo fmt --all -- --check`、clippy（`--all-targets -- -D warnings`）types／syntax／
  binder／program／emitter／checker／incremental／compiler／conformance／harness、test types 47、syntax 246、binder 78、program 602、
  emitter 635、incremental 29、compiler 451、conformance 52、harness 31、checker 1,797（全て 0 失敗）。workspace 全体の test と clippy は
  hosted の `rust` job に任せた。実 project の比較は行っていない（実 project の tsconfig は正しい JSON で、変わるのは誤った値の診断と
  `--noEmit` の option の診断だけ。conformance の出力は変わらない）。性能は計測していない（利用者の指示）。

### P4-5bのhostedの記録とmerge（2026-10-09）

- hosted：最終候補 `b1ae3da51`（code `dc1385033`・fixture `82762e5ae`・main の merge `384f12183`・packet の記録）の run 37869872820
  （`plan` 31s、`rust` 7m44s、`conformance (TypeScript 7.1)` 19m54s（lane A の後に suites 7 種の `--check`）、`gates` 17s。全て成功）。
  merge → `4e7658ec0`（merge commit）。
- 計測（conformance の全体実行、suites、probe、crate test）は上の記録のとおり、merge 前に最終 bytes で行った。実 project の比較と性能は
  計測していない（上の記録の理由と利用者の指示）。
- 次：P4-7c（`tsc -b --watch`）。branch `fix/ts71-build-watch`（`b1ae3da51` から）。

## P4-7c `tsc -b --watch`（2026-10-09）

tsgo の build orchestrator の watch（`build/orchestrator.go` の `Watch`・`DoCycle`・`checkTasksForEventChanges`・`computeDesiredWatches`・
`updateWatch`・`resetCaches`、`buildtask.go` の pending と `updateDownstream`）を移した。

- **orchestrator が持ち主になる**（`crates/compiler/src/build.rs`）：host・library catalog・`BuildCommand`・current directory を借りずに
  持ち、watch の cycle をまたいで残る。host は cycle ごとに作り直す（tsgo は cycle の終わりに cache を消す）。
- **task の状態**（tsgo `BuildTask`）：`pending`、`initial_cycle`（最初の graph の task で、まだ build していない）、`dirty`（config が
  変わり、次の graph で parse し直す）、watch の `downstream`、program が探した package.json（tsgo `packageJsons`）。graph を作り直す
  ときは、変わっていない task をそのまま使い、dirty の task は build info の entry だけ引き継いで作り直す（tsgo
  `GenerateGraphReusingOldTasks`）。`build_project` は tsgo どおり：pending なら status を求めて build か整理、pending でなければ持って
  いる errors をもう一度報告する。errors は cycle をまたいで残り（`Found N errors` に数える）、task の出力は cycle ごとに新しい。
  build した project は downstream に宣言が変わったかを伝えて pending にする（`updateDownstream`。最初の cycle ではしない）。
- **cycle**（`watch/mod.rs` の `BuildWatcher`、tsgo `DoCycle`）：event が無ければ何もしない。overflow なら全ての config を dirty に
  する。それ以外は変更が関わる project を求める：config か extends の file が変わったら dirty（graph を作り直す）、root の file、
  build info に載る root でない file、build info と program の package.json の lookup（file の変更か、それを含む directory の削除）が
  変わったら status を消す。include の pattern を持つ project は file 名を展開し直し、変われば status を消す。どれにも当たらず watch 下の
  directory が変わったら全ての project の status を消す。関わる project があれば `File change detected…`、必要なら graph を作り直し、
  順に build して `Found N errors. Watching for file changes.`（と統計）を出す。最後に time の cache を進め（`updateWatch`：build
  info を持たない project の出力の時刻だけ残る）、watch を張り直し（`computeDesiredWatches`：config の directory、extends の
  directory、include の directory を再帰で、root の directory、build info の file の directory、package.json の lookup の directory
  （node_modules の中なら node_modules を持つ directory まで）、`ResolveDesiredDirs`）、cycle の cache を消す。build info の file 名の
  default library は library directory で解決する（tsgo `resolveBuildInfoFileName`）。
- **書き込みの時刻**：tsgo は書いた時（`writeFile` の `Sys.Now()`）に、build info を持たない project の出力の時刻と build info の
  entry の時刻を取る。build の watch の cycle では、emit の直後、test harness が emit した file に時刻を押す前に file ごとに時刻を
  取る（`CliRoute.write_times`）。
- **見つけて直したもの**：書いたばかりの build info を compiler host で読み直していたため、OS の host では build の前に見た「無い」
  が返り、次の cycle で「output file … does not exist」になった（実 CLI の比較で見つけた）。file system から読む（tsgo は書いた
  object をそのまま持つ）。
- **command line**：`execute_command_line` の watcher は `CommandWatcher`（`Program`／`Build`）。`-b --watch` の usage error と、使う所の
  無くなった `CliError::Usage` を消した。watch の status 行は `WatchStatus` として program の watch と共有する（build では command line
  の option が画面を残すかを決める。tsgo と同じ）。project の run は program の package.json の lookup を返す
  （`BuildProjectRun.package_json_lookups`、tsgo `PackageJsonLookupPaths`）。
- **README**：`-b --watch` を書き、未対応の記述を消した。
- **tests**：compiler の system contract 1 件（memory の backend の上で 2 project の初回 build、変更の無い cycle は何も書かない、
  上流の宣言の変更で両方を build、下流だけの error で下流だけ）。`-b --watch` を usage error とした CLI の test 2 件を直した。
- **結果**：tsbuildWatch 63／65 が byte 一致。残りの 2 件（`demo/updates-with-bad-reference`・`demo/updates-with-circular-reference`）は
  `target: es5` の project で、tsgo は TS5108 を出して ES2015 で emit し、port は ES5 へ下げる（2026-09-29 の指示で ES5 を残す。tsc の
  ES5 の 12 件と同じ class）。
- **実 CLI と tsgo の比較**（OS の通知）：2 project（core を参照する app、composite）で core の変更・app の error・修正・core の
  tsconfig の変更を続ける scenario は、emit した file が全て一致し、出力も一致した。違いは tsgo が初回 build の自分の出力の event を
  遅れて受け取って走らせる余分な cycle（`TS_WATCH_DEBUG=1` で確かめた。P4-7b に記録した OS の時機の差）と、それによる status の文面
  （次の cycle の app の「oldest output」が build info か `index.js` か）だけ。
- **残り**：package.json の lookup の path を realpath にしない（tsgo は存在するものを realpath にする。symlink の node_modules だけで
  違う）。`--builders` の並列 build は従来どおり受けて無視する。
- 計測（code の最終 bytes `fa54ded99`（ratchet `fa45b9ee9`）、macOS、`nice -n 20`、2 worker）：conformance 12,748 case／479 s、
  errors full 13,451（mismatch 0）、emit full 13,443（not assessed 8）、types full 12,677／mismatch 90、symbols full 12,715／
  mismatch 52、sourcemap full 13,451、trace full 13,448／mismatch 3、harness error 15（P4-5b と同じ）、ratchet 0 regressions。suites
  （`--update` で tsbuildWatch の 63 行を `none` から `full` へ。730 行 → `--check`）0 regressions、tsbuildWatch 63／65・tscWatch 42・
  tsc 211／223・tsbuild 182／192・transpile 41・tsoptions 80・config 87。checker は変えていないので並列対照は行っていない。
- local（`nice -n 20`、2 job、最終 bytes）：`cargo fmt --all -- --check`、clippy（`--all-targets -- -D warnings`）types／syntax／
  binder／program／emitter／checker／incremental／compiler／conformance／harness、test types 47、syntax 246、binder 78、program 602、
  emitter 635、incremental 29、compiler 452、conformance 52、harness 31、checker 1,797（全て 0 失敗）。workspace 全体の test と clippy は
  hosted の `rust` job に任せた。実 project の比較と性能は計測していない（watch でない build の経路の変更は、orchestrator の持ち方、
  task の結果の cycle ごとの初期化、errors を消さずに集めること、build info を file system から読むことで、checker と emitter は変えて
  いない。conformance と suites の出力は変わらない。性能は利用者の指示）。

### P4-7cのhostedの記録とmerge（2026-10-09）

- hosted：最終候補 `15c95c8b7`（code `fa54ded99`・ratchet `fa45b9ee9`・main の merge `2636dc3b1`・packet の記録）の run 37872424752
  （`plan` 24s、`rust` 11m17s、`conformance (TypeScript 7.1)` 13m34s（lane A の後に suites の `--check`）、`gates` 13s。全て成功）。
  merge → `6badbb3da`（merge commit）。
- 計測（conformance の全体実行、suites、実 CLI と tsgo の比較、crate test）は上の記録のとおり、merge 前に最終 bytes で行った。実 project の
  比較と性能は計測していない（上の記録の理由と利用者の指示）。
- 次：P4-7d（api の encoder）。branch `fix/ts71-api-encoder`（`6badbb3da` から）。

## P4-7d api の encoder（2026-10-09）

tsgo の API が client に送る source file の binary 形式（`internal/api/encoder`、protocol 9：64 byte の header、string table、
extended data、msgpack の structured data、28 byte の node）を port の構文木から書く crate `crates/api`（`tsc-rs-api`、`tsc_api`）を
加えた。api suite の 2 baseline（`encodeSourceFile.txt`・`encodeSourceFileWithUnicodeEscapes.txt`）が byte 一致。

- **crate**：`tsc_api::encoder`（`encode_source_file`・`encode_node`・`build_node_index_table`、baseline の書式
  `format_encoded_source_file`。kind は tsgo の番号、位置は UTF-16、flags は tsgo の NodeFlags。木は再帰でなく stack で歩く）、
  `tsc_api::references`（tsgo `parser/references.go`：tsgo の parser が parse 時に記録する imports・module augmentation・ambient
  module 名。dynamic import と `require` は tsgo どおり text の `import`／`require` から `GetNodeAtPosition` で探す）、
  `tsc_api::parse_source_file`（API server が file を parse するとおり：script kind は拡張子から、JSDoc は全て）。
- **表の生成**（`scripts/generate_api_encoder.py` → `crates/api/src/encoder/generated.rs`、`--check`）：vendored の
  `internal/ast/kind_generated.go`（kind の番号と名前）と `internal/api/encoder/encoder_generated.go`（`getNodeDataType` と
  `getChildrenPropertyMask`。mask の bit 順が visitor の順）を port の `kind.rs`・`nodes.rs` に対応させる。名前の違う property
  （`PostfixToken` は `question_token`／`exclamation_token`、`DefaultType` は `default` など）、port に無い property（tsgo の JS reparser
  が埋めるもの。mask の bit は残る）、tsgo に無い port の子（6.0.3 期の文法 error の受け皿）は表に明示し、それ以外の食い違いは
  生成の error にした。vendoring に 2 file と `baselines/reference/api` を加えた（`scripts/vendor_typescript_native.py`）。
- **tsgo の木の形**（tsgo の parser が tsc と違う木を作るところ。encoder は `View` で tsgo の形を書く）：
  - interface の `extends` と class の `implements` の要素は TypeReference（tsgo `parseTypeHeritageClauseElement`。property access は
    QualifiedName に）。class の `extends` は ExpressionWithTypeArguments のまま。
  - `namespace A.B` の内側の宣言は暗黙の `export` modifier（位置は宣言の位置、flag は Reparsed）を持ち、NestedNamespace flag は
    無い。JSDoc の typedef／callback の namespace 名は NestedNamespace のまま、keyword は `namespace`。
  - array binding pattern の穴は子の無い BindingElement（tsc：OmittedExpression）、JSDoc link の `A#b` は QualifiedName
    （tsc：JSDocMemberName）。
  - JSDoc：node の位置は host の位置から（2 つ目からは前の JSDoc の end から）。comment は常に NodeList で、text だけの comment
    は JSDocText 1 つ、link の後に空白だけが続けば空の JSDocText で終わる（tag が続くときは無い）。`@template` の型引数 list の範囲は
    0（tsgo が設定しない）。type parameter には JSDoc を付けない。TypeScript の file では tsgo は JSDoc を最初に読む時に新しい parser
    で parse する（`@see`／`@link` を含む comment は host と一緒）ので、JSDoc の node は host の context flag を持たない。
    `PossiblyContainsDeprecatedTag` は TypeScript では tsgo の scanner の text 検査、file の `PossiblyContainsDynamicImport` は eager に
    parse されるもの（`import(`／`import<`、import type）だけで決まる。
  - NodeList の HasTrailingComma は tsgo の定義（最後の要素の end が list の end より前）。
  - flags：tsc の NodeFlags を tsgo の番号に移す。tsgo は `Namespace`・`GlobalAugmentation`（keyword）と checker の cache を持たず、
    `ThisNodeOrAnySubNodesHasError` を立てず、`HasJSDoc` を持つ。
  - literal の token flags：tsgo は scanner の flags を literal の node に持つ（`SingleQuote` も）。port の scanner で token を scan し
    直して求める（`tsc_syntax::literal_token_flags`。JSX の属性値は escape の無い scan）。tsgo は `\u` escape の数字を読む前に
    `UnicodeEscape` を立てるので、不正な `\u` にも立つ。
- **syntax の変更**：`JSDocComment::Text` が tsgo の comment list の範囲と JSDocText の end を持つ（parser は求めていて捨てていた。
  tag の comment の範囲は tsc の文字列からは決まらない）。`literal_token_flags` を公開した。
- **見つけて直したもの**：`parse_argument_list` が tsc `parseArgumentExpression` の `doOutsideOfContext(DisallowIn | Decorator)` を
  欠いていた。decorator の呼び出しの引数の `a[0]` が element access にならず、`for (x = f("a" in o);;)` の引数で `in` が二項演算に
  ならなかった（tsgo も tsc と同じ）。encoder の比較で decorator の引数の flags の差として見つけた。syntax の test 2 件（直す前は
  2 件とも失敗することを確かめた）。
- **decoder**：tsgo の `DecodeNodes` は API server が client から受けた node を印字する（`printNode` など）ためのもので、port では
  node を作る factory と printer への接続が要る。P4-7 計画の「decoder も移す」を改め、API server（roadmap の P5）と一緒に移す。
- **tests**：encoder_test.go の 4 件（baseline 2、`BuildNodeIndexTable`、protocol と content mapper の field）、module references 1 件、
  tsgo の fixture 12 file（`crates/api/tests/fixtures/tsgo/`：heritage、namespace、ambient、imports、JSDoc（TypeScript と
  JavaScript）、literal、pattern、decorator、JSX、`.d.ts`、script。`scripts/api_encoder_dump.py fixtures` が tsgo の binary で書いた
  encoding と byte 一致）。
- **corpus の比較**：vendored の compiler／conformance の case file 12,750 を 1 つの TypeScript file として tsgo と port で encode し
  比べた（`scripts/api_encoder_dump.py corpus` と ignored test `tsgo_corpus`）。12,705 が byte 一致、8 は UTF-8 でない入力（UTF-16
  の BOM などの case。port の parser は text を受ける）、37 が残り。始めは 5,491 だった。
- **検証**（最終 bytes：vendoring `8f0de02df`、修正 `79e09284e`、encoder `7347caa26`、ratchet `7f4734f0b`。macOS、`nice -n 20`、
  Cargo の job 2）：`cargo fmt --all -- --check`、Clippy（api・syntax・checker・emitter・conformance・harness、`--all-targets -- -D
  warnings`）は clean。`cargo test -p`：api（unit 5、fixture 12 file）、syntax 248、checker 1,797、emitter 635、conformance 52、program
  602、compiler 452、binder 78、incremental 29、harness 31（vendored の数の pin を 59,166 に）、types 47、全て成功。生成の `--check`、
  vendoring の `--check`（59,166 file）、fixture の `--check`（tsgo の encoder で 12／12）。`scripts/conformance_ts71.py --workers 2
  --check`：12,748 case を 469 s、全ての数が変わらない（errors full 13,451／mismatch 0、emit full 13,443、types 12,677／90、symbols
  12,715／52、sourcemap 13,451、trace 13,448／3、harness error 15）、ratchet の regression 0。`scripts/suites_ts71.py --update`、
  `--check`：api 2／2（suites の ratchet に 2 行、732 行）、他の suite は変わらない（config 87、transpile 41、tsbuild 182／192、
  tsbuildWatch 63／65、tsc 211／223、tscWatch 42、tsoptions 80）、regression 0。実 project と性能は計測していない（利用者の指示。
  checker と emitter は変わらず、parser の変更は呼び出しの引数の context だけで conformance の出力は同じ）。
- **残り**（記録）：
  - JavaScript の file で tsgo の parser が JSDoc の tag から作る宣言と型（`@typedef`・`@callback`・`@import` の文、`@type`・`@param`・
    `@returns` などの型の付け替え。Reparsed）。port の木は tsc の形で、checker は JSDoc を tsc どおり直接読む。corpus は TypeScript と
    して encode したので現れない。
  - parse error の回復：ThisNodeHasError の付く node（19：`a[]` で tsgo は element access、tsc は missing identifier）、位置（2）。
    port の parse recovery の記録（emit の回復が使う）に関わるので変えない。top-level await の reparse の AwaitContext（3）。
  - JSDoc の付く node（3：function type、回復中の if 文、arrow function の parameter）、JSDoc の text（6：tsgo は最後の断片だけを
    trim するので、改行の前の空白が残る）、JSDoc の範囲（4：comment list の終わり 2、tag の comment の text の終わり 1、
    backquote の `@param` 名の後の型の始まり 1）。
- **README・CI**：Rust API に `tsc-rs-api` を加え、limitation に encoder の残りを書いた。古くなっていた `--noEmit -p` の limitation
  （command line の emit override を受けない）を消した（`--target`・`--module` を tsgo と同じく受けることを確かめた）。CI の表に
  watch と api の suite を書き足した。`rust` job が `scripts/generate_api_encoder.py --check` も走らせる（`.github/ci/replay.py`、
  CLAUDE.md）。

### P4-7dのhostedの記録とmerge（2026-10-09）

- hosted：最終候補 `bcc0b0bab`（vendoring `8f0de02df`・修正 `79e09284e`・encoder `7347caa26`・ratchet `7f4734f0b`・packet の記録）の
  run 37878680826（`plan` 24s、`rust` 11m13s（`scripts/generate_api_encoder.py --check` を含む）、`conformance (TypeScript 7.1)` 12m24s
  （lane A の後に suites の `--check`、api 2／2）、`gates` 14s。全て成功）。merge → `774312f8d`（merge commit）。
- 計測（conformance の全体実行、suites、tsgo の encoder との corpus と fixture の比較、crate test）は上の記録のとおり、merge 前に最終
  bytes で行った。実 project の比較と性能は計測していない（上の記録の理由と利用者の指示）。
- P4-7（`--generateTrace`、watch、api の encoder）はこれで終わり。API server（roadmap の P5）は計画から始める別作業。

## P4-7 の後の小修正（2026-10-09）

P4-5b と P4-7c の記録に残した項目を確かめた。

- **`emitDeclarationOnly` だけの exit status**（P4-5b の残り）：`declaration`（か `composite`）の無い `--emitDeclarationOnly` は、tsgo
  では TS5069 を出して exit 2（出力あり）、port は exit 1（出力を飛ばした）だった。tsgo の `emitDeclarationFile` は declaration の
  path が無ければ何もせずに戻り、emit を飛ばしたとはしない（tsc 6.0 の `emitDeclarationFileOrBundle` は `emitDeclarationOnly` なら
  飛ばしたとしていた）。emitter の分岐を消した。CLI の contract 1 件（出力と exit status は tsgo の binary のもの、file は書かれない）。
- **build の watch の package.json の lookup の realpath**（P4-7c の残り）：差ではなかった。port の lookup は既に実の path にある
  （symlink の package は解決した directory で、link を通って開いた project は実の path で読まれる）。memory の system で link の
  package と link の project directory を試し、realpath にしても watch も結果も変わらないことを確かめた。P4-7c の記録の「realpath に
  しない」は誤りだった。
- **削除済みの `moduleResolution: node10`／`classic`**（P4-5b の残り）：小修正ではなく別の slice にする。tsgo
  （`GetModuleResolutionKind`）は削除済みの値を `module` から決まる既定（bundler／node16／nodenext）として扱い、trace は「not
  specified」と書き、解決そのものもそれで行う（`exports` と `types` の違う package で、tsgo は `exports` の file を、port は Node10 の
  解決で `types` の file を program に入れる。`--listFiles` で確かめた）。TS5108 が出ると意味の診断は出ないので、見える差は emit の
  経路の TS5098（`resolvePackageJsonExports`・`customConditions`）、program の file、宣言の出力、trace。port の
  `emit_module_resolution_kind` は書かれた値を返し（呼び出し 51 箇所）、tsgo どおりにすると program の test 30 件と compiler・checker の
  test が 6.0.3 の classic／node10 の解決を前提にしていて落ちる。slice は：既定への写像、trace、test の書き直し（6.0.3 の解決を
  前提にするものは 7.1 の結果に）、使われなくなる classic／node10 の resolver の削除。
- **tests**：compiler の CLI contract 1 件（上）。
- **検証**（最終 bytes：修正 `c20fb6f2d`。macOS、`nice -n 20`、Cargo の job 2）：`cargo fmt --all -- --check`、Clippy（emitter・
  compiler、`--all-targets -- -D warnings`）は clean。`cargo test -p`：emitter 635、compiler 453（新しい 1 件。直す前の port は同じ
  入力で exit 1 だったことを probe で確かめた）。`scripts/conformance_ts71.py --workers 2 --check`：12,748 case を 473 s、全ての数が
  変わらない（errors full 13,451／mismatch 0、emit full 13,443、types 12,677／90、symbols 12,715／52、sourcemap 13,451、trace
  13,448／3、harness error 15）、regression 0。`scripts/suites_ts71.py --check`：全ての suite が変わらず、regression 0。

### P4-7 の後の小修正の hosted の記録と merge（2026-10-09）

- hosted：最終候補 `7334a1ce2`（修正 `c20fb6f2d`・packet の記録）の run 37883377648（`plan` 30s、`rust` 11m5s、`conformance (TypeScript
  7.1)` 19m52s、`gates` 14s。全て成功）。merge → `2fdedb42f`（merge commit）。
- 次：API server の計画（[ts71-api-server](../ts71-api-server/README.md)、決定待ち）。削除済みの `node10`／`classic` の slice は
  その後か並行で（上の記録）。

## 削除済みの module 解決の設定（`node10`／`classic`／`baseUrl`）（2026-10-09）

P4-7 の後の小修正の記録に残した slice（利用者の判断で API server より先に。resolver は API server も使う）。TypeScript 7.1 には
classic と node10 の resolver も `baseUrl` の lookup も無い。tsgo の test runner はこれらの設定の case を skip する
（`SkipUnsupportedCompilerOptions`：node10／classic、`baseUrl`、UMD／System、ES5 など）ので、conformance では比べられない。tsgo の
source と binary で確かめた。

- **resolution kind**：tsgo `GetModuleResolutionKind` は未指定（unknown）と削除済みの classic／node10 を `module` から決まる既定
  （node16／node18／node20 は node16、nodenext は nodenext、他は bundler）にする。port の `emit_module_resolution_kind` は書かれた値を
  返していた。tsgo どおりにし、trace の「Explicitly specified module resolution kind」は書かれた値が使われる値のときだけ（削除済みの
  値は「Module resolution kind is not specified, using 'X'」）。TS5108 は書かれた値で出す（変えない）。見える差：program の file と
  宣言の出力（`exports` と `types` の違う package で tsgo は `exports` の file）、trace、TS5098（tsgo では出ない：使われる kind は常に
  node16／nodenext／bundler）、TS5109（`module: node16` に `node10` を書いても出ない）、TS5095（`amd`／`system` に `node10` を書くと既定の
  bundler に対して出る）。
- **消したもの**（約 2,400 行）：classic と node10 の resolver（`resolve_classic`、`resolve_node10` と非相対の lookup、classic の `@types`
  の lookup、Node10 の bundler の再試行と NotFound の alternate result）、optional settings の loader の区別、checker の classic／node10
  の分岐（JSX の import source と `resolveExternalModuleName` の TS2792、`createModuleNotFoundChain` の TS6280、host probe の classic の
  祖先の探索、node10 の条件）、module specifier の classic の分岐、`module_not_found_details` の kind の引数、`HostModuleResolution` の
  NotFound の alternate result（tsgo の alternate result は解決した JavaScript にだけ付く）。
- **同じ 6.0.3 の規則で残っていたもの**：
  - `baseUrl`：tsgo は parse するが resolver も module specifier も読まない（`tryLoadModuleUsingOptionalResolutionSettings` の
    「No more tryLoadModuleUsingBaseUrl」、`paths` の base は `GetPathsBasePath`：config の directory、無ければ current directory）。
    port は bare 名を `baseUrl` から探し、`paths` の base も `baseUrl` にしていた。両方消した（checker の host probe と augmentation の
    照合、宣言の module specifier も）。`outFile` の宣言の emit が common source directory を base にする tsc 6.0 の仕組みは残す
    （`outFile` は意図的に残す出力で、tsgo に参照が無い）。
  - TS5090：tsgo は `baseUrl` があっても非相対の置換を全て報告する。port は `baseUrl` があると出さなかった。
  - `paths` の pattern が合って置換が全て外れたとき：tsgo の `tryLoadModuleUsingPaths` は探索を続けるので、rooted な名前は
    `rootDirs` に進む（tsgo の trace で確かめた）。port は `rootDirs` を飛ばしていた。
  - type reference の features：tsgo は module と同じ features（node16／nodenext は固定、bundler は `resolvePackageJsonExports` を適用）
    で、resolution-mode の明示で AllFeatures を足さない（tsc 6.0 は足した）。`resolvePackageJsonExports: false` の bundler と、
    node16／nodenext で結果が変わる（tsgo の binary で 3 kind × 3 mode を確かめた）。exports の pattern trailer は 3 つの resolver の
    全てにあるので flag を消した。
  - resolution cache の libReplacement：tsc 6.0 の孤立した Node10 の options から、tsgo の `resolveLibrary`（program の resolver、
    CommonJS）に（program の loader は既にそうだった）。
- **tests**：classic／node10・`baseUrl` の解決を前提にした test は 7.1 の結果に書き直し（変わる期待値は tsgo の binary の trace と出力で
  確かめた）、その振る舞いが 7.1 に無いものは消した（resolver 6、suffix 1、checker 1、compiler 1）。全ての kind の loop から 1／2 を
  外し、単独の `Some(1|2)` は同じ振る舞いの `Some(100)`（bundler）に、classic／node10 を名に持つ test は名を直した。TS5108 が意味の
  検査を閉じていたために authoritative の row を見ずに通っていた compiler の session test 5 件は、7.1 の mode（CommonJS）の row に
  して検査を通すようにした。config の診断の fixture（`h2-8b-config-diagnostics.json`）の 13 行は 2026-10-02 の記録で「tsgo と違う、
  次の class」としていたもので、port の新しい値に記録し直し、76 case 全てを tsgo の binary（`tsgo -p <config> --pretty false`）と
  比べて一致（config の全ての行：file、行、列、code、message）。adjacent test 2 件（削除済みの kind の既定と trace、type reference の
  features。どちらも tsgo の trace の値）。
- **tsgo との比較**（`tsgo -p … --pretty false`、`--listFiles`、`--traceResolution` と出力の file）：node10（`commonjs`／`node16` の
  module）、classic（祖先の file、`resolvePackageJsonExports`・`customConditions`）、`baseUrl` と `paths`、`paths` の miss と
  `rootDirs`、type reference（bundler／node16、`resolvePackageJsonExports: false`）、libReplacement と node10 と `paths` の 9 project ×
  3。27 のうち 22 が一致（stdout、exit status、出力）、5 は下の残りの 1 つ目（`--traceResolution` だけ）。port を
  `--singleThreaded` で走らせると 27／27 が一致（merge 前に確かめた）。
- **残り**（記録）：
  - 既定の（並列の）loader の trace（merge 前に原因を確かめ直して訂正した）：port の既定の loader は file の request を visit より
    先に解決する（`resolve_requests_ahead`。32 未満は loading thread で visit の package.json scope の lookup より前に、32 以上は空の
    cache から始まる worker の resolver で）。tsgo は file の parse task で metadata の scope を読んでから解決する
    （`loadSourceFileMetaData`）。そのため最初の解決の package.json の lookup を、tsgo の trace は「according to earlier cached
    lookups」、port は「does not exist」と書く。port の `--singleThreaded` は上の比較の全てで tsgo と byte 一致。implied format の
    規則の差ではない（tsgo も package の `type` を使うのは node16／nodenext か node_modules の file だけ）。この slice の前からの差
    （removed option の無い bundler の project でも出る）で、次の slice にする。
  - P4-3 の trace の残り 3 件（`bundlerDirectoryModule` の `module` node18／node20／nodenext と `moduleResolution: bundler`。TS5095
    の設定）は別の原因：tsgo の `GetImpliedNodeFormatForFile` は package の `type` の無い `.ts` を CommonJS とし（port は tsc 6.0
    どおり未定）、node の module kind では emit の形式が implied format そのものなので、解決は `require` の condition になる。
    port の emit の形式の判定は package の `type` を tsgo の条件（node16／nodenext か node_modules）無しに読むので、既定を CommonJS
    にするだけでは `"type": "commonjs"` の bundler の project の emit が変わる。tsgo の metadata（条件付きの package の type と
    implied format）をそのまま移す。これも次の slice。
  - resolution cache の fixture（`resolution_cache/manifest.v1.json`）は node10 の options の世代と 6.0.3 と書かれた expected を持つ。
    通るが、7.1 では bundler として解決する。
- **README**：削除済みの option の段落を直した（`baseUrl`・`node10`・`classic`・`esModuleInterop: false`・
  `allowSyntheticDefaultImports: false` は tsgo と同じく効かない。他は効く）。
- **検証**（最終 bytes：`14d61b948`。macOS、`nice -n 20`、Cargo の job 2）：`cargo fmt --all -- --check`、Clippy（types・program・checker・
  compiler、`--all-targets -- -D warnings`）は clean。`cargo test -p`：program 597（新しい 2 件を含む）、compiler 452、checker 1,796、
  emitter 635、harness 31、conformance 52、incremental 29、types 47、全て成功。`scripts/conformance_ts71.py --workers 2 --check`：
  12,748 case を 477 s、全ての数が変わらない（errors full 13,451／mismatch 0、emit full 13,443、types 12,677／90、symbols 12,715／52、
  sourcemap 13,451、trace 13,448／3、harness error 15）、ratchet の regression 0（tsgo の runner が skip する設定なので、conformance は
  この slice の差を見ない）。`scripts/suites_ts71.py --check`：全ての suite が変わらず（api 2、config 87、transpile 41、tsbuild
  182／192、tsbuildWatch 63／65、tsc 211／223、tscWatch 42、tsoptions 80）、regression 0。tsgo との比較は上のとおり最終 bytes の
  release binary で 22／27（残り 5 の差は package.json の lookup の行の表記だけで、全 100 行を確かめた。port の `--singleThreaded`
  では 27／27）。実 project と性能は計測して
  いない（利用者の指示。checker と emitter の出力は conformance で変わらず、変わるのは削除済みの設定の解決）。

### 削除済みの module 解決の設定の hosted の記録と merge（2026-10-09）

- hosted：最終候補 `17357640e`（修正 `14d61b948`・packet の記録）の run 37890925922（`plan` 29s、`rust` 11m26s、`conformance (TypeScript
  7.1)` 15m59s、`gates` 16s。全て成功）。merge → `c5a50e196`（merge commit）。
- merge 前に残りの 1 つ目の原因を確かめ直し、上の記録を訂正した（PR の本文も）。port の `--singleThreaded` で tsgo との比較は
  27／27。P4-3 の trace の残り 3 件は別の原因（上）。
- 次：上の 2 つの残り（既定の loader の trace の順序と、node の module kind の implied format）の slice。その後 API server の決定
  （[ts71-api-server](../ts71-api-server/README.md)）。

## resolver の trace と implied format（2026-10-09）

削除済みの module 解決の設定の slice が残した 2 つ（その記録の「残り」）。

- **既定の loader の `--traceResolution`**：port の loader は、walk が file を visit する前に root と依存を読み、その request を
  解決する read-ahead を持つ（32 未満の request は loading thread で visit の package.json scope の lookup より前に、32 以上は空の
  cache から始まる worker の resolver で）。tsgo は file の parse task で metadata の package scope を読んでから import を解決する
  （`loadSourceFileMetaData`）ので、trace はその package.json を「according to earlier cached lookups」と書く。trace の表記は lookup
  の順で決まり、解決の結果は順に依らないので、`--traceResolution` の load は read-ahead をせず walk だけで進める（tsgo の
  single-threaded の work group の順。port の `--singleThreaded` は既にこの順で tsgo と一致していた）。trace の無い load は変わらない。
- **implied node format**：tsgo の `loadSourceFileMetaData` は package の `type` を node16／nodenext の解決（明示の `.mts`・`.cts`・
  `.mjs`・`.cjs` を除く）か node_modules の file でだけ使い、`GetImpliedNodeFormatForFile` は `.ts` 系の file を `type: module` なら
  ESM、他は CommonJS にする（type を使わない file も CommonJS。tsc 6.0 は未定にした）。emit の形式
  （`GetImpliedNodeFormatForEmitWorker`）は node の module kind では implied format そのもの、他では使われた type（か拡張子）が言う
  ときだけ。port の loader と、checker の fallback（authoritative な metadata の無い program）をこれにした。変わるのは node の
  module kind に bundler の解決の設定（TS5095）で、file は CommonJS になり `require` の condition で解決する（P4-3 の trace の残り
  3 件：`bundlerDirectoryModule` の `module` node18／node20／nodenext と `moduleResolution: bundler`）。他の設定の emit の形式は
  変わらない（`"type": "commonjs"` の bundler の project も ESM のまま。tsgo の binary で確かめた）。tsbuildinfo の
  `impliedNodeFormat` は既に tsgo の値を書いていた（incremental の `stored_implied_node_format`）。
- **tests**：adjacent test 2 件。parallel の worker の traced load が serial の load と同じ trace を書き、file の directory の
  package.json を cached と書く（read-ahead を残すと失敗することを確かめた）。nodenext／bundler と esnext／bundler の program の
  metadata（implied format、emit の形式、request の mode。未定の format に戻すと失敗することを確かめた）。
- **tsgo との比較**（前の slice の 9 project × 3 に、bundler の plain な project と `outFile` の project を加えた 33）：既定の loader で
  30／33 が一致（stdout、exit status、出力）、3 は `outFile` の project の出力の場所だけ（tsgo は削除済みの `outFile` を無視して
  file ごとに出し、port は意図的に残す `outFile` の出力をする）。stdout と trace は 33／33 が一致。
- **検証**（最終 bytes：修正 `c9b93dd6b`、ratchet `414276cae`。macOS、`nice -n 20`、Cargo の job 2）：
  `cargo fmt --all -- --check`、Clippy（program・checker・compiler、`--all-targets -- -D warnings`）は clean。`cargo test -p`：program 599
  （新しい 2 件を含む）、checker 1,796、compiler 452、incremental 29、emitter 635、harness 31、conformance 52、全て成功。
  `scripts/conformance_ts71.py --workers 2 --check`：12,748 case を 467 s、trace full 13,451／mismatch 0（13,448／3 から）、他の数は
  変わらない（errors full 13,451／mismatch 0、emit full 13,443、types 12,677／90、symbols 12,715／52、sourcemap 13,451、harness error
  15）、regression 0、3 構成が tier を上回った。`--update`（457 s、同じ数）で ratchet の 3 行（`bundlerDirectoryModule` の 3 構成の
  trace の tier を none から full）を上げた。並列対照（`--checkers 4`、451 s、`scripts/conformance_ts71_compare.py`）：15,224 構成が
  一致、違う 4 構成は記録済みの partition 依存の構成（`mutuallyRecursiveInference`、`incorrectRecursiveMappedTypeConstraint`、
  `typeParameterWithInvalidConstraintType`、`recursiveMappedTypes`）。前の slice（#717）は checker を変えたが並列対照を走らせて
  いなかった。その変更もこの対照に含まれる。`scripts/suites_ts71.py --check`：全ての suite が変わらず（api 2、config 87、transpile 41、
  tsbuild 182／192、tsbuildWatch 63／65、tsc 211／223、tscWatch 42、tsoptions 80）、regression 0。tsgo との比較は上のとおり最終 bytes
  の release binary で 30／33。実 project と性能は計測していない（利用者の指示）。
