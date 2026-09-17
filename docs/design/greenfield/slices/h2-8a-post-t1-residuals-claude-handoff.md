# Claude 一括依頼：R9 / R12 / private receiver map / コメント制御の残差

作成日：2026-09-17。依頼種別：原因別の設計・限定修復候補。
提案する親 ID：`H2.8a-A-RES-POST-T1`。以下の5子スライスを一つの依頼・worktreeで進める。
各子スライスの設計・before/after・差分は分け、合成した最終候補を一度に提出する。
本書は依頼資料であり、production readiness や H2.8 全体の admission の認定ではない。

## 1. 今回お願いする範囲

PR #549 と #550 を含む main から、C02/T1 の後に残る次の5項目を扱ってください。
最初の3項目は外部出力に再現済みの差があり、設計・修復・検証までを依頼します。
後半2項目は内部 metadata の差です。公開観測と現行のコメント所有権を確認し、
意味上必要な修復、または根拠と対照を伴う実装差の disposition を提出してください。

| 子 ID | 対象 | 現在の観測 | 到達点 |
| --- | --- | --- | --- |
| R9 | 匿名 default class の生成名と global / bundle 名前表 | complete command 2件の名前・出力差 | 2件 complete exact ×2、名前生成の隣接退行なし |
| R12 | System が hoist する class 文の末尾 source map | C02 2件 + T1 2件、計4 complete commands の map 差 | 4件 complete exact ×2、decorated / undecorated・export有無の対照 |
| RECEIVER-MAP | static private の複合代入の receiver temp | T1 1 complete command の余分な map segment | complete exact ×2、receiver/key/RHS の評価順序・回数と他の map を維持 |
| PRIVATE-SET-COMMENTS | private setter helper の右辺のコメント所有権 | T1 1 packet probe、Rustだけ `NoTrailingComments` | コメントを含む新規対照で意味を確認し、必要な修復または実装差の根拠を提出 |
| DECORATOR-COMMENTS | decorator 式の `NoComments` の producer / lifetime | T1 2 packet probes、upstreamだけ flagあり | sourceからconsumerまで追い、必要な修復または実装差の根拠を提出 |

外部出力の修復対象は **7 complete commands**、内部観測は別集合の **3 packet probes**。
probe の一致を compiler 互換成功に加算しません。後半2項目の既存 complete commands は既に exact です。
flag の整数値を同じにするだけの変更、正しいコメント抑制を消す変更は完了条件ではありません。

処理順は R9 → R12 → RECEIVER-MAP → PRIVATE-SET-COMMENTS → DECORATOR-COMMENTS。
同じ `class_fields/downlevel.rs` に触る3・4番目は、先の修復の上で順に進めてください。
原因別の commit / patch を残し、最終状態で組合せを検証します。子スライスごとに PR / 全件CIを回しません。
ある子の前提が未解決ならその理由・証拠・次 owner を残し、独立して進められる子を続行します。

## 2. 開始点と読む資料

確認済み main は `eb6dc2c7872b18442657f8eefde5efc9e8fb4cf7`（PR #550 merge）。
C01〜C05 と T1 の候補修復は統合済みです。依頼書の保存元は古い checkout のことがあるため、
保存場所の HEAD や旧 C02/T1 branch を実装ベースにしないでください。

```sh
cd /Users/hiramatsu/dev/tsc-rs
git fetch origin main
POST_T1_BASE="$(git rev-parse origin/main)"
git merge-base --is-ancestor eb6dc2c7872b18442657f8eefde5efc9e8fb4cf7 "$POST_T1_BASE" || exit 1
git worktree add -b draft/h2-8a-post-t1-residuals ../tsc-rs-post-t1-residuals "$POST_T1_BASE"
```

同名 branch/directory があれば別名を使い、既存作業を reset しません。
この依頼書が開始 SHA に未収載なら、新 worktree にコピーして保持します。
開始 SHA、status、source/input hash、toolchain、Node version を保存してください。

読む順序（リンク先は新 worktree の現行版を使う）：

