# Claude 向け高難度スライス：依頼資料一覧と共通手順

作成日：2026-09-14。更新日：2026-09-17。文書種別：依頼資料・research。
各資料を一つずつ Claude に渡して、現在の実装に残る差の監査と修復、または先行 prototype を依頼します。
production readiness、accepted profile、runtime activation は各 owner の正式な条件で判断します。

## 現在の状態と依頼の選び方

[全残タスクと完了までのスライス設計](../remaining-completion-slices.md) に、
Claude 担当 C01〜C05 と、それ以外の実装・統合・検証・配布を分けて記録しました。
本書の5件だけでプロジェクトの残タスクが尽きるわけではありません。
③ printer の隔離候補は受領済みで、[A-INT3 の統合レビュー](h2-8a-printer-failure/INTEGRATION.md)へ進みました。
④ transpile の候補も [PR #535 で統合済み](h2-8c-transpile/INTEGRATION.md)です。
⑤ cache はR1〜R4再提出を受領し、統合側の追加修正と専用CI入口を含む[PR #539](https://github.com/kazhiramatsu/tsc-rs/pull/539)で統合済みです。[統合記録](l2-3-resolution-cache/integration/README.md)を参照。
① literal は候補を受領し、[A-INT1統合](h2-8a-literal-update/integration/README.md)でCI入口を追加し、PR #542の全7 hosted job・両gate成功後にmainへ統合済みです。
② binding も [PR #549](https://github.com/kazhiramatsu/tsc-rs/pull/549) で統合済みです。
当初の5件は候補統合が一巡しました。追加依頼の
[T1：bundle の parse-node metadata 可搬性](h2-8a-bundle-metadata-t1-claude-handoff.md)は候補 `a17009c58` を受領し、[統合検証](h2-8a-bundle-metadata-t1/integration/README.md)へ進んでいます。
R9/R12 は別の追加依頼候補で、今回の T1 には含めません。
[A-INT3-CS](h2-8a-printer-comment-carry/README.md) と
[API1.2-HINT](api1-2-printer-hook-hints.md) は統合済みです。
統合担当の[A-PC1](h2-8a-compact-body-comments.md)もPR #538で完了しています。
①②は現行実装との対応表から始め、再現した差があれば優先順位を上げます。
各候補の本番統合と admission は統合担当が持ちます。

[PLAN-BASE の照合台帳](plan-base/README.md)も参照できます。旧 class 128失敗のうち88件は
同じ fixture の現在の hosted で exact、40件は未再測定です。C01/C02 では古い失敗一覧を
そのまま修復対象にせず台帳と照合してください。③を丸ごと再実装する依頼は不要です。

旧版は `6e298cda8` からの v18 復元を全依頼の開始点にしていました。
その後、main に UTF-16 の値・template flags、generated binding、parameter の修正が入りました。
**旧版の「未実装」という説明を、そのまま現在の不具合として依頼しないでください。**
個別資料の旧ベース・旧実装の説明・全件ローカル実行指示より、本書の開始点と検証方針を優先します。

2026-09-15 の確認基準は main `fbce8727f0f4a073f9149c190f2913db69952df2` と
[SUPER 統合 PR #523](https://github.com/kazhiramatsu/tsc-rs/pull/523) の
`f6444330025da144ce77f140990e898b4d7b2be6` です。
SUPER の hosted witness は primary 670（upstream 例外 2 は別記）、controls 408、
retained 530 が各 2 回一致し、direct は 28 一致・既知差分 4 を維持しました。
既存 acceptance も 46 分 23 秒で成功し、PR #523 は main
`d671d8417d725ce54a4cfc42b6e7127c03347646` にマージ済みです。
新規依頼の開始点は、下記のとおり **SUPER がマージされた main** に固定します。
CI 改修も [PR #524](https://github.com/kazhiramatsu/tsc-rs/pull/524) で main
`bb2d51c89` にマージ済みです。新規依頼ではこの改修も含む最新 main を使い、
古い worktree に新しい手順だけを当てないでください。

| 候補 | 個別資料 | 今回依頼する到達点 | 現在の扱い |
| --- | --- | --- | --- |
| ① UTF-16 リテラルの更新・伝播 | [A40-LITERAL-UPDATE](h2-8a-literal-update-claude-handoff.md) | 現行 factory の値更新・raw/quote/text-source/flags の残経路監査、差がある場合の修復 | `JsString` と templateFlags の移行済み部分を再実装しない。新規の失敗数は未計測 |
| ② decorator の生成名・binding | [A41-BINDING](h2-8a-generated-binding-claude-handoff.md) | 現行の型付き binding に対する global/synthetic/nested/lifecycle の残経路監査と修復 | PR #549 で統合済み。failure-carry を修復。残る T1 5件は追加依頼、R9/R12 4件は別 owner |
| ③ printer の失敗時状態・再利用 | [A40-PRINT-FAILURE](h2-8a-printer-failure-claude-handoff.md) | 失敗順序・継続状態の observer、必要な隔離修復 | 提出済み、A-INT3 が統合。PR #527 / #528 で統合済み。提出分24/25 exact。生成名は②、追加 hook hint 差は API1.2-HINT。全 API 完了とは扱わない |
| ④ noCheck / transpile パイプライン | [H2.8c 先行依頼](h2-8c-transpile-claude-handoff.md) | 3 経路の依存設計、source oracle、隔離 prototype | 提出・統合済み、PR #535。301入力の専用witnessを含む。CLI/config activationとH2.8c全体のqualificationは後続 |
| ⑤ resolution cache 無効化 | [L2.3 先行依頼](l2-3-resolution-cache-claude-handoff.md) | snapshot/dependency 設計、実 resolver を使う隔離 prototype | PR #539 で bounded prototype を統合済み。Program 再利用への組込みは L2.3a/b |
| 提出済み・統合済み | [A41-SUPER](h2-8a-decorator-super-claude-handoff.md) | after-18 の候補・8 patch・receipt は保存済み | PR #523。旧依頼文を新規実装依頼として再送しない |

①②の最初の成果物は、旧要求と現行 source/既存 witness の対応表です。
各行を「既存実装で観測済み」「追加対照が必要」「差を再現」「到達前提が未成立」に分け、
差を再現した範囲を実装候補にします。旧要求の全項目を未実装と仮定してコードを増やしません。
③④⑤の測定範囲と保留は各統合記録を参照してください。
この一覧は完了・未完了の網羅証明ではありません。

## 共通の読み順と開始点

1. [design index](../../README.md) の文書優先順位。
2. [emitter architecture](../emitter-architecture.md) と
   [schedule](../post-h1-completion-slices.md) の該当 owner / readiness 条件。
3. 本書、選んだ個別依頼、main の実装・既存 witness。

新規作業は専用 branch/worktree を使い、開始 SHA を記録します。例：

```sh
git fetch origin main
HANDOFF_BASE=$(git rev-parse origin/main)
git merge-base --is-ancestor f6444330025da144ce77f140990e898b4d7b2be6 "$HANDOFF_BASE" &&
  git worktree add -b draft/h2-8a-printer-failure ../tsc-rs-printer-failure "$HANDOFF_BASE"
```

branch/directory は選んだ依頼の名前にします。開始時の `HEAD`、`git status --short`、
source/input manifest、toolchain、vendor hash を保存してください。
main に統合済みの旧 patch を再適用しません。既存 worktree を reset して使い回しません。

```text
TypeScript semantics baseline: 6.0.3
vendor/typescript-6.0.3/lib/_tsc.js SHA-256:
1c59e77a54b186ec43fa7f3e0d3c4bb15ca5eb5ba43e96b1d3a267139eddd3e3
vendor/typescript-6.0.3/lib/typescript.js SHA-256:
569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39
```

⑤の Go reference pin は個別資料に従います。6.0.3 の採用 semantics と混同しません。

## 実験と観測の共通契約

- 期待値は固定 upstream から新しい保存先へ採取し、2 回一致を確認する。
  新規 ID、入力 manifest、対象件数を native 実行前に固定する。
- complete Program command は JS/declaration/map の bytes・path・順序、write callback
  metadata、reported/emit diagnostics、emit result、status、exit、partial writes を比較する。
- direct API はその API の公開観測を比較する。CLI exit や callback が存在しない API に
  架空の観測を追加しない。内部 probe、runtime control、direct metadata は別集計にする。
- upstream exception と Rust-only typed error を分ける。Rust-only error の安全性検証を
  TypeScript 互換性に加点しない。fixture/比較式/分母を変更して一致させない。
- before/after の入力と binary SHA、実 argv/env、実 exit、ログ、capture、比較 script、
  candidate patch と source snapshot を保持する。成功履歴で新しい before を代用しない。
- 既存の失敗と新規の退行を開始 SHA の対照で区別する。旧 v18 の test 数や lint green を
  現在の main へ引き継がない。今回の統合では emitter Clippy に main と同じ既存 16 指摘がある。

## 検証の分担：ローカルは focused、重い replay は hosted

2026-09-15 のユーザー指示です。個別依頼に残る「最後に 530 全件と emitter 全 suite を
ローカル実行」という旧指示は、この方針に置き換えます。

- 編集ループは失敗した新規ケースと変更 owner の focused test。対象の ID/件数を記録し、
  0 tests やフィルタ不一致を成功として報告しない。
- ローカルの隣接 regression は変更面に必要な集合を選び、最終 bytes でまとめる。
  530 / 672 等の重い replay、全 oracle chain、full legacy CI を一律のローカル終了条件にしない。
- 全件が必要な集合は hosted の対応 job/target で検証し、PR head・run URL・実結果を保存する。
  `cargo xtask acceptance` だけでは decorator witness target を実行したことにならない。
- hosted に載っていない新規 target は、対象・件数・想定時間と実行入口を提出物に明記する。
  未実行の全件を成功扱いせず、統合担当が対応する job を追加する。
- hosted は 60 分制限を前提に、長い集合を独立 job に分け、変更と無関係な集合を実行しない。
  共通実装の変更や依存が不明な変更では必要な全体 coverage を維持する。
- 重い実行はローカルで一度に一つ。macOS は `taskpolicy -b nice -n 15`、
  `CARGO_BUILD_JOBS=2`。計測中に入力を編集せず、同じ handle を終了まで監視する。
  性能測定は機能テストと分け、同じ優先度・環境の before/after を比較する。
- capture を有効にすると追加 emit が走る既存 observer がある。capture は必要な診断ケースに
  絞り、性能比較の両側で条件をそろえる。成功済みの重い chain に隣接変更を同乗させて
  同じ全件検証を繰り返す進め方を避ける。

### 現在使える focused 入口の例

以下から変更 owner に必要なものを選びます。全コマンドの一律実行リストではありません。
専用 worktree 内で stdout/stderr、終了コード、実行されたケース数を保存します。

```sh
# 先に対象 ID を確認（ビルドも replay もしない）
python3 scripts/witness.py followup3 --list

# retained の constructor reference 8 ケースを各 2 回比較
taskpolicy -b nice -n 15 python3 scripts/witness.py retained --case retained-constructor-references/

# SUPER helper-order の ES2015/set 8 ケースを各 2 回比較
taskpolicy -b nice -n 15 python3 scripts/witness.py followup3 --case es2015/set/

# direct synthetic controls
taskpolicy -b nice -n 15 python3 scripts/witness.py direct --all
```

`--case` は繰り返して和集合を指定できます。`--dry-run` で件数・実コマンドを確認し、
全件 replay は明示的な `--all` に限ります。新しい CLI は `xtask` の先行ビルドを省き、
manifest path と exact test name で対象を起動します。

hosted witness の入口は `.github/workflows/witness.yml` です。
既存 acceptance は `.github/workflows/ci.yml` の early / wide / late に分割し、
31 slice の重複・欠落がないことを canonical full command と照合します。
SUPER fixture 単独の変更は該当 collection、docs のみは Rust replay なし、
共通実装・未知の入力・変更範囲が取れない場合は全体を選びます。
詳細は [focused witness と hosted replay](../../../witness-testing.md) を参照してください。

## 引き継ぎと統合

①と③は factory/printer、②と SUPER は standard_decorators、④は compiler/emitter に
重なり得ます。依頼ごとの base SHA と変更ファイルを明記し、統合担当が順に取り込みます。
個別に成功した件数を合算して合成候補の成功と扱いません。

成果物の最低構成は `DESIGN.md`、入力/期待値、focused before/after の実行記録と観測、
解析 script、base→candidate patch、再現手順、hosted replay 入口と結果または未実行状態、
未完了項目の一覧です。隔離候補の提出と本番 admission を区別します。

## 旧 v18 の履歴再現に限って使う情報

旧 evidence commit は `6e298cda8dbf362f61ce6a8e690ec145270681df`。
[SUPER 資料 §2](h2-8a-decorator-super-claude-handoff.md#2-正確な開始点と復元) の v18 復元と
[attempt65 receipt](../../../../ratchets/h2-8a-decorator-receiver-emitter-checks.v1.json) の
1014 source/input・109 vendor の照合は、旧実験の再現専用です。
旧基準は 530/530 complete commands × 2、emitter 494 library / 452 contracts でした。
1350 declaration reprints 等との重複を合算しません。

前回の snapshot tar と full62 raw captures は、2026-09-14 の確認では参照先にありませんでした。
共有 `target/h2-8a-retained-lexical-design-workspace` は編集しません。
既存 `run-comma-printer-design-experiment.py` は候補を上書きするため、新しい実装の実行に使いません。
