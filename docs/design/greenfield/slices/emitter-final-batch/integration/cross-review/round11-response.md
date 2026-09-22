以上で必要な材料は揃いました。追加リクエストはありません。

## Primary: commonjs 変換の exported destructuring (10 行) — module 専用 flatten の 5 欠落

Rust の commonjs 変換は共有 `flatten_destructuring.rs` ではなく独自の `flatten_module_*` (builtins.rs:8624-9029) を使っており、upstream `flattenDestructuringAssignment(All)` + `createAllExportExpressions` (`_tsc.js:111609-111650`) と次の 5 点で食い違います。TS 側は fixture を scratch で実行し、`createAllExportExpressions`/`createExportExpression` に渡る node range を印字して確認しました。

**1. `__read` (JS 差: adjacent-helper/es2015 の 4 行)**
- upstream 93534-93547: `level < ObjectRest && downlevelIteration` なら `value = ensureIdentifier(setTextRange(createReadHelper(value, restあり ? undefined : n), location), reuse=false, location)`。level All の module flatten は target に関係なくこれを通る。
- Rust `flatten_module_array_pattern` (8813-8850) にこの分岐がない。同じファイルの namespace 版 (12990-13033、13005-13025) には既にあるので、それを写す (`helpers::read()` 要求、`create_unscoped_helper_identifier(Read)`、引数 `[value, n?]`、`set_text_range(read, original)`、`ensure_module_destructuring_identifier(…, reuse=false, original)`)。
- importHelpers=true でも inline になる理由: upstream の tslib import 判定は module 変換冒頭の `collectExternalModuleInfo` で行われ、flatten が後から要求した helper は import されない。CommonJS では `tslib_1` namespace が既に存在する場合のみ `substituteHelperName` で `tslib_1.__read` になり、なければ inline。Rust は `get_external_helpers_module_name` + `EmitFlags::HELPER_NAME` 置換 (builtins.rs:1367-1394、2793-2800) で同じ規則を持ち、`__rest` の es2018/true 行が JS 一致していることがその証拠。対照に「`import x from "y"` + esModuleInterop で tslib_1 がある file の exported array rest」(TS: `tslib_1.__read`、inline なし) を追加。

**2. leaf assignment に element range を付けている (map 差の主因)**
- TS 実測: `export const {a, ...rest} = o` では `getExports(name)` が undefined で、leaf は range なしの `createAssignment(name, value)` (111624)。`exports.a` は印字時 substitution (`setTextRange(createPropertyAccess(exports, cloneNode(node)), node)`) で **target 識別子の range [38,39]** を持つ。cloneNode は range を持たない (実測 `clone=[-1,-1]`)。
- Rust `flatten_module_destructuring_target` の `_` arm (8737-8741) は `set_original_and_range(assignment, original=element)` → 余分な (15→39)/(17→41)。修正: leaf は `set_original_node` のみ、range なし。`create_module_export_assignment` (8864-8873) の `create_export_access_from_module_name` が作る `exports.x` に **target 識別子の range** を付ける。
- 例外: `getExports` が定義される assignment 経路 (`({a, ...rest} = o); export {rest}`) では `createExportExpression(exportName, expr, location)` (111805) が **外側の export assignment に location=element の range** を付ける (実測 `exports.rest = rest = …` が [50,58])。Rust は 8870-8873 の `plan.exports` ラップ時のみ element range を付ければ一致。内側 `rest = …` の `rest` は 8867 の `create_identifier(&plan.local_name)` ではなく visited target node (range [54,58]) を使う (TS 実測 (25→54))。

**3. `value` の clone が range を失う**
- TS は `value` node を再利用する (`createDestructuringPropertyAccess` 93647、`createRestHelper` 25816)。Rust は `clone_node(value)` (8889、8990) で `alloc_node(pos=u32::MAX)` (factory.rs:5522-5557) → `o` の map (12→52),(13→53),(39→52),(40→53) が消える。修正: これらの clone 直後に `set_text_range(clone, value)`。

