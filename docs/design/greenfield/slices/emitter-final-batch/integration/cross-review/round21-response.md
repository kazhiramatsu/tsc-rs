## r21 照合結果

### 1. pattern head の名前 materialize(wrapper 案に同意、最小形を具体化)

**原因の確認**: 共有 flattener は leaf name として *parsed identifier ノードそのもの* を pending declaration に積み(`flatten_destructuring.rs:352-386`, `name: target`)、`create_variable_declaration` (2383-2401) がそれを `VariableDeclaration.name` に置きます。upstream はこの parsed name を print 時 `substituteIdentifier` → `getGeneratedNameForNode` で採番しますが、Rust の r11 方式は visit 時に materialize した名前だけを finalizer が print 順で採番するため、flatten 経由の leaf は provisional に落ちて反転します。非 pattern と同じ不足です。

**最小案**: ES2015 owner に `flatten_destructuring_binding_materialized(...)` を置き、shared fn の返却 declarations を走査して `NodeData::VariableDeclaration.name` を `colliding_declaration_name_substitute(name)?` に通し、`Some` なら `update_variable_declaration_name` (es2015.rs:5926-5941) で差し替える。根拠:
- `colliding_declaration_name_substitute` (5863-5920) は `parse_tree_identifier` (668-686) が generated / 非 parse-tree を `None` で弾くので、flattener の temp や hoisted 名には触れません。
- `update_node` (factory.rs) は `clone_node` で `original` チェーンを継ぎ、pos/end を再適用する (n+36..n+60) ので、flattener が付けた `set_original_node`/`set_text_range` (flatten_destructuring.rs:326-335) は保たれます。
- FlattenHost の拡張は不要。5 caller(5826 変数宣言 pattern、12388 for-of pattern head、4229/4429 parameter pattern、6119 catch pattern)を wrapper に付け替えるだけ。4229/4429 は parameter 要素が FunctionScoped なので resolver が非衝突を返し no-op、6119 は catch の destructured 要素が block-scoped なので実効あり。

**upstream 印字順との一致範囲**: 採番は base 名ごとの「最初の要求」順。flatten 後の list では、参照が宣言より先に印字される形(`for (let [v] of [[v]])`、`let [v] = [v]`)でも参照と宣言の間に印字されるのは別 base の temp だけなので一致します。一致しないのは、参照と宣言の間に *同 base の別衝突宣言* が印字される形(初期化子内の関数本体に `let v` がある等)で、これは非 pattern でも同じ finalizer 方式の既存限界です(今回の変更で新たに生じるものではない)。副作用面: `mark_generated_binding_print_order` は list 内の生成ノードに付くだけで、preallocated binding の識別子は `generated_names_for_nodes[(source, original)]` で参照側と共有されます。

### 2. captured-loop の map(直接 push 案に同意)

- factory は合成時に子の `parent` を書きません(`factory.rs` の parent 書き込みは `set_parent_from` 6579-6600 のみ、呼び出し元は es2015.rs:7049/7054 の 2 箇所)。parsed name を `call_arguments.push(name)` しても、parse-tree の parent(`is_name_of_declaration_with_colliding_name` 474-495 が読む側)は不変です。
- print 時は parameter 側が `substitute_identifier`、引数側が `substitute_expression_identifier` → resolver → 同じ binding を引くため JS は不変。`substitution_identifier_clone` (820-860) は生の pos/end から `SourceRange::Original` を作るので、parsed name をそのまま渡せば map 境界が復活します。現行の `clone_node` は pos/end を `u32::MAX` にし (factory.rs:5531-5535) metadata の range を付けても substitution clone には引き継がれない、が失敗の機構です。
- 同一ノードが parameter と引数の 2 箇所に現れるのは upstream (107340) と同じ構造です。確認点は 1 つ: finalize/`for_each_child` 系の walk が同じノードを 2 回訪れても副作用がないこと(`printer.rs:8575` 付近に再訪回避の記述あり)。parsed name は生成ノードではないので finalizer の採番対象にはならず、コメントも upstream 同様両サイトで出ます。
- 代案 `clone_node + set_text_range(argument, name)` (factory.rs:6601-6623 は生の pos/end を書く)でも map は直りますが、upstream と同じ producer に戻す直接 push の方が単純です。

### 3. 追加 negative controls(少数)

1. 通常 block の array pattern: `for (let v of []) { let v; { let [v, w] = [v, 1]; } }`(ES5、downlevelIteration off/on)。
2. object pattern の rename/default/rest: `let v; { let { a: v = 1, ...r } = o; }`(ES5; rest helper 経路)。
3. catch destructuring の衝突: `let v; try {} catch ({ v }) {} let w;`(6119 経路と visit 時 gate の確認)。
4. captured 非衝突: `for (let i of xs) { setTimeout(() => i); }`(rename なし、`_loop_1(i)` の map 境界)。
5. captured 衝突 + pattern head: `for (let [v] of [[1]]) { let v; setTimeout(() => v); }`(1 と 2 の合流)。
6. parameter pattern が no-op であること: `function f({ v }) { { let v; } }`。
7. 分類用 probe(必須対照にしない): `let [v] = [v, (() => { let v; })()]`。finalizer 方式の既存限界を記録する用途。

1・4・5 は `.js.map` まで一致を要求し、元 4 失敗は KNOWN 化せず全一致で閉じる方針に同意します。
