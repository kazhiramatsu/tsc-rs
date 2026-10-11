# TypeScript 7.1 の本家への追従：計画（2026-10-11）

vendoring の commit（`19dadef8`、2026-09-29）から本家の TypeScript（microsoft/TypeScript）の main に追従する計画。利用者の
決定（2026-10-11）：API server（P5）の後、LSP の前に行い、再 vendor の前に計画を立てて確認する。P5 は LS の handler（6）・
`formatNodeForInsertion`・`preserveSourceNewlines`（P5-5b）を除いて終わった（[API server の packet](../ts71-api-server/README.md)）。

## 本家の差分（2026-10-11 に fetch）

- main の先頭は `aa8149273`（2026-10-09）で、`19dadef8` から 82 commit。7.x の tag は `v7.0.2` が最新で、7.1 の beta の tag は
  まだ無い（`[api] Prepare for beta release`（#64681）は入っている）。
- 分野ごとの commit の数（重なりあり）：checker 29、program と build 21、API と client 21、emit 15、LS 15、parser 10、標準
  library 6、diagnostics と翻訳 6。baseline を変える commit は 42。
- baseline の file（約 780）：追加 394、変更 363、削除 21、名前の変更 2。compiler 433、conformance 160、tsbuild と
  tsbuildWatch と tsc と tsoptions と config と transpile で約 90、fourslash（LS。対象外）約 80。
- 主な変更：source phase import（#63915。新しい構文 `import source x from "…"` と `lib.esnext.modulesource.d.ts`）、標準
  library（DOM と webworker の更新、esnext の iterator method と `Promise.allKeyed`）、compiler option の定義の生成（#64457）、
  declaration emit（re-export する module の index、reverse mapped type の保存、循環と省略の報告）、checker の pool の排他の
  取得、file path の型付け（#64159）。
- API（11 commit）：`getSymbol(decl)` と merged symbol の checker の method、callback FS の改善、client での binder の symbol の
  所有（source file が持つ）、同期の接続での入れ子の request への応答の順（stack の順）、config の無い program の project
  reference の diagnostics、customized な module resolution での disk の配置の import diagnostics の省略、async の dispose。

## 手順

- **S0 再 vendor**：`scripts/vendor_typescript_native.py --commit <sha> --profile 7.1.0-dev-<sha の先頭 8 桁>` で新しい profile を
  作り、`scripts/typescript7.py` で同じ commit の tsgo を作る。profile の名前を使う所（約 30：conformance と suites の runner、
  `crates/compiler/build.rs` の埋め込みの library、翻訳、生成物（diagnostics、option、encoder の表）、test、CI の
  `test_replay.py`、README）を新しい profile に替え、生成物を作り直す。新しい profile の ratchet は、そのときの状態（新しい
  baseline との違いを含む）で始める。
- **S1〜**：違いを分野ごとに直す（標準 library と message → parser（source phase import）→ checker → emit → program と
  build → API の method と client の test）。各 slice は今までと同じく full の conformance と suites で 0 regressions、ratchet は
  上げるだけ。
- **終わり**：古い profile（`vendor/typescript-native/7.1.0-dev-19dadef8` と ratchet）を除く。client の test は新しい commit の
  `packages/typescript` で走らせる。

## 決めること（利用者に尋ねる）

- 追従する commit：今の main（`aa8149273`）か、7.1 の beta の tag を待つか。

## 決定（2026-10-11）

- 利用者の選択：**今の main（`aa8149273`）に追従する**。7.1 の beta の tag が出たら、その差分だけもう一度追従する。

## S0 再 vendor（2026-10-11）

- **profile**：`vendor/typescript-native/7.1.0-dev-aa814927`（`aa8149273b23401f0a79a5f0384c42de51888693`、59,618 file。manifest を
  検査済み）。同じ commit の tsgo の tsc と tsc -b の場面を記録した（`tsctests-scenarios.json`。記録の実行で tsgo の test は全ての
  baseline を再現した）。tsgo の binary は `target/typescript7/bin/tsc-aa814927`（repository の外）。
