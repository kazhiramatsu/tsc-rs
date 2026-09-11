# H2.8b–e 設計先行計画

2026-09-11。ユーザー依頼：今後の作業を細分化し、先行スライスを設計済みの状態で用意する。
基準productionは`5134bb0187f9f3f6ac2a0218f8d8e98859cf4fea`。
worktreeは`/Users/hiramatsu/dev/tsc-rs-h2-8-design`、branchは`design/h2-8b-e-slices`。
PR #513の候補から分岐した設計用branchで、runtime・現在のCI設定・profileを変更しない。
#512と#513はマージ済み。main `9806a4cb967a6b4e4336c2108c2c28edd54813b0`は
このproduction基準と同じtreeであり、候補headのhosted gatesはrun 34587126551で成功した。

機械可読の分割・依存・担当表は[registry](h2-8b-e-design-registry.v1.json)、
先行スライスは[H2.8b-LR1](h2-8b-library-replacement-baseline.md)。
検査コマンドは`python3 scripts/check-h2-8-design-plan.py`。
[次担当向け依頼文](h2-8b-library-replacement-claude-prompt.md)と
[準備時の検査・上流観測記録](../../../../ratchets/h2-8b-e-design-preparation.v1.json)も保存した。

## 状態と優先順位

この計画はH2.8b–eの**分割と設計順序**を定める。H2.8aは引き続きopenであり、
H2.8b–eのruntime activationやH2.9の完了を宣言しない。
Claudeは別worktreeでdecoratorのliteral/prologueを担当する。
その作業で変更され得るprinter・metadata・generated bindingは、この計画の編集範囲に含めない。

設計状態を次の3段階に分ける。

- **outline**：目的・依存・source入口・Rust owner・検証軸まで定義。未解決事項を列挙し、production変更はまだ不可。
- **ready-for-baseline**：source/入力/期待値が固定され、native比較を追加・実行する手順が確定。結果を採取するevidenceスライスとして着手可能。
- **runtime-ready**：native baselineで不一致を分類し、変更する型・関数・順序・fault境界・受入が確定。
  設計gateの未解決事項が0になったものだけ。現在この計画にはruntime-readyのスライスはない。

最初のLR1はready-for-baselineである。単体loader testsの存在を通常emitの互換性とみなさず、
12件の上流完全観測をRustと比較する。全件一致ならproductionを変更せずprofileの証拠を得る。
失敗した場合はその出力を保持してLR2の設計を具体化する。失敗を隠したfixture変更はしない。

全体769件の再測定とA6-41の閉じ方はH2.8a側の統合担当が持つ。
後続設計は並行して進められるが、H2.8a-closeを必要とするruntimeを先にactivateしない。
全領域の設計を仮定で完成扱いせず、先行結果で影響する後続設計だけを再検証する。

## H2.8b：config・host・library

| ID | 一つの作業単位 | 主なownerと成果 | 実装前の依存 |
| --- | --- | --- | --- |
| H2.8b-LR1 | fresh Programのlibrary replacementを通常emitまで比較 | program loader/library、compilerの新規比較test。source順・library membership・全出力を含む12件のbaseline | 固定base。evidence-onlyのためA-close待ちでなく着手可 |
| H2.8b-LR2 | LR1で再現したlibrary選択・membership・fallbackの差を修正 | loader/libraryの原因別変更。欠落packageとhostエラーを区別。差がなければno-op記録 | LR1の結果、A-close |
| H2.8b-CFG1 | config変換と診断の優先順位 | config/config_host/option_validation。extends由来・fileNames・wildcard・option provenanceを保持 | A-close、現行config owner監査 |
| H2.8b-HOST1 | optional host機能の有無とfallback | host adapter、default-lib位置・source取得・directory/realpath・traceの存在判定表。実際の読者ごとに必要なら再分割 | A-close、CFG1の呼出契約 |
| H2.8b-SYS1 | one-shot System/Memory/Fsの入力・出力・fault契約 | host/filesystem、compiler CLIのNativeEmitFileSystem、emitter sink。encoding・作成/書込失敗・順序を確認 | HOST1、A3の既存write契約 |
| H2.8b-CLOSE | B全体の照合とprofile更新 | compiler/conformance/projectのB所有行、Memory/Fs差、共有bandの移行を集計 | LR2、CFG1、HOST1、SYS1 |

