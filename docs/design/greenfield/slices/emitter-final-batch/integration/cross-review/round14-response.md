## r14 回答

両行とも根本原因を Rust 側で特定できました。どちらも診断位置・文面の後処理ではなく、upstream との呼び出し構造の差です。

### 1. JSX `"smth"` が `string` になる原因

下層(`is_literal_of_contextual_type`、`get_type_of_property_of_contextual_type` の intersection 経路、`get_contextual_type` の JsxAttribute 分岐、`check_expression_for_mutable_location`、`get_context_node`、`find_contextual_node`)はすべて upstream と一致しています。checker 内に JSX attribute 特例の欠落はありません。

**真因は elaboration 入口での attributes 再チェックです。** `crates/checker/src/calls.rs:6281` の intrinsic 経路が `elaborate_literal_assignment(attributes, result, …)` を呼び、その中(`crates/checker/src/elaboration.rs:971`)で `check_expression_cached(attributes, NORMAL)` が走ります。この時点で `check_expression_with_contextual_type` の contextual スタックは pop 済みなので、`create_jsx_attributes_type_from_attributes_property` (jsx.rs:312) の `get_contextual_type(attributes)` はスタックに当たらず、親 JsxSelfClosingElement 経由で `get_contextual_type_for_argument_at_index` (contextual.rs:951-973) に落ち、`resolved_signature` が resolving 中なので `resolving_signature` の unknown fallback を返します。結果、`"smth"` の contextual 型が無く `check_expression_for_mutable_location` が `string` に widen し、attributes 型が `{ prop: string }` として再構築されます。calls.rs:6149 の `source_type` はその `string` なので、6167 の specific/fallback も `string` のまま。

upstream は `checkTypeAssignableToAndOptionallyElaborate(attrType, result, tagName, attributes)` (77404) → `elaborateError(expr, source, …)` (63957) に **計算済みの `attrType` を渡し、attributes を再チェックしません**。

**最小修正**(calls.rs:6281、fixture 特化なし):
- `elaborate_literal_assignment_from_types(attributes, attr_type, result, Some(&Type_0_is_not_assignable_to_type_1))` を呼ぶ(elaboration.rs:993 を `pub(crate)` にする)。既存の `initially_related` 判定はそのまま。

**同一クラスの副次サイト**(同時に直すことを推奨):
- calls.rs:3304 component 経路の `capture_literal_assignment_elaboration(attributes, param_type, …)` も elaboration.rs:1024 で再チェックしています。upstream (76103 相当) は `checkAttributesType` を渡すので、source 明示版の captured 顔を追加して `check_attributes_type` を渡す。
- calls.rs:6153 `elaborate_literal_assignment_into_sink(initializer, target_type, …)` も initializer を再チェック。upstream `elaborateElementwise` (64147) は `sourcePropType` を渡すので `elaborate_assignment_relation(initializer, source_type, target_type, …)` を直接呼ぶ(`pub(crate)` 化)。ネストした object literal 属性で同じ widen が起きる経路です。

副作用として、現状の再チェックは attributes ノードの `resolved_type` に文脈なし型をキャッシュしており(expr.rs:3500)、後続の `get_type_of_expression` 系消費者も widen 済み型を見ます。再チェック撤去でこれも解消します。

### 2. relationComplexityError の length 5 の 2859 が欠落する原因

tsc の生成順: `checkAssignmentOperator` → `isTypeRelatedTo`(非報告)で **初回 overflow** → `checkTypeRelatedTo` 64872-64888 が cache に `Failed|ComplexityOverflow` を書いた直後に **`error(errorNode || currentNode, Excessive_complexity…)`** を発行。`currentNode` は `checkExpression` が設定した BinaryExpression `x = y` なので length 5。続く `elaborateError` 内の非報告呼び出しは cache ヒットで無音、最後の報告呼び出し(errorNode = `x`)が cache の overflow bit を `reportError` で再生して length 1 を出します。

Rust は後者の再生(engine.rs:3357-3373)は持っていますが、**非報告版 `check_type_related_to` (engine.rs:792-811) が cache 書き込みのみで `error()` を省略しています**。これが欠落行です。報告版 `check_relation_with_error_output_at_worker` (engine.rs:940-1029) も fresh overflow 分岐を持ちません。

**最小修正**(engine.rs:811 の直後):
- overflow 時、`relation_count <= 0` なら `Excessive_complexity_comparing_types_0_and_1`、それ以外は `Excessive_stack_depth_…` を選び、`self.error_at_js(self.current_node, message, &[type_to_string_slice(source)?, type_to_string_slice(target)?])` を発行。`current_node` は expr.rs:104-119 / check.rs:1724-1742 で upstream と同じ save/restore 済み。
- 報告版にも同じ分岐を追加し、`error_node.or(self.current_node)` で発行、upstream 通り `error_info` チェーンは返さない(`else if (errorInfo)` が飛ばされるため)。この fixture では報告呼び出しは cache ヒットなので前者だけで閉じますが、fresh overflow が報告側で起きる行のために揃えるべきです。

注意点:
- dedupe は `push_error_diagnostic` (state.rs:2161) が Diagnostic 全体一致なので length 1 と 5 は両方残ります。
- 投機ロールバック不要。upstream の `error()` も投機中に巻き戻さず、overload 候補ごとの非報告 probe でも 2859 を出します(この動作が upstream と一致する前提)。
- 予算(`relation_count`)は不変。
- 出力順は tsc が start→length でソートする前提で、Rust 側 program ソートが length を第二キーに持つか確認してください。

コントロール: relationComplexityError の f2(2 行)と f1(0 行)。ローカルに baselines ディレクトリが無いため corpus の他 2859 行は列挙できませんでした。観測レコードから `code == 2859` を持つ case を抽出して adjacent set にしてください。

### 3. sourceMap admission

`unsupported_config_scope` の noEmit 側だけに `sourceMap` を加え、`H0_SUPPORTED_CONFIG_OPTIONS` と凍結 allowlist アサートを維持する案は、前回の推奨と一致します。CLI 6 ケース + 85 noemit コントロールでの検証で十分です。H1 全フラグの admission はしないという線引きも同意です。
