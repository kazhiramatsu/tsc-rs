5 行すべてについて原因を局所的に特定できました。scratch (`/tmp/tsrs-probe/ts-rel.js`, `run.js`) に trace が残っています。

## 1. jsxIntrinsicDeclaredUsingTemplateLiteralTypeSignatures: Codex 仮説を確認、小修正

- upstream `elaborateElementwise` 64159: `specificSource = checkExpressionForMutableLocationWithContextualType(next, sourcePropType)` → `"smth"` を契約型 `"smth"` で再検査するので literal が保たれる。64169 で specificSource を検査し、通って `!== sourcePropType` なら 64170-64172 で元の property 型で再報告。
- Rust `calls.rs:6110-6189` は `source_type = get_type_of_symbol(source_property)` (widened `string`) をそのまま 6170 で報告。elaboration.rs:1277-1350 の object 経路が持つ「`push_contextual_type` → `check_expression_for_mutable_location(CONTEXTUAL)` → `pop` → specific/fallback」を 6148-6161 の literal elaboration 失敗後に写すだけで閉じる。JsxExpression initializer は children 経路 (591) と同じ扱い。
- 負の対照: `prop={10}` (不変 `number`)、`prop="smth"` (→ `"smth"`)、`prop={"smth" as string}` (→ `string`)、`prop={"smth" as const}`、target が `'literal' | 'other'` の union (message に union 表示)。

## 2. mappedArrayTupleIntersections: `get_actual_type_variable` 未適用

- TS trace: `RAT mapped={ [I in keyof T]: 1; } base=number[] & any[] result=1[]` → apparent 型が `1[]` になり `any[]` 制約を満たす。
- Rust `annotate.rs:2824-2839` `get_homomorphic_type_variable` は「getActualTypeVariable は Substitution 型が作れるまで identity」という stale コメントのまま、`Index { ty }` の型変数を直接 `TYPE_PARAMETER` 判定している。`Hmm<T>` の true 分岐では `T` が Substitution 型なので `None` → `mapped.rs:778-780` が generic mapped 型をそのまま返し、構造比較で `toString` 不一致の 2344 を出す。`get_actual_type_variable` は `indexed.rs:553` に実装済み。
- 修正: 2829 の後に `let variable = self.get_actual_type_variable(variable)?;` を挟む (upstream `getHomomorphicTypeVariable` と同形)。`instantiate_mapped_type` (instantiate.rs:1215) も同じ helper を使うので upstream と揃う。
- 負の対照: fixture T1–T5 (現状 exact のまま)、`T extends { x: string }` の substitution (generic のまま)、`as` 句付き mapped (nameType あり → 対象外)、`Hmm<[3,4,5]>` を `[1,1,1]` に代入。

## 3. deeplyNestedMappedTypes: 2 つの局所欠落

(a) **intersection constituent の写像** — `instantiate.rs:1299-1315` は任意の intersection を member ごとに写像して再交差する。upstream 63592-63594 は `isArrayOrTupleOrIntersection(t)` (全 member が array/tuple) のときだけで、それ以外は 63596 `instantiateAnonymousType` で intersection 全体を 1 つの mapped 型にする。これが `({} & {} & {} & { level1: … })` 表示の原因で、Input[]/Output[] 行 (2116/2315/2214) の関係形状も変えている。修正: 1299 の分岐に「全 member が array または tuple」guard。対照: `type E<T> = T extends infer O ? {[K in keyof O]: O[K]} : never` で `E<{a:1} & {b:2}>` が `{ a: 1; b: 2 }`、`E<string[] & number[]>` / `E<[1] & [2]>` は member-wise のまま。

(b) **InstantiatedMapped の recursion identity** — `engine.rs:4397-4440` `is_deeply_nested_type` に upstream 67467-67469 (`InstantiatedMapped → getMappedTargetWithSymbol`) と 67498-67505 (`hasMatchingRecursionIdentity` の同じ剥がし) がない。これは #55535 の修正そのもので、`Id<…>` の 6 段ネストが mapped symbol で同一 identity になり Rust では maxDepth 3 で Maybe → 代入可 → 242/499 の 2322 欠落。修正: `get_mapped_target_with_symbol` (67491-67497、`get_modifiers_type_from_mapped_type` mapped.rs:788 を使用) を port し、両所に適用。`is_deeply_nested_type` が `&self` 非 fallible なので modifiers 型取得の可否を確認。対照: fixture の Foo1/Foo2・Id2・RequiredDeep Test1-3、`type Deep<T> = { [K in keyof T]: Deep<T[K]> }` + `Deep<any>` (終了確認)。

## 4. recursiveConditionalCrash4: `is_error_type` の広さ

- TS trace: `GCT depth=97..100 node=104 check=StrIter.Prev<It> extends=StrIter.Iterator` — 未解決参照は upstream 60278-60292 で **per-alias の Any 系 error intrinsic** (`errorType` とは別 object) になり、62657 の `checkType === errorType` 判定を通過して再帰 → depth 100 で node 250 に 2589。
- Rust `conditional.rs:141` は `tables.is_error_type` (tables.rs:1336-1340 = `error || (Any && alias_symbol)`) を使うので、annotate.rs:3069-3105 が作る同じ per-alias error 型で早期 return → 再帰せず 251 が欠落 (394 の `Foo<T>` は unknown 経由なので影響なし)。
- 修正: 141 の 2 判定を `== self.tables.intrinsics.error` の identity に変更。対照: fixture (251+394)、循環 alias `type C = C extends string ? 1 : 0` (2456 のみ、本物の errorType)、`type U = Missing extends string ? 1 : 0` (2304 のみ)。他の `is_error_type` 利用箇所で upstream が identity のものは同型の grep 対象 (本行の範囲外)。

## 5. relationComplexityError: 2 つの局所欠落

- TS trace: f1 `T1 & T2 → T1` は非報告 call で `remaining=1998473` (ほぼ消費なし) で成功、f2 `→ T1 | null` は最初の非報告 call で `overflow=true remaining=-2` → cache に `Failed|ComplexityOverflow` → 報告 call が replay して 2859 ×2。
- (a) f1 が安い理由は upstream 65416-65424 の **union origin ショートカット**: source union の `origin` が intersection `T1 & T2` で、target `T1` が alias を持ち `origin.types` に含まれる → 即 True。Rust `engine.rs:2810-2814` はこれを「M4; M3 では None」として空にしているが、`intersect.rs:487-503` は cross-product 交差の origin を既に記録し、union は `origin` を持つ (ty.rs:301)。修正: 2 条件を port (source/target の `alias_symbol` と origin 型の member 判定)。これがないと Rust は 8192×4097 走査で overflow する。
- (b) `engine.rs:3320-3340` は overflow bit 付き cache entry を FALSE で返すだけで、upstream 65750-65756 の `reportError(Excessive_complexity | Excessive_stack_depth, typeToString(source), typeToString(target)); overrideNextErrorInfo++` がない → Rust は 2859 の代わりに generic 2322。修正: `report_errors && FAILED && (COMPLEXITY|STACK)` で該当 message を report し `error_state.override_next_error_info += 1` (1080)。
- 対照: f1 無エラー、f2 2859 ×2 (278 長 1 と 5)、小さな `T1`/`T2` で overflow しない通常 2322、alias なし target (`(T1) | null` を inline 展開) で origin 経路が効かない場合の挙動一致。

**分類**: 5 行とも局所修正候補あり。3(b) と 5(a) は relation core に触れるので、fixture 対照に加えて checker の full band (2xxx/conformance) の再生が必須。
