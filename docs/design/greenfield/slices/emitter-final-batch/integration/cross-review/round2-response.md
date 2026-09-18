確認が済みました。結論を簡潔に。

## 1. `visit_function_like_body` 最終形 (standard_decorators.rs:5662-5739)

**形は upstream の順序 (visitEachChild の子 visit → visitFunctionBody の環境終了と block 化 → 一回の update) と一致しており、承認できます。** 個別のリスク確認:

- **original chain**: `update_node(original, …)` (factory.rs:5626-5686) は `clone_node(original)` に pos/end を原点から写すので、二段だった chain が一段になる以外に差はない。temporaries なしの経路は `update_data` で従来と同一。
- **comment/text range**: 親 node の range は `update_node` が record から取り、`convertToFunctionBlock` 相当の `set_text_range(body)` (5717/5719) は変更前と同じ。子への metadata は visit 時に付くので順序変更の影響なし。
- **flags**: `flags_after_update(original, &data)` は `update_data` と同じ呼び方になった。`apply_parenthesizer_rules` は最終 data の body が Block なので `parenthesize_updated_arrow_concise_body` は no-op。合成 comma を再び包む経路は残っていない。
- **残る注意点 1 つ**: `visited?` (5689) の前に環境を pop しているので、visit が Err のときも environment stack は整合する。これは変更前と同じ挙動で問題なし。

追加確認として、`ClassStaticBlockDeclaration` と `FunctionExpression` の temporaries あり経路 (block は既存なので `merge_block_environment` のみ) が既存 suite に含まれることだけ見ておいてください。

## 2. comment guard の A/B 実験

**その手順で witnessless retirement の懸念は解消します。** 条件は「48 観測すべてを byte と map で exact 比較し、refusal 消失だけを成功と数えない」こと。既に明記されているので同意です。

抜けていそうな narrow control:
- `sourceMap: true` 変種。comment の所有移動は map 境界にも出るので、3 targets × removeComments × 8 shapes に sourceMap 軸がない。
- private-in の downlevel 経路 (ES2015 target の `__classPrivateFieldIn`) と native (ES2022+) の両方が 3 targets に入っているか。comment の付け替えは class_fields transform 側で起きる。
- 行コメント `#x // c` + 改行 + `in obj` と、JSDoc 形 `/** */`。
- optional chain 側は `a?.b /*c*/ !` (non-null) と `<T>/*c*/a?.b` (前置 angle)。
- 負の対照: 本当に未対応の shape が残った場合、typed error で止まることを assert する test (silent divergence を許さない)。

## 3. numeric target

両条件とも成立を確認しました。
- `ScriptTarget` は `i32` newtype で定数のみ (types/src/flags.rs:2137-2172)。`ScriptTarget::X =>` 形式の exhaustive match は crates 全体で 0 件、比較は `>=`/`<` のみ (checker/src/resolve.rs:589 が唯一の checker 側読み取り)。
- default lib は `library.rs:121-137` が `_ => "lib.d.ts"` で、upstream `_tsc.js:11255-11274` と同じ。JSON=100 も両者で `lib.d.ts`。

したがって **上限ガード撤去 (execute.rs:116、builtins.rs:285) が API 専用正規化より望ましい**。正規化は upstream にない値変換を導入し、TS6046 併存の観測と食い違う。下限 (ES3=0 は unset 計算) は維持。対照 99/100/1234 は妥当で、加えて declaration emit と `lib` 未指定時の default lib 選択を 1234 で確認しておくと安全です。

## 4. CLI の module/target 名

**保持すべき設計上の理由はありません。** `cli.rs:785-813` の制限は H2 初期の admission 履歴で、config/Program 経路は全 module と ES5 を受け付けています。推奨:
- 手書き表を増やさず、config 側の option catalog (`config_options.rs` の名前→値) を CLI からも再利用する。upstream は大小無視で `none/commonjs/amd/umd/system/es6/es2015/es2020/es2022/esnext/node16/node18/node20/nodenext/preserve` を受ける。
- `es5` は 6.0 で deprecation 診断 (TS5101 系、`ignoreDeprecations` で抑止) を伴うため、CLI 経由でも config 経由と同じ診断が出ることを frozen TS 出力で固定する。
- 検証は「同一 options の config emit と CLI emit が byte 一致」+ TS 凍結出力、の範囲に限定。CLI 全体の parity 主張はしない、という方針に同意します。