**4. `__rest` の除外配列の location**
- upstream 93523: `createRestHelper(value, elements, computedTemps, pattern)` → 配列 literal の range = **pattern** (実測 (42→37),(47→49))。Rust 8800 は `element.original` を渡す (→ (42→41))。`flatten_module_object_pattern` に pattern node を渡し 8985 で pattern を使う。

**5. (確認事項)** es2017 の rest 行は ES2018 変換が先に `rest = __rest(…)` 宣言へ分割するため、module 側は `transformInitializedVariable` の非 pattern 枝 (111638-111648: `exports.rest` PA の range = `node.name`) を通る。Rust の (17→44),(25→44),(29→48) は一致しているので、この行は 2・3 の修正だけで閉じる。

**負の対照**: 非 export の `const {a, ...rest} = o` (module 変換対象外、不変)、`export let [x = 1, ...ys]` (default 値 + rest)、computed key `export const {[k]: v, ...r}`、tslib_1 あり/なしの `__read`/`__rest`、`export {rest}` 後置 export、target es5 の同形 (ES2015 変換が先行する経路が不変)。

## Secondary: `is_deeply_nested_type` を fallible `&mut` に

**置き場所**: `CheckerState` (engine.rs:4397) のまま署名を `fn is_deeply_nested_type(&mut self, ty, stack: &[TypeId], depth, max_depth) -> CheckResult<bool>` に変更し、同じ impl に upstream 67491-67505 の 2 helper を追加:
- `get_mapped_target_with_symbol(&mut self, ty) -> CheckResult<TypeId>`: `object_flags(ty) ⊇ INSTANTIATED_MAPPED (flags.rs:1833 = 96)` の間 `target = self.get_modifiers_type_from_mapped_type(ty)?` を取り、`target` に symbol があるか intersection で some member に symbol があれば `ty = target`、なければ break。
- `has_matching_recursion_identity(&mut self, t, identity) -> CheckResult<bool>`: 上で剥がしてから intersection なら any、それ以外は `get_recursion_identity == identity`。
- 本体先頭 (`depth >= max_depth` の内側) で `ty = get_mapped_target_with_symbol(ty)?`、stack 走査の一致判定を新 helper に置換 (現行 4419-4428 の intersection 手書きを置き換え)。

**呼び出し元 8 箇所の guard**:
- engine.rs 3422-3447 (`recursive_type_related_to`): push/depth++ の後で `?` すると 3475-3493 の depth--・flags 復元・`reset_maybe_stack` を飛ばす。2 つの判定を `outcome` を作る閉包の中 (3461) に移す (handler push の後でも意味は同じ: upstream は handler を参照しない)。既存 Err arm (3484-3493) がそのまま unwind を担う。閉包内は `self.st.is_deeply_nested_type(source, &self.source_stack, …)?` で field 分割 borrow が成立する。
- structural.rs 906 / 1215: worker 内なので `?` のみ。Err は上の arm に流れる。
- inference.rs 1005-1035 (`infer_reverse_mapped_type`、CheckerState 自身): `self.is_deeply_nested_type(source, &self.reverse_mapped_source_stack, …)` は借用衝突するので、閉包内で `let stack = std::mem::take(&mut self.reverse_mapped_source_stack)` → 判定 → 戻す、を両 stack で行い、判定 2 つ + `infer_reverse_mapped_type_worker` を 1 つの閉包にまとめる。閉包の後に pop 2 回と `reverse_expanding_flags = saved_flags` を無条件で実行し、その後 `?`。cache insert は Ok のみ (現行どおり)。
- inference.rs 2382-2393 (`invoke_once`、walker): `?` で可。2361-2364 の記述どおり Err では walker 全体が破棄されるので `visited` の CIRCULARITY 残留は問題にならない。
- engine.rs 4413 (再帰自呼び): `?`。

**リスク**: `get_modifiers_type_from_mapped_type` は制約解決を強制し得るが upstream も同じ。関係 core に触れるため、fixture (deeplyNested 242/499、Test1-3 が false のまま) に加え、`type Deep<T> = { [K in keyof T]: Deep<T[K]> }` の `Deep<any>` 終了、intersection modifiers 型 (`M<A & B>` vs `M<A & C>`) の対照、checker の full band 再生を必須にしてください。
