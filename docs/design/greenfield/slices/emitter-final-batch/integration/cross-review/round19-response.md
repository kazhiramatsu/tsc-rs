## r18 照合結果

### 1. default-library fact の配線案

**呼び出し元の分類は提案どおりで正しいと source で確認できました。**

- legacy 系 `check_program_with_libs*`(1036/1181/1252/1278/1297/1317)は `ProgramJson.libs` を別配列で受け取る oracle モデル(noLib + libs-as-roots)です。呼び出し元は `crates/xtask/src/main.rs:5850`(conformance)と `m8_evidence.rs:1305`。ここは **ORDINARY**。
- authoritative 系(`_authoritative_modules_at`、`_for_emit`、`_with_harness_lib_bundle`、`_harness_cached`)はすべて `ProgramSession::run_inner` (compiler lib.rs:1548-1575) から `project_checker_inputs` → `prepared.library_files()` で Program 所有の catalog lib を渡します。ここは **DEFAULT_LIBRARY**。`harness_lib_cache` と `LibraryPrefixCompletion` はバンドル再利用と診断スケジュールの違いだけでホストモデルを変えないので、代用にしない判断に同意します。`check_program_with_owned_libs_at` (1399) も DEFAULT。
- 配線は `check_program_with_prebound_libs_at_observed` に `lib_facts: ProgramFileFacts` を追加し、`lib.rs:2068` の定数をそれに置換。呼び出し 6 箇所(1348/1378/1423/1702/1724/1745)。authoritative の private `_cache_mode` (1648) は 3 箇所とも DEFAULT ですが、値は public wrapper 側から渡して境界で typed のまま保つのが良いです。

**ORDINARY 化で意味が変わる他の consumer(いずれも oracle 側の真値と一致する方向)**
- `should_skip_type_checking_file` (lib.rs:751): tsc も noLib では `libFiles` が空で skipDefaultLibCheck は何も飛ばさないので忠実化。lib の file diagnostics は局所変数 `lib_count` の `skip` で公開されないため出力不変のはず。
- `is_lib_type` (node_builder/context.rs:506、upstream 55452 は `host.isSourceFileDefaultLibrary`): 型展開の抑止判定が lib 型に対して false になります。メッセージ内の型展開が変わり得るので、負の対照が必須です。

**負の対照**
1. r18 の 38 行(T3 25 + T4 13)が exact に戻る。
2. legacy 経路で `@skipDefaultLibCheck` / `@skipLibCheck` を持つ corpus 行に差分がないこと。
3. `noErrorTruncation`/型展開を含むメッセージ行(`is_lib_type` の影響面)を含む full conformance の再生。方向は upstream 寄りなので gain は許容、loss はゼロであること。
4. フィルタ導入の動機になった owned/CLI 行(r8 の related-info 4 行、`import-helpers.json` の `comes from this index signature` 行)が DEFAULT のまま exact。
5. アダプタの entry family ごとの fact 選択を固定する単体テスト(legacy → ORDINARY、owned/authoritative → DEFAULT)。Some/None の代用を将来も禁止する唯一の pin になります。

### 2. for-of 20 の loop 名

**原因と最小案に同意します。upstream との照合で追加の見落としは 1 点だけ(下記 (i))。**

- upstream `convertForOfStatementHead` は `createForOfBindingStatement` で *元の宣言* を `updateVariableDeclaration(firstDeclaration, firstDeclaration.name, …, boundValue)` で更新し、その statement を `visitNode(binding, visitor, isStatement)` で **visit** します。そこから `visitVariableDeclarationList`(block-scoped で substitution 有効化)→ `visitVariableDeclaration(InLetDeclarationList)` に入り、名前は print 時の `substituteIdentifier` → `getGeneratedNameForNode(元の name)` の単一 cache で解決されます。
- Rust の非 pattern 分岐 (es2015.rs:12415-12451) は `create_variable_declaration_plain(name, bound_value)` を作るだけで visit しないため、名前が materialize されず、print 時の `print_state_generated_name` (595-608) が `generated_names_for_nodes[(source, original)]` を引けません。
- `colliding_declaration_name_substitute(name)?.unwrap_or(name)` の適用は、この分岐に限って upstream の visit と等価です。`get_generated_name_for_node` は `get_original_node` で parsed name をキーにし (1264-1265)、参照側 `substitute_expression_identifier` も `get_name_of_declaration` → 同じキーで引くので binding を共有します。初期化子があるため explicit initializer 判定は無関係、`visit_each_child` の対象は生成 temp のみ。新規状態は不要です。

**番号順の等価性**: tsc は print 順の最初の *要求* で採番します。内側 loop では head の `[v]` 参照が `var v = _c[_b]` より先に要求します。Rust の finalize walk は宣言名の位置で採番しますが、head 式と binding statement の間に print されるのは temp(`_b`, `_c`、別 base)だけなので同 base の衝突名は割り込めず、結果は一致します。この不変条件を対照で固定してください: 3 段ネストの `for (let v of [v])` + body の `let v`、および pattern head `for (let [v] of [[v]])`。

**追加で確認すべき点**
- (i) converted-loop 経路: upstream の `createConvertedLoop` は loop parameter を元の name node で作り print 時に substitute します。Rust の loop parameter 生成が同じく substitute を通っていなければ同種の fallback が起きます。対照: `for (let v of []) { let v; for (let v of [v]) { setTimeout(() => v); } }`。
- (ii) for-in は `visitEachChild` 経由で Rust 側も既に 5772 の substitute を通るため対象外。
- (iii) `first_original_declaration` が None の temp 分岐は影響なし。
