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
- **hosted と merge**：PR #742 の hosted run 38110517433（rust 9m56s、conformance (TypeScript 7.1) 12m52s、gates pass）。
  merge commit `da383c9e7`（2026-10-11）。

## S3 declaration emit（2026-10-11）

- **本家の 7 commit**（違う 29 stem、うち 3 は crash）：#64573（isAccessible：表の entry とその alias の解決に、raw と merged の
  両方で比べて似ているか、解決の alias の鎖に沿って似ているか。11）、#64461（createCyclicStructurePlaceholder は循環ごとに循環の
  構造を報告し、symbol の深さの上限は残りを truncate する。11、うち 1 は crash）、#64556（array と tuple は普通の reference の
  identity で visitAndTransformType を通り、visited な型はそこで循環、cache の key に infer の型 parameter。2、うち 1 は crash）、
  #63989（default export の accessibility の診断と error の fallback は、export する式がなる class／function の宣言も覆う。2、
  診断の文脈が無いと報告していた）、#64558（reverse mapped の placeholder は表示だけ、declaration emit では全体を visit し、
  mapped の宣言で深さ 100 まで。1）、#64469（下記。1）、#64545（alias のような expando の export は namespace の前の member も
  export する。1）。
- #64469 の index は速さの変更で、container は port の走査と同じ。test に要ったのは、囲む file の import の検索が、場所に対する
  resolveExternalModule の mode（周りの import call・import／export 宣言・import-equals の module specifier）を使うこと：宣言する
  module が re-export する module より前に残る。
- **新しい test で分かった tsgo との違い**：shouldUsePlaceholderForProperty の条件 (3) は最後の 4 つのどれかが property の mapped
  型から（tsc 6.0 は最後の 3 つ全て。node builder と表示の道の両方）。instantiation expression の型は `typeof` の node を試す間
  visited にし、fallback は循環の placeholder で止まる（`.types` の writer が `declarationEmitUnserializableTypes` で 1 GiB の
  stack を使い切っていた。`.types` の writer は node builder を使う）。getSpecifierForModuleSymbol の mode は囲む宣言の module
  specifier から checker で取る（GetModeForUsageLocation。preserve の re-export は `.js` の末尾）。表示の道にも
  visitAndTransformType の symbol の深さの上限。
- **test**：JavaScript の expando の contract は tsgo の export された前の member に、cached copy の contract は循環する class 式の
  TS5088（宣言 file は書かない）に、specifier の reuse の unit test は host から囲む宣言の mode を与えないように替えた。
- **結果**（release build、`a5138c1eb`）：full の conformance（`--workers 2 --check`、485 s）は 0 regressions、39 構成が受け入れの
  tier を上回る。errors full 13,536 → 13,557（mismatch 21 → 4、text 1）、emit mismatch 37 → 13、types mismatch 104 → 85、
  symbols mismatch 56 → 49、sourcemap 11・trace 1、harness error 19 → 15（全て content mapper）。S2 の report と構成ごとに比べると
  errors +21・emit +28・types +23・symbols +11・sourcemap +4・trace +4（前の harness error 4 が比べられるようになった）で、
  下がった列は無い。上がった stem は slice の 29 の対象と types の 2（`declarationEmitNoInvalidCommentReuse1`、
  `reactImportDropped`）。
- **並列対照**（`--checkers 4`、`scripts/conformance_ts71_compare.py`）：15,335 構成が一致、違うのは記録済みの順序の 4 構成だけ
  （partition で変わる crash は直った）。
- ratchet は `--update` で 13,558 構成（新しい 21、上げた 18）。suites（`--check`）は 0 regressions、1 つ上がった
  （`tsbuild/incremental/reverse-mapped-declaration-consumption`。#64558）ので `--update` で記録した（tsbuild 152 → 153）。
- **hosted と merge**：PR #743 の hosted run 38115411840（rust 9m6s、conformance 14m7s、gates pass、head `dae9944cf`）、merge commit
  `d9b82ae85`。

## S4 emit（2026-10-11）

