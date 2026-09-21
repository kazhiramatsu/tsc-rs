レビュー完了です。結論から。

## 結論: Codex 仮説 (boundary 関数の map 未記録) は原因ではない

**1. まず「JS bytes 一致」は未確認です。** 失敗はすべて `h2_7b_w4a_controls.rs:402` の source-map assertion で panic しており、callback bytes の assertion (同ファイル 465 行以降) には到達していません。ログは bytes 一致を証明していません。

**2. mapping 復号が示す事実。** `x` の直後の生成列を c、source の `x` 直後を X+1 とすると:

| 位置 | TS | Rust |
| --- | --- | --- |
| c | X+1 → X+23 → X+24 (上書き) | X+24 |
| c+1 | X+2 (comment 開始) | X+25 (statement 終端) |
| c+12 | X+13 → X+24 (上書き) | なし |
| c+13 | X+25 | なし |

Rust は statement 終端 map を列 c+1 で記録しています。writer は comment も `append_and_measure` で列を進める (writer.rs:240-300) ので、列 c+1 で終端に達したなら **comment と `;` のうち comment 本文が書かれていない** のが最も整合的な説明です。Codex は該当 6 行の JS を直接 dump して確認してください。

**3. upstream で誰がこの comment を書くか。** `emitPartiallyEmittedExpression` の `emitLeadingCommentsOfPosition(expression.end)` (119777) ではありません。`iterateCommentRanges` は leading 収集 (trailing=false) で `collecting = trailing` から始まり (8497)、改行を見るまで同一行 comment を採らない (8515-8520)。Rust の `collect_source_comment_ranges` も同じ (18526)。書くのは **子ノード自身の trailing phase** で、`emitTrailingCommentsOfNode` (121033-121046) → `forEachTrailingCommentToEmit` (121234-121238) が `containerEnd` (内側 PEE の end X+23) ≠ 子 end X+1 なので発火し、`emitTrailingComment` (121179-121190) が `emitPos` 対で map を付けます。

**4. Rust 側で落ちる場所。** PEE arm (printer.rs:6865-6895) は親の deferred 状態を `emit_node_id_with_forwarded_source_comments` (14299) で子へそのまま転送します。子の trailing lane `emit_deferred_expression_trailing_comments` (15587-15646) は次のどちらかで書きません。
- 15594: 転送された extent が `LeadingOnly` で `owns_trailing()` 偽 → `None`。
- 15614-15621: 転送 container の scope が `retains_end(X+1)` (comment_cursor.rs:209-211) → `RetainedByParent`。
どちらでも PEE の after-side boundary は同一行を除外するため、誰も書かない。`write_source_comment` (19117-19136) 自体は両端の map を記録するので、経路に乗りさえすれば map は upstream と同じになります。

## 最小修正

PEE を upstream どおり **自前の comment container** にする。TS では PEE も `pipelineEmitWithComments` を通り、`emitLeadingCommentsOfNode` (121007-121029) が PEE の comment range を containerPos/End に claim してから子を emit します。Rust では PEE arm で、SourceRanged paren 経路 (14582-14608) と同じ形で `DeferredExpressionSourceComments::nested(<PEE range を claim した scope>, LeadingAndTrailing)` を作り、それを子へ渡す。子の lane は X+1 を PEE end X+23 と比較して `VisitedHere` になり、既存の `emit_same_line_trailing_comments` → `write_source_comment` が map 付きで書きます。得られた outcome は現状どおり `record_outcome` (14325) で外側へ返し、後続 token cursor が同じ範囲を再 claim しないようにする。

**触らないもの**: `emit_partially_emitted_boundary_comments` の after-side。改行後の comment は upstream もこの leading 経路が書くので、今の除外規則が正しい。

**重複 map/所有の危険**: 子 lane は「同一行 trailing」のみ、boundary after-side は「改行後 leading」のみで、upstream の trailing/leading 分割 (8497/8515) と同じく互いに素。外側の親の trailing phase は自身の end (X+24) で動くので X+1 を再訪しない。map 順序 (子 end → comment 対 → PEE end 上書き) は子 pipeline 内で書く限り自動で upstream と一致します。

## 必要な対照

- 6 行本体 + `satisfies` 形。
- 同一行 2 連 `x /* a */ /* b */ as T`、行コメント `x // c\n as T` (trailing に改行あり)、改行後 `x\n/* c */ as T` (boundary leading 経路の単独所有を維持)。
- 3 targets × sourceMap on。optional-angle と private-in 群が不変であること。
- harness 改善: bytes assertion を map より先に置くか全 assertion を収集する。今回のように map で止まると bytes 差が隠れます。
