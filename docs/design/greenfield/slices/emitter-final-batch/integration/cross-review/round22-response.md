## r22: parser facts の設計照合

### 所有権に関する source 事実

- `push_parse_diagnostic_with_index` (parser.rs:9895-9928) は dedupe で診断を落とした場合も **event は必ず 1 件 push** します。したがって `create_missing_node` (1229-1262) の message 経路では直前 event が常に fresh ですが、これを暗黙にせず index を返す形にすべきです(後述)。
- JSDoc は comment 全体の parse 後に `parse_recovery.restore(checkpoint)` します (jsdoc.rs:1955-1985)。JSDoc 由来の event/origin は record に残らず、診断だけが JS ファイルで `js_doc_diagnostics` へ移ります。append-only + count checkpoint の記録なら同じ restore で消えます。ただし JSDoc の `create_missing_node` (jsdoc.rs:200/716/944) が作った **arena ノードは残る**ので、census を「pos==end の Identifier を arena で探す」方式にすると JSDoc の欠落ノードを拾います。census は event 起点にするか `NodeFlags::JS_DOC` を除外する必要があります。
- 投機は `try_parse`/`look_ahead` (1319-1360) と JSDoc 側 (jsdoc.rs:655-663, 976-983, 1144-1146) がすべて count truncate のみ。checkpoint 以降に *作成時* に書く field は truncate で戻りますが、既存 event の後付け変更は戻りません。
- reparse: `reparse_top_level_await` は `take` した後に再 parse を live に走らせるので、再 parse 中に生まれた event は「take 以降に push された event」そのものです。`retain_reparse_recovery` (9498-9533) が扱うのは非再 parse 範囲の保持だけです。REPARSED を retain 側で付けるのは逆になる、という指摘は正しい。
- skip 生産者の完全列挙(前回の 6 箇所は不完全でした):
  1. `abort_parsing_list_or_move_to_next_token` 1842(event あり、dedupe で index None になり得る)
  2. `parse_list` の無進捗 guard(要素 push 後の `next_token`、**event なし**)
  3. `parse_delimited_list` の semicolon-as-delimiter 1632-1636 と無進捗 guard 1639-1641(直前に `parse_expected(Comma)` の event)
  4. block 直後の `=` 3640-3644
  5. parameter の modifier 単独 6977(**event なし**、直前の missing-name event は別呼び出し)
  6. type predicate/return の `=>` 7035-7039
  7. type annotation 内の `(` 8980-8984
  8. `reparse_top_level_await` の無進捗 guard(**event なし**)

  event を持たない skip が 3 種あるため、「event へ flag」だけでは表現できません。

### A/B/C の比較と推奨

- **C(flags 維持)**: 上の 3 種の silent skip に owner event がなく不可。却下。
- **B(全部 action)**: missing を action に出すと `SilentMissingNode` と二重になり、messaged missing と event の対応を位置で結ぶことになる。dedupe(同 start, index None)で曖昧化する、という懸念どおり。却下。
- **A(推奨)**: missing だけは *作成時* に fresh event へ書き、skip/reparse は append-only action。event 数・origin 数・`is_literal_only` は不変。

具体形:

```rust
// recovery.rs
pub struct ParseRecoveryEvent {
    /* 既存 5 field */
    pub missing_node: Option<SyntaxKind>,   // create_missing_node の message 経路だけが作成時に設定
}
pub enum ParseRecoveryAction {
    TokenSkipped { token: SyntaxKind, start: u32, length: u32, statement_start: u32, site: SkipSite },
    Reparsed { start: u32, end: u32 },      // 実再 parse run ごと
}
pub enum SkipSite { ListAbort, ListNoProgress, DelimitedSemicolon, DelimitedNoProgress,
                    BlockTrailingEquals, ParameterModifier, TypePredicateArrow,
                    TypeAnnotationCall, TopLevelAwaitReparse }
pub struct ParseRecovery { diagnostic_origins, events, actions: Vec<ParseRecoveryAction> }
struct RecoveryCheckpoint { diagnostic_count, event_count, action_count }
```