- **本家の 7 commit**（違う 13 stem）：#64544 の emit の部分（空の sourceRoot の field を書かない
  （`json:"sourceRoot,omitzero"`）。map file と inline の map。7）、#64460（declaration emit の export assignment（identifier、
  class／function 宣言の名前、`_default`）を文に map する。2）、#64470（JSX の attribute の値は 1 つの要素として parse し、兄弟の
  要素の recovery は外の要素を包む。2）、#64670（下記。1）、#64422（名前が binding pattern の rest 要素は assignment pattern に
  変える。tsc は identifier を assert していた。1）、#64578（verbatimModuleSyntax で、型だけの消去で空になった named import は
  default import の隣なら落とす。書かれた `{}` は残す。1）、#64574（消えた `with` の本体は labeled statement と同じく空文。1）。
- **#64670**：tsgo の private name の環境は名前ごとに 1 つの entry を source の順に持ち、そのまま残す member
  （`PrivateIdentifierKindUntransformed`）は前の entry を置き換え、後の entry を invalid にする。member の宣言は自分の class の
  entry を読み、それが untransformed か invalid ならそのまま残る。port は untransformed の名前を順序の無い別の set に持ち、宣言の
  slot を全ての環境から引いていたので、decorated class の `static #a() {}` と `#a() {}` で transform が失敗していた。環境を tsgo の
  entry（`PrivateEntry::{Untransformed, Slot}`）にし、使う場所は entry のある一番内側の環境で止まり（`accessPrivateIdentifier`）、
  member の宣言は今の環境を読む。ES2022 の invalid な static field は static block を作らずに残る。
- **ほかに**：tsgo `emitBinaryExpression` は側の優先度より低い operand を全て括弧で囲む（port は TypeScript の式だけ。parse した木
  では recovery から来る。#64470 の兄弟要素の comma）。
- **`.js.map` の列**：runner は emit の比較を始めた時から構成ごとに `.js.map` の baseline を比べていた（report の `map`）が、
  `scripts/conformance_ts71.py` は数えも ratchet もしていなかった。19dadef8 では比べた 13,451 構成が全て一致、再 vendor（S0）で
  150 が違った（全て空の sourceRoot）が、summary の列を読んだ S0 の分析はそれを見落とした。ratchet に 8 列目の `.js.map` の tier
  （`full`／`none`。列の無い行は `none`）、summary に `map_full`／`map_mismatch`／`map_not_assessed` を足した。
- **test**：空の sourceRoot を固定していた 16 の contract と unit test（CLI 10、API 2、transpile 2、emitter 2）は tsgo の省略に
  した（emitter の凍結された 6.0.3 の bundle の観測は field を除いて比べ、空でない sourceRoot を固定した）。7 つの変更の tsgo の
  byte を 6 つの CLI contract で固定した。
- **結果**（release build、`840d710c9` = rebase 前の `b768f82a7` と同じ code）：full の conformance（`--workers 2 --check`、
  495 s）は 0 regressions。errors full 13,557 → 13,560（mismatch 4 → 1、text 1）、emit mismatch 13 → 1、sourcemap 11 → 0、
  `.js.map` 150 → 0（13,562 全て一致）、types 85・symbols 49・trace 1・harness error 15（content mapper）は変わらず。S3 の report と
  構成ごとに比べると errors +3・emit +12・sourcemap +11・`.js.map` +150 で、下がった列は無い。上がった stem は slice の 13 の対象
  だけ。同期で残るのは errors 2 と trace 1（#64159）、emit 1（#64632）で、S5。
- **並列対照**（`--checkers 4`、`scripts/conformance_ts71_compare.py`）：15,335 構成が一致、違うのは記録済みの順序の 4 構成だけ。
- ratchet は `--update` で 13,561 構成（新しい 3、上げた 11、`.js.map` の列は全て `full`）。suites（`--check`）は 0 regressions、
  空の sourceRoot が直したものが 49 上がった（transpile 38 → 41、tsbuild 153 → 182、tsbuildWatch 49 → 63、tsc 208 → 211）ので
  `--update` で記録した。suites に残る違い（config 2、tsbuild 11、tsbuildWatch 2、tsc 18、tscWatch 2、tsoptions 24）は S5 以降と、
  前からの範囲（ES5 の意図した差、content mapper）。