LR1のsource読解で確認した既存実装：`loader::resolved_library_path`はconfig directoryまたはcwdの
synthetic containing fileを用い、独立したNode10 resolverを使う。`LibraryCatalog`と
`PreparedProgram::library_files`は既存の型である。これらを未実装として作り直さない。
oldProgramのlibrary resolution再利用・無効化はL2/BLD1、任意の公開host hook群はAPI1との境界を
明示する。Bでは通常one-shotから到達する部分と、外部ownerへ留保する部分を別々に記録する。

## H2.8c：軽い検査・transpile

| ID | 一つの作業単位 | 主なownerと成果 | 実装前の依存 |
| --- | --- | --- | --- |
| H2.8c-NC1 | noCheckの診断/linked-referenceスケジュール | compiler ProgramSession、checkerのemit resolver準備、executeのtyped guard。通常semantic check省略とemitに必要な処理を区別 | A-close、CFG1 |
| H2.8c-MOD1 | isolatedModules/verbatimModuleSyntaxのemit境界 | types/options、checker linked references、既存module transform。option受付と実際のrouteを対照 | NC1、A-close |
| H2.8c-TM1 | transpileModuleの独立したsingle-file経路 | 新規compiler側adapter。fileName/JSX/JSDoc、option fixup、diagnostics、outputText/sourceMapTextの有無と順序 | NC1、MOD1、既存map/transform |
| H2.8c-TD1 | transpileDeclarationの強制declaration経路 | TM1の入力adapterと既存forced declaration/session。barebones lib、isolatedDeclarations、結果の形 | TM1、H2.7c forced declaration境界 |
| H2.8c-CLOSE | legacy transpile wrapper・corpus・性能/API証拠 | 元37行のrunnerを再分類し、全対象を実行。通常Programへの付け替えで性能を水増ししない | TM1、TD1、MOD1 |

`execute.rs`は現時点でもFiles操作の`noCheck`、`isolatedModules`、`verbatimModuleSyntax`を拒否する。
typed optionがあるだけで対応済みとはしない。sourceの`transpileWorker`はoptionを上書きし、
小さいhostとdeclaration時のbarebones libraryを持つ。通常の全件checker経路の使い回しを設計前提にしない。
caller-supplied custom transformsの公開契約はAPI1へ留保し、built-in-only profileとの境界対照を作る。

## H2.8d：emit要求・callback・cancel

| ID | 一つの作業単位 | 主なownerと成果 | 実装前の依存 |
| --- | --- | --- | --- |
| H2.8d-SEL1 | 対象source fileとoutput unitの選択 | emitter planのTargetSourceFile、compiler entry、bundle/outFile時の全体resolver選択 | A-close、C-CLOSE、H2.7d |
| H2.8d-MODE1 | ordinary emit-only/declaration-only/noEmitの分岐 | request enumとhandleNoEmitOptions相当。既存forced declarationと通常宣言専用を区別 | SEL1、NC1 |
| H2.8d-WRITE1 | request callbackとhost writeの優先順位 | compiler/host/sinkの借用境界。部分書込・onError・結果/diagnosticsの有無を固定 | MODE1、SYS1 |
| H2.8d-CANCEL1 | cancellationの観測点と失敗後の状態 | compiler/checker session、typed token/abort。開始前・診断中・emit中・callback境界をsourceから列挙 | WRITE1、NC1 |
| H2.8d-CLOSE | requestの組合せと従来whole-Program回帰 | 普通のtargeted emit、既存37 ordinary references、source-less/d.ts/JSON/outFile対照 | MODE1、WRITE1、CANCEL1 |

現状`EmitOutputPlan::targeted`は型として存在するがbootstrap shapeで拒否される。
H2.7cのforced declarationsを普通のtargeted emitの成功として数えない。
builder signature、emitBuildInfo、incremental再利用はBLD1/L2 controlsとして別ownerへ置く。

## H2.8e：CLI

