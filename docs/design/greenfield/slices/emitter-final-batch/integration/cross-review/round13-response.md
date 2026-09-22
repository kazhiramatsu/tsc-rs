追加リクエストは不要です。

## A. module 差分 6 行

**(1) assignment 4 行: comma 式に外側 assignment の range を付けている**
- 差分は wrapper 末尾の 1 map のみ: expected (48→58 / 69→51) = element end、actual (48→63 / 69→61) = 元の分割代入式全体の end。同じ生成列に後から記録された comma 式の end map が wrapper の end を上書きしている。
- 原因: `flatten_module_destructuring_assignment` の `self.set_original_and_range(expression, original)` (builtins.rs:8680)。upstream `flattenDestructuringAssignment` は `inlineExpressions(expressions)` を range なしで返し (93326)、`visitDestructuringAssignment` も setTextRange しない。宣言側 (8709) は既に range なしで、statement 側 6815 が upstream 111577 相当の range を付けているので整合。
- 修正: 8680 を `set_original_node` のみに変更。先頭 map は子 (paren / temp assignment / PA) が持つので upstream と一致する。

**(2) alias 2 行: binding pattern の direct export が末尾 `exports.b = exports.a;` を出さない**
- upstream `appendExportsOfVariableStatement` → `appendExportsOfBindingElement` (111707-111721) は pattern を leaf まで再帰し、各 leaf の export specifier ごとに `exports.<alias> = <name>` を追加 (111743-111762)。direct export 名は印字時 substitution で `exports.a` になる。
- Rust: direct + binding pattern 分岐 (6689-6700) が `continue` するため、identifier 経路の `alias_targets` (6753-6760 / 6774-6781) にも、非 direct 経路の `create_declaration_export_statements` (6805) にも到達しない。
- 修正: その分岐で `trailing.extend(self.create_declaration_export_statements(declaration)?)` を呼ぶ (6802 と同じ `appends_declaration_exports()` guard)。`declaration_export_plans` (7305-7346) は既に `binding_name_leaves` と `export_specifiers_by_local` を使うので leaf 列挙はそのまま使える。ただし value は 7409 の raw clone だと `exports.b = a` になる。direct export leaf (`!is_local_name(leaf)`) では `create_substituted_declaration_export_value(leaf)` (7382) を使い `exports.b = exports.a` にする。順序は leaf 順 → specifier 順、statement の後ろに追加 (6825 で先頭挿入される inlined statement の後) で upstream と一致。
- 対照: `export const {a} = o; export {a as b};` / 先行 alias (追加済み) / 入れ子 `export const {x: {y}} = o; export {y as z};` / 配列 `export const [p, ...q] = arr; export {q as r};` / 2 specifier `export {a as b, a as c}` / 非 export `const {a} = o; export {a as b}` (6805 経路が不変)。

## B. noEmit + sourceMap の H0 admission

**可**。根拠:
- gate は config.rs:1876-1878: `!(H0 || emitting && H1 projected) && value_requests_feature` で、no-emit (`emitting=false`) では `sourceMap: true` だけが「H0 no-emit driver 外」として拒否される。`H0_SUPPORTED_CONFIG_OPTIONS` (2110-2183) に `"sourceMap"` を 1 語追加すれば閉じる。他の emit/build/watch flag は同条件のまま。
- checker/program の消費なし: upstream checker は `sourceMap` を読まず、`noEmit` の `program.emit` は emitSkipped で終わる。Rust も `CompilerOptions.source_map` は projection 済みで no-emit command は emit を返さない (user 確認どおり)。printer/profile 契約に触れない。
- option 診断は既に揃っている: upstream `verifyCompilerOptions` の sourceMap 関連は 5053 (`sourceMap`+`inlineSourceMap`、124772)、5069 系 (`sourceRoot`/`mapRoot` は sourceMap or inlineSourceMap/declarationMap が必要、124863 ほか)。Rust `option_validation.rs:162-165, 325-344` に 4 violation とも実装され、`validate_compiler_options` は config projection (config.rs:3456) で no-emit でも走る。ただし相手側の `inlineSourceMap`/`mapRoot`/`sourceRoot` は H0 外のままなので、これらを含む config は依然 scope refusal (今回の変更で悪化はしない)。
- 不足 facts なし。H0 宣言診断や build へ広げる必要もない。

**注意 2 点**: (a) CLI には `--sourceMap` parse がない (cli.rs に該当なし) ので「普通 CLI」比較は config 経由か、先の module/target 名と同じ catalog 再利用で flag 追加が別途要る。(b) `sourceMap` を H0 に入れても `emitting=false` 時の意味は「値を持ち越すが消費しない」であり、H1 projected 判定 (2199) と重複登録になるが gate は OR なので副作用なし。

**最小対照** (config 経由、各 zero emitter activity と診断同一を確認):
- `noEmit:true, sourceMap:true` / `sourceMap:false` / sourceMap 省略 → 診断・writes 同一
- `noEmit:false|absent, sourceMap:true` → 既存 emitting 経路が不変 (map 出力あり)
- `noEmit:true, sourceMap:true, inlineSourceMap:true` → TS は 5053 を出して続行、Rust は inlineSourceMap の scope refusal (既存境界として記録)
- `noEmit:true, sourceMap:true, mapRoot:"m"` → TS OK、Rust は mapRoot refusal (既存境界)
- `noEmit:true, sourceRoot:"s"` (sourceMap なし) → TS 5069、Rust refusal (既存境界)