- **hosted と merge**：PR #744 の hosted run 38118692919（rust 11m31s、conformance 23m44s、gates pass、head `0bc0b9af7`）、merge commit
  `9f0c0cb86`。

## S5 program と build（2026-10-11）

- **conformance の 4 stem**：#64632（node_modules の深さが下がって再び届いた file は subtask をやり直す（`filesParser.start`）。
  `symlinkedPackageReachedRelativelyIsNotExternalLibrary`）、#64159（区切りで終わる相対の ESM specifier は TS2834 で拡張子を
  提案しない（`./foo/.js` を提案していた）。`nodeNextDirectoryImportExtensionSuggestion`。custom の type root はディレクトリとして
  書かれた名前（空か区切りで終わる）の file を探さない。`typeReferenceDirectiveTrailingSlashName` の trace。root file の include
  の理由は config のディレクトリで include spec と照らす。`tsconfigRootdirInclude`）。最後のものは port が既に config のディレクトリで
  照らしていた（CLI の config の道は理由を渡す）が、conformance の harness は config の fixture を root 名だけで load していた
  （tsgo の harness の matcher が空の current directory を使っていた間は一致）。tsgo の compiler runner は Program に parse した
  config を渡すので、harness は config plan の各 root の理由で load する（理由つきの loader を public に、
  `ConfigRootPlan::root_reasons()` を追加）。
- **suites の 35 baseline**（再 vendor で壊れた 18、新しい 17）：#64457 は watch option（watchFile、watchDirectory、
  fallbackPolling、synchronousWatchDirectory、excludeDirectories、excludeFiles、watchInterval）を消した。両方の command line で
  未知（TS5023、TS5072）、tsconfig の `watchOptions` は root の option ではない（読まない、診断しない。port の変換は `${configDir}`
  の置換で config を失敗させえた）。宣言、command line の watch group、config の watch scope、enum 名を消し、写した table から
  `commandLineParsing` の watch option の 17 case を除いて `TestRemovedWatchOptions`（7）を加えた。tscWatch の「Reject removed
  watch interval option」も一致。`--showConfig` は options model が `showConfig: false` とする `listFiles` を出さない（4）。#64159
  の tsconfig の table に「handles empty file name in files list」（2。振る舞いは既に tsgo のもの）。#64544 の referenced map は
  triple-slash の参照を file に対して解決してから redirect を問う（参照先の composite project の source は出力として記録。2）。
  #64556 の incremental の scenario：tsgo の emitter は declaration の診断を `DiagnosticsCollection` に足し、等しい診断は 1 つ、
  並べ替える。port は報告を全て file の signature に入れていた（再帰する tagged tuple が TS5088 を数千回。tsgo の 2,108 byte に
  対し 327,863 byte の `.tsbuildinfo`）。`sort_and_dedupe_equal_diagnostics` を declaration unit の診断に（1）。
- **新しい test で分かったこと**：tsgo は command line の errors を順に 1 つずつ報告する（port は program の診断のように並べ替えて
  いた）。記録だけ：checker の拡張子の提案（TS2835）は program が読んだ file を見るが、tsgo は host の `FileExists` を問う。
  program の外の file への拡張子の無い ESM import は TS2834 になる（conformance は全ての file が root なので見えない）。checker に
  host の file の存在を見せる必要がある。
- **test**：program の watch option の contract は tsgo のもの（base の `watchOptions` は読まない、root scope は無い）、`h2-8b` の
  config fixture（tsgo と違う所で再記録する port 自身の記録）は watch option を probe しない（`verification` に記し、
  `source_commit` を `aa8149273` に）。command line の unit test と API の `parseCommandLine` の test は未知の option に、
  `incremental_steps_explain_schedule_two` の「shared imported first」は tsgo `aa8149273` の出力に（tsgo の binary で再生、他の
  step は同じ）。3 つの CLI contract で tsgo の byte を固定。
