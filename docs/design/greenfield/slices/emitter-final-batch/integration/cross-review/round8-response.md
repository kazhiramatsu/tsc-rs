追加読み取りは不要です。10 行の具体化結果です。

## 関連情報 4 行: 単一原因、1 箇所の guard 追加で閉じる

- **原因**: upstream `elaborateElementwise` は related info を付ける前に `host.isSourceFileDefaultLibrary(getSourceFileOfNode(decl))` で除外する (`_tsc.js:64181` index signature、`64188` property)。`isSourceFileDefaultLibrary` は `libFiles.has(path)` (`123562-123564`)。
- **Rust**: `elaboration.rs:426-486` `elementwise_elaboration_related` は両分岐でこの guard を持たず、doc comment (421-425) が「oracle host は noLib で lib を通常 root として渡すので常に偽」と根拠づけている。4 行はすべて whole-program route で default lib が有効な観測であり、この前提が崩れている (4 行とも Rust が **余分に** lib.es5.d.ts への 6500/6501 を付けている)。
- **修正**: 442 と 481 の前に `self.binder.file_facts(self.binder.file_index_of_node(decl)).is_default_library()` を判定して `None` を返す (`ProgramFileFacts::is_default_library`、`program.rs:705`; `node_builder/context.rs:534` に既存利用あり)。index-signature 側は upstream どおり `issuedElaboration` を立てず property 側へ落ちる順序も維持。
- **負の対照**: ユーザー宣言の property/index signature (related 維持)、`lib` option / `/// <reference lib>` 経由の lib 宣言 (抑止; `file_facts` がそれらも DEFAULT_LIBRARY か確認)、`noLib` + lib を明示 root にした場合 (upstream は ordinary root なので related 維持)。

## JS 6 行

**1. contextuallyTypedParametersOptionalInJSDoc: 小修正。**
- upstream `assignParameterType` (`78426-78440`) は `isOptional = !!declaration && !declaration.initializer && isOptionalDeclaration(declaration)`。
- Rust `functions.rs:533-539` は `question_token.is_some()` だけを見る。`is_optional_declaration` (`annotate.rs:8762-8802`) は既に JSDoc `[b]`/`JSDocOptionalType` を扱うので、そこへ差し替えるだけ。fn2 が通るのは型付き `@param {number} [b]` が `tryGetTypeFromEffectiveTypeNode` 経路 (8530 付近は正しい) を通るため。
- 負の対照: TS の `b?`、JS `@param {number=} b`、`@param [b=1]` (JSDoc default は initializer ではない)、initializer 付き `(a, b = 1)` (optional を付けない)。

**2. genericDefaultsJs / jsExtendsImplicitAny (2351 ×7): 小修正、同一原因。**
- upstream `getDefaultConstructSignatures` (`57981-57990`): `isJavaScript = isInJSFile(baseTypeNode)` で型引数個数不一致を許容し、`fillMissingTypeArguments(..., isJavaScript)` で any 埋め。
- Rust `annotate.rs:6253-6255` は `type_arg_count < min || > max` で無条件 `continue`、`6262` は `/*is_javascript*/ false` 固定。修正は `let is_javascript = self.is_in_js_file(base_type_node);` を条件に `||` で加え、`fill_missing_type_arguments` の第 4 引数に渡す (署名は `instantiate.rs:2747-2753`、`is_javascript_implicit_any`)。
- 負の対照: 同形の `.ts` (`class B extends A {}`、A<T>) は 2314 + 2351 が **残る**、JS `@augments A<number>` は無エラー、JS 過剰型引数 (D) は 2314 のみ、JS `new B().x` が any。

**3. jsdocTypedefBeforeParenthesizedExpression: parser の 1 条件。**
- upstream `parseExpressionOrLabeledStatement` (`33318-33336`) は文の**先頭 token が `(`** なら `hasJSDoc = false`。
- Rust は後段 pass (`parser.rs:9685-9699`) で「statement の expression が ParenthesizedExpression かつ同 pos」のときだけ skip。`(2 * 2) + 1;` は expression が BinaryExpression なので skip されず、statement と内側 paren (`jsdoc_comment_ranges` 460 で paren も収集) の両方に typedef が付き 2300。
- 修正: 判定を「`skip_trivia(text, pos)` の位置の文字が `(`」に変更 (upstream の `token() === OpenParenToken` と等価)。
- 負の対照: `(2*2);`、`(2*2) + 1;`、`((2*2));`、`(function(){})();` の直前 typedef、`x = (1);` (先頭が `(` でないので statement が保持)、labeled `a: (1);`。

**4. jsExportMemberMergedWithModuleAugmentation2 (2300 ×2 欠落): 盲目修正不可、要トレース。**
- Rust は `merge_one_module_augmentation` (`merge.rs:1040-1148`) で `resolve_external_module_symbol` → `merge_symbol` を upstream `47830-47875` どおりに行い、`get_common_js_export_equals` (`modules.rs:7366-7431`) も移植済み。2551 (string 側解決 = `combineValueAndTypeSymbols` 経路) は一致しているので、import 解決は正しい。
- 欠けているのは object literal property `a` (test.js:21) と `export const a` の **宣言衝突報告**のみ。upstream で `mergeSymbol(export=, aug)` は `Assignment` flag で衝突を素通りする (`(source.flags | target.flags) & Assignment`) ため、2300 の発生源は別 (候補: `getCommonJsExportEquals` の `merged.exports.set(... mergeSymbol(...))` `49706` が第二の解決経路で衝突、または checker 側 duplicate 検査)。読み取りだけでは特定できないので、TS 側で `module.exports = { a: "ok", b: 1 }` と augmentation なし/あり、`.d.ts` augmentation の 3 変種を観測して発生条件を絞ってから着手する行。

**5. jsDeclarationsTypedefAndLatebound (余分な 2345): 要調査、binder は upstream と一致。**
- binder の typedef 宣言 (`bind.rs:2994-3083`) は upstream `delayedBindJSDocTypedefTag` と同構造。差は checker 側: `@type {LazySet}` (require alias 経由) と `@param {LazySet}` (同ファイル) が **別の instance 型**になり、片方だけ late-bound `[Symbol.iterator]` を欠く。表示が `import("/.src/LazySet")` になる点から、alias 経由は `export=` を経た別 symbol (clone/merged) に落ちている可能性が高い。
- 必要な証拠: 両経路の `resolve_entity_name` 結果 SymbolId と `get_declared_type_of_symbol` の TypeId を dump し、typedef 有無で差が出る箇所 (`get_merged_symbol` / `cjs_export_merged` / `resolve_declared_members` の late-bind 対象宣言) を特定する。修正は小さい可能性があるが、原因未確定なので今回の train には入れない。

## 集計
- 即入れられる: 関連情報 4 行 (1 guard)、JS 3 行 (`assign_parameter_type`、`get_default_construct_signatures`、parser `(` 判定) = **7 行、4 変更**。
- 要トレース: jsExportMember… (2300 発生源)、jsDeclarationsTypedefAndLatebound (symbol 同一性)。
