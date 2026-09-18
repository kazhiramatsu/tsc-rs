レビュー結果です。

## 1. comment 設計: pending `LeadingOnly` は PEE で再 container 化が必要

**現 prototype (`emit_access_target_with_source_comments` を PEE arm から呼ぶ) は Inactive 状態には正しい**が、Pending は 14355-14365 で無加工転送されます。転送元が `LeadingOnly` (`emit_child_after_token_with_context` 13904-13923 経由、例: 14856-14870 の token 所有 paren lane、`emit_required_node_with_context_and_source_extent` 14249 の LeadingOnly 呼び出し) のとき、子の trailing phase は 15594 で `owns_trailing()` 偽 → `None` となり、`(object?.x /* c */ as number)` を `return`/`throw`/paren token 配下に置くと同じ comment 消失が再発します。upstream では親が containerEnd=親 end を claim し、PEE が自分の range を claim し直す (121007-121029) ので、子 end ≠ PEE end なら子が書きます。

**推奨する最小形** (PEE arm で Pending を受けたとき):
1. `DeferredExpressionSourceComments { container: Some(Scope(<PEE を claim した scope>)), preceding_token: pending.preceding_token, extent: LeadingAndTrailing }` を子へ渡す。`preceding_token` は必ず保持する。leading 側の no-ASI/token resume は 15560-15564 が `preceding_token` だけを見ており、container は `container_pos` の prefix にしか使われない (15565-15577) ので、container の差し替えは no-ASI 契約に触れません。`nested()` は `preceding_token: None` にするので流用不可 (279-285)。
2. 子が返した ownership が `RetainedByParent` (PEE scope が子 end を保持 = `<number> obj?.x /* c */` のように wrapper.end == expression.end の形) のときは、**PEE 自身が元の pending deferred で PEE end の trailing phase を実行**する。これが upstream の「PEE の emitTrailingComments が親 container に対して発火する」段に相当し、LeadingOnly なら `owns_trailing` 偽で親に戻る、LeadingAndTrailing なら PEE が書く、と自然に分岐します。
3. 外側へ返す outcome は元の extent に合わせて翻訳する: LeadingOnly なら `LeadingConsumed` (13922/14264 の assertion)、LeadingAndTrailing なら手順 2 の結果を `Complete{trailing}` として `record_outcome`。子の `VisitedHere` anchor (X+1) は親 token 境界ではないので捨てて安全です。

形としては access-target helper より SourceRanged paren 経路 (14563-14621: 内側 nested phase + 外側 deferred phase) の写しが近い。重複の危険は、子 lane = 同一行 trailing、boundary after-side = 改行後 leading、PEE 自身の phase = PEE end のみ、で位置が互いに素なので構造的にありません。

**Pending 用の追加対照**: `return (object?.x /* c */ as number);`、`return <number> /* b */ object?.x /* c */;` (PEE end == 子 end)、`throw`、`(x as T) /* c */` の paren lane、call 引数 `f(x as T /* c */, y)`、default param `(a = obj?.x /* c */ as number) => a`、no-ASI 対照 `return\n(x as T)` の不変。

## 2. target=100: B は小さくない。A を採る

- upstream は `createSourceFile` (28870-28881) で **ファイル名に関係なく** JSON parse し、以後は `isJsonSourceFile` (= scriptKind) が `sourceFileMayBeEmitted` (16617-16634: JSON は outFile/outDir がなければ emit しない)、printer (118216/118625)、module transform (110131/110171/110208)、checker (44541 ほか 5 箇所) を分岐します。Program route では lib.d.ts も JSON parse されます。
- transpile route の "Output generation failed" は `sourceFileMayBeEmitted` が偽 → outputText undefined → `Debug.fail` (typescript.js:146130) で、**入力内容に依らず target=100 なら常に発生**します。decorator/define は無関係で、その control は route クラスの witness です。
- Rust には SourceFile の script_kind fact が存在せず、JSON 性はファイル名判定です: checker/lib.rs:1895、emitter 側 `is_json_file_name` 利用 10 箇所 (execute.rs:984、source_map.rs:463、declarations/orchestration.rs:90/140/248、declarations/bundle.rs:46、builtins.rs:596/2711、printer.rs:1746/2829)、program の pre-parse (module_requests.rs:323)、transpile adapter。B = SourceFile に `script_kind` を導入し全消費者を差し替える別 slice です。

**A の正確な境界**: `emit_script_target() == ScriptTarget::JSON` (types/flags.rs:2169) を `validate_emit_options` と builtins admission で typed refusal (owner: source-kind facts 未表現)。他の上限は撤去のまま。到達性は API 数値のみ (CLI/config に 100 へ写像する名前はない)。1234 の retire は妥当、100 の control は「transpile route では TS が全入力で例外」を明記した strict native KNOWN として archive。将来 B に進む際の前提は script_kind fact と、Program route で outDir ありのとき TS が `.ts` を JSON として印字する挙動の観測です。
