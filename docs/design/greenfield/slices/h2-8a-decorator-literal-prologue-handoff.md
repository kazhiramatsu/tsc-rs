# A6-41 次作業：literal computed name・lexical prologue

準備日：2026-09-11。これは着手資料です。新規120件の上流観測・Rust検証・修正完了を意味しません。
短い依頼文は[Claude用プロンプト](h2-8a-decorator-literal-prologue-claude-prompt.md)、
開始点は[manifest](h2-8a-decorator-literal-prologue-start.v1.json)、
提案入力は[120件の入力](h2-8a-decorator-literal-prologue-proposed-inputs.v1.json)です。

## 1. 開始点と並行作業

- 専用worktree：`/Users/hiramatsu/dev/tsc-rs-dec-literal-prologue`
- branch：`prep/h2-8a-decorator-literal-prologue`
- production基準：`5134bb0187f9f3f6ac2a0218f8d8e98859cf4fea`
- 基準tree：`1e632db73fbcd8e37a6e3c63b03daf57dfbdafdc`
- [PR #512](https://github.com/kazhiramatsu/tsc-rs/pull/512)は2026-09-11 10:00:07 UTCに
  `main`へマージ済み。merge commitは`720f2d4b9df77f82e472157b9768d6826164e0ee`。
  前提63コミットを含めるか・マージ先は何かという以前の未決事項は解消しています。
- [PR #513](https://github.com/kazhiramatsu/tsc-rs/pull/513)は上記基準headの
  [hosted gates](https://github.com/kazhiramatsu/tsc-rs/actions/runs/34587126551)を実行中。
  Codexが監視・マージを継続します。完了時点の状態はGitHubで確認します。
- この準備branchは上記headに資料だけを追加します。production、harness、fixture、
  observer、Cargo、workflowは変更しません。資料commitのHEADとproduction基準headを区別します。
- 元follow-up `prep/h2-8a-decorator-followup`の`68519a6a5`は保持しています。
  統合候補はそれに#512の記録・受領証だけを加えたtreeで、production・テスト・CI設定は
  成功済みhosted head `b7d388886`と同一です。

着手時にmanifestのSHAと作業treeを照合し、`gh pr view 513`で状態を確認します。
マージ済みなら`git fetch origin main`後に基準headとmerge commitを比較してください。
同じtreeならproduction再検証をしたことにはせず、同一性を記録します。
差分があれば原因・影響・再検証範囲を確認し、新規作業を自動reset/rebaseしません。
旧root `6e298cda8`、attempt65、旧candidate patchへ戻す必要はありません。

保全対象：root、dec-next、dec53、dec-followup、dec-mergeの既存worktree・branch・target・capture。
特にrootの未追跡`h2-8a-decorator-next-implementation-handoff.md`は古い開始点の資料であり、
今回の開始指示には使用しません。この準備資料が現状を反映しています。

## 2. 継承する成果と今回の境界

前作[follow-up報告](h2-8a-decorator-followup-report.md)・
[設計記録](h2-8a-decorator-followup.md)を読みます。

- 準備48件とsource-file直下18件は修正済み、完全タプル一致×2。
- 既存126件（60/42/24）と既存530件も一致×2。full62比変更0・欠落0・追加0。
- emitter lib 495 / contracts 452通過。受領証は実際の計測head `a4c089c7b`を保持。
- hosted `eedbe221d`と`b7d388886`はsuccess。#513の候補で新しいgateを実行中。
- H2.5hの22行削除は開始headでも同じ差分で、前作follow-upのdecorator修正に起因しません。
  残り28行とH2.5gの6/510 deferredは今回の対象外です。
- 先行17失敗、今回48+18件の修正、二重hoist、source-file environment、
  H2.5h/6a/6bの全差分報告を元に戻しません。

## 3. Source根拠と仮説

行番号は開始manifestの`vendor/typescript-6.0.3/lib/_tsc.js`に対応します。
全体SHA：`1c59e77a54b186ec43fa7f3e0d3c4bb15ca5eb5ba43e96b1d3a267139eddd3e3`。
入口だけでなくcallee/predicateと後続passを読み、実装前にsource→Rust→witnessを記録します。

| 順序 | 調べる経路 | 固定sourceの入口 | Rust owner・確認点 |
| --- | --- | --- | --- |
| A | literal computed method/getter/setter/auto-accessor | `partialTransformClassElement` 99831、`isPropertyNameLiteral` 15888 | `decorator_property_name` 5527付近、`collect_method_plan`、`partial_transform_method_plan`、`create_method_decorator_context`、`create_access_object`。kind別のhas/get/set、computed/name、static/instance、不要temp/helperの有無 |
| B | numeric・no-substitution-templateの表記所有者 | `createStringLiteralFromNode` 21535、`getLiteralTextOfNode` 120467 | `create_string_literal_from_property_literal` 3587付近、`LiteralNodeProperties`（metadata.rs）、printer.rsのStringLiteral分岐2734付近・template token 9060付近。source text、cooked text、quote、escape、range/mapを区別 |
| C | 関数bodyのstandard prologueとtemp挿入順 | `visitFunctionBody` 91277、`mergeLexicalEnvironment` 24889 | `visit_function_like_body` 5316付近→`merge_block_environment` 5841。現実装は`statements.insert(0, declaration)`。directiveがある通常sourceで実際に通るか確認 |
| D | hoisted varのCustomPrologueと後続pass | `hoistVariableDeclaration` 116104、`endLexicalEnvironment` 116163、`isHoistedVariableStatement` 14173、`mergeLexicalEnvironment` 24889 | `create_hoisted_declarations` 5738、`merge_source_file_environment` 5760付近、metadata.rsの`CUSTOM_PROLOGUE`、es2015.rs等の読者。付与の差と出力への影響を別に判定 |

Bのsource上の事実：`createStringLiteralFromNode`はsource種別を問わず`textSourceNode`を設定します。
printerはnumeric sourceをそのtextからquoteし、template source等はsource側のliteral出力へ委譲します。
Rustのdecorator helperは現時点でStringLiteral sourceにだけtext-sourceを設定しています。
ただし数値やtemplateを使ったcomplete commandのRust不一致は、今回まだ実測していません。
numericのraw hex表記がそのままcontext.nameへ出る、と決めつけません。

Cは今回の読解で追加した仮説です。source-fileのdirective付き18件は前作で通りましたが、
関数bodyのdirective付き経路とは別です。`merge_block_environment`のコメントだけを根拠に
「関数にはprologueがない」と扱わず、caller・生成node・通常sourceで到達性を確認します。
この仮説はCUSTOM_PROLOGUE未付与とは分けてbaseline・原因・修正を記録します。

Dはsource上のflag差が既知ですが、通常sourceの失敗witnessは未確定です。
まずsource-file／関数／decorator IIFE／property initializerの所有者と後続passの読者を対応付けます。
必要ならCommonJS/AMD/UMD/System、parameter初期化、hoisted function等を最小ケースで追加します。
通常compiler pipelineに出現しないsynthetic配置だけでcomplete-command修正を主張しません。
direct transformのflags/identity契約は、通常sourceの完全タプル検証と件数・主張を分けます。
共有flagの一括付与、scope cache全面変更、出力文字列の置換から始めません。

## 4. 提案入力と最初の作業

入力JSONの`cases`は既存observerの入力形式です。`proposed_groups`は登録候補を示します。
今回は**提案だけ**で、TSの型診断・例外・Rustの成否は未観測です。

| group候補 | source数 | target×field mode | complete command候補数 |
| --- | --- | --- | --- |
| `literal-member-kinds` | 8（method/getter/setter/auto-accessorと各identifier対照） | 3×2 | 48 |
| `literal-key-spelling` | 8（decimal/hex/separator、plain/escaped template、string対照、template method、補間template対照） | 3×2 | 48 |
| `lexical-prologue` | 4（関数/arrowのdirectiveあり・なし） | 3×2 | 24 |
| 合計 | 20 | | 120 |

A/Bの各sourceにはinstanceとstaticの両方を含めています。失敗したときはどちらに由来するかを
分類し、必要なら別sourceへ分けて追加観測します。Dの後続pass到達性はこの120件だけでは保証しません。
ESNext/defineでnativeのまま残る条件は隣接対照として数え、loweringの実行済みとは扱いません。

手順：

1. 提案JSONをグループ別に新しい入力fixtureへ分け、既存
   `scripts/observe-decorator-next-witnesses.mjs`に新しいgroupだけを登録します。
   既存5グループの入力・期待値を変更せず、新しい観測を各入力2回採取します。
2. TSの診断や例外で対象経路へ届かない入力は、その初版を保存して修正版へ進みます。
   不都合な診断を除外・正規化して観測を成功扱いしません。
3. `crates/compiler/tests/integration/h2_8a_decorator_next_witnesses.rs`の既存方式で
   新規グループを登録し、productionを変更せずRust baselineを保存します。
   fixture登録だけのcommitとproduction修正commitを分けます。
4. 完全一致・不一致・未到達を分類し、不一致が再現した原因だけを修正します。
   JS、map、d.ts、declaration map、診断、write callback、status、exit、emit result、
   失敗時のpartial writesを保ちます。expected failure化を合格の代わりにしません。

## 5. 検証と実行資源

専用targetとrun directoryを使い、ローカルの重い実行は一度に1つです。
ユーザーが指示した並行作業は、別環境のhosted CI待ち中にこのworktreeで進められます。

```sh
export CARGO_TARGET_DIR="$PWD/target/decorator-literal-prologue-acceptance"
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0
export RUSTC_WRAPPER= TSRS_H2_5G_WORKERS=2 TSRS_CONFORMANCE_WORKERS=2
```

新規groupの通常実行は、group登録後に`TSC_RS_DECORATOR_NEXT_CASE_SET`を指定して行います。
capture先は実行ごとに新規作成します。既存5グループを確認する際はgroup名を個別に指定するか、
`all`に新規groupも含まれることを明記し、既存192件と新規件数を分けて集計します。

```sh
taskpolicy -b nice -n 15 cargo test -p tsc-rs-compiler --test contracts \
  h2_8a_decorator_next_witnesses::decorator_next_witnesses_match_complete_typescript_observations \
  -- --exact --nocapture --test-threads=1
```

productionを変更した最終候補で既存192件（60+42+24+48+18）と既存530件の期待値を変えず
各2回一致させます。530件の選択は`TSC_RS_RETAINED_ACCESSOR_CASE_SET=all`、test名は
`h2_8a_retained_accessor_owners::retained_accessor_owners_match_complete_typescript_observations`。
不要な`TSC_RS_DECORATOR_NEXT_CASE_FILTER`等を残して母数を狭めないでください。

full62比較元と比較器は前作の
`/Users/hiramatsu/dev/tsc-rs-dec-followup/target/decorator-followup-runs/full62-reference/`および
`.../tools/compare-with-full62.py`を参照できます。使用前に
`ratchets/h2-8a-decorator-followup-final530-full62.v1.json`のSHAと照合し、書き換えません。
最終captureを比較し、旧バイナリの結果を新headへ付け替えません。

変更ownerに関係するemitter tests（既存の基準はlib 495 / contracts 452）、format、
必要なfocused checksを実行します。production無変更なら同一性を記録し、
成功済みの無関係なsuiteを繰り返しません。
最終受入は`taskpolicy -b nice -n 15 cargo xtask acceptance`です。
walk、chain-walk、歴史的証跡の一括再生成、`cargo xtask ci`は通常CIとして実行しません。
前作PR #513のrunを停止・再dispatchしたり、CI設定や既知差分manifestを便宜的に変更したりしません。

## 6. 成果物

- source→Rust owner→witness、到達性、修正判断を記した設計記録と原因別commit。
- production無変更baseline、新規最終結果、既存192/530とfull62比較、関連suiteの結果。
- 実行head・source/fixture/observer SHA・入力archive・実バイナリSHA・実exit・ログ・全capture。
  最終計測後の変更はその適用範囲を明記し、必要な再検証を行います。
- 開始`5134bb018`と最終production commitを固定した、今回専用名の候補patch。
  過去exporterの`306930ab7`や`0fda49509..a4c089c7b`は変更しません。
- 不一致なし、通常source未到達、未観測を分けた残存表。120件の一致を全source経路の完了へ拡張しません。

前作PR #513への反映とmainへのマージはCodexが担当します。今回のClaude候補は専用branchで
保持し、独立してレビューできる状態にしてください。
