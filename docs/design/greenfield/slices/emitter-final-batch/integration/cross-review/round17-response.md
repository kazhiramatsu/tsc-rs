## r17 判断

**結論: 局所的に安全に閉じる案はありません。r12 と同じ parser missing/skip facts 待ちです。** 12 行は negative boundary controls として凍結する案に同意します。

**1. source 上の根拠**
- 拒否述語は `ParseRecovery::is_literal_only` (crates/syntax/src/recovery.rs:67-77) で、scanner の文字列/テンプレート起源だけを admit します。今回の 3 診断はすべて `Parser` 起源です: 1005 は `parse_expected` (parser.rs:1099-1113)、1434 は missing-semicolon 報告 (parser.rs:8943-8948)、1128 は `abort_parsing_list_or_move_to_next_token` (parser.rs:1837-1844) で、これは `next_token()` による token skip を伴います。admit するには起源述語の無条件拡張が必要で、r12 で退けたクラスそのものです。
- TS の出力は missing/skip facts に直接依存しています。own-line 行の TS 出力は `/* value */` を 2 回出します(1 回目は欠落 `)` を `expression.end` から探す `emitTokenWithComment` の comment scan、2 回目は paren の end = `as` トークンの full start を pos とする次文 `as;` の leading comment)。さらに skip された `)` の後ろの `;` が EmptyStatement として `;` 行になります。line-comment 行では `// value` が variable statement の trailing comment として `); // value` に付きます。再現には ParenthesizedExpression の end 規則、欠落トークン時の comment scan、skip gap の所有、ASI 終端の 4 点が必要で、printer の comment owner 修正では届きません。
- Rust は診断 3 に対して recovery event 4 で、1 つは diagnostic index を持たない event(dedupe 抑止か SilentMissingNode)です。TS の recovery 木との形状一致も未検証なので、admit 側に倒す根拠がありません。

**2. 凍結の最小案(既存比較器流用)**
- `h2_8a_utf16_identity_recovery_controls.rs:209-254` の `compare_complete_cases` が最も近い。`typed_recovery_refusal` は `ParseDiagnosticsDeferred { owner_slice: "H2.9" }` を matches で判定し、`partial_writes` を Err 時に採取済みです。boundary 行にはこれを流用し、`typed_refusal == true`、`partial_writes == []`、typed error の `count == 3` / `recovery_events == 4` を厳密に assert する。
- retire assertion: 結果が `Ok` になった boundary 行は「境界が閉じた、ordinary 比較へ移せ」で fail させる(`emitter_final_universe.rs:196` の KNOWN stale 判定と同形)。typed でない `Err` は panic 扱いで新 panic を免除しない。
- strict native 比較は noEmit route で可能です。同じ入力を `ProgramSession::run()` にかけ、syntactic diagnostics を TS の `reported_diagnostics` (1005@94/2, 1434@97/6, 1128@103/1) と code/start/length で完全一致させる。emit route の typed 拒否だけでなく parser facts 自体を TS と突き合わせる比較になります。
- `emitter_final_rows.rs:351` の `emit_refused` 分岐は `UnsupportedCompilerOption` 専用で、拒否を差分として数え他の Err は panic するため、boundary 用には使わない。

**3. 新 valid 2 形状の TS 直接確認**
vendored tsc で `(object?.x as // value\n number)` と `(object?.x as\n /* value */ number)` を parse/transpile した結果、両方 parse diagnostics 0、出力は次の 1 行で同一でした。

```
export const value = object === null || object === void 0 ? void 0 : object.x;
```

型位置のコメントは `as` と共に消え、外側の括弧も PartiallyEmittedExpression 経由で落ちます。ordinary complete 比較の期待値はコメント無しになる点に注意(r3 の comment ownership ではなく型位置 erasure の対照になります)。

**4. 計上**
468 exact / 12 typed boundaries として別集合に置き、EF7 残 36 と足さない方針に同意します。

**5. r16 訂正の受領**
all-report contract 0 件という私の結論は誤りでした。走査対象を fixture JSON に限定し、`cli_contract` の inline contract を見ていません。direct TS 比較への置換と 58 CLI 全 exact で閉じているので追加対応は不要です。
