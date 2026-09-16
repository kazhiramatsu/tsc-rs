# Claude 実装依頼②：A41-BINDING — decorator の生成名と binding identity

**2026-09-16 次のClaude担当：C02。** C01は[PR #542](https://github.com/kazhiramatsu/tsc-rs/pull/542)で統合済み。
開始時にorigin/mainを取得し、merge `7df1a8ed138ec8060fdd8bfc33cb539d92b8302e` を含む実際の開始SHAを固定してください。
C01で追加したtyped literal updateと既存のbinding/finalizerを基準にし、旧候補patchを重ねて適用しません。
まず下記のprinter failure/reuseの `x_2` / `x_1` 差を再現し、関連する命名状態の寿命を調べます。
ローカルは新規・隣接ownerのfocused比較、全件hosted検証・PR・mergeは統合担当が行います。

作成日：2026-09-14。親：H2.8a / A6-41。状態：隔離候補の research。

**2026-09-15 具体的な追加入力**：[C03 統合レビュー](h2-8a-printer-failure/INTEGRATION.md)の
`printer-failure/printNode/unique-name/after/statement-1/recover-new-unique#op2` は、
失敗後に上流が `x_2`、Rust が `x_1` を出す既知差分です。上流は printer 内で lazy 生成し、
Rust は transformation 所有で eager 確定します。正常時の生成名を全面的に作り直す前に、
この failure/reuse の state 寿命と既存 finalizer の対応を監査してください。


**2026-09-15 更新**：開始点と検証分担は[共通手順](claude-high-difficulty-handoffs.md)の最新版に従います。
SUPER 統合後の main の SHA を固定し、ローカルは新規失敗・関連 owner の focused set、
重い全件 replay は hosted で実行します。以下の技術要件は現行実装と照合し、既実装部分を再実装しません。

## 依頼

standard decorator の既存 typed generated-binding / finalizer を基準に、
source identifier、global 名、入れ子 scope、synthetic 名の衝突規則の残経路を監査・修復してください。
computed-name cache の宣言・代入・decorator context・accessor 名が同じ binding を参照する
ことも対象です。source 由来の衝突 witness、設計、隔離 patch、完全観測を提出してください。

[共通手順](claude-high-difficulty-handoffs.md)の開始点で
`draft/h2-8a-generated-binding` / `../tsc-rs-generated-binding` を作り、source/input manifest を保存します。
SUPER のマージ後の実装を含め、旧 v18 や旧 SUPER candidate を重ねて適用しません。

## 現在の開始状態と残経路の確認

現在の `StandardDecoratorVisitor` は `ParsedSourceIdentifierNames::collect` で
FileLevel の parse census を作り、各 decorator/descriptor/initializer/computed key は
`TargetBinding` を持ちます。旧版が指摘した `computed_temp_bindings` の文字列検索は、
現在の実装にはありません。これらの移行そのものを新規の修復として再依頼しません。

SUPER 統合では discarded/required の共有ノード、computed object key の named evaluation、
helper request order、生成 default binding と runtime name の区別も修復しています。
その controls を基準に、global oracle の hit/miss/error、synthetic census、nested reservation、
再 print/別 source/clone/dispose の組合せについて要求と既存観測の対応表を作ってください。
追加対照の失敗数は未計測です。旧 530 件や SUPER の成功を全 naming 経路の閉包と扱いません。

対象は decorator の生成名 owner と、差を再現した共有 binding 境界です。
`super` lowering や全 transformer の名前生成の再実装、resolver 全体の刷新は含めません。
既存の正しい owner は維持し、修復は開始 SHA との差分として提出します。

## Source と Rust の入口

行番号は固定 `_tsc.js`。全体 hash は共通手順にあります。

| Source | 固定する規則 | Rust |
| --- | --- | --- |
| `isFileLevelUniqueName` 12907 | parse identifiers と global predicate、候補 suffix | `ParsedSourceIdentifierNames::collect`、`PreferredNameDomain::FileLevelOptimistic` |
| `createClassInfo` 99241 | static private/auto-accessor の有無による `_classThis` の FileLevel / ReservedInNestedScopes 選択 | `StandardDecoratorVisitor::transform_class_like`、class decoration plan |
| `visitReferencedPropertyName` 100345 付近、`transformDecorator` 100554 | computed cache と context/name の identity、生成タイミング | `prepare_decorators_and_computed_names`、`MemberPlan::computed_temp`、`create_binding_identifier` |
| `reserveNameInNestedScopes` 120503、`generateName` 120624、`generateNameCached` 120633 | printable spelling と generated identity の区別、scope lifetime | `TargetBinding`、[generated_bindings.rs](../../../../crates/emitter/src/builtins/generated_bindings.rs) |
| `isUniqueName` 120638、`makeUniqueName` 120741、`generateNameForNode` 120876 | optimistic/ordinary/private の衝突 domain、final print order | target-binding finalizer、[printer.rs](../../../../crates/emitter/src/printer.rs) の global-name adapter |

`createUniqueName`、generated identifier flags、getGeneratedNameForNode と呼出先の全分岐も
source 範囲/hash に固定します。FileLevel は異なる binding が同じ spelling を選ぶことが
あり得るため、「全生成名を一つの集合で一意にする」という設計を採用しないでください。

編集候補は `builtins/standard_decorators.rs`、`builtins/target_bindings.rs`、
`builtins/generated_bindings.rs`。printer / `transform.rs` / resolver の変更が必要なら、
到達する source query と consumer を示して設計メモへ追加し、差分を分けます。
一般 identifier の綴りだけを手掛かりに generated identity を割り当てないこと。

## 実装手順

1. decorator が生成する各種類の binding を一覧化する。
   `_classThis`、`_classSuper`、metadata、decorator/descriptor/initializer 配列、computed key、
   backing field ごとに source node、scope、flags、参照箇所、最終命名 owner を記録する。
2. parse census / synthetic collision / global oracle / nested reservation を別々に測る
   source witness と direct factory/printer control を作り、before を固定する。
3. `ParsedSourceIdentifierNames` と既存 finalizer の契約・全 producer の接続を検証する。
   global query の失敗は元の typed error を保ち、差を再現した owner のみ修復する。
4. computed key の既存の型付き cache plan について、宣言・代入・参照が同じ identity を
   運ぶことを検証する。通常 identifier への誤った identity 付与も対照にする。
5. 入れ子/sibling class の scope 復元、private/auto-accessor の reservation、名前割当順を検証する。
   暫定 spelling と final spelling のどちらを source range/name metadata に用いるかも確認する。
6. 最終 tree における命名、再 print、source を切り替えた print、後続 class-fields/module pass を
   完全観測で確認する。新規 binding の before/after identity trace を別保存する。

## 必須 witness

ID は `decorator-binding/<family>/<target>/<mode>/<variant>`。
標準軸は ES2015 / ES2022 / ESNext × set/define、module ESNext、sourceMap true。
module publication の合成対照は CommonJS も追加します。direct control は独立した ID 集合です。

| Family | 必須の対照 |
| --- | --- |
| parse-census | `_classThis` / `_metadata` / `_a` 等が参照・宣言に現れる場合、文字列/comment だけにある場合 |
| synthetic-census | earlier pass が同じ spelling を合成する場合と、同じ spelling が元 source にある場合 |
| global | 別 script file の global 宣言、external module の同名 local、明示 global oracle の hit/miss/error |
| nested | outer/inner decorated class、sibling class、通常関数 scope、同名 generated identity の合法な再利用 |
| reserved | static private field / static auto-accessor の有無、public static field だけの場合、private と public の名前 domain |
| computed | effectful key を宣言・代入・decorator context・accessor descriptor が共有。複数 key と同名 user identifier |
| ordering | decorator expression と heritage の双方で生成名が必要、pending computed expressions、constructor を含む member 順 |
| lifecycle | 再 print、別 source file、clone、dispose 後の node property と emit metadata、エラー時の scope 復元 |

出発例は、外側 script で `_metadata` を宣言し、別 file の decorated class が
`@dec static accessor [key()] = 1` を持つケースです。同じ宣言を external module に移した対照、
さらに earlier pass の synthetic identifier にした direct 対照を分けます。
期待する suffix を手書きせず upstream から採取してください。

computed key の `key()` は event log で評価回数を比較します。
spelling の一致だけで binding 同一性を証明せず、native ID の宣言→参照対応を保存します。
upstream/native の生 node ID を直接比較せず、それぞれの identity 関係を同じ schema に投影します。

## 検証と提出

新規 target 案：`crates/emitter/tests/decorator_binding_contract.rs`、
`crates/compiler/tests/decorator_binding_pipeline_contract.rs`。
新規 observer は `scripts/observe-decorator-bindings.mjs`。
既存 `system-generated-names.json`、target-binding unit tests は隣接対照として保持します。

既存 focused 入口（開始 SHA を固定した worktree、共通の低優先度/env を適用）：

```sh
cargo test --offline --manifest-path crates/emitter/Cargo.toml --lib target_bindings -- --test-threads=1
cargo test --offline --manifest-path crates/emitter/Cargo.toml --lib generated_bindings -- --test-threads=1
```

フィルタで実行された test 数を記録し、0 tests を成功扱いしません。
ローカルは新規完全観測の focused set × 2 と変更 finalizer の collision controls、
hosted は必要な全件 regression を担当します。旧 530 / 494 / 452 をローカル終了条件にしません。
新規 global control と通常 compiler control の件数は別記します。

完了条件は binding inventory 全行の disposition、source/name domain の一致、宣言・参照の
identity 一貫性、regression 0、再適用可能な patch と全観測の保存です。
既存 code が正しいと示された分岐は証拠を残して維持します。全 corpus の naming や
SUPER の完了を、この限定候補の結果から推論しないでください。