- **本家の配置の変更に合わせた道具**：
  - option の宣言は本家で生成になった（`tools/scripts/tsc/generate-options.ts`）。`tsoptions/declscompiler.go`・`declsbuild.go`・
    `declswatch.go` は `declarations_generated.go`（と `options_generated.go`）に、core の enum と `CompilerOptions` の struct は
    `core/options_generated.go` に移り、watch の option は無くなった（`core/watchoptions.go` を削除）。vendor する file と
    `scripts/tsgo_option_declarations.py` は新しい file を読む。
  - file の path に型が付いた（#64159）：tsc の場面の記録の `testFs.Chtimes` と API の encoder の dump の道具は `tspath` の型を使う。
  - `scripts/typescript7.py` は `aa8149273` を pin し、`target/typescript7` の実体の path を使う（worktree が共有の checkout に
    link していると、`go.work` の module の相対 path が合わない）。
- **切り替え**：profile の名前を使う所（44 file）、生成物（diagnostics：message 8 件を追加、18061 は `source` も挙げる。option：
  watch の option が無くなり help の WATCH OPTIONS の節も無くなる（tsgo の `printAllHelp`）、library 3 件（`esnext.iterator`・
  `esnext.promise`・`esnext.modulesource`）、help が省く `module` の値。API の encoder と decoder の表：tsgo の `SourceKeyword` で
  以降の kind が 1 つずれる）、library の catalog（tsgo の `LibMap`、116 entry・102 file）と埋め込みの library（116 file）。古い
  profile と ratchet を除いた（計画では「終わり」に除くはずだったが、runner と CI は新しい profile だけを使い、古い ratchet を検査する
  ものは無いので S0 で除いた。古い答えは git の履歴と `target/typescript7/bin/tsc-19dadef8` で比べられる）。
- **test**：tsgo `19dadef8` の答えで pin した test を `aa8149273` の答えに替えた（API の encoder の fixture を encode し直し、session
  の test の node の handle・encoding・kind の引数はずれた kind、CLI の contract は tsgo の新しい出力（`--lib` の値、build info の
  library の version、WATCH OPTIONS が無い）、数の pin は新しい profile の数）。
- **新しい profile での状態**（release build、`e0fcda2dd`。ratchet は `--update` で作り直した：`7.1.0-dev-aa814927.tsv` に 13,478
  configuration、`suites-7.1.0-dev-aa814927.tsv` に 748 baseline）：
  - conformance：configuration 15,339（`19dadef8` では 15,228）、lane A 13,577（13,466）。errors full 13,475・mismatch 80・text 2・
    category 1・harness error 19（content mapper 15 は前から、新しい 4 のうち 2 は crash）、emit mismatch 65、types mismatch 143、
    symbols mismatch 83、sourcemap mismatch 11、trace mismatch 1。
  - suites：api 2/2、config 87/89、transpile 38/41、tsbuild 152/193、tsbuildWatch 49/65、tsc 208/229、tscWatch 40/42、
    tsoptions 63/87。
  - 違いの class（S1 以降で直す）：source phase import（`importSource/*`、#63915：errors 約 30・emit 約 25・symbols 約 25）、
    本家の修正の新しい test（enum の computed name の NoFlake 9、noFlakyDiagnostics 系、declaration emit の merged／default の
    export alias・re-export の index・reverse mapped type・循環と省略の報告（crash 2）、emit の with 文・入れ子の using の rest・
    JSX の属性・decorated class の private name・inline の source map）、types と symbols（`duplicatePackage_*` ほか checker の
    変更）、text（18116 の新しい message）、suites の watch の option の削除（tsoptions）と `tsbuild/sample` 系。
- **client の test**（`aa8149273` の `packages/typescript`、Node 25、release `e0fcda2dd`）：tsgo `aa8149273` は sync 356/356・
  async 365/365。tsc-rs は sync 6/356、async は 2 件目の後で client が待ち続けるので止めた。新しい client は server を
  `--useCaseSensitiveFileNames=<bool>` と callback の既定（`realpath:identity`・`stat:fakeStat`・`writeFile:noop`・
  `removeFile:noop`、tsgo の `internal/api/callbackfs.go`）を付けて起動し、tsc-rs の `--api` はどちらも受け付けずに終わる（flag
  だけを wrapper で除いても callback の名前で終わる）。API の slice（最後の S）で直す。main（`19dadef8` の client）では sync
  310/327・async 318/335 だった。
