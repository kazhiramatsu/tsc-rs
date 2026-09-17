# Emitter後・移行基盤batchの依頼案

状態：**design / 後続実装の準備書**、2026-09-17。
親計画：[emitter後ロードマップ](../../post-emitter-roadmap.md)。
既存owner：VER1.0-SYNC1/2、MAP/PIN設計、API1.0/L3.0/L5.0のinventory部分。
担当案：Codex。ClaudeへのGo→Rust移植依頼は、このbatchで原因・境界・testを確定した後にまとめる。

この文書だけでruntimeやrunner実装をreadyにしない。最初に下記の設計・調査を行い、
実装対象file、固定ref、schema、test IDs/件数・hash、checker、commandとreadinessを持つ子packetを作る。
本書作成によるsource変更・helper pin変更・7.1 adoptionはない。

## 依頼の目的

**固定した7.1参照と現在のRustの間で、どの機能・test・仕様が不足しているかを追える基盤を作る。**
Issue/roadmap、詳細仕様、上流差分、test inventoryを結び、次のbatchでGo traceと実装移植まで通す。
最初の成果物は台帳・比較準備だが、半自動化の完成条件は次のFEATURE-PILOTの統合までである。

## 開始点と読むもの

- 実装開始時のemitter完了mergeとreceiptをRust baseに固定する。本書の棚卸しbase
  `3b1f5fe87fd31e3b303bb44bd257342735452ed9`をemitter完了SHAと誤認しない。
- Go起点は`1f70213d4922b434345f639b441681e470c7cfc1`、比較元候補は
  v7.0.2の`1e4744d68260a7cb91b62b12edc3f6a2187faaf1`。
  最新mainを動的oracleにせず、追加参照も完全SHAで保存する。
- [参照・移植方針](../../typescript-7-direction.md)、[7工程の追従設計](../../typescript-7-upstream-sync.md)、
  [実装済みhelper](../../typescript-7-workflow.md)、[残作業台帳](../../remaining-completion-slices.md)。
- [PLAN-BASE](../plan-base/README.md)、[C04統合](../h2-8c-transpile/INTEGRATION.md)、
  [C05統合](../l2-3-resolution-cache/integration/README.md)、emitter最終receipt。
- `scripts/typescript7.py`、`crates/compiler/src/{lib,transpile,cli}.rs`、
  `crates/checker/src/program.rs`、`crates/program/src/`、既存harnessとCI planner。

設計中に許可する作業は、上記source・docsと固定upstreamの読解、source/test/file inventoryの収集、
次節に示すdocs内の設計・schema案・packet案・調査記録の作成。
Go実行の既存入口は次の通りで、source patch/probeを採る場合は既存workflowの保存手順に従う。
compiler/FourSlash以外のfamilyはまだhelperのcommandとして存在しない。

```sh
python3 scripts/typescript7.py setup
python3 scripts/typescript7.py build
python3 scripts/typescript7.py compiler checkingObjectWithThisInNamePositionNoCrash.ts
python3 scripts/typescript7.py compiler deleteExpressionMustBeOptional.ts
python3 scripts/typescript7.py fourslash TestBasicEdit
python3 scripts/typescript7.py fourslash TestGoToDefinitionImport3
```

これらはhelperのsmoke候補であり、対象機能のGo traceや7.1 Rust互換性を証明するtestではない。
今回の計画作成では実行していない。原本checkoutを切り替えて既存証拠を上書きしない。

## 子スライスと成果物

成果物の配置は本directory配下の`DESIGN.md`、`REPORT.md`、`records/`、`packets/`を案とする。
schemaを確定する前に空の完了記録を量産しない。重いcheckout/build/cacheは`target/typescript7/`、
再現に必要な小さいmanifest・source/probe patch・hash・receiptは追跡可能な配置へ保存する。

| 子 | 具体的な作業 | 提出するもの |
| --- | --- | --- |
| A0：P0引継ぎ | emitter完了scopeと旧known/deferred、C04/C05の適用範囲を照合 | 最終Rust SHA、修復済み/再測定要/後続owner表。古い失敗数を転記しない |
| A1：SYNC1a | ref registry、layout/toolchain判定、ref別checkout/cache/lock/resultの設計と実装packet | versioned refs schema、A/B選択の契約、入力不整合のerror、既存helper互換・移行test案 |
| A2：SYNC1b | 全test familyの構成・元相対path・設定・operation・baseline対応を採集 | cases schema、重複/未知/除外台帳、layout adapter設計。importを実装するfamilyとinventoryのみのfamilyを明示 |
| A3：SYNC2a/b | Issue/PR更新・固定commit差分をfeature ledgerへ接続 | feature/event schema、差分分類・再調査queue・checkpoint規則、収集失敗と未分類を残す設計 |
| A4：MAP/PIN設計 | 現Rust/PLAN-BASEを7.1の機能/testへ対応付け | retained/changed/removed/new/unknown表、adoption案、必要producer、pilot候補の比較 |
| A5：API/LS/LSP inventory | wire method、内部LS query、LSP capability、runnerとclientの対応 | 三つの独立したsurface表、最初のasync API/editor経路の依存、上流待ちのAPI条件 |
| A6：次batch具体化 | SYNC3/4＋pilotを実装可能な単位にする | Go trace計画、比較driverの観測schema、exact test/対照、Rust owner、変更file、focused/hosted案 |

