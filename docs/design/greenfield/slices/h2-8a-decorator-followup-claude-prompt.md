# Claudeへの次回依頼（貼り付け用）

標準decoratorの残存source経路を調査し、Rustとの不一致が再現した経路を修正してください。
準備worktreeは `/Users/hiramatsu/dev/tsc-rs-dec-followup`、ブランチは
`prep/h2-8a-decorator-followup` です。

最初に、このworktree内の次の2ファイルを読んでください。

- `docs/design/greenfield/slices/h2-8a-decorator-followup-handoff.md`
- `docs/design/greenfield/slices/h2-8a-decorator-followup-start.v1.json`

開始HEAD・production SHA・証跡の適用範囲はmanifestを基準にしてください。
[前作PR #512](https://github.com/kazhiramatsu/tsc-rs/pull/512)の最新head・base・
CI・merge状態も確認してください。マージ先には前提63コミットを含むかどうかの
論点があります。次作業の開始確認だけを根拠に、前作PRのマージ先を決めたり、
root productionへ反映したりしないでください。確定したmerge treeに更新する際は、
manifestとの差分を確認し、必要な検証範囲を記録してください。

前作の17不一致は修正済みです。修正前`f8e47ecf4`では新規126件と既存530件が
完全タプル一致×2、通常テストexit 0、full62比変更・欠落・追加0です。
追加で見つかったCIの4件も修正し、元の4コマンドとemitter 495＋452件は通過しました。
修正後`2953ecb8a`の最終126件・530件・hosted gatesは検証中です。
前作の残る検証はCodex側で継続中です。次の48件のbaseline採取は開始できます。
この準備worktreeを使い、前作worktreeのproductionや実行中の検証入力は変更しないでください。
開始後のHEAD載せ替えは、前作の結果と自分の変更を照合して判断してください。
新しい48件はTypeScriptの観測のみ準備済みで、Rustの合格を意味しません。

今回の対象は次の4つです。各仮説と隣接対照の2ソースを、
ES2015／ES2022／ESNext、set／defineで観測する計48 complete commandsを用意しました。

1. `@dec ["x"]`など、リテラルcomputed nameのcontext・access・temp。
2. object literalを含むcomputed nameで、先行decoratorのpending式を消費する位置。
3. decorated computed field内の匿名classで、同じbindingの二重hoistが必要な経路。
4. undecorated outer classのcomputed propertyにdecorated匿名classを置いた場合のtemp所有scope。

まず既存のcomplete-commandテスト方式に新規48件を登録し、変更前のRust観測を保存して
TypeScriptとの差分を分類してください。既に一致している仮説は、その理由を記録します。
失敗が再現していない段階で、推測によるproduction修正を始めないでください。
準備済み入力の到達性が不足する場合は、旧観測を保持して新しい版を採取してください。

不一致が再現したら、pinned TypeScriptのcallee・predicateまで読み、
source → Rust owner → witnessの対応、scope・訪問順・range/flags・生成名の所有権を
実装前に設計記録へまとめてください。別ownerを変更する必要があれば、その根拠と
境界を先に追記し、原因ごとにコミットを分けてください。

合格条件は、新規complete-commandテストの通常実行exit 0です。receiptへの状態固定、
expected failure、期待値の書換え、未実行条件の除外で合格にしないでください。
既存126件・530件の完全一致×2とfull62比較を維持し、変更ownerの関連テストを確認します。
最終計測後にproductionを変えた場合は、証跡のheadを付け替えず再検証範囲を更新してください。

CIは編集時のfocused検証と、hosted `gates`と同じ `cargo xtask acceptance` です。
`walk.sh`系、`scripts/chain-walk.sh`、古い証跡の一括再生成、`cargo xtask ci`を
通常のローカルCIとして実行しないでください。成功済みの無関係なsuiteも繰り返しません。
重い実行は一度に一つ、専用target、`taskpolicy -b nice -n 15`、`CARGO_BUILD_JOBS=2`。
acceptanceの環境変数一式は詳細handoffに記載しています。

`/Users/hiramatsu/dev/tsc-rs` と `/Users/hiramatsu/dev/tsc-rs-dec53` の既存状態を
保全してください。旧rootやattempt65へ戻して候補patchを二重適用しないでください。
過去候補exportのdefault endpoint `306930ab7` は変更しません。
新候補のpatchは開始・終点を固定し、別のファイル名で出力してください。

成果物は、原因別コミット、source対応表、baselineと最終結果、残る未判定行、
head・source/fixture SHA・実行バイナリSHA・実exit・ログ・captureを束ねた証跡です。
「準備48件が通る」と「標準decorator全経路／H2.8全体の完了」を区別して報告してください。
