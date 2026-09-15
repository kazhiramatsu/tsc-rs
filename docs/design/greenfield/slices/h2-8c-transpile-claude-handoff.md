# Claude 先行実装依頼④：H2.8c — noCheck / transpile 専用パイプライン

作成日：2026-09-14。状態：依存設計・source 観測・隔離 prototype。
H2.8c の正式な runtime activation を許可する ready packet ではありません。


**2026-09-15 更新**：開始点と検証分担は[共通手順](claude-high-difficulty-handoffs.md)の最新版に従います。
SUPER 統合後の main の SHA を固定し、ローカルは新規失敗・関連 owner の focused set、
重い全件 replay は hosted で実行します。以下の技術要件は現行実装と照合し、既実装部分を再実装しません。

## 依頼

`Program` の `noCheck` emit、`transpileModule`、`transpileDeclaration` の三経路を
固定 TypeScript 6.0.3 から調べ、必要な parser/binder/checker/resolver/transformer の
実行範囲と診断・戻り値を定義してください。各経路の oracle と依存表を作り、
既存 Rust compiler/emitter を使う隔離 prototype と検証結果まで提出してください。
全型検査を走らせて診断だけ捨てる実装を、専用 pipeline の完成とは扱いません。

[共通手順](claude-high-difficulty-handoffs.md)の開始点で
`draft/h2-8c-transpile` / `../tsc-rs-transpile` を作り、source/input manifest を保存します。
現在の compiler/emitter と lossless な文字列・path API を使います。
新 route は隔離 worktree 内で接続し、root の option admission / CLI / profile は変更しません。

## 前提と段階ごとの成果

[H2.8 packet](h2-8.md) は H2.8c を依存 inventory 待ちとしています。
現在の [execute.rs](../../../../crates/emitter/src/execute.rs) は通常 Files emit の
`noCheck=true` を unsupported option として扱います。
[ProgramSession](../../../../crates/compiler/src/lib.rs) は one-shot PreparedProgram を所有し、
checker と emitter の resolver を接続します。型検査の省略で resolver まで消してよいとは限りません。

| 段階 | 成果物 | 次段階へ進む条件 |
| --- | --- | --- |
| A：inventory / 設計 | 3 route の呼出 graph、option 正規化、診断/戻り値 schema、Rust gap、具体的入力 manifest | 各 route に unresolved dependency の disposition があり、限定 prototype の変更面が決まる |
| B：隔離 prototype | 実 parser/binder/emitter と必要な resolver を使う 3 route の研究候補、source/native 比較 | 選定 scope の before/after があり、未対応 case を隠していない |
| C：本番組込み用 packet | B の設計・証拠を正式 readiness に載せるための残項目と手順 | 本依頼では activation しない。前提の transform/map/declaration を正式に再検証する |

本依頼の到達点は A+B と C の引き継ぎです。前提が不足する場合も、新規 observer と
実コンポーネントに接続した prototype まで進め、到達しなかった経路と理由を分けて提出します。
その状態を H2.8c 完了とは報告しません。

## Source と Rust の入口

`transpile` API は `_tsc.js` ではなく pinned **`typescript.js`** にあります。
両 bundle の hash は共通手順にあります。行番号を取り違えないでください。

| Source | 調査内容 | Rust |
| --- | --- | --- |
| `typescript.js:145985` transpileModule、145993 transpileDeclaration、146022 transpileWorker | 3 public result fields、default options、forced options、host と source の構築 | 新規 compiler route。既存 `ProgramSession` / `DeclarationSession` の必要部分 |
| 同 transpileWorker と fixupCompilerOptions | fileName default、jsx、implied format、reportDiagnostics、moduleName、renamedDependencies、jsDocParsingMode | [compiler/lib.rs](../../../../crates/compiler/src/lib.rs)、[program/prepared.rs](../../../../crates/program/src/prepared.rs)、既存 options/source 型 |
| `_tsc.js:115903` getScriptTransformers、createProgram と noCheck の全 consumer | built-in pass 選択、emit resolver に必要な遅延処理 | [builtins.rs](../../../../crates/emitter/src/builtins.rs)、[execute.rs](../../../../crates/emitter/src/execute.rs)、[checker/emit.rs](../../../../crates/checker/src/emit.rs) |
| `_tsc.js:129412` emitFilesAndReportErrors | Program/CLI 診断順、emitSkipped、status/exit | [compiler/cli.rs](../../../../crates/compiler/src/cli.rs)、CliEmitSessionOutcome、既存 complete-command adapter |
| `typescript.js:126961` getPreEmitDiagnostics と source の各診断 getter | syntactic/options/global/semantic/declaration の別経路 | checker phase entry と declaration diagnostic session |

`transpileWorker` は JS route では declaration/map の一部 option を強制し、
declaration route では isolatedDeclarations と小さな lib を使います。
`reportDiagnostics=false` でも option fixup / emit 由来の診断がどうなるかを測定し、
「全診断を空にする」処理を導入しません。noCheck の内部で必要な型計算も測定対象です。