- **hosted と merge**：PR #740 の hosted run 38101672593（rust 8m30s、conformance (TypeScript 7.1) 16m4s、gates pass）。
  merge commit `6b62f735d`（2026-10-11）。

## S1 import の phase（2026-10-11）

- **slice の分け方**：S0 の状態で新しく違う 97 stem（errors 83・emit 65・types 54・symbols 34・sourcemap 11・trace 1・harness
  error 4）は、全て本家の 29 commit の baseline に対応する（標準 library による違いは無い：新しい library は S0 で埋め込み済み）。
  S1 import の phase（#63915・#64640・#64243）、S2 checker、S3 declaration emit、S4 emit、S5 program と build、S6 API と
  client の順に直す。計画の「標準 library と message」は S0 で済んだ。
- **source phase import**（#63915）：
  - syntax：文脈キーワード `source`（DeferKeyword の次の SourceKeyword。以降の kind は 1 つずれる。kind.rs・keywords.rs・
    tokens.rs は生成の道具が 6.0.3 の profile と共に無くなった（b27eb13bb）ので手で直し、見出しを直した）。import の phase は
    token の原文で決め（escape した `source`・`defer` は binding）、`import source x from` を読み、binding の無い phase は空の
    clause を残し、文の始まりで `source` を `defer` と同じに扱い、escape した `import.defer`・`import.source` の名前に TS1260 を
    出す。
  - tsgo の ast の helper（IsImportCall・IsImportPhaseMetaProperty・IsImportDeferMetaProperty・IsImportSourceMetaProperty・
    IsSourcePhaseImport・IsSourcePhaseImportCall）を `tsc_syntax::utilities` に一つ置き、checker・program・emitter の前処理・API
    の各自の `defer` の判定をこれに替えた。
  - program：source phase の import は module の request を作らない（tsgo は解決しない）。
  - checker：AbstractModuleSource（memo する global）、source import の target（その型の変数、alias ごとに一度作る）、
    `import.source()` はその Promise、source phase の specifier は module を解決しない、`import.source()` の usage の mode は
    ESNext、clause と call の grammar error、tsgo の plain JS の error の一覧、source import では package の target の fallback を
    しない（nodenext の CLI が止まっていた）。
  - emit：printer は `source` の phase を出し、meta-property の keyword と dot は comment つきの token（tsgo emitMetaProperty）、
    `import.source()` は相対 import の拡張子の書き換えと dynamic import の fact に入る（CommonJS は `import()` だけを require に
    する）。
  - API：clause の phase の index 3（encoder と decoder）、kind の表の生成、file の import call、source phase の specifier の
    resolved module は null（getResolvedModuleFromModuleSpecifier）。
- **#64640**：deferred import は namespace の binding が要る（18117）。**#64243**：import attributes の型の property は string
  literal だけ（template は不可）。
- **tsgo emitModuleSpecifier**：import と export の宣言の module specifier の後ろの comment は `with` の前に出る（importSource8
  で分かった。port は attributes のある全ての宣言で落としていた）。
- **test**（tsgo の答えで pin）：tsgo の parser_test.go の case、CLI の contract（nodenext で無い package の source import と二つの
  comment）、tsgo が encode した encoder の fixture（phases.ts）、decoder の phase の index、session の source specifier の
  resolved module。TypeScript 6.0.3 から記録した contract の fixture は kind の数を持つので、167 以上を 1 上げた（各 test に記す）。
  list format の test は AssertClause・AssertEntry の alias に 6.0.3 の数を pin しない。
- **結果**（release build、`7afacbd0c`）：full の conformance（`--workers 2 --check`、595 s）は 0 regressions、34 構成が受け入れの
  tier を上回る。errors full 13,475 → 13,509（mismatch 80 → 46）、emit mismatch 65 → 39、types mismatch 143 → 112、symbols
  mismatch 83 → 56、sourcemap 11・trace 1・harness error 19 は変わらない。S0 の report と構成ごとに比べると errors +34・emit
  +26・types +31・symbols +27 で、下がった列は無い。上がった stem は slice の 23 の対象（`importSource1`〜`21`、
  `importDeferInvalidDefault`、`importAttributeType1`）だけ。
