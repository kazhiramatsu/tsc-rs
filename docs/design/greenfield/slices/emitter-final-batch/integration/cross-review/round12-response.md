以上で確認できました。指摘のみ簡潔に。

## 1. checker 側 diff の照合

- **annotate.rs CJS 合成 (9121-9162)**: upstream 56348-56446 と同じ条件 (Value&&Value、alias 除外、valueDeclaration のファイル不一致) で 2 件を発行。位置は upstream が `error(s.valueDeclaration)` → `getErrorSpanForNode` で VariableDeclaration の名前 (index.ts:70 長 1)、object 側は `tryCast(isNamedDeclaration)?.name` → Rust の `name_of_named_declaration(...).unwrap_or(decl)` と一致。診断数: TS 側 trace では同じ位置の発行が 2 経路から起きるが `lookupOrIssueError` で 1 件に畳まれ、Rust も `push_error_diagnostic` (state.rs:2161) が related 込みの完全一致で dedupe するので同数。related の内容が両回で同一である限り安全。意味差なし。
- **late member 再統合 (4814-4839)**: upstream 57747-57762 と同形。`!resolved → resolved = original` 分岐は Rust では空 table でも per-name merge に包含されるので等価。
- **calls.rs JSX (6167-6215)**: elaboration.rs の object 経路と同じ specific/fallback。差は upstream 64160 の `exactOptionalPropertyTypes` 不一致分岐が JSX 側にないことのみ (既存の欠落、今回の対象外)。
- **conditional.rs 141-145**: identity 比較に変更。per-alias error 型は通過するので upstream 62657 と同じ。
- **instantiate.rs 1299-1300**: `is_array_or_tuple_or_intersection` gate。upstream 63592 と一致。
- **engine.rs origin (2812-2835)**: 65418/65422 の両向きを正確に移植 (source 側は origin=Intersection、target 側は origin=Union)。
- **engine.rs cached overflow (3357-3373)**: 65750-65756 と同じく `reportError` の後に cached 判定を返す。`override_next_error_info += 1` で generic 2322 が抑止され、診断数は変わらず code が 2859 に置換される (f2 で 2 件、f1 は origin 経路で無エラー)。

## 2. 深さガード

- **engine.rs (3401-3500)**: push + 判定 + handler push + structured を 1 closure に入れ、外で handler pop → depth snapshot 復元 → flags 復元 → Err なら `reset_maybe_stack` して return。upstream の decrement と snapshot 復元は単一 frame では等価。stack の余剰要素は upstream も上書き方式で残すので同じ。`pushed_handler` は closure 前に false 初期化されていることだけ要確認 (判定で Err すると代入前に抜ける)。
- **inference.rs (1005-1037)**: clone 渡しで借用衝突を回避、closure 外で pop/flags 復元、cache は Ok のみ。upstream は判定時点の live stack を読むだけなので snapshot と等価。再入時の持続 state の懸念は clone で正しく回避されている。
- **invoke_once (2382-2393)**: walker 破棄前提の `?` で問題なし。
- **is_deeply_nested_type / get_mapped_target_with_symbol / has_matching_recursion_identity (4442-4516)**: 67465-67505 と同形。`INSTANTIATED_MAPPED` は `contains` (両 bit) で判定しており正しい。

## 3. module flatten

適用 5 点は upstream と整合。読み取りで見つかった残り:

1. **direct + alias 複数 export の wrapper 順 (8917-8931)**: upstream は direct 名を `exportedBindings` に持たず、`exports.a` は印字時 substitution で常に最内側、alias wrapper は `getExports` の順で外側。Rust は `plan.exports` を順に巻くので、`export {a as b}` が宣言より **前** にある場合に plan 順が [b, a] になると `exports.a = (exports.b = …)` と逆転する。`direct` を必ず最内 (最初) に固定し、alias は plan 順のままにする。対照: `export {a as b}; export const {a} = o;` (TS: `exports.b = (exports.a = o.a)`)。
2. **0 要素パターンの temp**: upstream の `reuseIdentifierExpressions = !isDeclarationBindingElement(parent) || numElements !== 0` (93490、93549) は、宣言側 `export const {} = o` / `export const [] = o` で temp を切る (`_a = o`)。Rust は object 8807-8809 と array 8869-8871 で `true` 固定、array の `all_omitted` も空配列で偽 (upstream `every([])` は真)。対照に両形を追加。
3. `__read` 分岐 (8844-8868) は level All 前提で upstream 93534-93547 と一致。`set_text_range(read, original)` と reuse=false も正しい。

## 4. 残る boundary の分類

**parse-recovery 36 行**: 本番 gate は `builtins.rs:16166 preflight_source` → `has_only_literal_recovery` (syntax/lib.rs:176) → `recovery.rs:67-77` (diagnostic origin が literal のみ、`SilentMissingNode` なし)。行の内訳は async 系 32 行 (`(a = await)`, `[await]`, `await => await` の各 target)、decoratorExpression.3 ×2、topLevelAwaitErrors.1 ×2。不足している parser fact は `Diagnostic(Parser)` event が「missing node を作ったか」「token を skip したか」「reparse したか」を持たないこと (recovery.rs:28-30 の設計コメントどおり message 付き missing node が Diagnostic に畳まれている)。この fact なしには次の 3 クラスを区別できない:
- (a) 完全な node への純粋 report: printer 側の追加契約なしに admit 可能。ただしどの行が (a) かは census が必要で、読み取りでは断定できない。
- (b) missing identifier (async の `await)`/`[await]` 系の大半): TS は空文字で印字する (`a = await `)。Rust printer に missing node の印字契約が見当たらない (grep 0 件) ので、`pos == end` の Identifier を空印字し map を 0 幅で記録する契約と JS 対照が先に要る。
- (c) skip/reparse (`await => await` 系): 復元木の形が TS と一致する保証が要り、最も広い。
したがって今回の統合では typed refusal を据え置き、後続で「event に `missing_node`/`skipped` fact を追加 (admission は変えない data-only)」→「census で (a)(b)(c) に分類」→「(a) の admit と (b) の printer 契約」の順に進めるのが根拠ある分割。無条件の guard 除去は (b)(c) で TS と異なる出力を黙って出すので不可。

**JSON target=100**: 合意どおり A (typed refusal) のまま。B は SourceFile の script_kind fact と `is_json_file_name` 利用 10 箇所超の置換を要する別 slice。
