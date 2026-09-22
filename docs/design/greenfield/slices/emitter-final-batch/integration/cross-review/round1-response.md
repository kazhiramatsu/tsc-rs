レビューを完了しました。以下、優先度順にまとめます。

## Priority 1: decorator-super arrow の map/text 回帰

**原因は Codex の指摘どおり更新順序で、私も独立に同じ結論に至りました。** 根拠:

- `crates/emitter/src/builtins/standard_decorators.rs:5681-5683` が lexical environment を閉じる前に `update_generic` を呼ぶ。これは `factory.update_node` を経由し、`crates/emitter/src/factory.rs:5929` の `parenthesize_updated_arrow_concise_body` が visited 済みの comma body を `set_text_range(body)` 付き ParenthesizedExpression で包む。
- その後 `standard_decorators.rs:5684` で temporaries を取得し、`5713` の `create_return_statement(body)` は既に括弧付きの body を受け取る。結果 `return (Reflect.set(...), _a);` となる。
- upstream は逆順。`visitFunctionBody` (`_tsc.js:91277-91290`) が環境を閉じて `convertToFunctionBlock` (`20665-20672`) で生 body から `return` を作り、`createReturnStatement` (`23196`) に parenthesizer はなく、`emitReturnStatement` (`118884`) も `parenthesizeExpressionForNoAsi` のみ。Block になった後で `updateArrowFunction` が呼ばれるので `parenthesizeConciseBodyOfArrowFunction` (`20514`) は no-op。
- map の余分な `CAAA` 対は、この合成 paren が body の source range を持つため `printer.rs:7274-7339` の paren 出力で node 境界 map を記録することの帰結。ログの mapping を復号すると `return ` 直後の `(` と閉じ `)` の位置に一致する。text 差分は Codex の complete capture に依拠しており、私は fixture (zst) を展開できていない。

**最小修正 (推奨、Codex 案と同じ骨格)**: `visit_function_like_body` を「子 visit → 環境終了 → 必要なら生 body から block 化 → 最後に一回だけ `update_node(original, …)`」に並べ替える。

```rust
self.start_lexical_environment();
let visited = self
    .visit_parameter_list(parameters)
    .and_then(|()| try_visit_each_child(&mut data, self));   // update_generic の前半だけ
let (temporaries, init) = self.end_lexical_environment_with_initialization_statements();
visited?;
if !temporaries.is_empty() || !init.is_empty() {
    // data.body は未括弧の生 visited body
    let block = if Block { body } else { convertToFunctionBlock(body) };  // 既存 5712-5719 と同じ
    body_slot(&mut data) = merge_block_environment(block, temporaries, init)?;
}
let flags = flags_after_update(arena, original, &data)?;
factory.update_node(original, data, flags)
```

保全義務: `update_node` を `original` に対して一回だけ呼ぶので original chain が単段になる (現状は original → updated → final の二段)。comment range と text range は `update_node` が原点から付けるので同等になるはずだが、ここは suite で検証が必要。`ClassStaticBlockDeclaration` は body が常に Block なので影響なし。`visit_parameter_list` が `self.arrays` に memo する lowered parameter は `try_visit_each_child` がそのまま拾うので変更不要。

**却下する代替案**: `5713` の直前で合成 paren を剥がす。parse 済み paren (`() => (super.x++, 1)`) は TS も `return (...)` を保つため、合成/parse の判別ヒューリスティックが必要になり本質修正ではない。

**partial-wrapper の 2 変更 (factory.rs:3864-3867、printer.rs:12738-12744 と 13075-13083)**: upstream に忠実な refinement だが本件とは無関係。通常 source では comma は必ず parse 済み paren の中にあるので観測可能な witness を作れないと判断。witness を出せないなら本 train から外すのが最小影響。

**必要な回帰テスト**:
1. `TSC_RS_DECORATOR_SUPER_CASE_SET=discard/arrow-expression-body` と `..._FOLLOWUP_CASE_SET=nested-arrow-in-default` の 10 行を bytes と map で exact ×2。
2. generator に新 witness を追加: `static f = () => (super.x++, 1);` (parse 済み paren は `return (...)` を保持する対照)、`static f = () => ({ v: super.x++ });` (object-literal 規則も block 化で括弧が消える)、`static f = () => super.x;` (temporaries なし、concise 維持)。
3. decorator-super 全 suite、followup2/3、controls、emitter factory/printer unit、emitter-final universe。

## Priority 2: module-request の変更

**upstream との対応は正確で、失われた正当な不変条件はありません。**