| ID | 一つの作業単位 | 主なownerと成果 | 実装前の依存 |
| --- | --- | --- | --- |
| H2.8e-ARGS1 | argv/response fileとoption診断・mode dispatch | compiler cli、program option catalogue。重複boolean・alias・unknown・parse errorsの順序 | CFG1、HOST1 |
| H2.8e-INFO1 | version/help/allと無入力時の表示 | Program生成前のcli表示、terminal width/newline能力。parse errors→init→version→helpの順序を保持 | ARGS1、TERM1の必要部分 |
| H2.8e-INIT1 | initとconfig生成/write失敗 | generateTSConfig/writeConfigFile相当、既存ファイル・write failureの結果 | ARGS1、SYS1 |
| H2.8e-SHOW1 | showConfigの読み込み・変換・表示 | parse結果→convertToTSConfig、絶対/相対pathとextends由来、診断先行 | ARGS1、CFG1 |
| H2.8e-LIST1 | listFilesOnly/explainFiles/listEmittedFiles | compiler報告順とProgramの根拠情報。noEmit/watch等の組合せを明示 | ARGS1、D-MODE1、CFG1 |
| H2.8e-OBS1 | diagnostics/extendedDiagnostics/trace/profile | System capability・計測/trace callback。機能的出力と変動する性能値の契約を別に固定 | ARGS1、SYS1、C-CLOSE、D-CLOSE |
| H2.8e-TERM1 | English locale・pretty/TTY・省略可能なterminal能力 | cli formatter/System adapter。locale検証・不在fallback・stdout/stderrとexit | ARGS1、SYS1 |
| H2.8e-CLOSE | 全CLI routeと終了状態の照合 | CLI inventory、全mode precedence、通常compile/non-emit回帰 | INFO1、INIT1、SHOW1、LIST1、OBS1、TERM1 |

`--version`と`-v`は現行parserで認識されるが、早期returnはargv内の`--version`を直接探している。
これはINFO1/ARGS1で観測すべき現行差である。今回ここをproduction修正しない。
`--build`/watchの選択は境界対照を持つが実際のbuild/watch製品はBLD1/W1。
非vendored locale catalogはREL1へ留保する。

## 共有箇所と担当

| 編集owner | 専有する共有箇所 | 並行して進められる作業 |
| --- | --- | --- |
| decorator担当（Claude） | standard_decorators、printer、共通metadata、generated bindingの今回の修正 | 他担当のsource調査、独立oracle、host/config資料 |
| program/host担当 | program config/loader/library、host trait/adapter | CLI表示用の入力・期待値作成、transpile source inventory |
| compiler-entry担当 | compiler lib、emit request/diagnostic schedule、checker resolverへの接続 | CLIの独立表示関数、host fixture、結果の分析 |
| CLI担当 | compiler cli、引数と表示・mode dispatch | program/host側の境界確定後の各表示mode |
| 統合担当 | 共有test登録、共通comparator、xtask/profile/manifest、merge | 各担当の専用test/fixtureは別名で準備 |

同じ共有ファイルのproduction変更を複数branchで同時に進めない。修正が必要ならownerへ
最小再現とcallee根拠を渡す。複数の新機能から使う型は、最初の実装者が単独で決めず、
producer/consumerの必要条件を先に設計表で確定する。ファイル分割だけの並列化は行わない。

設計の並行作業はread-onlyのsource inventoryと独立名のwitnessから始める。
同じMacの重い実行は一つずつ、専用target、`taskpolicy -b nice -n 15`、`CARGO_BUILD_JOBS=2`。
hostedは既存`cargo xtask acceptance`を維持し、walk/chain-walk/full `cargo xtask ci`を増やさない。

## 設計を完成させる順序

1. LR1を実行してfresh Programの実測を得る。同時にCFG1とARGS1のsource inventoryを深める。
2. native差分に基づきLR2を仕上げ、CFG1→HOST1→SYS1の具体的な型とcallback契約を固める。
3. NC1→MOD1→TM1/TD1を設計する。compiler-entry担当はDのrequest型への影響を同時に確認する。
4. Dの選択/モード→write→cancelの順に設計し、CLIの表示部分は確定したSystem能力から独立して進める。
5. 各closeスライスで元の全対象IDを再集計し、H2.9の広範なqualificationへ渡す。

下流をruntime-readyへ進める条件は、固定baseでのsource hash、実在するRust symbol、
現在のgap分類、上流witness、変更手順、全ownerの受入、共有編集権、未解決0がそろうこと。
この計画のoutline表だけでそのgateを通したことにはしない。
先行productionの変更で前提SHAが変わったら、依存するpacketだけをstaleへ戻して再確認する。

## CFG1の実装済み小単位（2026-09-11）

- [CFG1a config変換・継承](h2-8b-config-extends-report.md)：28 config cases ×2、通常command 8件。
- [CFG1b config外の探索開始点](h2-8b-config-discovery-report.md)：24 config cases ×2、開始点計算22件 ×2、通常command 8件。

いずれも報告書に定めた範囲の完了。CFG1/B全体のclosure・profile activationではない。
次のCFG1c候補はoption relationship diagnosticsの優先順位とconfig由来位置情報。
