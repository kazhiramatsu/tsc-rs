## r20: parse-recovery 36+12 の閉じ方(source 調査結果)

### 分類(TS 6.0.3 実観測 × Rust parser source)

TS 側は vendored `typescript.js` で各 fixture の parse 診断・零幅ノード・emit を採取しました。Rust 側は parser の到達経路で分類しています。

| 群 | 行数 | 内容 | Rust の記録経路 |
|---|---|---|---|
| (b) messaged missing Identifier のみ | 24 | asyncArrowFunction6/7/8, asyncFunctionDeclaration6/7/9 × 4 target。`await )` / `[await ]` の位置に零幅 Identifier + 1109、skip なし | `create_identifier_node` (parser.rs:1288-1302) → `create_missing_node(Identifier, false, Some(Expression_expected))` (1229-1262) → `Diagnostic(Parser)` event、`alloc_missing` (arena.rs:399) |
| (c) token skip | 4+4+12 | asyncFunctionDeclaration10(`=>` を list abort で skip、同 start dedupe で index None)、asyncArrowFunction9(`try_parse` 失敗→call として再解析、1005×2、文リストで `<void>` 群 skip、EmptyStatement/Block)、malformed-comment 12(欠落 `)` report、1434 report、1128 で `)` skip) | `abort_parsing_list_or_move_to_next_token` (1837-1844) ほか skip 5 箇所 (1639, 3644, 6977, 7039, 8983) |
| MissingDeclaration | 2 | esDecorators-decoratorExpression.3。TS 木は `Block[MissingDeclaration{@g}, ExpressionStatement(TypeAssertion(<number>, ClassExpression))]`、1146 は `Declaration expected`(零幅 Identifier ではない) | parser.rs:2495-2508 / 6087-6098 で同形に回復。printer に `MissingDeclaration` arm なし(upstream は 117464/117678 で `return;`) |
| (d) reparse | 2 | topLevelAwaitErrors.1。`reparse_top_level_await` + 零幅 Identifier 12 + 1005×3、legacy decorator に欠落 operand(`__decorate([(await )])`, `__param(0, )`) | `retain_reparse_recovery` (9498-9533) |

**純 report-only (a) は 36+12 の中に 0 行です。** decoratorExpression.3 は record 上は messaged missing node、malformed 12 は skip を含みます。

### 1. data-only instrumentation

現 record は messaged missing node と report-only を区別できません(両方 `Diagnostic(origin)`、差は `diagnostic_index` のみ)。event を増やさず、`ParseRecoveryEvent` (recovery.rs:37-48) に 2 field を追加します。

- `missing_node: Option<SyntaxKind>`: `create_missing_node` の messaged 経路で、直前に push した event(dedupe 時も push される: parser.rs:9915)に kind を書く。
- `flags: TOKEN_SKIPPED | REPARSED`: skip 6 箇所は直前 event に立てる(6977 の silent skip は `parse_identifier_or_pattern` の missing-name event が直前にある)。`retain_reparse_recovery` が retained event に REPARSED を立てる。

consumer 一覧(全て count/kind ベースで契約不変): `is_literal_only` (recovery.rs:67)、`checkpoint/restore` (79-90)、`try_parse`/`look_ahead` (parser.rs:1319-1360)、incremental の再利用拒否 (1372-1385、start/length のみ)、`incremental.rs:313` の record 等値比較(derive に新 field が入る。`tests/unit/incremental/tests.rs:191` に recovery 入力を 1 件追加して fresh==incremental を固定)、`syntax/tests/unit/parser/recovery.rs` と `recovery_provenance.rs`(coverage/count のみ)、`utf16_literal_recovery_census.rs:145-165`(count のみ、任意で class 別 counter 追加)、emitter preflight (builtins.rs:16269-16275、count のみ)、`discard_parse_recovery_for_harness` (syntax/lib.rs:188)、`program/src/json.rs`(参照のみ、要確認)。