依存はA0→A4、A1→A2→A3/A4、A2→A5、A3/A4/A5→A6。
読解・schema設計はemitter完了前にも進められるが、A0の完了とpilotの実差判定には最終baseが必要。
各子の実装はready packetを整えて進め、互換な変更を一つの候補へ合成してhostedへ出す。

### 参照とtest identity

refにはrepository、完全commit、release/dev状態、toolchain、layout、client/package/libs/generated data、
runner/probe/source patchのhashを結び付ける。現在の単一pinと旧recordは引き続き再現できるようにする。
同名basenameを同一testと扱わず、**ref＋family＋元相対path＋configuration/operation identity**を保存する。

| family | 既存pinで調べる場所 | 保存する意味 |
| --- | --- | --- |
| compiler/conformance | `tsc/testdata/tests/cases/{compiler,conformance}`、`tsc/internal/testrunner` | 仮想file/config・全option展開・診断とJS/d.ts/mapの対応 |
| transpile | `tsc/testdata/tests/cases/transpile`、transpile runner | 呼ぶAPI、options、結果・診断。通常compiler runnerへ置換しない |
| FourSlash | `tsc/internal/fourslash/tests` | marker/range、編集・query・assertionの順序とcapability |
| project/build/watch/LSP | `tsc/internal/project`、`execute`、`lsp`等のGo tests | 世代、イベント/仮想時計、要求・応答、restart、直接assertion |
| 公開API/client | `tsc/internal/api`、`ipc`、`packages/typescript/test/{async,sync}` | Go契約とclientの両側、async JSON-RPC / sync MessagePackの区別 |
| baseline | `tsc/testdata/baselines/reference` | 原本pathとbytes、生成元case/config。baselineを持たないassertion testも漏らさない |

7.0と7.1でpackage layoutも異なるため、固定refのtreeで判定する。
test入力をRust向けに変形する場合も原本・変換内容・hashを保存し、単純な`tests/`全移動はしない。
testsの構造へ合わせることと、Rust module/crateの名前をGoへそろえることは別である。

### 差分と未完成機能

feature IDにIssue番号だけを使わず、移管・分割・複数PR・revert後も同じ責務を追えるようにする。
最低限、導入世代と根拠、A/Bにおける実装範囲、spec/source/test、Rust owner、依存、優先度、
上流状態、Rust状態、未確定条件、再調査triggerを記録する。
Issue本文の更新はGit差分とは別eventで保存し、未取得を「変更なし」にしない。

選定候補には変更testだけでなく、lib/option/config/runner・共有producer変更で影響する既存testを含める。
全部を機械的に最小集合へ縮めない。依存が不明なら未分類を明示し、調査または広いhosted対照へ回す。
上流未完成、Rust未実装、Go/Rust差、旧版非対応、skip、baseline削除は別の結果状態にする。

## 実装時の検証設計

runner/ledger実装は状態を持つため、次の失敗を検出する意味のあるfocused testsが必要。
これは今回のdocs変更に新しいtestを追加する指示ではなく、A1〜A3実装packetの要求である。

- ref違い・toolchain/layout不明・dirty/probe付きsourceを取り違えない。旧resultを上書きしない。
- 同名test、configuration追加、operation順の変更、rename/delete/baseline-onlyを失わない。
- Git/Issue収集の欠測・pagination不足・不完全diffを成功扱いしない。
- 同じイベントの再処理、checkpoint更新、feature分割、upstream revertでも保留条件と証拠を保持する。
- zero-selected、skip-only、非zero exit、`.delete`やbaseline不一致をrunnerのgreenにしない。
- source/client/libs/runnerが対応しないA/B比較を拒否し、比較不能の理由を残す。

最終候補ではschema/ledger tests、既存helperのcompiler＋FourSlash smoke、採用した複数refの隔離を確認する。
別refで同一testを実行できない場合は代替smokeと理由を記録し、互換性比較成功には数えない。
Rust replayは実装変更の影響範囲だけ選ぶ。新runnerのhosted入口は統合担当が用意し、
共有CIの変更は最新[witness台帳](../witness-coverage/README.md)へ接続する。

## 次の依頼：SYNC3/4＋FEATURE-PILOT

A6は次の内容を一つの依頼にまとめる。

1. **不足一件を固定**：emitter後のbaseで再現し、Bで完成している独立scopeを選ぶ。第一候補はC04由来のnoCheck/transpileまたはemit要求境界だが、実差確認後に決定する。
2. **Goで実際に追う**：A/Bのsourceを読み、対象branchを通るprobeを実行。入力→判断→state→消費先→観測結果を記録する。
3. **同じ契約で比較**：Go A、Go B、Rust beforeの設定・test意味を揃える。旧版非対応はそのまま残す。instrumentationなしでも結果を確認する。
4. **Goの必要責務をRustへ移す**：ownerへ実装し、BとRust afterを比較。既存の6.0.3 scopeに変化があればintentional deltaと移行を明記する。
5. **まとめて検証・統合**：対照・隣接回帰・合成結果、hosted、PR/merge、profileと台帳を更新。report生成やGo test成功だけでは完了しない。

提出物はDESIGN/REPORT、ref/test/feature manifest、原本とprobe/source patch、実command・exit・hash、
before/after、原因別commit、次のsliceと残条件。実装候補、統合済み、profile-qualifiedを別状態にする。
公開API未完成や未対応runnerは該当scopeに残し、調査batch全体を「TypeScript 7.1対応完了」としない。