- **結果**（release build、`77296c7f0`）：full の conformance（`--workers 2 --check`、487 s）は 0 regressions、4 構成（slice の
  stem）が上がった。errors full 13,560 → 13,562（mismatch 0）、emit 1 → 0、trace 1 → 0。比べた全ての構成が tsgo `aa8149273` と
  errors・emit・source map・trace・`.js.map` で一致し、違うのは `.types`／`.symbols` の tail（85／49、同期の後）と content mapper の
  harness error 15。
- **並列対照**（`--checkers 4`、`scripts/conformance_ts71_compare.py`）：15,335 構成が一致、違うのは記録済みの順序の 4 構成だけ。
- ratchet は `--update` で 13,562 構成（新しい 1、上げた 3）。suites（`--check`）は 0 regressions、18 上がった（config 87 → 89、
  tsbuild 182 → 183、tsc 211 → 217、tscWatch 40 → 42、tsoptions の `removedWatchOptions` 7）。`--update` で記録し、消えた
  watch option の 17 baseline は ratchet から落ちた（748 → 731）。suites に残る違いは前からの範囲の 24（tsbuild 10、
  tsbuildWatch 2、tsc 12。ES5 の意図した差、content mapper など）。
- **hosted と merge**：PR #745 の hosted run 38122507114（rust 11m53s、conformance 14m8s、gates pass、head `770404b2d`）、merge commit
  `e1b66157e`。

## S6a API の transport と callback file system（2026-10-11）

- `aa8149273` の TypeScript client（`packages/typescript/test/{sync,async}/api.test.ts`、356／365 test。tsgo は全て通る）は
  request の前に要るものが 3 つある。`--api` の `--useCaseSensitiveFileNames`（Go の bool、既定は OS の file system。client は
  常に渡す。server は callback か case sensitivity があれば file system を包む）、#64447 の callback file system（答えは全て
  `{ kind, value }`：値、または `useOS`・`missing`・`identity`・`fakeStat`・`noop`。`stat` の callback（Node の mode・size・
  RFC 3339 の mtime）、`name:error` はその操作を失敗させる、`realpath:identity`・`stat:fakeStat`・`writeFile:noop`・
  `removeFile:noop` は client が答えない操作の既定、`error` の答え・kind の欠落・不正な kind は tsgo の message で panic）、
  source file の response の node id（tsgo `SetSourceFileID`。client はこれで file の cache と remote の参照を引く。program の
  file は parse の identity（新しい `ParsedDocument::id`、parse を再利用する snapshot で共有）、作った file と config の file は
  新しい id）。
- **client の test**（release build、`node --conditions=@typescript/source --test`）：sync 6 → 202／356、async 210／365。残りは
  #64518 の symbol の所有（126／127。S6b）、新しい method（`getSymbolOfDeclaration` 7、`retainSourceFile` 2）、program の
  file と parse を共有する `createSourceFile`（2）、LS（completion 5 など 8）と `formatNodeForInsertion`（3）、その他 7。
- **test**：callback file system の test は tsgo の `callbackfs_test.go` の case（既定、stat と entries、write の pass-through と
  no-op、呼び出しごとの fakeStat と identity、error の callback、remove の no-op）と kind／value の答えに、flag の test は新しい
  flag と既定に。
- **結果**（release build、`75274a66c`。checker の crate の `ParsedDocument` に触れたので）：full の conformance（`--workers 2
  --check`、479 s）は 0 regressions で、S5 の report と全ての構成が全ての列と digest で同じ。並列対照は記録済みの順序の 4 構成だけ、
  suites（`--check`）は 0 regressions・上がったもの無し（ratchet は変えない）。
- **hosted と merge**：PR #746 の hosted run 38125615894（rust 11m47s、conformance 15m48s、gates pass、head `1397c519f`）、merge commit
  `670995a6c`。

## S6b file が持つ symbol（2026-10-11）