木の照合には census 拡張として、case ごとに event(kind, missing_node, flags, start)と Rust 木の零幅ノード(kind, pos, parent kind)を出し、tsc 側の同形 dump(今回の scratch と同じ walk)と突き合わせる artifact を提案します。

### 2. 最小で安全に閉じられる群 = (b) 24 行

printer/transform の既存経路で動く根拠:
- missing Identifier は `text: ""` の通常 Identifier (nodes.rs:1958-1960)、`pos == end`。
- `SourceByteRange::new` は `start > end` だけ拒否 (position.rs:14) → 零幅 Original range が map/comment 経路に流れる。TS も pos と end の 2 回 `emitSourcePos` を同一生成位置に出し generator が重複を落とすため、mapping 1 個。
- `TextWriter::write("")` は no-op (writer.rs:381-386)。`AwaitExpression` arm は keyword + space + child (printer.rs:4827-4850) なので `await )` と一致。
- es2017 `visit_await_expression` は operand をそのまま yield へ (es2017.rs:722-743)。ES5 では generator が `[4 /*yield*/, ]` を印字する必要があり、配列要素の空 Identifier 印字で自然に成立するはず。

admission は共有 facts の述語で行います: `is_emit_safe_recovery()` = 全 event が literal、または `Diagnostic(Parser)` かつ `diagnostic_index.is_some()` かつ `missing_node == Some(Identifier)` かつ flags 空。case ID/本文特例なし、typed refusal は他群に残ります。ただし messaged missing Identifier は `a.` や `let = 1` など他文脈でも発生するため、admit 前に census でその述語に合致する corpus 全 unit を列挙し、`discard_parse_recovery_for_harness` 経由で bytes 比較して文脈ごとの printer 契約(property access 末尾、binding name 欠落など)を確認する。失敗文脈があれば preflight で missing node の parent kind(arena の parent link から取得、これも共有 fact)で絞る。

negative controls: `async (a = await /*c*/)`, `{ [await /*c*/]: x }`(零幅ノード周りの comment 所有)、`.js.map` 一致、ES5/ES2015/ES2017 の 3 target、有効な兄弟 `a = await 1`、そして skip/reparse 群が引き続き typed refusal のまま(asyncFunctionDeclaration10、as-newline 12)。

### 3. 実装順と async32 の分離

1. Stage 0: instrumentation + census artifact + tsc 側 dump。async32 の「木差 vs transform 差」はここで分離できます。木は零幅ノード/event の一致で判定し、transform は既存 hidden hook `discard_parse_recovery_for_harness` (active_transform_contract.rs:6405 で使用中)で admission を変えずに bytes 比較する。
2. Stage 1: (b) 24 行(async32 のうち 24)。
3. Stage 2: MissingDeclaration。printer に upstream 同様「何も出さない」arm、TS transformer が `modifiers` を visit して decorator を落とす経路、admission に `missing_node == Some(MissingDeclaration)` を追加。+2 行。対照: top-level `@dec<number> class C {}`、`@dec() <T> class {}`、experimentalDecorators 両値。
4. Stage 3: skip 群。asyncFunctionDeclaration10 は parameter list 内の skip で gap が露出しないため先行可能(+4)。asyncArrowFunction9 と malformed 12 は文リスト skip、EmptyStatement、欠落 `)` に対する `emitTokenWithComment` の comment scan(r17 の二重 `/* value */`)が必要で、printer の欠落トークン契約を先に設計する。
5. Stage 4: reparse(topLevelAwaitErrors.1)。reparse 木の完全一致に加え、legacy decorator transform が欠落 operand を `(await )` / `__param(0, )` として印字する契約。最後。

### 4. 不可欠な変更と製品境界

36 を全て閉じるのに不可欠: instrumentation、(b) 述語、MissingDeclaration arm + transformer、skip 群の printer 欠落トークン/gap comment 契約、reparse 一致 + decorator transform。いずれも emitter 製品内(H2.9)で、別製品境界に当たる行はありません。ただし規模順は (b) 24 ≪ MissingDeclaration 2 ≪ skip 20 ≪ reparse 2 で、skip 群が最大の設計項目です。