- **並列対照**（`--checkers 4`、582 s、`scripts/conformance_ts71_compare.py`）：15,317 構成が一致、違う 22 構成は、記録済みの
  順序の 4 構成と、S0 で新しく入った本家の test の 18 構成：flaky な診断を捕まえる test 17（`noFlakyDiagnostics*` 6、
  `*EnumComputedNameNoFlake` 9、`flakyDiagnosticsJsClassPropertyInitializedToUndefined`、`importElisionTypeofImportTypeQualifier`。
  順序に依らない診断にする修正は S2 の commit）と、partition で crash が変わる `declarationEmitUnserializableTypes`（S3）。
  18 構成の入力は S1 が触れた構文（phase の import、meta-property、import attributes）を含まず、1 checker の結果も S0 と同じ。
- **hosted と merge**：PR #741 の hosted run 38107146066（rust 11m38s、conformance (TypeScript 7.1) 21m13s、gates pass）。
  merge commit `67a759ffd`（2026-10-11）。

## S2 checker（2026-10-11）

- **本家の 10 commit**（違う 28 stem）：#64674（enum member の computed name を式として検査し、markLinkedReferences も
  飛ばさない。15 stem）、#64636（import type の qualifier の名前は印を付けない）、#64479（getExternalModuleFileFromDeclaration
  は ignoreErrors で解決）、#64646（JavaScript の class は自分の constructor が宣言した property の型を解決する：
  checkConstructorDeclaredProperties）、#64482（isInExpressionContext：function／class 式の名前は宣言の名前）、#64525（static
  property は自分の class で文脈型を付けない）、#64530（再帰の call の解決でも純粋な return type の推論は制約で絞る）、#64523
  （strictPropertyInitialization の `this` の access は代入宣言の枝に入らない）、#63969（truncate 中の visitAndTransformType は
  最初に省略する）、#64584 の checker の部分（`using` に `await using` の hint 18116）。
- #63969 は checker の別の表示の道（`check.rs`：node builder の文字列版）にも入れた：symbol のある object、mapped、conditional、
  deferred な reference の所で同じ check をする。node builder だけでは `unionElementErrorTruncation` は直らなかった。
- **tsgo GetJSDocDeprecatedTag**：deprecated の suggestion の関連情報は宣言自身の JSDoc を読む（変数の JSDoc は文にあるので
  無い。`deprecatedReferenceInEnumComputedNameNoFlake` で分かった）。getFirstJSDocTag は使う所が無くなり除いた。
- **test**（tsgo の答えで pin）：CLI の contract（`using` の hint と enum の computed name の 2304）。truncation の表示の test は
  省略した alias の引数を tsgo の `...` に、deprecated の alias の test は変数の宣言の関連情報が無いことに替えた。
- **結果**（release build、`155dc8a73`）：full の conformance（`--workers 2 --check`、597 s）は 0 regressions、31 構成が受け入れの
  tier を上回る。errors full 13,509 → 13,536（mismatch 46 → 21、text 2 → 1、category 1 → 0）、emit mismatch 39 → 37、types
  mismatch 112 → 104、symbols 56・sourcemap 11・trace 1・harness error 19 は変わらない。S1 の report と構成ごとに比べると errors
  +27・emit +2・types +8 で、下がった列は無い。上がった stem は slice の 28 の対象と、#63969 の truncation で直った types の 2
  （`hugeDeclarationOutputGetsTruncatedWithError`、`nestedSpreadsAndWidening`）。
- **並列対照**（`--checkers 4`、`scripts/conformance_ts71_compare.py`）：15,334 構成が一致、違う 5 構成は記録済みの順序の 4 構成と
  partition で crash が変わる `declarationEmitUnserializableTypes`（S3）。S1 の対照で違った flaky な診断の test 17 は S2 の修正で
  一致した。
- ratchet は `--update` で 13,537 構成（新しい 25、上げた 6）。suites（`--check`）は 0 regressions で数は S1 と同じ。
