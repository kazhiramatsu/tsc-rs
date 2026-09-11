# 次回実装依頼：A6-41 source witness残存経路

準備日：2026-09-11。これは次作業の開始資料です。新規Rust実装の完了・
新規48条件のRust一致・PR #512のマージ完了を意味しません。

Claudeへ渡す短い依頼文は
[貼り付け用プロンプト](h2-8a-decorator-followup-claude-prompt.md)です。
この文書は、その詳細な開始条件・source根拠・証跡をまとめています。

## 開始点と現在のPR

- 準備worktree：`/Users/hiramatsu/dev/tsc-rs-dec-followup`
- 準備ブランチ：`prep/h2-8a-decorator-followup`
- 開始HEAD：`2953ecb8a8817c623a361a73c8db24780f7945ca`。
  準備ブランチをPRの最新候補へ載せ替え、受入のmodule経路計数修正と
  retained synthetic constructor・private helper receiverのコメント所有権修正、
  hoisted exportを越えるdetached comment状態の引き渡し修正、および
  object spread・automatic JSXの生成ノード所有権とstatic訪問順の4件修正を含めています。
  初回準備の`20be5c767`も開始manifestに記録しています。
- production 9ファイル、TypeScript、提案入力のSHAは
  [開始manifest](h2-8a-decorator-followup-start.v1.json)に固定しています。
- 前作：[設計・検証記録](h2-8a-decorator-next.md)。新規126件と既存530件は
  完全タプル一致×2、emitter 494 lib＋452 contracts通過。v2 receiptsは
  **整形前のソース**の証跡です。別HEADでの再計測として読み替えないでください。
