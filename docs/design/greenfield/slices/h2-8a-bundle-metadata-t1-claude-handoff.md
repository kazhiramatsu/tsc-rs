# Claude 追加依頼：T1 — bundle の parse-node metadata 可搬性

作成日：2026-09-17。依頼種別：調査・限定修復候補。
提案スライス名：`H2.8a-A-RES-BUNDLE-METADATA-T1`。
既存 owner は H2.7d / A-INT3-CS。正式な runtime admission は統合担当が持つ。

## 1. 依頼すること

PR #549 を含む main から開始し、System bundle の JavaScript emit から declaration emit へ
parsed node の metadata を引き継ぐ際の `ParsedEmitMetadataNotPortable` を修復してください。
既知の T1 5件を固定した TypeScript 6.0.3 の完全な command 観測と一致させ、
source identity・コメント・metadata lifetime の隣接対照を添えた候補を提出してください。

最初に現在の差を再現し、upstream の所有権と寿命、現行 Rust との差を `DESIGN.md` に記録します。
その設計に従って専用 worktree で候補を実装してください。調査だけで終了する依頼ではありません。
新たな共通前提や別原因が判明した場合は、到達した結果と残る owner を明記します。

今回の依頼は C02 全体の再実装ではありません。C01〜C05 の提出候補は統合済みで、
C02 は [PR #549](https://github.com/kazhiramatsu/tsc-rs/pull/549) に入りました。
R9 の default class 名2件、R12 の文末 source map 2件、dispose 後の print の typed 差分2件は別スライスです。

## 2. 開始点と資料

確認済み main：`84da0c0278c296fd295f15e48177ada87810f841`（PR #549 の merge）。
この依頼書の保存元 worktree は古い checkout の可能性があります。保存場所の HEAD を開始点にせず、
最新 `origin/main` に #549 が含まれることを確認して、新しい branch/worktree を作ってください。

```sh
cd /Users/hiramatsu/dev/tsc-rs
git fetch origin main
T1_HANDOFF_BASE="$(git rev-parse origin/main)"
git merge-base --is-ancestor 84da0c0278c296fd295f15e48177ada87810f841 "$T1_HANDOFF_BASE" || exit 1
git worktree add -b draft/h2-8a-bundle-metadata-t1 ../tsc-rs-bundle-metadata-t1 "$T1_HANDOFF_BASE"
```

同名 branch/directory が既にあれば別名を選びます。既存の作業を reset して流用しません。
以下は新 worktree で実行してください。開始 SHA、status、toolchain、Node version、vendor hash を保存します。
この依頼書は開始 SHA に未収載のローカル資料なので、必要なら新 worktree へコピーして保持してください。

読む順序（パスは新 worktree 内）：

1. `docs/design/README.md`、`docs/design/greenfield/emitter-architecture.md`、
   `docs/design/greenfield/post-h1-completion-slices.md` §1.1。
2. `docs/design/greenfield/slices/h2-8a-generated-binding/REPORT.md` §3 / §5 の T1、
   `h2-8a-generated-binding/integration/revised/README.md`。
3. `docs/design/greenfield/slices/h2-8a-printer-comment-carry/README.md`、
   `crates/emitter/src/factory/parsed_metadata.rs` と下記の現行 source。
4. `docs/witness-testing.md` の focused / hosted 分担と実装をまとめる方針。

[#549 時点の残差報告](https://github.com/kazhiramatsu/tsc-rs/blob/84da0c0278c296fd295f15e48177ada87810f841/docs/design/greenfield/slices/h2-8a-generated-binding/REPORT.md)
と [既知差分 fixture](https://github.com/kazhiramatsu/tsc-rs/blob/84da0c0278c296fd295f15e48177ada87810f841/crates/compiler/tests/fixtures/decorator-binding-known-native.json)
は固定した参照です。過去の報告中の「hosted 未登録／検証待ち」は採取時の状態です。
#549 の CI は全13 checks成功後にマージされています。新候補の検証をこの成功で代用しません。

共通 handoff index に残る「次は② binding」と旧 v18 復元手順を、今回の開始指示として使いません。
元 C02 patch の再適用も不要です。

## 3. 固定する対象5件

共通 prefix：`decorator-binding/lifecycle/`。以下が完全な case ID です。

```text
decorator-binding/lifecycle/es2015/define/bundle-file-level-then-scoped-across-files
decorator-binding/lifecycle/es2015/set/bundle-file-level-then-scoped-across-files
decorator-binding/lifecycle/es2022/define/bundle-file-level-then-scoped-across-files
decorator-binding/lifecycle/es2022/set/bundle-file-level-then-scoped-across-files
decorator-binding/lifecycle/esnext/set/bundle-file-level-then-scoped-across-files
```

開始点では5件とも `class: T1`。凍結した native 結果は
`Emit(Transform(ParsedEmitMetadataNotPortable(TransformNode { source: TransformSourceId(1), node: NodeId(_) })))`。
`NodeId(_)` は記録上の表現です。特定の node 番号や fixture 文字列で production を分岐させません。

入力は `crates/compiler/tests/fixtures/decorator-binding-inputs.json`、期待値は
`decorator-binding.json.zst`、native の既知差分は `decorator-binding-known-native.json`。
既存入力・upstream期待値・比較対象を変更して一致させず、追加対照は別 ID として採取します。

## 4. 確認済みの境界と調査点

T1 の第1原因だった `InternalEmitFlags` の未伝播は #549 で修復済みです。
残る第2原因は、parsed Identifier の `comment_range: EndOnly` を snapshot が拒否する経路です。
これが現在の before と一致することを確認し、annotation の生成から消費までを追ってください。

| 現行 source / symbol | 所有している処理 |
| --- | --- |
| `crates/emitter/src/factory/parsed_metadata.rs` — `ParsedEmitMetadata`、`ParsedNodeMetadata`、`snapshot_parsed_emit_metadata`、`restore_parsed_emit_metadata` | parsed metadata の明示的な許可項目、Program source identity、snapshot/lease 検証、再 mount、atomic restore |
| `crates/emitter/src/metadata.rs` — `CommentRange`、`CommentSourceRange` | source とコメント両端の意味。`EndOnly` / `StartOnly` / `Original` / `Synthesized` を区別 |
| `crates/emitter/src/execute.rs` | 通常 Bundle の JS print 完了後、dispose 前に snapshot を採取。SourceFile root は同じ持ち越しをしない |
| `crates/emitter/src/declarations/orchestration.rs` | 新しい declaration arena を mount した後、transform 前に metadata を restore |
| `crates/compiler/tests/h2_7d_declaration_bundles.rs` | ordinary bundle と fresh/forced declaration の metadata lifetime、visitor/printer、map 対照 |

`CommentRange` は arena 内の `TransformSourceId` を含みます。そのままコピーして別 arena の
source として解釈すると、mount 順序が変わったときに別ファイルを参照し得ます。
現在の snapshot は `(SourceFileId, NodeId)` と source の snapshot / lease / parsed interval を使います。
comment range の source にも適切な identity と再 mount の対応が必要か、上流と現行実装から設計してください。
range の source と annotation が付いた node の source が同じという仮定も検証対象です。

必須の設計上の確認点：

- `comment_range` の producer、JS print 中の利用、declaration 側への持ち越しと利用、dispose の順序。
- byte位置と UTF-16位置の区別、一方だけを持つ端点と synthesized の意味。
- mount 順序変更、別 Program／同じ数値 NodeId、別 snapshot／parse lease、既存 metadata との衝突。
- restore 途中の失敗が対象 arena に部分的な書き込みを残さないこと。
- synthetic node と `original` chain を parsed metadata として誤って持ち越さないこと。
- 通常 Bundle から同じ emit の declaration への寿命と、SourceFile／fresh getter／forced emit の寿命の区別。

許可項目を増やす場合は capture・identity 検証・restore・consumer を一組として扱います。
`remainder.comment_range = None` だけで拒否を消す変更や、`EmitMetadata` 全体の無条件 clone は避けてください。
別の未対応 field の拒否は維持します。コメント annotation 自体を消す修復なら、上流との同一性を実証します。

upstream は vendored TypeScript **6.0.3**。入口の探索用アンカーは `_tsc.js` の
`disposeEmitNodes`（25302）、`getCommentRange`（25358）、`setCommentRange`（25362）、
`transformNodes`（115977）、`emitLeadingCommentsOfNode`（121007）です。
行番号はこの pin の探索用です。`DESIGN.md` には実際に読んだ body span と hash、caller/callee、
branch、所有者、失敗順序まで固定してください。TS7 の仕様変更は別の disposition とします。

## 5. focused 検証手順

新 worktree 内で次の関数を定義すると、対象を5件に固定できます。

```sh
t1_witness() {
  CARGO_BUILD_JOBS=2 taskpolicy -b nice -n 15 python3 scripts/witness.py decorator-binding-pipeline \
    --case decorator-binding/lifecycle/es2015/define/bundle-file-level-then-scoped-across-files \
    --case decorator-binding/lifecycle/es2015/set/bundle-file-level-then-scoped-across-files \
    --case decorator-binding/lifecycle/es2022/define/bundle-file-level-then-scoped-across-files \
    --case decorator-binding/lifecycle/es2022/set/bundle-file-level-then-scoped-across-files \
    --case decorator-binding/lifecycle/esnext/set/bundle-file-level-then-scoped-across-files \
    "$@"
}
t1_witness --list
t1_witness --dry-run
# before / after はそれぞれ source と binary を固定し、stdout/stderr・実 exit を保存する。
t1_witness
```

`--list` / `--dry-run` は build・oracle・native replay を行いません。
この資料の作成時に、#549 と同じ runner/input の dry-run が **selected 5 input cases** になることを確認済みです。
今回の資料作成では新たな Rust replay は行っていません。

開始点の期待 summary は `exact=0 known=5 failed=0 selected=5`。
これは5件の拒否が凍結どおり再現したという意味で、互換成功ではありません。
既存 harness は各対象を2回比較します。

修復で結果が変わると、known-native の比較が失敗するのが正常です。完全一致を確認するために、
候補側の known fixture から対応する T1 行だけを取り除き、同じ5件を通常の完全比較へ通してください。
目標は `exact=5 known=0 failed=0 selected=5`。before 記録と基点 fixture は保存します。
エラーを解消した後に別の出力差が見えた場合は、その差を記録し、T1 5件完了とは報告しません。

比較対象は JS / declaration / map の bytes・path・書込順序、診断、emit result、status、exit、
callback metadata と partial writes を含む既存 complete-command 契約です。
typed error の解消や JS 単独一致だけで完了にしません。

追加対照は §4 のリスクに合わせて選び、特に異なる source の mount 順序、source identity の拒否、
片側 comment endpoint、UTF-16を含む source、ordinary/fresh/forced の寿命を観測してください。
上流期待値は新規保存先へ採取し、2回一致を確認します。Rust 固有の拒否対照は互換性の件数と分けます。

隣接検証の入口（変更面に応じて選び、件数と実結果を保存）：

```sh
CARGO_BUILD_JOBS=2 taskpolicy -b nice -n 15 cargo test --manifest-path crates/emitter/Cargo.toml --lib parsed_metadata_ -- --nocapture
CARGO_BUILD_JOBS=2 taskpolicy -b nice -n 15 python3 scripts/witness.py bundle-declarations --all
CARGO_BUILD_JOBS=2 taskpolicy -b nice -n 15 python3 scripts/witness.py bundle-program --all
```

`bundle-declarations` は登録済みの3 testsと意図した1 filtered testを持ちます。
`bundle-program` は4 testsです。0 testsや想定外の filterを成功扱いしません。
printer や generated binding 自体を変更した場合は、その変更面に応じた direct controls も選択します。

全767 complete commands の pipeline replay、SUPER / acceptance の重い集合は hosted が担当します。
統合担当は依存関係の合う候補を合成した最終 source でまとめて CI を実行します。
T1 のみが完全一致へ移った場合、既存分母の pipeline は **763 exact / 4 known / 767 complete** が目標です。
残り4 known は R9/R12。別記の upstream exception 1件に互換成功を加点しません。
追加入力を採用する場合は既存767件と別に件数を記録し、選択・CI登録も整合させます。

## 6. 提出物と完了条件

提出先の目安：`docs/design/greenfield/slices/h2-8a-bundle-metadata-t1/`。

- `DESIGN.md`：開始 SHA、upstream span/hash、現行 gap matrix、型と source identity の設計、
  所有権・寿命・失敗順序、変更ファイル、既存 architecture/schedule との対応。
- `REPORT.md`：対象5件と追加対照の before/after、完全一致・既知差分・Rust固有対照・未実行を別集計。
- 入力と期待値、observer / 比較手順、実 argv/env/exit、ログ、source/binary hash、再現コマンド。
- base から候補への patch または commit、known-native から除去した ID の一覧。
- hosted 入口、必要な suite、未実行の重い検証、発見した別 owner の残差。

Claude の候補提出条件は、設計と修復、対象5件の complete exact ×2、必要な新規対照と隣接検証、
上記の再現可能な記録です。新 source で未実行の検証は未実行と明記してください。
base 照合、合成、hosted CI、PR/merge、正式な admission と進捗表の更新は統合担当が行います。

この資料は追加依頼の入口であり、それ自体が implementation-ready / runtime-ready の認定ではありません。
production 編集前に、schedule §1.1 の設計項目と参照する architecture 行を候補側で満たしてください。