producer 側:
- `push_parse_diagnostic_with_index` を `(Option<usize>, usize /*event index*/)` を返す形にし、`parse_error_at_position` に index 返却版を追加。`create_missing_node` はその index にだけ `missing_node = Some(kind)` を書く(投機内なら index ≥ checkpoint なので truncate で消える)。silent 経路は既存の `SilentMissingNode` のまま。
- `fn record_token_skip(&mut self, site: SkipSite)` を各 skip 箇所で **`next_token()` の直前**に呼び、現在 token の kind/start/len と `recovery_statement_start.unwrap_or(start)` を積む。
- `reparse_top_level_await` は各 run の while 終了後に `Reparsed { start: run 先頭の pos, end: 最後の statement end }` を push。再 parse 中の event/action は live push なので範囲照合で分類でき、後付け tag は不要。`retain_reparse_recovery` は保持範囲の action も `statement_start` で複写。
- 位置は既存 event と同じ UTF-16。NodeId は持たない。

consumer:
- `is_literal_only` 不変(admission 不変)。emitter preflight (builtins.rs:16269-16275) と census の count 不変。
- incremental: `current_node` の再利用拒否 (parser.rs:1372-1390) に action の `start` 交差も加える。これを欠くと `incremental.rs:313` の record 等値(derive に actions が入る)で fresh≠incremental になります。`tests/unit/incremental/tests.rs:191` に skip を含む入力を 1 件追加。
- `discard_parse_recovery_for_harness` は `Default` で actions も空になる。
- JSDoc: 上記のとおり comment 単位 restore で action も消える(checkpoint に action_count が入るため)。

census 面: `ParseRecovery::classify(&self) -> RecoveryClasses { messaged_missing: Vec<(SyntaxKind, u32)>, silent_missing, skips, reparsed_ranges, report_only }` を追加し、report_only = `missing_node == None` かつ自 statement 範囲に action がない Diagnostic event。全 corpus census はこれを出力し、TS 側 `recovery-typescript-ast-r20.json` と位置/kind で照合します。

### 24 行向けの狭い第一段

`create_missing_node` は親を知らないので、親事実は preflight 側の arena 事実として読むのが自然です: `missing_node == Some(Identifier)` の event について、`start` に対応する byte 位置で `pos == end` の Identifier ノードを arena から引き、`JS_DOC` flag を除外し、`parent` の kind を取る。述語:

`emit_safe_recovery(arena)` = 全 event が literal、または (`Diagnostic(Parser)` ∧ `diagnostic_index.is_some()` ∧ `missing_node == Some(Identifier)` ∧ parent kind == `AwaitExpression`) ∧ `actions.is_empty()`

24 行は `a = await )` と `[await ]` の両形とも AwaitExpression の operand なのでこれで揃い、`let = 1`(親 VariableDeclaration)や `a.`(親 PropertyAccessExpression)は除外されます。asyncFunctionDeclaration10 は index None と skip action で自動的に外れます。以後の拡張は census と対照を経て parent kind の集合を広げる形で、case ID/本文特例なしです。Codex の指摘どおり `SourceByteRange::without_leading_trivia` (position.rs:65) が空 range を保持するので、printer 特例は前提にしません。

### 第 0 段の negative controls(unit)

1. `async function f(a = await => await) {}`: event 2(2 件目 index None)、`TokenSkipped{ListAbort, =>}` 1 件、1 件目 `missing_node == Some(Identifier)`。
2. `.js` で JSDoc 内エラーのみ: record 空、`js_doc_diagnostics` 非空、arena に JS_DOC missing ノードがあっても census が拾わない。
3. asyncArrowFunction9(try_parse 失敗→restore): fresh と再 parse で event/action 数一致。
4. incremental: skip を含む文の外を編集し、fresh == incremental(actions 込み)。
5. topLevelAwaitErrors.1: `Reparsed` 範囲が TS の run と一致し、保持 event が範囲外のみ。
6. `let = 1`, `a.`: `missing_node == Some(Identifier)` だが親が AwaitExpression 以外で拒否。
7. as-newline 12: `TokenSkipped{ListAbort, )}` で拒否のまま。