- **#64518**：binder の symbol は bind した source file のもの（`SymbolReference { kind: 0, file, id }`。`file` は descriptor：file 名・
  path・content hash（text の xxh3-128、上位 64 bit が先）・parse options key（JSX 1、Force 2）・script kind・node id）、checker の
  symbol（transient）は snapshot のもの（`{ kind: 1, snapshot, project, id }`、snapshot に登録）。他の response の中の symbol は
  `CompactSymbolReference { id, file }`（file の node id。snapshot の symbol は空）：type の `symbol`・`aliasSymbol`、signature の
  `parameters`・`thisParameter`、symbol の `parent`・`exportSymbol`。request は symbol を reference で渡す（symbol の数値と symbol
  の `objectId` は無くなった）。
- file の symbol の parent・export symbol・members・exports は checker なしに file の binder data で答える（table は最初の宣言の
  位置、宣言の無いものは後、tsgo の持つ名前の byte 順、番号の順。空の table は Go の nil slice で、tsgo の JSON は `[]`）。snapshot
  の symbol は snapshot の checker で答える（table は checker の順）。tsc-rs の binder の symbol の番号は file の identity の中の
  位置で、file を共有する program で同じなので、file の symbol の番号はその位置 + 1。
- **新しい method**：`retainSourceFile`（descriptor の parse に lease）、`getCachedSourceFile`、`getSymbolOfDeclaration`（#64571。
  encoding の node index の宣言の binder の symbol。tsgo の `ast.IsDeclaration` は node の data が `DeclarationBase` を持つこと
  （source file も含む）で、tsc 6.0.3 の kind の list より広い）、#64598 の checker の `getSymbolOfNode`・
  `getSymbolOfDeclarationForChecker`・`getParentOfSymbolForChecker`・`getMergedSymbol`。descriptor は parse cache の key（node id
  以外）で live な parse（lease、共有の document、live な snapshot の program。lib も）を探し、node id で同じ parse か確かめる。
  `createSourceFile` は同じ file・text・script kind の live な parse を再利用し、空の file 名は tsgo の error。
- descriptor は parse ごとの cache（tsgo は hash を source file に持つ）。snapshot と lease の release で、live な snapshot と lease
  のどれも持たない parse のものを捨てる。
- checker の crate：`get_late_bound_symbol` を `pub` に、`DocumentRegistry::documents_of` を追加（compiler の動作は変わらない）。
- **tsgo との照合**：同じ file に同じ raw request（symbol、property、checker の 4 method、不正な reference 13、cached source
  file、node index）を tsgo `aa8149273` と tsc-rs に送り、node id と番号を正規化すると全て同じ（違うのは `initialize` と
  `createSnapshot` の project の field だけ：#64159 と #64681、S6c）。
- **client の test**：sync 202 → 331／356、async 210 → 340／365。残り各 25 は LS（completion・auto-import・references・
  signature usage・JSDoc：16）、`formatNodeForInsertion` 3、その他 6（program の file と parse を共有する `createSourceFile`、
  `parseJsonConfigFileContent` の null の要素、raw の compiler option の path、FS の変更の rebuild、VS Code の URI、読めない
  virtual file）。
- **test**：API の unit test は新しい reference に（`shape` は reference を `file:<名前>` か `snapshot:<project>` に）、新しい
  `session_symbols.rs`（descriptor の hash と script kind、checker なしの関係、snapshot の symbol、checker の 4 method、不正な
  reference、node index の宣言、descriptor の prune）。どれも tsgo の raw の response に合わせた。
- **結果**（release build、`4c68c56a1`。checker の crate に触れたので）：full の conformance（`--workers 2 --check`、475 s）は
  0 regressions で、S6a の report と全ての構成が全ての列と digest で同じ。並列対照（`--checkers 4`、469 s）は記録済みの順序の
  4 構成だけ、suites（`--check`）は 0 regressions・上がったもの無し（ratchet は変えない）。
- **hosted と merge**：PR #747 の hosted run 38129643276（rust 11m16s、conformance 20m50s、gates pass、head `fa2e2588c`）、merge commit
  `4b532ffdc`。