- [PR #512](https://github.com/kazhiramatsu/tsc-rs/pull/512)は準備時点でdraftです。
  main比較には前提63コミットも含まれ、マージ先の指定は未確定です。
  着手時にはPRの最新head・base・merge状態を取得し、開始manifestとの
  production差分を確認してください。マージ済みなら確定したmerge treeを基準にします。
  旧root HEAD `6e298cda8`やattempt65 snapshotへ戻して候補を二重適用しません。
- ローカルproduction `/Users/hiramatsu/dev/tsc-rs` と既存の
  `/Users/hiramatsu/dev/tsc-rs-dec53`の未コミット変更を保全してください。

## CIの運用を取り違えないこと

ユーザーが選択しているのは、編集時の対象を絞った検証と、マージ前の
hosted `gates`と同じ **`cargo xtask acceptance`** です。
[現行スケジュール冒頭](../post-h1-completion-slices.md)にも、historical
certificate walkとfull developer CIを省略することが明記されています。
旧README・過去のslice本文に残る完全ゲート手順より、この現行運用を優先します。

`scripts/chain-walk.sh`、証跡の一括再生成、`cargo xtask ci`を通常CIとして
実行しないでください。古いqualification/ORDERチェックの不一致を直す目的で、
walkを繰り返す運用へ戻しません。実行する受入コマンドは次のものです。

```sh
CARGO_TARGET_DIR="$PWD/target/decorator-followup-acceptance" \
CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 \
RUSTC_WRAPPER= TSRS_H2_5G_WORKERS=2 TSRS_CONFORMANCE_WORKERS=2 \
taskpolicy -b nice -n 15 cargo xtask acceptance
```

PR受入中に見つかった問題と現在の状態：

- `commentsAfterSpread.ts`は旧refusal行を元の完全観測との一致へ昇格。
  H2.1a全295行のfocused acceptanceがexit 0。
- AMD/UMD/Noneの直接module経路でH2.1aのimplied-format計数を要求していた
  受入を修正。H2.1c全8行、H2.2d全9行と、CommonJS/AMD/UMD/NodeNext/Noneの
  5つの完全観測によるfocused検証がexit 0。
- `autoAccessor9.ts`でclassコメントを生成constructorにも重複出力していた
  retained class-fields経路を修正。ES2015/ES2022/ESNextのコメント対照と
  H2.4b全44行（42 exact / 2 deferred）のfocused検証がexit 0。
- private helper receiverのコメント範囲をTypeScriptと同じEndOnlyに修正。
  先行文のコメントをhelper引数でも出す問題を解消し、既存private契約23件と
  H2.5b全72行（68 exact / 4 deferred）のfocused検証がexit 0。
- `NoComments`のhoisted exportがdetached commentの消費状態を先取りする
  printer経路を修正。CommonJS/AMD/UMDの12条件、既存detached契約8件、
  `exportDefaultDuplicateCrash.ts`を含む3 complete commands×2がexit 0。
  H2.5gは実行済みの全失敗をまとめて表示します。判定条件と実行範囲は同一です。
- 修正前候補`f8e47ecf4`の既存126件は完全一致×2、通常テストexit 0。
  [最終receipt](../../../../ratchets/h2-8a-decorator-next-pr-witnesses.v3.json)を
  この準備ブランチにも保存しました。252 capture、バイナリ、入力archive、ログは
  前作worktreeの`target/dec-next-runs/pr-final-witnesses-r3/`です。
  source・fixtureは実行前後で不変です。これは次作業の新規48件のRust計測ではありません。
  `399360efc`のv1、`8aa39f277`のv2 receiptも元の内容で保持しています。
- 同じ修正前head・バイナリの既存530件も完全一致×2、通常テストexit 0。
  [530件receipt](../../../../ratchets/h2-8a-decorator-next-pr-full530.v1.json)と
  [full62比較receipt](../../../../ratchets/h2-8a-decorator-next-pr-full530-full62.v1.json)を
  保存しました。full62比は変更0・欠落0・追加0。全capture・バイナリ・入力archive・
  ログ・比較元の1,060 captureのarchiveは、前作worktreeの
  `target/dec-next-runs/pr-final-full530-r2/`です。
- 修正前headの[hosted gates](https://github.com/kazhiramatsu/tsc-rs/actions/runs/34561303900)は
  H2.5fまで通過し、H2.5gの全command実行後に4件不一致で失敗しました。
  対象はobject spreadの改行（3946）、static initializerのhelper順（5039）、
  automatic JSXの改行（7157/7158）です。
- `2953ecb8a`では上記4件を修正し、元のcomplete commandsが2回ずつ一致、
  通常テストexit 0です。emitterの495 lib＋452 contractsも通過しています。
  H2.5b、最終126件・530件と[新しいhosted gates](https://github.com/kazhiramatsu/tsc-rs/actions/runs/34565157958)は検証中です。
  全体CI成功やマージ完了として読み替えないでください。

原因・source根拠・個別ログSHAは[前作のPR admission記録](h2-8a-decorator-next.md)に
あります。初回停止を含むログは前作worktreeの`target/dec-next-runs/pr-*`です。
着手時にPRの最新gate結果とmerge treeを確認してください。

## 引き継ぎ時点の並行作業

ユーザーは4件のfocused検証通過後にClaudeへ次作業を依頼します。
この準備worktreeは`2953ecb8a`を開始点として固定し、次の48件のRust baselineを
採取できます。前作worktreeではCodexが残る126件・530件・hosted gatesの検証を
継続します。前作worktreeのproductionや入力を編集せず、次作業はこのworktreeで
進めてください。引き継ぎ後にCodexがこの準備ブランチを自動rebaseすることはありません。
前作の最終結果とmerge treeはPRおよび追加の完了報告で確認してください。

## 依頼と順序

前作で列挙した、まだRustの失敗witnessがない4つの疑いを調べてください。
以下の入力を開始候補として、pinned TypeScriptの出力とRustを比較してから
修正要否を決めます。仮説が再現しなければ、現在の経路で一致する理由を記録し、
既に閉じた不具合として修正を追加しません。

| 順序 | 仮説と準備済みsource名 | 主なowner・入口 | 確認する差分 |
| --- | --- | --- | --- |
| 1 | `literal-computed-decorated-field`／`identifier-decorated-field-control` | TS `partialTransformClassElement`、`visitComputedPropertyName`；Rust `transform_class_member`、`visit_referenced_property_name`、`previsit_property_name` | `@dec ["x"]`のcontextのcomputed/name/access、不要な`__propKey`・tempの有無。単なる文字列一致でbindingを付与しない |
| 2 | `object-computed-pending-absorption`／`class-computed-pending-control` | TS `visitComputedPropertyName`（100369付近）、`injectPendingExpressionsCommon`（100511付近）；Rust汎用visitorと`inject_pending_expressions` | 先行member decoratorのpendingが内側object literalのcomputed nameで吸収されるか、外側class computed nameまで残るか。評価順序・回数とmapを区別して確認 |
| 3 | `anonymous-in-decorated-computed-field`／`named-in-decorated-computed-field-control` | TS named evaluation（93817、93916付近）と`hoistVariableDeclaration`；Rust `prepare_property_named_evaluation`、`hoist_temp_variable`、computed cacheのhandoff | 同じgenerated bindingの二重hoistがTSに存在するか。`var _b, _b`を推測で消したり、別bindingを同じ綴りにしたりしない |
| 4 | `undecorated-outer-property-hoist`／`decorated-outer-property-hoist-control` | TS `transformNamedEvaluationOfPropertyDeclaration`とlexical environment；Rust `prepare_property_named_evaluation`、`visit_property_initializer`、`class_fields.rs`/`downlevel.rs`のcache引継ぎ | undecorated outer classのcomputed property＋decorated匿名class initializerで、tempを所有する関数scope・宣言順・参照identityが一致するか |

各2ソース×ES2015/ES2022/ESNext×set/define＝**48 complete commands**です。
ESNext/defineでdecoratorsがnativeのまま残る条件を、lowering成功と混同しません。
各仮説の到達性が不足していれば最小入力を改訂し、改訂前の観測を保持して
新しい版へ採取してください。最初の入力だけでsource全分岐の網羅を主張しません。

## 準備したファイル

- 入力：`crates/compiler/tests/fixtures/decorator-source-followup-inputs.json`。
  4仮説＋4隣接対照を独立したcase IDにしています。
- 観測器：既存の`scripts/observe-decorator-next-witnesses.mjs`に
  `source-followup`（48件）だけを追加しています。既存3グループの入力・処理は変更していません。observerファイルのSHAは
  変わるため、旧3グループのreceiptに記録されたobserver SHAを新しいSHAへ
  付け替えません。旧観測の再現には元のコミットの観測器を使います。
- 観測の生成：下記コマンド。出力が既にある場合は上書きせず、照合には`--check`を使います。

```sh
taskpolicy -b nice -n 15 node scripts/observe-decorator-next-witnesses.mjs source-followup --write
taskpolicy -b nice -n 15 node scripts/observe-decorator-next-witnesses.mjs source-followup --check
```

観測器は各入力を新しいTypeScript Programで2回実行し、完全タプルの同一性を
検査します。準備済み観測は48/48件、96 Program実行、診断0・例外0・exit 0です。
観測JSONと入力・compiler・observer・ログのSHAは開始manifestに固定しました。
採取ログは`target/decorator-followup-prep/oracle-r2/`です。Rust側の新規テスト登録・
48件のbaseline実行・実装修正は次作業です。

初版ではclass fieldの`[key()]`にTS1166が出たため、該当4ソースを
`const fieldKey = "x";`と`[fieldKey]`へ変更しました。非リテラル構文のcomputed
name経路を保ち、初版入力・観測・ログは`oracle-r1/`に保存しています。

準備観測から確認できた上流の事実：

- 匿名classをdecorated computed fieldへ入れたES2015/setとES2022/defineでは
  `var _a, _a;`が実在します。named class対照は`var _a;`です。
- literal computed fieldのES2022/defineではdecorator contextのnameは`"x"`、
  accessは`obj["x"]`です。identifier対照との区別を維持します。
- object literalを内包するcomputed methodのES2015/setとES2022/defineでは、
  pendingは`[(_first_decorators = [dec], ({ [key()]: "x" }).x)]`の外側で消費されます。
  この入力で「内側object literalへ吸収される」とは主張しません。
- undecorated outerのES2015/setのtemp宣言は`make`関数内、decorated outer対照は
  decorator IIFE内です。宣言場所とbinding参照の両方をRustで比較します。

これらはTypeScript側の到達性確認です。Rustの不一致や修正完了を意味しません。

## 実装前に読む範囲

1. [前作のReview corrections](h2-8a-decorator-next.md)：元の17失敗は解消済み。
   cross-file global-name処理やstatic-private set経路を未実装へ戻しません。
2. [現行emitter architecture](../emitter-architecture.md)と
   [スケジュール§1.1](../post-h1-completion-slices.md#11-mandatory-implementation-ready-design-gate)。
3. `vendor/typescript-6.0.3/lib/_tsc.js`：全体SHAは開始manifestに記録済み。
   `transformESDecorators`、named evaluationと各callee/predicateをたどり、
   owner・scope・訪問順・range/flags・生成名ポリシーを対応表にしてください。
4. `standard_decorators.rs`のreceiver frames、memo audit、typed helper plans、
   `target_bindings.rs`のprint finalization。`class_this_binding`は今では
   generated binding identityの読者です。古い「文字列しか読まない」前提を使いません。

中心は`standard_decorators.rs`です。別ownerを変更する必要が分かった場合は、
先にsource根拠と境界を設計記録へ追加し、原因ごとにコミットを分けます。
広域のcache変更やprinterでの出力書換えから始めず、最小再現が通る正しいownerを直します。

## 受入条件と成果物

- 48件のRust baselineとTypeScriptとの差分を保存し、修正対象・既に一致・
  未到達の仮説を区別する。未作成／未計測を成功数へ含めない。
- 新規のcomplete-commandテストを通常実行でexit 0にする。receiptだけの状態固定や
  expected failure化を合格の代わりにしない。
- 既存126件・既存530件の期待値を変えず完全一致×2を維持する。full62との比較も
  現在のcaptureを対象に行い、既存バイナリの結果を新候補へ付け替えない。
- 変更したownerに関係するemitter testsを実行する。無関係な成功済みsuite、
  walk、完全ローカルCIを繰り返さない。最終受入は上記の`cargo xtask acceptance`。
- 実行前manifest、sourceとfixture SHA、実行バイナリとSHA、実exit、ログ、
  各captureを束ねる。最終計測後にproductionを変更したら結果の適用範囲を更新する。
- per-cause patchの開始・終点を両方固定する。既存exportのdefault endpoint
  `306930ab7`は過去候補再現用なので変更せず、新候補は別ファイル名へ出す。
- source→Rust→witness対応表、残る未判定行、実測結果をまとめる。
  「48件が通る」と「A6-41全ソース経路／H2.8全体が完了」は別の主張です。

重い実行は一度に一つ、専用target、`taskpolicy -b nice -n 15`、
`CARGO_BUILD_JOBS=2`。他worktreeの実行・変更を巻き込まず、既存run名や証跡を再利用しません。
