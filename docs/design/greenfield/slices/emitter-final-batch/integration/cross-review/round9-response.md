両行とも原因を確定しました。scratch は `/tmp/tsrs-probe/` (run.js、probe2-4.js、instrument 済み ts-instr.js) にあり、repo/vendor は無変更です。

## 1. jsExportMemberMergedWithModuleAugmentation2 (2300 ×2 欠落): 小修正

**根拠 (instrument した stack)**: 2 つの 2300 はどちらも
`createDiagnosticForNode ← error ← getInitializerTypeFromAssignmentDeclaration ← getWidenedTypeForAssignmentDeclaration ← getTypeOfSymbol` から出ています。`mergeSymbol` の `reportMergeSymbolError` は一度も呼ばれません (instrument で確認)。発生箇所は vendor `_tsc.js:56348-56446` 内の `module.exports = <object>` 結合分岐:

```js
if (s.flags & Value && exportedMember.flags & Value) {
  if (s.valueDeclaration && exportedMember.valueDeclaration
      && getSourceFileOfNode(s.valueDeclaration) !== getSourceFileOfNode(exportedMember.valueDeclaration)) {
    const exportedMemberName = tryCast(exportedMember.valueDeclaration, isNamedDeclaration)?.name || exportedMember.valueDeclaration;
    addRelatedInfo(error(s.valueDeclaration, Duplicate_identifier_0, name), createDiagnosticForNode(exportedMemberName, _0_was_also_declared_here, name));
    addRelatedInfo(error(exportedMemberName, Duplicate_identifier_0, name), createDiagnosticForNode(s.valueDeclaration, _0_was_also_declared_here, name));
  }
  const union = createSymbol(...)  // 以降は Rust と同じ
```

**Rust**: `annotate.rs:9096-9125` `combine_common_js_export_members` の VALUE&&VALUE 分岐は union 合成だけで、上のファイル不一致報告がない。2551 が一致しているので経路自体は到達済み。

**最小変更**: 9103 の直前に、`export_member` (augmentation 側 `s`) と `object_member` (object literal 側 `exportedMember`) 双方の `value_declaration` があり `file_index_of_node` が異なるとき、upstream と同じ 2 件を `_js` 系 helper で発行。error node は augmentation 側が `value_declaration` そのもの (VariableDeclaration → span は名前)、object 側が名前付き宣言なら `name_of_named_declaration`、なければ宣言。

**負の対照**: 同一ファイル (`module.exports = {a: 1}; module.exports.a = 2;` → union のみ)、type-only augmentation (`export type a = number` → VALUE でないので `merge_symbol` 分岐)、`.d.ts` augmentation (ファイルが異なるので報告)、`checkJs: false` (index.ts 側のみ残り test.js 側は plain-JS 抑止)。

## 2. jsDeclarationsTypedefAndLatebound (余分な 2345): 小修正、原因は cjs clone の late-bound member 取り込み欠落

**根拠 (TS probe)**:
- typedef があると `SomeObject` が LazySet.js の **module export** になり (`file.symbol.exports = [export=, SomeObject]`)、`getCommonJsExportEquals` (`49691-49714`) が `exports.size !== 1` で class symbol を `cloneSymbol` (transient、ValueModule、`recordMergedSymbol`)。typedef なしでは clone されず両参照が同一型。
- 2 つの class symbol が同じ ClassDeclaration を共有する。TS が両方の型に `__@iterator` を持たせる機構は 2 つ:
  (a) `resolveEntityName` が `getMergedSymbol` を返すので、clone 生成後の参照は clone 型に落ちる。
  (b) `getResolvedMembersOrExportsOfSymbol` (`57747-57762`): `symbol.flags & Transient && links.cjsExportMerged` のとき、各宣言の `decl.symbol` の resolved table を自分の resolved に fold する。instrument で、orig 先行順のとき clone 側は `lateBindMember cached=true` (late table に登録されない) のに、この fold で `__@iterator` を得ていることを確認。

**Rust**: `annotate.rs:4798-4815` は `combineSymbolTables` までで (b) の fold がない。`late_bind_member` 4949-4951 は upstream 同様キャッシュ時に早期 return し late table に入れない。`clone_symbol` は `record_merged_symbol` 済み (`merge.rs:177`)、`get_common_js_export_equals` は `cjs_export_merged` を設定済み (`modules.rs:7427-7430`)。つまり後から解決された親 (今回の観測では clone = `@type {LazySet}` 側) だけが `[Symbol.iterator]` を欠く。表示が `import("/.src/LazySet")` になるのは transient clone の symbol 表示で、別件 (cosmetic)。

**最小変更**: 4815 の `Ok(resolved)` 直前に upstream 57747-57762 を追加:
```
if symbol_flags(symbol) ∋ TRANSIENT && links.symbol(symbol).cjs_export_merged.is_some() {
  for decl in declarations { let original = node_symbol(decl);
    let table = if is_static { links.symbol(original).resolved_exports.resolved() } else { …resolved_members.resolved() };
    for (name, s) in table { match resolved.get(name) { None => insert s, Some(e) if e == s => {}, Some(e) => resolved.insert(name, merge_symbol(e, s, false)) } } } }
```
読み取りのみ (元 symbol の解決を強制しない) で upstream と同じ順序依存性になります。static 側 (`resolved_exports`) にも同じ分岐が必要。

**負の対照**: typedef なし (clone なし、分岐は不活性)、typedef あり + computed member なし、roots を `[LazySet.js, index.js]` に反転 (解決順の入替)、`static [Symbol.iterator]()` を `LazySet[Symbol.iterator]` で参照 (resolvedExports 経路)、`module.exports = LazySet; module.exports.extra = 1;` (typedef 以外で `exports.size > 1` にする別経路、同じ clone が起きる)。
