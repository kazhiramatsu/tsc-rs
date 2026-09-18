設計確認の結論: 方向は正しいが、**報告側での直接 `error_at` 発行は外す**べきです。理由と細部は以下。

**1. 報告側の直接発行は関連情報の二重化を生む**
`finish_relation_overflow` が報告側でも `error_at_js` を即発行し、同じ診断を `RelationErrorOutput` として返すと、消費側が関連情報を後付けする箇所で別物になります。`elaboration.rs:1359-1367`、`elaboration.rs:659-667`、`elaboration.rs:1183-1189`、`calls.rs:6216-6223` は返却された診断に `related` を push してから publish するため、program には「related 無しの直接発行分」と「related 付きの publish 分」が並び、`push_error_diagnostic` (state.rs:2161) の全一致 dedupe では落ちません。upstream は `diag2` が同一オブジェクトで、`addRelatedInfo(resultObj.errors[last], …)` が program 側にも反映されます。
→ 報告側は cache 書き込み + 出力返却 + early return のみにし、発行は既存の消費者(Program sink は 1 回 push、Captured sink は applicability 経由)に任せる。これで単一オブジェクト意味論に一致します。
→ container 責務については、Rust の報告側 relation は選択済み候補の報告時にしか走らず(overload probe は boolean 版で upstream の `isTypeRelatedTo` 同様 `currentNode` に直接発行)、Captured で捨てられて失う行は実質ありません。残差として記録すれば十分です。

**2. boolean 側は直接発行で正しい**
`check_type_related_to` (engine.rs:792-811) に `error_at_js(self.current_node, message, [type_to_string_slice(source)?, …])`。`current_node` は expr.rs:104-119 と check.rs:1724/4213 で save/restore 済み。upstream も投機中に巻き戻さないので `speculation_depth` ゲートは不要。

**3. 出力の形**
出力の `message` は overflow 1 行のチェーン、`error_node` は `error_node.or(current_node)`、`related` 空。code がチェーン先頭から採られることは既存の cache 再生経路(engine.rs:3363-3372、現在 length 1 の 2859 が正しく出ている)で実証済みなので、`check.rs:4557` / `elaboration.rs:215` の `create_error(head) → message 差し替え` 消費はそのまま使えます。`related` の戻り値は upstream の `result !== False` のまま(boolean は `is_true`、報告側は `!is_false`)。`report_incompatible_stack` は upstream でも overflow 分岐で結果が捨てられるので、early return で飛ばして差し支えありません。

**4. `used_containing_message_chain` フィールド**
妥当で最小です。通常分岐は engine.rs:1001-1019 で実際に inner を連結したとき true、overflow は false。`check_relation_with_shared_message_chain_at` の呼び出し元は `elaboration.rs:206` の 1 箇所だけで、calls.rs:3316 を含む shared-chain 消費はすべて `capture_type_assignable_to_diagnostic_with_containing_chain` 経由でタプル第 3 要素を受けているため、アダプタ 1 箇所の修正で全消費者に伝播します。追加点検は不要です。

**5. JSX 隣接 batch**
計画どおりで問題ありません。into_sink に `source_type` 引数追加(唯一の利用者)、component に `check_attributes_type`、intrinsic 側は `check_type_assignable_to_and_optionally_elaborate(attr_type, result, Some(tag_name), attributes, head)` 置換で upstream 77404 と同形です。ordinary command への quoted / expression / nested object × intrinsic / component × exactOptional の追加も適切です。
