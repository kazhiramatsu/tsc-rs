追加読み取りは不要です。到達性の主張を確認できました。

## 結論: 提案した Pending/LeadingOnly の穴は通常入力から到達不能。既存 semantics 維持に同意

根拠:

- **Pending 転送は 14654-14677 の 1 経路のみ**。条件は `carries_no_asi_left_edge` かつ deferred あり かつ `no_asi_left_edge_will_parenthesize`。no-ASI context を子に付ける親は `return`/`throw` (3225/3256) と `yield` (5106) の 3 箇所で、いずれも `emit_child_after_token_with_complete_source_comments` = **LeadingAndTrailing**。
- **LeadingOnly の生成元は parsed no-ASI paren の内側 (14873) だけ**で、そこは `for_wrapper` (751-754) が syntax を `NORMAL` にリセットするため `no_asi_left_edge` が落ち、内側の deferred は転送されず子ノード自身の worker で消費される。つまり LeadingOnly が PEE arm に Pending として届く経路は存在しない。
- したがって PEE arm が受け取る Pending は常に LeadingAndTrailing で、helper が無加工転送しても子 (parenthesized no-ASI node または次の PEE) が完全 phase を持つ。私の round4 の懸念は、この no-ASI 限定の到達性を見落としたものでした。

## 推奨 (surgery なし)

1. **PEE arm に到達性不変条件を明示**: 取り出した Pending の extent が `LeadingAndTrailing` であることを `debug_assert!` (または typed error) で固定。将来 LeadingOnly な no-ASI 親が増えたとき黙って comment を落とさず即座に赤にする、1 行の保険です。
2. **転送 Pending が PEE を跨ぐ形の対照を追加** (既存 7 形に未収載の組合せ):
   - `return (\n/* a */ obj?.x as number) /* c */ satisfies unknown;` (外側 PEE に trailing gap、内側は parsed no-ASI paren)
   - `throw` と `function* g() { yield (\n/* a */ obj?.x as number) /* c */ as unknown; }` の同形
   - `return (\n/* a */ obj?.x /* c */ as number)!;` (non-null 外側、comment は内側 gap)
   いずれも 3 targets × sourceMap on、JS/map/exit の完全一致で固定してください。

これで round4 の再 container 化提案は取り下げます。
