## r23 照合結果

### 1. object chunk の `set_original_and_range` 除去に同意

- upstream は `flattenObjectBindingOrAssignmentPattern` (93500-93530) の全 emit 点で `createObjectBindingOrAssignmentPattern(bindingElements)` = `makeObjectBindingPattern` (93681) の **fresh factory pattern** を渡し、`emitBindingOrAssignment` が `setTextRange`/`setOriginalNode` を付けるのは返却 VariableDeclaration だけです。pattern 自体は pos/end = -1。
- native map を vendored tsc で decode して確認しました(`let { a: v = 1 } = o, r = __rest(o, ["a"])`、ES2015/ES2017 とも同一): 宣言開始 `{`(gen 14:8 → src 55、これは declaration の range)、要素、`o`、`r` 側の segment だけで、**`}` (gen 14:19) と `=` (gen 14:21) 付近に segment はありません**。Rust の余分 (14:20 → 73) は retained pattern の end(元 `}` の直後 = 73)から出る end 側 mapping で、`=` token の skipTrivia 位置 74 ではなく 73 を指すことからも pattern の range 由来です。
- Rust 側: `flush_object_pattern_chunk` (es2018.rs:3520-3533) の Binding 限定 `set_original_and_range(pattern, original_pattern)` (3529) が唯一の producer。`set_original_and_range` (5276-5285) は `set_text_range` + `set_original_node` の両方を書きます。除去で range も original も外れ、fresh 状態になります。
- declaration 側の provenance は `materialize_binding_plan` (3720-3748) が `step.original` から `set_text_range`/`set_original_node` を付けており、pattern と独立です。retained leaf(visited 元要素)の provenance は `self.visit(node)` の返却に乗ったまま (3491-3493) で影響しません。
- `NO_TRAILING_SOURCE_MAP` 追加案を採らない判断に同意します。fresh node の状態を直さず、pos 側 mapping(宣言開始と重なり dedupe されているだけ)も残るためです。

### 2. array 側も同じ divergence、修正すべき

- upstream `flattenArrayBindingOrAssignmentPattern` の末尾 (93594-93596) も `createArrayBindingOrAssignmentPattern(bindingElements)` = `makeArrayBindingPattern` (93675) / `makeArrayAssignmentPattern` の fresh pattern を `emitBindingOrAssignment(..., value, location, pattern)` へ渡します。object と同形です。
- native probe `let [a, {b, ...r}] = xs`(ES2015)の出力 `let [a, _a] = xs, { b } = _a, r = __rest(_a, ["b"])` の map は `[`=宣言開始 (13:8→58)、`a`、`xs`、内側 `{ b }`(こちらは element の location)…で、**`]` (13:12) と `=` (13:14) に segment なし**。Rust の `flatten_array_pattern` 3587 `set_original_and_range(retained_pattern, pattern)` は mode を問わず付けているので、retained 配列 pattern が印字される形(deferred element を含む ES2015+ target、および Assignment mode の `[a, {b, ...r}] = xs`)で同じ余分 segment が出るはずです。既存 28 対照にこの形が無いだけと判断します。3587 も除去してください。
- 除去後、`plan_push` (3652-3667) と materialize が declaration/assignment 側に `step.original` の range/original を付ける点は不変。

### 3. downstream で pattern の original/range を要する箇所

- ES2015 の共有 flattener は `flatten_destructuring_binding(self, node=declaration, …)` を declaration 起点で受け取り、`location` も declaration/element から取ります (flatten_destructuring.rs:296-335、`record.location`/`record.original`)。pattern ノードの range/original は読みません。
- resolver 照会 (`parse_tree_resolver_node`) は declaration/BindingElement/loop variable に対して行われ、pattern には行いません(r21 で確認した `is_declaration_with_colliding_name`/`process_loop_variable_declaration` 経路)。
- printer の pattern 印字は `{`/`}`/`[`/`]` の token map と comment を node range から取るため、range を外すと upstream 同様に出なくなります。これが今回直したい挙動そのものです。
- 付記(今回の範囲外): upstream の declaration は `setOriginalNode(declaration, pattern)`(4 番目の引数)ですが、Rust は `step.original` に caller の original(declaration)を使っています。既存 26 行が exact なので map/JS には出ていませんが、`get_original_node(declaration)` を読む consumer が増えた時に差が出得る点だけ記録を推奨します。

### 4. negative controls(少数)

1. object 側 comment: `{ let { a: v = 1 /*c1*/, /*c2*/ ...r } /*c3*/ = o; }`(ES2015、iteration on/off)。JS と map の両方。`c2`/`c3` が retained pattern から出ないこと。
2. array 側 map: `{ let [a, {b, ...r}] = xs; }`(ES2015)と assignment 形 `[a, {b, ...r}] = xs;`(ES2015/ES2017)。`]`/`=` に segment がないこと。
3. array 側 comment: `let [a /*c1*/, /*c2*/ {b, ...r}] /*c3*/ = xs;`。
4. 不変対照: rest を含まない `let { a: v = 1 } = o;` と `let [a, b] = xs;`(fresh 化の影響がない通常経路が JS/map とも不変)。
5. ES5 target で同入力(ES2015 側 flattener に渡る経路)が JS/map 不変であること。