1. [design index](../../README.md)、[emitter architecture](../emitter-architecture.md)、
   [schedule §1.1](../post-h1-completion-slices.md#11-mandatory-implementation-ready-design-gate)。
2. [C02 REPORT §3](h2-8a-generated-binding/REPORT.md#3-残差の-disposition)、
   [C02 統合レビュー](h2-8a-generated-binding/integration/revised/README.md)。
3. [T1 REPORT §3.2](h2-8a-bundle-metadata-t1/REPORT.md#32-既知差分として凍結した行本スライス外の-owner)、
   [T1 DESIGN](h2-8a-bundle-metadata-t1/DESIGN.md)、
   [T1 統合記録](h2-8a-bundle-metadata-t1/integration/README.md)。
4. [共通手順](claude-high-difficulty-handoffs.md)、[focused / hosted 方針](../../../witness-testing.md)。

基準 semantics は TypeScript **6.0.3**。既存の期待値をこの版から変更しません。

```text
vendor/typescript-6.0.3/lib/_tsc.js
SHA-256 1c59e77a54b186ec43fa7f3e0d3c4bb15ca5eb5ba43e96b1d3a267139eddd3e3
vendor/typescript-6.0.3/lib/typescript.js
SHA-256 569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39
```

過去の REPORT の T1 5失敗や「hosted未実行」は採取時点の記録です。
#550 は T1 修復を含み、現在の pipeline known は R9/R12 の4行です。
開始点が進んでいる場合は現在の fixture と差分を照合し、既に修復済みの項目を再実装しません。

## 3. 固定する既存 ID

R9（`decorator-binding-known-native.json` の2行）：

```text
decorator-binding/global/esnext/define/script-let-default_1
decorator-binding/lifecycle/esnext/define/bundle-default-two-files
```

R12（上の fixture の2行と `bundle-metadata-t1-known-native.json` の2行）：

```text
decorator-binding/lifecycle/esnext/define/bundle-computed-temps-two-files
decorator-binding/lifecycle/esnext/define/bundle-file-level-then-scoped-across-files
bundle-metadata-t1/residual/es2015/system-export-class-map
bundle-metadata-t1/residual/es2015/system-hoisted-class-map
```

RECEIVER-MAP（`bundle-metadata-t1-known-native.json` の1行）：

```text
bundle-metadata-t1/receiver/es2015/static-compound-second-file
```

PRIVATE-SET-COMMENTS / DECORATOR-COMMENTS（`bundle-metadata-t1-known-packet.json` の3行）：

```text
bundle-metadata-t1/receiver/es2015/static-set-second-file
bundle-metadata-t1/decorated/es2022/static-get-second-file
bundle-metadata-t1/decorated/esnext/static-get-second-file
```

fixture はすべて `crates/compiler/tests/fixtures/`。
既存入力は `decorator-binding-inputs.json` / `bundle-metadata-t1-inputs.json`、
上流期待値は `decorator-binding.json.zst` / `bundle-metadata-t1.json`。
入力・期待値・比較対象・分母を変更して成功にしません。新規対照は別 ID・別 artifact にします。

## 4. 子スライスの調査・修復契約

各子の production 編集前に `DESIGN.md` の対応節を完成させてください。
upstream body span/hash・calleeと分岐、現行 Rust の producer/consumer/lifetime、gap matrix、
変更ファイル、型・identity・rangeの設計、正負の witness、検証コマンドを固定します。
関係する architecture 行（少なくとも `E-NAMES-BASE`、`E-NAMES-CLASS-G`、
`E-METADATA-BASE`、`E-METADATA-G-CLASS` と実際の printer/map 境界）を現行 source と照合します。
本書の owner は探索の入口です。旧 REPORT の修復案をそのまま現行 gap と決めつけません。

### R9

主な入口は `crates/emitter/src/builtins/es_next.rs` の `hoist_binding_identifier` と
default class の生成、TypeScript transform、`builtins/target_bindings.rs` / `generated_bindings.rs`。
現行 `es_next.rs` にも `TargetBinding::allocate_numbered` の利用があるため、
「型が未導入」と仮定せず、問題の class の宣言・参照がどこで文字列へ落ちるか追います。

global `default_1`、前 source の生成名、同名の parsed/synthetic identifier を区別します。
宣言・hoist・export・参照が同じ binding identity を共有し、finalizer の global oracle と
bundle の名前表を使うことを source と照合してください。runtime の class name と印字名も区別します。
隣接対照は衝突あり/なし、匿名/名前付き、単一/複数 source、source順反転、
ESNext define と既に降格する ES2015/ES2022/set 経路から必要な集合を固定します。

### R12

主な入口は `crates/emitter/src/builtins/system.rs`。
System の `A = class A … };` の末尾に upstream が発行する class 終端の2 segmentが欠けています。
decorator のない class、export なしにも再現しています。class expression、代入、
ExpressionStatementそれぞれの original/range/emit flags が printer のどの境界で消費されるかを追います。
印字後の `.map` の書換えや、特定の行・column・case名による特例は使いません。

4対象に加え、宣言emitなし、class末尾のコメント・改行、非BMPのsource位置、
他の module 形式を対照にします。JS / d.ts / d.ts.map が既に一致することも保護します。

### RECEIVER-MAP

主な入口は `crates/emitter/src/builtins/class_fields/downlevel.rs` の receiver 安定化・clone と
private compound assignment。`B.#p += 1` の `_b = _a` / `_b` が source の `B` に余分に map されます。
C02 の receiver 修復との違いを調べ、temp の map provenance と original/text/comment provenance を分けます。
すべての receiver range を一括削除せず、source owner に対応する temp の経路を修復してください。

隣接対照は read / simple set / compound / update、static / instance、安定した receiver / 副作用あり、
値を利用 / 破棄、bundle の第1 / 第2 sourceから必要な集合を選びます。
変換された実行式に変更が及ぶ場合は、評価順序・回数・式の値を runtime controlでも確認します。

### PRIVATE-SET-COMMENTS

主な入口は同じ `downlevel.rs` の `create_private_set`。
parsed 右辺に残る `NoTrailingComments` は helper 引数内と外側式のコメント二重印字を防ぐ
Rust固有の境界です。まず前置・後置・行末・引数境界コメントと removeComments を含む
complete-command witness を採り、producer→helper引数→outer original→bundle packet→declaration の寿命を追います。

public output が壊れるならその owner で修復します。内部 flag を source 同等にする修復なら、
抑制の正しい所有者へ移して全対照を保護します。現行の実装差が必要なら、
理由・失敗する代案・対照の一致と残る範囲を記録し、known-packet を維持します。
比較から flag を除外したり、理由なしに既知差分を exact と呼んだりしません。

### DECORATOR-COMMENTS

主な入口は `crates/emitter/src/builtins/standard_decorators.rs`。
upstream `transformDecorator`（探索用 `_tsc.js:100554–100556`）の
`setEmitFlags(expression, NoComments)` の適用対象・訪問順・parsed node の寿命を追います。
parsed node が共有される場合、visited/synthetic/clone のどの identity に flag が付くかも確認します。

decoratorの前後・式内部・computed nameとの境界のコメント、単一/複数 decorator、
native / lowered、removeComments、bundle / 通常 sourceを対照にします。
必要な修復または実装差の disposition の条件は PRIVATE-SET-COMMENTS と同じです。
既存2件で JS が一致することだけを、コメント全経路が正しい根拠にはしません。

### 変更範囲と成果の分離

上記 owner を中心に、再現した原因に必要な factory / metadata / finalizer / printer seam を
設計節へ追記してから修復します。追加する test / observer / fixture の具体的パスも先に固定します。
独立した新 subsystem、dispose 後 print の session contract、同一 Program の API 再emit、
TS7移行、transpile/cacheの再実装は今回の対象外です。
T1 の packet source identity・atomic restore・未対応 field の typed refusal は維持します。
profile、STAGE、accepted-state ratchet、共有CI policyの変更・PR/merge・admission は統合担当が持ちます。

## 5. ローカル検証と最終合成

新 worktree 内で以下を使用できます。macOS では低優先度・Cargo 2 worker、重い実行は一度に一つ。

```sh
post_t1_pipeline() {
  CARGO_BUILD_JOBS=2 taskpolicy -b nice -n 15 python3 scripts/witness.py decorator-binding-pipeline \
    --case decorator-binding/global/esnext/define/script-let-default_1 \
    --case decorator-binding/lifecycle/esnext/define/bundle-default-two-files \
    --case decorator-binding/lifecycle/esnext/define/bundle-computed-temps-two-files \
    --case decorator-binding/lifecycle/esnext/define/bundle-file-level-then-scoped-across-files \
    "$@"
}
post_t1_pipeline --list
post_t1_pipeline --dry-run
post_t1_pipeline

python3 scripts/witness.py bundle-metadata-t1 --all --dry-run
CARGO_BUILD_JOBS=2 taskpolicy -b nice -n 15 python3 scripts/witness.py bundle-metadata-t1 --all
```

後者は18 complete commands /15 packet probesを実行する小さい owner suiteで、case selector はありません。
2 tests /8 filtered を期待します。0 tests、想定外のfilter、knownの再現だけを互換成功に数えません。
pipeline runner の oracle `--check` は native の4件選択とは別に全入力を検証します。
開始点・最終候補では正規 runner を使い、編集中は同じfixtureの必要なnative testだけを
選ぶ場合も実argv/env・件数・exitを保存し、oracleを実行したとは報告しません。

| 集合 | #550で保持する基準 | 今回の到達目標 |
| --- | --- | --- |
| 上の pipeline focused 4 commands | 0 exact /4 known /0 failed | 4 exact /0 known /0 failed、各2回 |
| T1 complete 18 commands | 15 exact /3 known /0 failed | 18 exact /0 known /0 failed、各2回 |
| T1 packet 15 probes | 12 exact /3 known /0 failed | 調査後の修復件数と残る実装差を明記、既存exact退行0 |
| pipeline 全767 commands（hosted） | 763 exact /4 known | 767 exact /0 known、upstream exception 1件は別集計 |

これは目標であり、依頼書作成時の新規 Rust replay 結果ではありません。
作成時にはmain `eb6dc2c78`のrunnerで、上記dry-runがpipeline 4入力・T1 18入力を選ぶこと、
native knownが4＋3行・packet knownが3行で全IDが本書に含まれることを確認しました。
Rust/Nodeの実観測はこの資料作成では実行していません。
known-native/known-packet は、修復が確認できた ID だけ候補側で retire して通常比較へ通します。
既知差分が exact に変わると既存 guard が fail するため、guardを無効化せず行の除去と証拠を組にします。
新しい差を元の known の許容範囲に追加しません。before と元のfixtureは保存します。

比較対象は JS/declaration/maps の bytes・path・書込順序、diagnostics、emit result、status、exit、
callback metadata・partial writesを含む既存complete-command契約です。
内部probe、direct API、runtime controlsはそれぞれ別集計にします。

各子の編集ループは対象失敗と必要な隣接対照。合成後に4件とT1 suite、新規対照を再確認します。
既存の `decorator-binding`、`direct`、`bundle-program`、`bundle-declarations`、
`module-identities`、`bundle-original-javascript` は変更ownerに必要なものを選んで実行してください。
source/map/generationを共有するため、全件pipeline・SUPER・retained・acceptanceは最終合成候補のhostedで検証します。
ローカルの全767・530・672やfull legacy CIを、子ごとの終了条件に戻しません。

新規fixtureは固定upstreamから新しい保存先へ2回採取し一致を確認します。
専用targetを追加する場合は、実行コマンド・入力件数・test件数・依存・所要時間を提出し、
既存hosted targetに含まれたと推測しません。統合担当がrunner/planner/CIに登録します。

## 6. 提出物

提出先の目安：`docs/design/greenfield/slices/h2-8a-post-t1-residuals/`。

- `DESIGN.md`：共通baseと5子それぞれのsource map、gap、意味上の所有権、変更範囲、設計判断。
- `REPORT.md`：子別・最終合成別のbefore/after、7 complete commandsと3 probeの独立した集計、
  新規対照、retireしたID、未解決のowner、内部実装差の根拠、未実行の集合。
- 原因別patch/commitと合成候補SHA、source/input/binary hash、実argv/env/exit、ログとcapture、
  解析scriptと再現手順。測定後にproductionを変えた場合は最終候補の該当検証をやり直す。
- hostedへ渡す対象・入口・依存・時間予算。候補提出、統合、qualificationの完了を区別する。

最初の3子は対象7件のcomplete exact ×2と隣接退行0、後半2子は意味のある対照と
根拠を伴う修復/dispositionが候補提出条件です。未完了の子があれば明示し、
独立して完了した子とレビュー可能な差分を同じ提出物に保持してください。