原典 fixture の入口は [transpile inventory generator](../../../../crates/oracle/h1-transpile-inventory.mjs)
と `ts-tests/tests/cases/transpile`。固定 inventory は **22 source files** を記録します。
option expansion 後の API call 件数とは異なります。旧 H1 の除外はこの新 route の
永続的な除外根拠にせず、全 source file を新 inventory に対応付けます。

## API ごとの観測契約

| Route | 必須の比較 | 別記する内部証拠 |
| --- | --- | --- |
| Program noCheck | 完全 command：write bytes/path/order/metadata、診断 stream、emit result、status、exit、partial writes | phase/query counters、lib/module/host request trace |
| transpileModule | outputText、diagnostics の全構造と順序、sourceMapText の値と有無、throw 時の exception | 内部 host と pass の trace。public API に CLI exit を追加しない |
| transpileDeclaration | 同 public result に加え declaration/map の正確な出力と isolated declaration 診断 | barebones lib、declaration resolver の必要処理 |

JavaScript の UTF-16 text と UTF-8 materialization を区別し、absent / empty string /
empty array を schema で表現します。public API の内部 write trace は公開戻り値と別に数えます。
caller-supplied custom transforms は API1 の独立 control に置き、本依頼で全対応を約束しません。

## 実装手順と変更面

1. source の 3 route を別々に測定し、option と phase graph を固定する。
   normal Program checking、noCheck、transpile の差を同一入力で観測する。
2. 正規化済み options、route kind、source context、diagnostic policy、必要 resolver capability
   を typed request/plan として設計する。文字列の option 分岐を各 pass に散らさない。
3. Prototype の compiler adapter を作り、JS transpile の parse → 必要な bind/resolver →
   built-in transforms → print に接続する。現在の full-check route を経由しただけの結果を区別する。
4. declaration route を必要な lib/isolated declaration diagnostics/NodeBuilder に接続する。
   JS route の設定を再利用して必要な型・診断を失わないようにする。
5. noCheck の gate と診断選択を isolated prototype 内で接続する。
   既存 noEmit/noEmitOnError と選択順・失敗 precedence を観測する。
6. 実行済み phase/query counters で経路を検証し、通常 route の結果と共有 producer の
   回帰を確認する。性能は機能一致後、固定入力・同環境で別記する。

編集候補：新 `crates/compiler/src/transpile.rs`、compiler の `lib.rs` / declaration session、
`crates/emitter/src/execute.rs` と route plan、checker の `emit.rs` / phase entry、
prepared source/options の必要な adapter。新 route の名前と API は研究案であり semver 安定化ではありません。
shared planner や checker を変更する前に具体的 symbol・source owner・影響範囲を記録します。
一つの大規模 checker 再編を必須にせず、必要な capability の境界から実装してください。

## 必須 witness と検証

ID は `h2-8c/<program-no-check|transpile-js|transpile-dts>/<family>/<variant>`。
全 22 source file の disposition と、次の追加 family を具体的 manifest に固定します。

- 空入力、TS/TSX/JS、fileName absent/explicit/非標準拡張子、moduleName、依存名の置換。
- 構文エラー、option エラー、semantic-only error、declaration-only error、reportDiagnostics on/off。
- noCheck × noEmitOnError、noEmit、declaration、isolatedDeclarations、JS declaration。
- import/export/type-only/未解決 import、enum、namespace、decorator、JSX、class field と target ladder。
- sourceMap/inlineSourceMap/declarationMap、LF/CRLF、Unicode、map 未生成/空文字の区別。
- 不正 option 値、未対応 extension、source throw、Program sink failure、反復呼出の状態分離。

実行時の意味を補う control は、生成 JS の event log を別集計にします。
正常入力の byte 一致だけで診断・option coercion の一致とはしません。

新規 target 案：`crates/compiler/tests/transpile_routes_contract.rs`。
新規 observer は `scripts/observe-transpile-routes.mjs`。作成後の focused 入口は：

```sh
cargo test --offline --manifest-path crates/compiler/Cargo.toml --test transpile_routes_contract -- --test-threads=1
```

上は作成予定 target です。共通の低優先度/env を適用し、0 tests を認めません。
ローカルは新 route と変更した compiler/checker の focused tests、
hosted は必要な emitter/compiler の全件 regression を担当します。旧 530 件を一律にローカル実行しません。
API 専用の performance observation は、noCheck を「全 checker query ゼロ」とは仮定せず、
どの work が省略/維持されたかを counters と source graph で説明します。

## 提出と完了判定

3 route の設計と実依存表、source/Rust 観測 schema、全 fixture disposition、隔離 prototype、
before/after、regression、未接続 capability、activation 用の次 packet 案を提出してください。
期待値と byte が一致しても full checking を隠れて実行している場合は専用 route の未完了です。
依存未成立の経路は具体的な再現例・実エラー・必要 owner を記録し、全 H2.8c の成功にしません。