- `module_requests.rs:1293-1314` は `getEmitModuleFormatOfFileWorker` (`_tsc.js:125493-125494`) の `?? getEmitModuleKind` と一致。
- `1318-1336` の `0 | 2..=4 | 100..=199 => Unspecified` は `122306-122307` (CJS→CJS、5..99 と 200→ESNext、それ以外 undefined) と一致。dynamic は `125486-125491`、import-equals/require は `122296-122302` と一致。
- 旧 refusal「Node module kind には implied format が必須」は upstream の不変条件ではない。upstream が保証するのは Node16+ の module *resolution* 下で `.ts/.js` に impliedNodeFormat が計算されることだけで、これは `loader.rs:3768-3781` に構造的に残っている。
- unknown と authoritative-none の区別は planner には不要。upstream は両方を `??` で扱う。必要な区別 (raw と for-emit) は `prepared.rs:196-207` で既に保持されている。`Some(Unspecified)` を error にする `1301-1306` は upstream に存在しない表現への fail-closed なので維持でよい。
- type reference の既定 mode (`362-365`) は `getDefaultResolutionModeForFileWorker` (`125510-125511`) が module へ fallback しない worker を使うのと一致しており問題なし。

**追加を推奨するテスト**: (a) `module: 101 + moduleResolution: 100` で `/index.mts` → [CJS, EsNext, EsNext]、`/index.cts` → [CJS, CJS, EsNext] (authoritative 経路が依然勝つこと)。(b) loader 側 unit: resolution 3..=99 下では `.ts/.js` の implied が必ず `Some` (削除した refusal の代替不変条件)。(c) 4 行の EF7 と周辺 15 commands は Codex 報告どおり exact ×2 で、config 診断 (TS5095 系) が不変であることを同じ suite で固定する。

## Priority 3: 残る refusal 境界

**parse recovery (`builtins.rs:16166`)**: 今の train では緩和しないのが正しい。安全に admit する前提は次の 3 つ。
1. parser fact の拡張: `recovery.rs:28-35` のとおり、message 付き missing node は `Diagnostic` event に畳まれていて、「完全な node への純粋な report」と「missing node 生成」「token skip」「reparse」が区別できない。event に `missing_node: Option<SyntaxKind>` と skip/reparse の有無を記録し、`has_only_reporting_recovery()` を追加するのが bounded 拡張の中核。
2. printer 契約: 36 行の大半 (`async (a = await)`、`[await]`、`await => await`) は `await` の直後で `Expression expected` の missing identifier を作る。TS はこれを空文字で印字する (`a = await )`)。missing node の空印字と zero-width map の契約が先に要る。純粋 report のみで閉じる行は decorator-expression の 1 行程度と推測 (census 未実施、推測)。
3. 計測: `lib.rs:188` の `discard_parse_recovery_for_harness` と `utf16_literal_recovery_census.rs` 型の census を 36 行に当て、event kind で bucket 化してから admit 集合を決める。
以上は H2.9 の別設計であり、本 train は「emit 欠陥」ではなく「parser/printer の未実装」として据え置く。

**stableTypeOrdering**: upstream 読み取りは `46478-46479` (fileIndexMap)、`50137/50146` (typeof facts 順)、`61328-61350` (union 挿入の compareTypes)、`90576` (compareSymbols)。checker crate に読み取りは 0 件で、union 順序と診断/宣言テキストに波及する checker 作業。`execute.rs:158-161` の typed refusal は維持、owner=checker の KNOWN として別 slice。

**assumeChangesOnlyAffectDirectDependencies**: 唯一の読み取りは builder の `126755`。option 宣言 (`36801-36809`) の affectsEmit は buildinfo 無効化用で、fresh Program の emit/診断は不変。`incremental/composite/tsBuildInfoFile` の refusal が残る限り builder に入らないので撤去は健全。4 対照で十分。

**target > ESNext (round2)**: `ScriptTarget` は total な `from_bits` (`types/ty.rs:246-250`) なので数値比較は全域で成立し、upstream にも上限検査はない。`execute.rs:116` と `builtins.rs:285` の上限だけ外すのは妥当だが、条件が 2 つ。(1) Rust 側で既知定数に対する exhaustive `match` がないこと (grep で確認要)、(2) default lib 名解決が未知値で `lib.d.ts` に落ちること。JSON=100 は scriptKind 由来の languageVersion であり option 経路では「ESNext 超」として振る舞うだけ。対照は target=99/100/1234 の JS bytes 一致と TS6046 同一。下限 (ES3 は unset 計算) は維持。

**has_advanced_comment_placement (`builtins.rs:16211-16281`)**: 生テキスト走査は文字列/コメント内で誤 refuse する。最狭経路は A→B の順。A: parse 済み tree 上で PrivateIdentifier `in` の左辺 end と演算子 pos の間、および optional chain の end と `as`/`>` の間に comment trivia があるかを scanner の comment range で判定する typed 検出に置換。B: 真陽性を oracle に通し、exact なら witness 付きで retire。witness なしの retire は不可。

**不確実な点**: fixture 未展開のため text 差分は Codex capture 依存、単回 `update_node` の comment range 同等性は suite 未検証、parse-recovery 36 行の bucket は upstream テスト文面からの推測です。
