# H2.8a G5c: JSDoc return annotation の所有関係と宣言再利用

2026-09-14。ユーザーが次の高難度スライスを Claude に依頼するための選定・調査・実装依頼。
状態は **source inventory complete / runtime design pending**。
この資料だけで runtime-ready、原因確定、native 修復成功を宣言しない。
Claude は固定 base の baseline、設計 gate、原因別実装、focused 検証、push、引き継ぎまで担当する。
PR と統合 hosted acceptance は Codex が担当する。

## 1. 開始点と選定理由

- worktree: `/Users/hiramatsu/dev/tsc-rs-jsdoc-return`
- branch: `work/h2-8a-jsdoc-return`
- runtime base: `7d6bc9848e97c26b0438da5ec73bd08c9a175c9a`
- 統合元: [PR #521](https://github.com/kazhiramatsu/tsc-rs/pull/521)。#516 と #520 の履歴、UTF-16 修正、レビュー修正、hosted 側の2修正を含む。
- base tree、元ケース、artifact 行、source hash、上流12関数の正確な span/hash は
  [selection](h2-8a-jsdoc-return-selection.v1.json) に固定した。
- 手渡し用の短い依頼文は [Claude prompt](h2-8a-jsdoc-return-claude-prompt.md)。

読解順は `docs/design/README.md` → 現行 `emitter-architecture.md` →
`post-h1-completion-slices.md` の現在状態 → 本資料 → 参照する原 source。
設計 gate では今回到達する宣言/型/追跡 callback の architecture 行を現行実装に照合し、
変更して再検証するもの・前提を維持するもの・別 owner を明示する。
古い `active-qualified` のラベルや G5c cause map の推測だけを根拠に閉包完了と扱わない。

本スライスは現在の H2.8a 原ケースに残る一つの原因候補を追う。H2.8a-close を前提にする
NC1/MOD1/transpile の activation へ先行しない。H2.5h の16残件を一括して別の巨大スライスにもしない。
難所は、parser が所有する JSDoc、semantic signature、generic の宣言 identity、
syntactic shortcut、declaration annotation reuse の優先順位を一致させること。
修正行数が多いとは限らない。必要な原因閉包だけを直す。

原ケースは次の1件。原 input/options/TypeScript tuple を変更しない。

`typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsFunctionsCjs.ts#default`

`module.exports.d = function d(a, b) ...` の外側には `@return {string}`、
`module.exports.e = function e(a, b) ...` には `@template T,U` と `@return {T & U}` がある。
いずれも body の唯一の return は `/** @type {*} */(null)`。
従来の完全比較では `.d.ts` の d/e が `any`、TS は `string` / `T & U`。
G4a のコメント位置は前スライスで修復済み。この既知結果を最新 base の新計測と取り違えない。
G5c の strict test は現在も意図的に callable で、期待失敗扱いの annotation は付いていない。

### 準備時の最新 native before

[before 受領証と完全 captures](h2-8a-jsdoc-return-before.v1.json) は、上記 base の統合 worktree で
2026-09-14 15:07 JST に実行した実測。strict test は **exit 101**、13.27秒、入力変更なし。
元 command の2回の actual は同一で、期待 observation も元 artifact の行と同一。
全 JSON 差分は2番目の write の callback/materialized UTF-8 bytes とその長さの4 fields だけ。
その `.d.ts` の内容差は d/e の `any` 対 `string` / `T & U` の2行だけ（899対904 bytes）。
JS write、path、順序、BOM、callback metadata、diagnostics、command result/exit は同一。
これは第一仮説の因果証明や修復成功ではなく、最新 base に残る症状の証明である。
raw log と input manifest は `target/jsdoc-return-preparation/before/` にもコピーした。
この receipt と開始 source が一致する場合、同一 baseline の重複再実行は必須ではない。
Claude の初手は原因を切り分ける trace と fresh controls の before に進める。
準備時の統合 native 検証とこの before 実行は終了済みで、ローカルの重い実行枠は解放した。

## 2. 現行 source で確認した事実と第一仮説

| 経路 | 実在する入口 | 確認済みの動作 / 次に記録する値 |
| --- | --- | --- |
| parser 所有の関連付け | `crates/binder/src/node_util.rs::visit_owned_jsdoc_tags`、`get_next_jsdoc_comment_location` | AST の attachment と親をたどる既存経路。source 文字列の再走査を追加しない。実際の FunctionExpression → BinaryExpression → ExpressionStatement の通過・停止を記録する |
| semantic JSDoc | `crates/checker/src/jsdoc.rs::get_jsdoc_tags`、`first_jsdoc_tag`、`get_jsdoc_return_type` | `get_jsdoc_tags` は所有するコメント列をたどり cache を持つ。戻り型 worker は `@return` を先に読み、次に callable な `@type` を読む |
| semantic return | `functions.rs::effective_return_type_node`、`contextual.rs::get_return_type_from_annotation`、`annotate.rs::get_return_type_of_signature` | annotation/body/instantiation/resolution の既存経路。d/e で semantic return が既に正しいかを先に測る。これらを未実装として作り直さない |
| declaration syntactic path | `syntactic_type_node_builder.rs::effective_return_type_node`、`get_jsdoc_return_type`、`direct_jsdoc_tags` | 現行の return worker は `direct_jsdoc_tags` を使う。同ファイルの通常 `get_jsdoc_type` は既に binder の所有関係を使う。この不一致は source 上の事実 |
| annotation / shortcut / fallback | 同ファイルの `create_return_from_signature`、`type_from_single_return_expression`、`infer_return_type_of_signature_signature` | annotation が見つかれば再利用、なければ single-return expression、最後に resolver。外側 annotation を見落とすと body の `*` が shortcut から採用される、という順序を trace で検証する |
| semantic からの宣言生成 | `node_builder/signatures.rs`、`node_builder/serialize.rs` | 元 signature と location、symbol、type parameter、annotation reuse の資格を保持。名前を文字列で再解決する補修にしない |

**仮説**：syntactic builder が assignment 側の JSDoc return annotation を見落とし、
semantic fallback の前に body の `*` から `any` を選ぶ。source はこの仮説を支持するが、
選定時点では Rust/TS の同一ノード trace による因果証明は未完了である。
semantic 側が既に正しければ、推論エンジン全体の変更を不要な前提にしない。

## 3. 上流の調査閉包

すべて `vendor/typescript-6.0.3/lib/_tsc.js`。正確な function body hash は selection にある。
同名の `serializeReturnTypeForSignature` は二つあり、semantic と syntactic の両方を区別する。

| 上流関数 / 行 | 追う branch |
| --- | --- |
| `getJSDocReturnTag` 11708–11710、`getJSDocReturnType` 11728–11744 | return tag、欠落 typeExpression、type tag の function/JSDocFunction/call-signature type |
| `getJSDocTagsWorker` 11745–11759 | cached / uncached、所有列の順序と重複 |
| `getJSDocCommentsAndTags` 15429–15450、`getNextJSDocCommentLocation` 15465–15474 | initializer、assignment、variable statement、親の停止条件、最終 JSDoc block の ownership |
| `getEffectiveReturnTypeNode` 16768–16770 | 直接 type、JS の JSDoc、JSDocSignature |
| semantic `serializeReturnTypeForSignature` 53524–53546 | signature の resolved return、type predicate、tracking と location |
| `getReturnTypeOfSignature` 59810–59841、`getReturnTypeFromAnnotation` 59842–59871 | annotation 優先、body fallback、generic instantiation、resolution stack、accessor/construct の既存対照 |
| syntactic `serializeReturnTypeForSignature` 133807–133829 | node kind ごとの dispatch |
| `createReturnFromSignature` 134397–134406、`typeFromSingleReturnExpression` 134407–134441 | annotation → syntactic body → semantic fallback、reportFallback 条件 |

ここから到達する `serializeTypeAnnotationOfDeclaration`、`canReuseTypeNodeAnnotation`、
既存型ノードの再利用、`@template` parameter の解決については Claude が caller/callee を追い、
実際に修正する branch の宣言・body span/hash を設計 gate に追加する。
selection の span hash は「AST getStart〜end の UTF-8」であり、既存 ledger の `tsc-hash` 算法と同一とは主張しない。
production の ledger はリポジトリの既存算法で更新する。

## 4. 実行順序

1. `git status --short`、HEAD、base ancestry と selection の source pins を確認する。
   `node scripts/check-h2-8a-jsdoc-return-selection.mjs` は準備時の一致検査であり runtime readiness gate ではない。
   後から main が進んでも着手中の worktree を reset/checkout しない。取り込みは測定後、明示した統合段階で行う。
2. 元 G5c の strict complete-command test の固定 base receipt を検証し、不一致や stale input があれば再実行する。2回の actual/expected と
   全差分、実 exit、source/binary hash を保存する。既知 exit 101 を成功に変換しない。
3. TS と Rust で、同じ d/e の host node、選ばれた JSDoc tag/type node、semantic signature return、
   syntactic return node の有無、shortcut/fallback の採用理由を記録する。
   trace を入れた場合、無改造の上流による完全観測との一致を確認し、instrumented output を oracle 代用にしない。
4. 下の対照群を source branch ごとに具体化し、独立名の observer/fixture/test を作る。
   fresh pinned TS Program で各2回の完全観測を固定する。Rust の before を実行して positive / target / adjacent failure を分ける。
5. production 編集前に `h2-8a-jsdoc-return-design.md` と readiness 記録を作る。
   upstream owner、Rust の型/関数/寿命、各変更手順、witness 対応、境界、未解決事項0を満たす。
   単にこの依頼の表を転載して gate 完了にしない。閉包を具体化できたら、既に依頼された範囲で実装へ進む。
6. 原因を失う最初の producer/consumer を修復する。新 helper が必要なら既存 binder の
   ownership traversal を再利用し、semantic と syntactic の重複実装の差を小さくする。
   borrowed SourceFile/NodeId と TransformSourceId/TransformNode の帰属を保存し、別 source の node を混ぜない。
7. 元 G5c と focused 全件を完全比較 ×2。下記回帰を実行し、原因別 commit と after 記録を残す。
   期待値、case selector、typed refusal、比較項目の削減によって通さない。
8. branch を push して、最終 HEAD、commit、run directory、全 command/exit、captures、未解決/別 owner を Codex に渡す。
   PR 作成・既存 hosted acceptance・必要な純削除の manifest 更新は統合担当へ渡す。

baseline の実コマンド（各 invocation の中で原ケースを2回実行する）：

```sh
mkdir -p target/jsdoc-return-runs/before/captures
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR="$PWD/target/jsdoc-return" \
  TSC_RS_H2_8A_FAILURE_DIR="$PWD/target/jsdoc-return-runs/before/captures" \
  taskpolicy -b nice -n 15 cargo test --offline -p tsc-rs-compiler \
  --test h2_8a_declaration_comment_ranges original_shared_g5c_complete_command \
  -- --exact --nocapture --test-threads=1
```

log は subprocess の stdout/stderr と returncode をそのまま保存する。
pipe の末尾コマンドや `|| true` の exit をテスト結果にしない。
before で既知差分が消えていた場合は修復を捏造せず、元 tuple の全一致を記録して選定を再確認する。

## 5. 必須対照

数合わせの全直積ではなく、到達する各 branch に positive と隣接 negative を割り当てる。
各具体的 case ID・元 source・options・期待 observation を設計 gate に固定する。

| 群 | 必須の違い | 観測するもの |
| --- | --- | --- |
| R1 原因の最小対照 | d/e の original、外側 `@return {string}` / `@returns {string}`、body `*`、annotation なし | annotation が body より優先するか。無 annotation の既存推論を保つ |
| R2 ownership | `module.exports.f` / `exports.f`、変数 initializer、関数宣言、arrow、括弧付き関数、prototype/ordinary property | どの attachment が選ばれるか。異なる owner を一括で「親」と扱わない |
| R3 順序と停止 | 外側/内側の競合、複数 block、型なし return tag、`@return` と callable `@type`、兄弟の無関係 JSDoc | upstream の選択順、fallback、隣の宣言から拾わないこと |
| R4 generic identity | `@template T,U` + `T & U`、shadowing、constraint/default、import/typedef を含む return、別 source の同名 T | 正しい宣言 identity、scope、可視性、TS の全診断。文字列置換による T/U の復元をしない |
| R5 戻り型 route | literal annotation、union/intersection、callable type literal、JSDoc function type、async/generator、single/multiple return | syntactic annotation reuse / body shortcut / semantic fallback を branch ごとに分離 |
| R6 優先・負対照 | TS の直接 return type と JSDoc、getter/setter、JSDoc construct、annotation と body の不一致、無効な annotation | 直接型と既存 accessor/construct 規則、診断内容/順序、diagnostics gate |
| R7 command | allowJs + checkJs on/off、declaration / emitDeclarationOnly、noEmitOnError、noEmit + declaration、declarationMap、removeComments | 全診断、全 artifact、出力順/bytes/BOM、callback data、結果/exit。既存支持外の組合せは別 owner として記録 |
| R8 UTF-16 と既修復 | return literal に孤立 surrogate / U+FFFD / pair、G4a/G4b の所有コメント、parameter tag と detached prefix | JsString/JsStr の identity、raw UTF-16 callback と materialized bytes の境界、既修復のコメントと型 |

独立名の新規成果物を使用する：
`scripts/observe-h2-8a-jsdoc-return.mjs`、
`crates/compiler/tests/fixtures/h2-8a-jsdoc-return.json`、
`crates/compiler/tests/h2_8a_jsdoc_return.rs`。
完全 command producer の参考は `scripts/observe-h2-8a-declaration-specifiers.mjs` と
`scripts/observe-utf16-review-fix-controls.mjs`、比較の参考は
`crates/compiler/tests/integration/h2_7c_declaration_blocking.rs`。
shared comparator の都合で観測項目を減らさない。scalar-only の既存 adapter に孤立 surrogate を渡す場合は、
lossless な独立観測を設計し、U+FFFD 置換や missing field を一致扱いにしない。

## 6. 編集境界と回帰

初期の production 範囲は `crates/binder/src/node_util.rs` と
`crates/checker/src/syntactic_type_node_builder.rs` の原因に関係する関数。
semantic 側の `jsdoc.rs` / `functions.rs` / `contextual.rs` / `annotate.rs`、
`node_builder/signatures.rs` / `serialize.rs` は先に読み取りと対照を行う。
変更が必要なら、実際の失敗と upstream branch を設計へ追記してから、その閉包だけを変更する。
定型的な閉包の補完にユーザーの再承認は不要。無関係な推論・binder・printer の全面再設計へ広げない。

syntax parser、emitter printer/factory、program path、compiler command scheduling、harness、xtask、
共有 comparator、`.github/`、global qualification/manifest、`crates/oracle/` は初期変更対象外。
必要性が証明された隣接修正は first failure と完全 tuple を残して別 owner として引き継ぐ。
generated `nodes.rs` を直接編集しない。名前/tag/source text に依存する production 分岐を追加しない。

最低限の回帰は以下。新しい懸念がなければ重複した全実行を増やさない。

- 元 G5c、および `h2_8a_declaration_comment_ranges` の全6 tests（41 controls と G4a/G4b を含む）。
  修復後は G5c を skip せず全 target が成功すること。
- binder の JSDoc ownership tests、checker syntactic builder / node builder tests と checker library。
  checker の semantic return と declaration text の片方だけで成功にしない。
- `h2_8a_declaration_specifiers`、`h2_8a_utf16_review_fix_controls`、
  `h2_8a_utf16_identity_recovery_controls`、今回到達する UTF-16 declaration literal 対照。
- emitter の既存 declaration reprint 対照は共有 type-node 出力が変わった範囲で実行。
- 最終 source の `cargo fmt --all -- --check`、変更 crate の all-targets check。
  共有型を変えた場合は workspace all-targets check。codegen/schema は該当変更時。

既知の main/pre-WTF8 由来の compiler contracts、h2_7e、emitter list_comment_flags、xtask 6c test の失敗は
[前修復 §33](h2-8a-utf16-adjacent-repair.md#33-implementation-review-fix-round-2026-09-14)
とレビュー回答を参照する。それらを期待値更新で消さない。
G5c は今回の必須修復なので、この「既知失敗」免除には含めない。

重い native 実行は同じ Mac で1本、専用 target、`taskpolicy -b nice -n 15`、
`CARGO_BUILD_JOBS=2`、`--test-threads=1`。oracle も1 processずつ。
着手時には前統合の実行が終わったことを確認する。full `cargo xtask ci`、walk/chain-walk、
全原769/クラス1228の再測定はこの focused スライスの代わりに実行しない。

## 7. 完了の主張

元 G5c の全 tuple が2回一致し、target controls が一致、before positives が保たれ、
新規回帰と未解決原因がなく、実装/型/関数/witness/commit/実 exit の対応を説明できれば本スライスを閉じる。
元769件の新しい exact 総数、H2.8a 全体、NC1 以降の activation はこの1件から推算しない。
hosted acceptance は H2.7d/e までで、G5c を含む H2.8a focused runner の代替にならない。

終了時は `h2-8a-jsdoc-return-report.md` に、before/after の全差分、最終 source/binary/input hash、
各 command と exit、正負対照、原因別 commit、継続が必要な別 owner、次の統合手順を残す。
新しい未完了事項を「ユーザー側に残る」へ丸投げせず、今回の原因閉包に必要な作業は完了まで進める。
