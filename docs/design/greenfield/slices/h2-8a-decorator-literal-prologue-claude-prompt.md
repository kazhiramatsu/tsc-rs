# Claudeへの次回依頼：literal computed nameとlexical prologue

次のworktreeで、標準decoratorの残るliteral computed nameとlexical prologueの経路を
調査し、TypeScriptとの不一致が再現したものを修正してください。

```text
worktree: /Users/hiramatsu/dev/tsc-rs-dec-literal-prologue
branch: prep/h2-8a-decorator-literal-prologue
production開始点: 5134bb0187f9f3f6ac2a0218f8d8e98859cf4fea
```

最初に同worktreeの次の資料を読んでください。

- `docs/design/greenfield/slices/h2-8a-decorator-literal-prologue-handoff.md`
- `docs/design/greenfield/slices/h2-8a-decorator-literal-prologue-start.v1.json`
- `docs/design/greenfield/slices/h2-8a-decorator-literal-prologue-proposed-inputs.v1.json`

PR #512は前提63コミットを含めてmainへマージ済みです。PR #513は上記production開始点の
hosted acceptanceをCodex側で実行・監視中です。今回の資料はその統合候補から分岐しており、
Claudeはこの専用worktreeで開始できます。着手時にPR #513の最新状態を確認し、完了済みなら
merge treeと開始点の差分を記録してください。新しい修正を自動でリセットしたり、前作のCI中に
その候補・branchを書き換えたりしません。旧root、attempt65、候補patchの再適用は不要です。

優先順は次のとおりです。

1. decorated method/getter/setter/auto-accessorのliteral computed nameとidentifier対照。
2. numeric/no-substitution-templateキーのtextSourceNode、context.name、access、出力表記。
3. 関数・arrow bodyの先頭directiveとnamed-evaluation tempの挿入順。
4. hoisted varのCUSTOM_PROLOGUEについて、後続passの読者と通常sourceからの到達性を調査。

提案入力は20ソース×ES2015/ES2022/ESNext×set/define＝120件です。
**入力だけの提案で、TypeScript観測もRust baselineも未実行です。**
既存observerの方式でTypeScriptを各2回観測し、独立した新規グループとして登録してから
production無変更のRust baselineを保存してください。既に一致する経路は理由を記録し、
推測で修正しません。到達しない入力は旧版を保持して別版で補います。

sourceのcallee/predicate、Rust owner、witness、scope・range/flags・bindingの対応を
実装前に設計記録へまとめてください。templateの表記差をquote正規化で消したり、
CUSTOM_PROLOGUEを出力差・到達性の根拠なしに一括付与したりしません。
関数directiveの挿入順とCUSTOM_PROLOGUEの伝播は別原因として検証します。

新規完全タプル比較を通常実行exit 0にし、既存5グループ192件と既存530件を期待値不変で
各2回一致させ、full62との差分も確認してください。変更ownerの関連emitter testsを実行します。
productionを変更しなかった場合は、既存成功suiteを無条件に再実行せず、無変更の証拠を残します。

ローカルの重い実行は1つずつ、専用target、`taskpolicy -b nice -n 15`、`CARGO_BUILD_JOBS=2`。
前作のhosted CIとは別環境なので並行して着手できますが、共有worktree・target・captureは使いません。
最終CIは`cargo xtask acceptance`です。walk、chain-walk、過去証跡一括再生成、
`cargo xtask ci`へ戻しません。PR #513の監視とマージはCodexが継続します。

原因別コミット、baselineと最終受領証、source対応表、固定した開始・終点の候補patch、
残る未判定行を提出してください。既存exporterの固定endpointは変更しません。
修正候補はこのbranchに保持し、今回の並行作業からmainやPR #513へ直接反映しません。
標準decorator全経路・H2.8全体の完了は主張しません。
