# C02 / A41-BINDING — decorator の生成名と binding identity：現状対応表・inventory・設計

作成日：2026-09-16。状態：隔離候補（research）。
依頼：[h2-8a-generated-binding-claude-handoff.md](../h2-8a-generated-binding-claude-handoff.md)、
共通手順：[claude-high-difficulty-handoffs.md](../claude-high-difficulty-handoffs.md)。
結果は [REPORT.md](REPORT.md)、統合仕様は [INTEGRATION.md](INTEGRATION.md)。
本書は production readiness や accepted profile の admission を主張しない。
全 corpus の naming や SUPER の完了を本候補の結果から推論しない。

## 1. 開始点と保存物

| 項目 | 値 |
| --- | --- |
| worktree / branch | `~/dev/tsc-rs-generated-binding` / `draft/h2-8a-generated-binding` |
| 開始 SHA | `ccb6661c16f75cd6824e9fedb9281be68d01e542`（origin/main。C01 merge `7df1a8ed1`、SUPER PR #523 head `f64443300`、PR #524 の CI 改修を含むことを `merge-base --is-ancestor` で確認） |
| toolchain / pin | rustc 1.93.0、cargo 1.93.0、node v25.2.1、python 3.14.2、`_tsc.js` `1c59e77a…ddd3e3`、`typescript.js` `56917765…12be39`（[records/start.txt](records/start.txt)） |
| 入力 manifest | `crates/compiler/tests/fixtures/decorator-binding-inputs.json`：**768** ID（128 variant × ES2015/ES2022/ESNext × set/define）を native 実行前に固定。生成器 `scripts/generate-decorator-binding-inputs.mjs` |
| observer | `scripts/observe-decorator-bindings.mjs pipeline|direct --write|--check`（各 case を同一 process で 2 回観測して一致を要求、`--check` は別 process で byte 一致） |
| 凍結 expected | `crates/compiler/tests/fixtures/decorator-binding.json.zst`（複合 command）、`crates/emitter/tests/fixtures/decorator-binding-direct.json`（direct 96 row：synthetic 48、global 22、lifecycle 26） |
| Rust target | `crates/compiler/tests/decorator_binding_pipeline_contract.rs`（`TSC_RS_DECORATOR_BINDING_CASE_SET` 選択、1 test）、`crates/emitter/tests/decorator_binding_contract.rs`（1 test、`TSC_RS_DECORATOR_BINDING_REPORT_DIR` で native 観測を保存） |
| witness runner | `scripts/witness.py decorator-binding-pipeline --case …`、`scripts/witness.py decorator-binding --all` |
| 既存の focused 入口（開始 SHA） | `--lib target_bindings` 5 tests、`--lib generated_bindings` 19 tests、`witness.py printer --all` 10 tests（[records/before](records/before/)、全て exit 0；printer suite は C03 既知差分 `recover-new-unique#op2` を KNOWN として通過） |
| 上流 source span | [records/source-spans.json](records/source-spans.json)（49 span、行範囲と SHA-256） |
| 候補 patch | [records/candidate.patch](records/candidate.patch)（`crates/` + `scripts/`、開始 SHA からの差分、binary fixture 込み）、`candidate.patch.sha256`、`candidate.stat` |
| capture の要約 | 最終候補：[records/after7/](records/after7/)（統合レビュー後、`capture-diff-full.txt`、`pipeline-binary.sha256`）；レビュー前の最終 [records/after5/](records/after5/)（`capture-diff-subsets.txt`、`capture-diff-full.txt`）；第 2 候補の family subset [records/after4/capture-diff-subsets.txt](records/after4/capture-diff-subsets.txt)；第 1 候補：[records/before/capture-diff.txt](records/before/capture-diff.txt)、[records/after/capture-diff-subsets.txt](records/after/capture-diff-subsets.txt)、[records/after/capture-diff-full.txt](records/after/capture-diff-full.txt)（生の capture は repo 外、`TSC_RS_H2_8A_CAPTURE_WRITES_DIR` で再生成） |

## 2. Source graph（pinned `_tsc.js`、span = 行範囲、hash = span の SHA-256）

生成名の意味論は factory（flags を運ぶ）と printer（lazy に綴りを決める）に分かれる。
決定表は `makeName`（Unique/Auto/Loop）と `generateNameForNode`（Node 由来）で、
collision domain は三つ：`isFileLevelUniqueName`（parse census + `hasGlobalName`）、
`isUniqueName`（それに `reservedNames` stack と file-wide `generatedNames` を加える）、
private 名（`reservedPrivateNames`、`privateNameTempFlags`）。

| 入口 | 行 | SHA-256（先頭 16） | 意味 |
| --- | --- | --- | --- |
| isFileLevelUniqueName | 12907-12909 | `86b2e1d4e23cbbea` | `!hasGlobalName(name) && !sourceFile.identifiers.has(name)`（generated 名を見ない） |
| createBaseGeneratedIdentifier / createUniqueName / getGeneratedNameForNode | 21598-21608 / 21647-21651 / 21652-21666 | `ebf2399948410fb6` / `435d2c5a71fc6e16` / `1953cada220809cd` | autoGenerate {flags, id, prefix, suffix}。FileLevel は Optimistic 必須。Node 由来は `generated@<id>` + original |
| createTempVariable / createLoopVariable | 21626-21634 / 21635-21646 | `ec6638a1aefe18f3` / `2bf7a1a2170a15e2` | Auto / Loop、`reservedInNestedScopes` |
| createUniquePrivateName / getGeneratedPrivateNameForNode | 21688-21692 / 21693-21706 | `7e3230d0370b4602` / `df9752959d0646c1` | private 名は常に ReservedInNestedScopes |
| getNodeForGeneratedName / formatGeneratedNamePart / formatGeneratedName | 28084-28102 / 28103-28112 / 28119-28124 | `15fe0551d80fa459` / `56f6b2070c603bf9` / `6554f97e86964e34` | original chain を辿る cache key。prefix/suffix の合成 |
| hasGlobalName（checker） | 88396-88398 | `8522b19ce36e625d` | `globals.has(escapeLeadingUnderscores(name))`：script file の top-level と lib、意味を問わない |
| getHelperVariableName / createHelperVariable | 99214-99221 / 99222-99224 | `f7a46976b189a964` / `d5cb7376dbdffdd1` | `_[static_][private_][get_/set_]<name>_<suffix>`、Optimistic\|ReservedInNestedScopes |
| createClassInfo | 99241-99318 | `9df4966871c15c88` | `_metadata`（FileLevel）、`_classThis`（static private/auto-accessor があれば ReservedInNestedScopes、無ければ FileLevel）、`_staticExtraInitializers`/`_instanceExtraInitializers`（FileLevel） |
| transformClassLike | 99319-99616 | `52ad986c5f808ec7` | `getLocalName`（class_1/default_1）、`_classDecorators`/`_classDescriptor`/`_classExtraInitializers`/`_classSuper`（FileLevel）、`_outerThis`（Optimistic のみ = file-wide `generatedNames`） |
| visitReferencedPropertyName / visitComputedPropertyName | 100345-100362 / 100369-100375 | `480f124559721622` / `4676f56ef719a62c` | `getGeneratedNameForNode(computedName)` + hoist、`__propKey`、pending の吸収 |
| transformDecorator | 100554-100569 | `98ffa88aa2a9fe00` | access 式は `createCallBinding(..., cacheIdentifiers)` の temp |
| writeBundle / writeFile2 / beginPrint / endPrint / reset | 117058-117071 / 117072-117081 / 117082-117084 / 117085-117089 / 117117-117141 | `9296d49978401c91` / `9deeb25a93b0e819` / `a3e60017c3d312ed` / `e94d00a44500fd4f` / `88bcd8c0391f9585` | 名前表の寿命：`reset()` は成功した write/print の末尾でのみ走る（`finally` 無し）。bundle は source をまたいで `generatedNames` を保持 |
| pushNameGenerationScope / popNameGenerationScope / reserveNameInNestedScopes / reservePrivateNameInNestedScopes | 120480-120492 / 120493-120502 / 120503-120508 / 120509-120514 | `e1fa362147667c49` / `9285b617947c4c38` / `25232240e2f0c1e4` / `b1d3d7074e5186ac` | scope stack。ReuseTempVariableScope は private 部分だけ push |
| generateNames / generateMemberNames / generateNameIfNeeded | 120515-120599 / 120600-120614 / 120615-120623 | `6cbf565498a22f91` / `cbbb4181555bd319` / `ec28b61c02d9715` | scope 入口で宣言名を先に決める（function 本体には降りない） |
| generateName / generateNameCached | 120624-120632 / 120633-120637 | `80cad260d417933c` / `f9eefc1a56d9084c` | `autoGeneratedIdToGeneratedName` / `nodeIdToGeneratedName` cache |
| isUniqueName / isReservedName / isFileLevelUniqueNameInCurrentFile / isUniqueLocalName | 120638-120640 / 120641-120664 / 120665-120667 / 120668-120678 | `9313bd45283d2066` / `321a51c5f131eff2` / `cdfb9e1e2524e9ef` / `cb6668cf96909d7c` | 三 domain の判定 |
| getTempFlags / setTempFlags / makeTempVariableName | 120679-120688 / 120689-120702 / 120703-120740 | `17fbdd3f39ab9c3d` / `f396da619c82bded` / `6928789997987359` | `_a`…（`_i`/`_n` を飛ばす）、prefix/suffix ごとの counter |
| makeUniqueName / makeFileLevelOptimisticUniqueName | 120741-120779 / 120780-120795 | `5015e05e44403f4e` / `b92012a553ccb832` | optimistic は base のまま、else `base_1`…；scoped → reservedNames、private → reservedPrivateNames、他 → `generatedNames` |
| generateNameForModuleOrEnum … generateNameForMethodOrAccessor | 120796-120875 | `a01bf8ac…` … `8af56ec9…` | Node 由来の各 arm |
| generateNameForNode / makeName | 120876-120942 / 120943-120977 | `07482e59be2df336` / `bee33a4f128a3641` | 決定表 |

## 3. Binding inventory（decorator が生成する binding の種類）

「scope」は宣言が置かれる runtime scope、「owner」は最終綴りを決める Rust 側。
FileLevel = `PreferredNameDomain::FileLevelOptimistic`（finalizer `file_level_unique_name`）、
Scoped = `ScopedOptimistic` + `reserve_in_nested_scopes`、FileWide = `ScopedOptimistic` 非 reserved
（`allocate_planned_file_wide_optimistic`）、Temp = ordinary temp（`allocate_temp_with_policy`）、
Numbered = `allocate_source_numbered_with_policy`。

| # | binding | source producer | flags / domain | scope | 参照箇所 | Rust producer | 命名 owner |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `_metadata` | createClassInfo 99242 | Optimistic\|FileLevel | class IIFE の static block（`const _metadata = …`） | 全 `__esDecorate` context の `metadata:`、`Object.defineProperty(_classThis, Symbol.metadata, …)` | `allocate_file_level_name("_metadata")`（`standard_decorators.rs:1797`） | FileLevel |
| 2 | `_classThis` | createClassInfo 99257 | `needsUniqueClassThis`（static private / static auto-accessor）なら Optimistic\|ReservedInNestedScopes、他は Optimistic\|FileLevel | class IIFE `let _classThis;` | `static { _classThis = this; }`、`_classDescriptor = { value: _classThis }`、`C = _classThis = …`、`__runInitializers(_classThis, …)`、`__setFunctionName(_classThis, …)`、return | 1614-1617：`allocate_name` / `allocate_file_level_name` | Scoped / FileLevel |
| 3 | `_classSuper` | transformClassLike 99369 | Optimistic\|FileLevel | class IIFE `let _classSuper = <heritage>;` | extends 句、`_classSuper[Symbol.metadata]` | `prepare_class_super` 2363 | FileLevel |
| 4 | `_classDecorators` / `_classDescriptor` / `_classExtraInitializers` | 99350-99352 | Optimistic\|FileLevel | class IIFE | `_classDecorators = [dec]`、`__esDecorate(null, _classDescriptor = {value: _classThis}, …)`、`__runInitializers(_classThis, _classExtraInitializers)` | 1607-1610 | FileLevel |
| 5 | `_staticExtraInitializers` / `_instanceExtraInitializers` | 99271 / 99279 / 99285 | Optimistic\|FileLevel | class IIFE | `__esDecorate(…, null, _staticExtraInitializers)`、`__runInitializers(this, _instanceExtraInitializers)`（constructor / 先頭 field） | 1775 / 1780 | FileLevel |
| 6 | member helper `_[static_][private_][get_/set_]<name>_{decorators,initializers,extraInitializers,descriptor}`（name 不能なら `class`/`member`） | createHelperVariable 99222 | Optimistic\|ReservedInNestedScopes | class IIFE `let _x_decorators;` | pending `_x_decorators = [dec]`、`__esDecorate(…)`、`__runInitializers(this, _x_initializers, 1)`、descriptor の get/set | 1636-1656 `allocate_name`（stem は `decorator_helper_stem`） | Scoped |
| 7 | `_outerThis` | transformClassLike 99398 | Optimistic のみ（file-wide `generatedNames`） | class definition statements の先頭 `let _outerThis = this;` | pending expression 内の lexical `this` | `DecoratorClassThisRewriter` 6943 `allocate_preferred_optimistic` | FileWide |
| 8 | computed key cache temp `_a` | visitReferencedPropertyName 100356（`getGeneratedNameForNode(computedName)` + hoist） | Node 由来 → ComputedPropertyName arm → `makeTempVariableName(Auto, reservedInNestedScopes=true)`、node id で cache | class IIFE hoisted `var _a;` | 宣言 `[_a = __propKey(key())]`、context `name: _a`/`access`、accessor descriptor `obj[_a]`、`__setFunctionName(_classThis, _a)` | `visit_referenced_property_name` 2632：`hoist_temp_variable(true)`、named-evaluation temp と同じ binding を `hoist_existing_temp_variable` で再宣言 | Temp（reserved in nested） |
| 9 | decorator call-binding temp | transformDecorator 100561（`createCallBinding(cacheIdentifiers)`） | Auto、非 reserved | class IIFE hoisted | `(_a = ns).dec.bind(_a)` | `transform_decorator_expressions` → `hoist_temp_variable(false)` | Temp |
| 10 | super / update / parameter temps | 100115 / 100267 / 100284 / 100320 / 100326 / 100401 | Auto | — | — | SUPER slice の owner（既存 witness 670+） | Temp（本 slice の対象外） |
| 11 | class reference `class_1` / `default_1` | transformClassLike 99328 `getLocalName` → generateNameForNode ClassExpression/ClassDeclaration arm → `makeUniqueName("class"/"default", 非 optimistic)` | file-wide numbered（`generatedNames`） | class IIFE `var class_1 = class …; return class_1 = _classThis;` | `class_1 = _classThis = _classDescriptor.value` | `allocate_generated_reference_name` + `TargetBinding::allocate_numbered`（1596 / 5240 / 5392） | Numbered |
| 12 | named-evaluation temp | transformNamedEvaluation（`getGeneratedNameForNode(updatedName)`） | Node 由来（#8 と同じ node → 同じ cache entry） | #8 と同じ | `__setFunctionName(_classThis, _a)` | `prepare_property_named_evaluation` 2431 | #8 の binding を共有 |
| 13 | private accessor 記憶域 `#<name>_accessor_storage`（class-fields 97168、`getGeneratedPrivateNameForNode(name, undefined, "_accessor_storage")`）、descriptor の `getGeneratedPrivateNameForNode(node.name)`（100654 / 100678） | private Node 由来 | private domain（reservedPrivateNames） | class 本体 | `this.#x_accessor_storage` | `allocate_private_storage` / `allocate_computed_private_storage`（6640 / 6656）、class-fields 1795-1818 | private（SUPER followup2 `private-name` で観測済み） |

## 4. 現状対応表（旧要求 → 現行 owner → 既存 witness → 分類）

分類：**A** 既存観測済み、**B** 追加対照が必要（本 slice で追加）、**C** 差を再現（本 slice で修復または記録）、
**D** 到達前提が未成立（Rust に同じ API が無い。direct control のみ / 記録のみ）。
「結果」欄は [REPORT.md](REPORT.md) §2 の before/after 集計を指す。

| # | 旧要求（handoff §実装手順 / 必須 witness） | 現行 owner | 既存 witness | 分類 | 本 slice の対照 |
| --- | --- | --- | --- | --- | --- |
| 1 | parse census（`isFileLevelUniqueName` の `SourceFile.identifiers`） | `ParsedSourceIdentifierNames::collect`（parse 所有 node の Identifier text）、visitor `file_level_names`、print 時 finalizer `PrintSource(reserved)` | unit `parsed_identifier_snapshot_retains_erased_file_level_collisions`、`print_generated_name_reserves_identifiers_outside_the_printed_node` | B | `parse-census` 29 variant：宣言 / property key / string のみ / comment のみ / JSDoc tag のみ / type 位置 / 入れ子 scope / escape / `_1` 既取得 / temp 列 / label |
| 2 | global 名（`hasGlobalName`）の hit / miss / error | `file_level_unique_name`（FileLevel）、`allocate_numbered_name_with_global_oracle`（numbered）、finalizer の oracle loop（scoped / temp / private）；compiler は `ResolverGlobalNameOracle`、checker `emit_has_global_name` = `globals.contains_key` | なし（decorator 経路）。`h2_7d_*` の resolver stub は常に false | B | `global` 21 variant（script `let`/function/type alias/namespace/`declare let`/block-scoped/numbered chain/temp chain、module local、`declare global`、script+source）＋ direct `global` 22 row（hit / hit-chain / miss / error × 5 domain、parsed+hit） |
| 3 | synthetic 名（earlier pass が同じ綴りを合成） | transformer-time finalize は `collect_untagged_identifier_texts`（synthetic を含む）、print 時は parse census のみ。planned 綴り（node text）が両者を橋渡しする | unit `print_generated_name_ignores_an_ordinary_synthetic_sibling`（print 時のみ） | B / C（direct） | direct `synthetic` 48 row：`_metadata`/`_classThis`/`_x_decorators`/`_a`/`_outerThis`/`class_1` を before transformer で合成、`print`（oracle 無）と `print_javascript_with_global_names`（compiler 経路）の両方 |
| 4 | nested reservation（ReservedInNestedScopes、sibling の再利用） | `reserve_in_nested_scopes` metadata → `GeneratedBindingScopes::names_reserved_in_descendants`；visitor は `used_names` を class ごとに保存・復元（1519 / 2211） | SUPER `phase-order/decorator-and-heritage-nested` 等、unit `descendant_reserved_preferred_bindings_still_reuse_in_siblings` | A + B | `nested` 15 variant：static block / method / constructor / field initializer / decorator 式 / heritage 内の inner、sibling（同名 member、`_outerThis` ×2、class expression ×2、default + expression、computed temp ×2）、通常関数 scope、三重入れ子 |
| 5 | reserved（`needsUniqueClassThis` の domain 差） | 1614-1617：scoped `_classThis` は `allocate_name`（ScopedOptimistic）、file-level は `allocate_file_level_name` | SUPER `handoff/static-private-*`（単独 class） | B / C | `reserved` 28 variant：10 trigger × 2 順序、scoped+scoped、outer/inner の 3 組合せ、private accessor 記憶域の nested / sibling / 同 stem、parsed `_classThis` + scoped |
| 6 | computed key cache の identity（宣言・代入・context・accessor 名が同じ binding） | `visit_referenced_property_name`（1 binding、`hoist_existing_temp_variable` で再宣言）、class-fields `computed_name_binding`（`binding_of_generated_identifier` で同じ id を再利用） | SUPER extra `phase-order/decorated-field-then-*`、followup `decorated-field-no-later-computed-name-declaration`、retained 530 | A + B | `computed` 16 variant：static/instance accessor、method、getter+setter、field、複数 key、user `_a`、undecorated computed の吸収、class expression、member-only、literal+expression、decorator temp 併用、static block 内 inner |
| 7 | ordering（decorator 式と heritage の双方の生成名、pending、constructor を含む順） | `transform_class_like`（decorators → `prepare_class_super` → members → pending） | SUPER phase-order 36+、extra 24 | A + B | `ordering` 6 variant |
| 8 | lifecycle：bundle（別 source、`generatedNames` の持ち越し） | `finalize_bundle_generated_binding_names_for_print`（bundle_generated_names） | `system-generated-names.json` 12 row（System bundle） | A + B | `lifecycle` bundle 7 variant（System outFile、`ignoreDeprecations: "6.0"`）：`_outerThis`/class_1/default_1 の連番、file-level→scoped の跨ぎ、per-file census、scoped 再利用、computed temp |
| 9 | lifecycle：CommonJS module publication | module transform（CJS） | SUPER は module ESNext のみ | B | `lifecycle` cjs 6 variant |
| 10 | lifecycle：再 print / clone / dispose | printer `finalize_generated_names_for_print`（print ごとに再確定）、`clone_node`（`merge_from` が binding id を複製）、`dispose`（`clear_session_metadata`） | unit `generated_name_state_starts_fresh_for_each_print`、C01 lifetime 10 row（literal） | B | direct `lifecycle` reprint 5 / clone 5 / dispose 2 |
| 11 | lifecycle：failure 後の名前表の寿命（C03 `x_2` / `x_1`） | Rust は print 前に transformation 上で eager 確定、失敗しても次の print で fresh；上流は printer 所有の表が `reset()` されるまで残る | `printer_failure_contract` KNOWN_DIVERGENCES 1 row | C | direct `lifecycle/failure` 14 row：numbered / file-level / scoped / file-wide / temp / node-derived(same, other) × before/after（§6 の state 寿命表） |
| 12 | lifecycle：別 source file で同じ node を print（`printNode(hint, node, otherSourceFile)`） | Rust の StandaloneNode request は node の source を使い、別 source を受ける API が無い | — | D | 記録のみ（§6.4） |
| 13 | private 名 domain（accessor 記憶域の nested / sibling） | `allocate_private_storage`（used 集合）、finalizer `PrivateTemp` | SUPER followup2 `private-name` 6 variant | A + B → C（nested） | `reserved/private-accessor-storage-*` 3 variant |

計測後の分類（REPORT §2 / §3）：行 1（parse census）、2（global；direct 22 row と複合 21 variant）、
4（nested）、6（computed）、7（ordering）、9（CommonJS の生成名）は before で全 variant 一致 = **A**
（追加対照が既存 owner の正しさを確認）。行 3（synthetic）は direct で差を再現 = **C**（決定 2・3）。
行 5（reserved）は sibling の domain 差 = **C**（決定 1）、行 8（bundle）は file-level / file-wide の
持ち越し = **C**（決定 1）と hoisted default 名 = **OUT-OF-SCOPE**（system.rs）、行 13 の nested
private = **C**（決定 6）。行 11（failure 後）と行 10 の dispose = **C（typed KNOWN）**、行 12 = **D**。

## 5. 設計決定

再現した差（REPORT.md §2）ごとの決定。候補 3 file（決定 1・2・3・6・14・15）の外に及ぶ決定 4・7〜13 は
分離差分として INTEGRATION.md §1 に file 単位で列挙する（handoff の「printer / transform.rs / resolver は
設計メモ + 分離差分」に従う）。

1. **`generatedNames` を一つの file-wide 集合として持つ（`generated_bindings.rs`）。**
   tsc は `makeUniqueName` が `scoped` / `privateName` 無しで返した綴り
   —— FileLevel optimistic（`_metadata`、`_classThis`、`_classSuper`、`_classDecorators`…）、
   file-wide optimistic（`_outerThis`）、numbered（`class_1`、`default_1`、`x_1`）——を
   `generatedNames` に加え、`isUniqueName`（scoped optimistic、temp、numbered、private temp の
   hoisted 変数）がそれを参照する。`isFileLevelUniqueName` は参照しない。
   Rust の `GeneratedBindingScopes` は file-level 名を「現在 scope と子孫に予約」するだけだったので、
   **sibling** class の scoped `_classThis`（static private / static auto-accessor）が
   file-level `_classThis` の後で `_classThis` のまま（上流 `_classThis_1`）になり、bundle でも
   file-level / file-wide 名が次の source に持ち越されなかった（上流 `_classThis_1`、`_outerThis_1`）。
   `generated_names` を追加し、`reserve_planned_file_level_optimistic_with_policy` /
   `reserve_file_wide` / `allocate_source_numbered_with_policy`（非 reserved）が書き込み、
   `reserve_in_current` / `reserve_in_source` が拒否条件に加える。bundle は
   `seed_generated_names` で前 source の集合を受け取り、finalize 末尾で集合全体を返す
   （従来は numbered 名だけを返していた）。
2. **transformer-time finalize の reserved set は parse census（`target_bindings.rs`）。**
   `GeneratedNameReservedSetPolicy::TransformerRoot` は `collect_untagged_identifier_texts`
   （synthetic な plain identifier を含む）を使っていた。tsc の三 domain はいずれも
   `SourceFile.identifiers` しか見ないので、earlier pass が `let _x_decorators = 0;` を合成すると
   Rust だけ `_x_decorators_1` になり（direct `synthetic` の 30 row）、さらに eager に決めた綴りが
   node text（planned）として print 時の再確定に持ち込まれ、compiler 経路
   （`print_javascript_with_global_names`）でも `_outerThis_1` が残った。
   `ParsedSourceIdentifierNames::collect` に統一する。他 transform（es2015 / es2017 / es2018 /
   es2021 / generators）は `_this` / `_super` / `_newTarget` を tagged binding で作るので、
   synthetic plain identifier に依存する生成名は無い（`records/` の調査 grep）。
3. **decorator planner の `used_names` は parse census から始める（`standard_decorators.rs`）。**
   `collect_identifier_texts`（arena 全体）ではなく `file_level_names` の複製から始め、
   planner 自身が計画した綴りだけを加える。nested / sibling / 同 class 内の衝突判断は変えない。
4. **failure 後の名前表の持ち越しを実装する（§6.4；`printer.rs` / `printer/bundle.rs` /
   `transform.rs` / `target_bindings.rs` / `generated_bindings.rs`、分離差分）。** 当初は
   typed KNOWN として残す予定だったが、残作業を減らす方針で実装した。C03 の
   `recover-new-unique#op2`（`x_2`）と direct `lifecycle/failure` の after-fault 5 row が exact に
   なり、両 contract の KNOWN と `printer-failure-known-native.json` の行を撤去した。dispose 2 row
   だけが typed KNOWN として残る。
6. **private 生成名は enclosing class に予約される（`standard_decorators.rs`）。**
   tsc は `getGeneratedPrivateNameForNode(name, undefined, "_accessor_storage")` を
   `makeUniqueName(…, privateName = true)` で決め、`reservePrivateNameInNestedScopes` に入れる。
   入れ子の decorated class が同じ private accessor 名を持つと inner は `#a_1_accessor_storage`、
   sibling は `#a_accessor_storage` を再利用する。Rust の `allocate_private_storage` は class ごとの
   `used_private`（parsed private 名 + 自分の記憶域名）しか見なかった。`reserved_private_generated_names`
   （visitor field）に enclosing class の `backing_name` を積み、inner の `used_private` に加える。
   class の終わりで復元するので sibling は影響を受けない。class-fields 所有の undecorated
   `accessor #a` との cross-transform 予約は対象外（REPORT §5）。
7. **bundle の parse-node metadata に internal flags を含める（`factory/parsed_metadata.rs`、候補 3 file 外・分離差分）。**
   （第 2 の原因：同じ bundle で parsed Identifier に `comment_range: EndOnly` が付く
   —— printer の comment ownership が parsed node に書く注釈 —— も snapshot が拒む。生成名の
   owner ではなく、A-INT3-CS / bundle metadata 可搬性の面として REPORT §3 T1 に残す。）
   T1（REPORT §3）の原因：decorator transform は tsc と同じく parsed `static #p` member に
   `InternalEmitFlags.TransformPrivateStaticElements`（bit 32）を stamp する。Bundle root は parse
   SourceFile を持たず、JS transformation の dispose 後も parse-node の emitNode（flags / internal
   flags / sourceMapRange …）が declaration emit に残る（`transformNodes.dispose` は SourceFile root
   だけ `disposeEmitNodes`）。Rust の `snapshot_parsed_emit_metadata` は flags / typeNode /
   constantValue だけを可搬にし、それ以外を typed refusal にしていたので、System bundle ＋ static
   private ＋ decorated class の 5 combo が `ParsedEmitMetadataNotPortable` で止まっていた。
   `internal_flags` を packet に加え、restore で戻す。
8. **System の hoisted 宣言は binding identity を運ぶ（`builtins/system.rs`、分離差分）。**
   `collect_declaration_list_hoists` が宣言名を文字列で集め、`var` list の identifier を
   `create_identifier(text)` で作り直していたため、decorated default class の `default_1`
   （numbered binding）が 2 番目の bundle source で本体は `default_2`、hoist は `default_1` になった。
   名前 node を集め、generated binding を持つものは `generated_bindings` に登録して、
   `create_identifier` が同じ binding metadata を書く（tsc は同じ generated identifier node を hoist する）。
9. **CommonJS の `export default` 文は `setTextRange` のみ（`builtins.rs` CJS visitor、分離差分）。**
   `createExportStatement` は `setTextRange(statement, node)` だけで `setOriginalNode` しない。
   Rust は `set_original_and_range` で original を結び、decorator が ExportAssignment に置いた
   sourceMapRange（`moveRangePastDecorators`）が merge され `exports.default = default_1;` に
   mapping が出ていた（上流は無し）。range のみ複製する。
10. **generated 名を持つ property の初期化文は leading mapping を出さない（`class_fields/downlevel.rs`、分離差分）。**
    `moveRangePastModifiers(property)` は与えられた property 自身の `name.pos` から range を始める。
    decorated private accessor の backing field `#a_accessor_storage`（`getGeneratedPrivateNameForNode`）
    は name が synthesized（pos −1）なので上流の range は `{pos: -1, end: property.end}`：
    `emitSourceMapsBeforeNode` は leading を skip し trailing だけ出す。Rust の
    `property_source_map_range` は parsed original の name pos を使うので leading が出ていた。
    property 自身の name が synthesized なら `NO_LEADING_SOURCE_MAP` を付ける（既存の
    `generated_backing_in_static_block` と同じ機構）。4 か所（inline 式、instance / private
    storage / static define の各文）。
11. **private access の receiver clone は位置を保つ（`class_fields/downlevel.rs`、分離差分）。**
    `createCallBinding` は identifier / `this` / `super` の receiver を temp に入れず同じ node を
    target と `thisArg` に使うので、`B.#m()` → `__classPrivateFieldGet(B, …).call(B)` の 2 つの
    `B` は元の位置に map される。Rust の `stabilize_receiver` は `clone_node`（pos −1）だったので
    両方が unmapped だった。clone に `set_original_and_range` で位置を戻す。
13. **class-fields の private 記憶域名は class 内の synthesized private member を避ける
    （`class_fields.rs`、分離差分）。** decorated private accessor の backing field
    `#a_accessor_storage`（esDecorators 由来、synthesized PrivateIdentifier）と同じ class の
    public `accessor a` を class-fields が下げると、tsc は `generateMemberNames` の member 順で
    private 名を決めるので後者が `#a_1_accessor_storage` になる。Rust の
    `retained_private_storage` は parsed private 名と自分の割当だけを見ていたので同名が重複した
    （`reserved/private-and-public-same-stem` の ES2022+）。class の private-name scope に
    synthesized private member 名を seed する。逆順（public accessor が先）では上流が decorator
    側の field に `_1` を付けるが、decorator の名前は transform 時に確定するため未対応（記録）。
14. **print 時の optimistic 名は base から決め直す（`target_bindings.rs`）。** finalizer は
    「planned 綴りが空いていれば維持」していた。planned は transform-time planner の provisional
    名か、前回の print が node に書き戻した綴りなので、失敗した printer の表で `_s_1` / `_o_1`
    になった node を fresh printer が再確定すると `_s_1` のまま残った（上流は `makeUniqueName`
    を print ごとに base から評価する）。`PrintSource` policy では FileLevel / Scoped / role-suffix
    の planned を base（+suffix）に置き換える。transformer-time finalize は planned を保つ。
    planner の衝突判断（同 class 内の重複 member 名、入れ子の予約）は finalizer の scope model と
    決定 1 の `generated_names` が再現する。
15. **decorated private auto-accessor の descriptor forwarder は member 全体の range を持つ
    （`standard_decorators.rs`）。** `transformAutoAccessorPropertyDeclaration` の descriptor 経路
    （`_tsc.js:100103-100133`）は backing field / getter / setter の三つに
    `setSourceMapRange(…, getSourceMapRange(node))`（decorator を含む member の range）を与える。
    `moveRangePastModifiers` を使うのは非 descriptor 経路の `finishClassElement` だけで、public な
    `@dec accessor a` はそちら（class-fields が下げる）を通るので getter が decorator の後ろに map される。
    Rust は両経路に `set_source_map_range_past_decorators` を使っていたため、ES2022 / ESNext × set の
    `get #a()` / `set #a(value)` の先頭 mapping が decorator の後ろ（`(10, 9)`）になっていた
    （上流 `(10, 4)`；`reserved/private-accessor-storage-{nested,siblings}`、`private-and-public-same-stem`
    の 9 row）。descriptor 経路の getter / setter を `set_source_map_range_from(…, plan.original)` にする。
    ES2015 では class-fields が forwarder を `_B_a_get = function …` に下げ、その行に mapping は無い
    （before / after とも exact）。
16. **持ち越し表の識別子は arena を含み、binding 単位の cache を持つ（`transform.rs` / `printer.rs` /
    `target_bindings.rs` / `factory.rs`、決定 4 の修正；統合レビュー F1 / F2）。** 決定 4 の
    `node_names` は `TransformNode`（arena 内の source / node 番号）だけを key にしていたため、
    同じ printer で別の transformation を印字すると番号が一致した node が前の綴りを引き当てた
    （`x` 由来の宣言の後に別 arena の `y` 由来の宣言が `x_1`；同じ `x` でも上流 `x_2` に対し `x_1`）。
    上流の `nodeIdToGeneratedName` は process 全体の `getNodeId` を key にする（`_tsc.js:120633-120637`）。
    `CarriedNodeKey { arena: TransformArena::id(), node }` を key にし、`TransformArena` の `Default` を
    `new()`（process 一意の id）にする（`derive(Default)` は id 0 を配っていた；clone は元から新 id）。
    また上流は `autoGeneratedIdToGeneratedName`（`_tsc.js:120624-120632`）で generated identifier ごとの
    綴りを cache するので、失敗後に同じ binding を再印字しても `x_1` / `_s` / `_o` / `_a` のまま
    （Rust は集合と ordinal だけを持ち越し、再割当で `x_2` / `_s_1` / `_o_1` / `_b` に進んでいた）。
    `CarriedBindingKey { arena, binding }` → 綴り の `binding_names` を `note_generated_identifier` が
    全 domain で記録し、finalizer は割当の前に参照する（cache hit は集合・ordinal を進めない）。temp の
    ordinal は失敗を繰り返しても binding ごとに 1 回だけ数える（`named_by_earlier_print`）。
    対照は direct `lifecycle/reprint-after-failure/*`（6 kind × before / after）、`cross-arena/*`（8）、
    `double-failure/*`（5）の 25 row と、統合レビューの probe 7 row（`records/after6/integration-probe/`）。
17. **持ち越し表は失敗時に open な scope を再現する（`printer.rs` / `printer/bundle.rs` / `transform.rs`、
    決定 4 の修正；統合レビュー項目 2）。** 決定 4 は temp の ordinal と scoped 名の予約を root に集約し、
    bundle は全 source の root 宣言名を開始時に記録していた。上流は scope が閉じるたびに `tempFlags` /
    `reservedNames` を pop し、失敗時に open な scope だけを stack に残す（最内の `tempFlags` が次の
    standalone print の現在値、`reservedNames` は stack 全体が可視）。また `emitSourceFileWorker` は
    print ごとに新しい scope を push するので、失敗後の source file print の temp は `_a` から始まる。
    printer に scope stack（§6.4）を持たせ、`emit_transformed_node_worker` が tsc の push 点
    （function-like / class / static block / module block / object・type literal / interface；
    `ReuseTempVariableScope` は除く）で push、完了時だけ pop する。失敗時の `temp_ordinal` は最内 scope の
    数、`reserved` は open な scope の和。source file / bundle の print は `temp_ordinal = 0` で seed する。
    bundle の root 宣言名は各 source の worker 入口で記録する。対照は direct `scope-fault`（15）と
    `file-after-failure`（5）。
18. **統合再レビュー：temp ordinal は割当後の counter を保持する。**
    `_tsc.js:120703-120740` の `makeTempVariableName` は衝突候補と `_i` / `_n` の slot も数える。
    提出候補の `temp_count += 1` は parsed `_a` を避けた `_b` の後に失敗すると、次の別 binding に
    `_b` を再割当した（上流は `_c`）。最終割当時に `GeneratedBindingScopes::current_temp_ordinal`
    を binding ごとに記録し、全 occurrence の `EmitMetadata::generated_binding_temp_ordinal` に運ぶ。
    printer は最初の emit でこの値を scope counter に反映し、同一 binding の cache hit は進めない。
    通常 temp の `FinalizerTraversal` に適用し、既存の planned spelling / loop-variable policy は維持する。
    対照は別 observer `observe-decorator-binding-carry.mjs` の 8 row（parsed collision、`_i` / `_n`
    skip、数字列移行 × before/after fault）。提出の shared observer / pipeline artifact は変更しない。
19. **統合再レビュー：関数本体入口の `generateNames(body)` を持ち越しにも反映する。**
    `_tsc.js:119021-119032` は本体内の全宣言名を、最初の statement の hook より前に生成する。
    関数内に2宣言を置き1つ目の後に失敗させた対照では、候補の次の temp は `_b`（上流 `_c`）、
    scoped は `_s_4`（上流 `_s_5`）。source file 入口で使う宣言収集を function-body Block の入口
    でも呼び、未印字の宣言の名前・ordinal・cache を現在の scope に記録する。scoped と temp の
    2 row を補足 observer に追加し、既存8 rowと合わせ10 rowにする。完成した関数 scope は従来どおりpop。
12. **native ID の trace 用 accessor（`factory.rs` / `transform.rs`、分離差分）。**
    `TransformArena::generated_binding_identity(node) -> Option<u64>`（opaque）。direct contract は
    synthetic group の変換後 tree を歩き、binding ごとの宣言 / 参照 occurrence と綴りを
    `identity_trace` として保存し、1 binding = 1 綴りを assert する。
5. **witness の入力形。** 計算 key を持つ field / accessor は entity name の getter `keys.x`
   （late-bindable なので TS1166 が出ず、simple-inlineable でないので `__propKey` と cache temp
   が出る）。method は `key()` のまま。bundle 行は `ignoreDeprecations: "6.0"`。期待値は
   すべて上流から採取し、手書きの suffix は無い。

## 6. Failure 後の名前表の寿命（C03 `x_2` / `x_1` の監査）

### 6.1 上流の state と寿命

`createPrinter` の名前表は closure 変数（`nodeIdToGeneratedName`、`nodeIdToGeneratedPrivateName`、
`autoGeneratedIdToGeneratedName`、`generatedNames`、`tempFlags` と stack、`formattedNameTempFlags`、
`privateNameTempFlags`、`reservedNames` / `reservedPrivateNames` と stack）で、
`reset()`（117117-117141）だけが初期化する。`reset()` を呼ぶのは `writeFile2` / `writeBundle` /
`writeNode` / `writeList` の**正常終了**の末尾であり、hook が throw すると呼ばれない
（`createPrinter` に `finally` は無い。C03 の writer/comment 持ち越しと同じ構造）。
したがって失敗した print で**その時点までに決まった**名前だけが次の print に残る。
名前が決まる時点は (a) scope 入口の `generateNames`（SourceFile / Block / function body の宣言名。
`printNode` の standalone statement には無い）、(b) identifier の emit（`getTextOfNode2` → `generateName`）。

### 6.2 domain ごとの持ち越し（direct `lifecycle/failure`、上流観測、fault = 最初の statement の before / after）

| kind（producer） | 表 | after-fault の 2 回目（同じ printer） | before-fault の 2 回目 | fresh printer |
| --- | --- | --- | --- | --- |
| numbered `createUniqueName("x")` | `generatedNames` | `x_2` | `x_1` | `x_1` |
| file-level `createUniqueName("_m", Optimistic\|FileLevel)` | なし（FileLevel は generatedNames を見ない） | `_m` | `_m` | `_m` |
| scoped `createUniqueName("_s", Optimistic\|ReservedInNestedScopes)` | `reservedNames`（root 段） | `_s_1` | `_s` | `_s` |
| file-wide `createUniqueName("_o", Optimistic)` | `generatedNames` | `_o_1` | `_o` | `_o` |
| temp `getGeneratedNameForNode(non-member)` | `tempFlags` | `_b` | `_a` | `_a` |
| node-derived 同一 node | `nodeIdToGeneratedName` | `x_1`（cache hit） | `x_1` | `x_1` |
| node-derived 別 node 同名 | `generatedNames` | `x_2` | `x_1` | `x_1` |

before-fault では何も生成されておらず持ち越しは無い。file-level だけは after-fault でも影響を受けない。

### 6.3 Rust の対応

`finalize_generated_binding_names_for_print`（`target_bindings.rs:979`）は print ごとに
`GeneratedBindingScopes` を新規に作り、`record_print_finalized_generated_binding` で
「次回も再確定」を記す。printer 側（`begin_print` / `end_print` / `finish_print`）は
writer と comment container を持ち越すが、生成名の表は持たない。
そのため after-fault の 2 回目は常に fresh と同じ綴りになる（C03 `#op2`、§6.2 の 5 行が差）。
`node-derived` 同一 node の行と file-level の行は「fresh と同じ」が正解なので一致する。

### 6.4 持ち越しの実装（決定 4・16・17）

printer が「失敗した print で決まった名前」を持ち、次の print の finalizer に seed する。
実装：`Printer::carried_generated_names: Option<CarriedGeneratedNames>`（`transform.rs`）と
print 中の accumulator `generated_names_this_print`（printer 全体の表 `names` と、tsc の
`pushNameGenerationScope` / `popNameGenerationScope` に対応する scope stack `scopes`）。

記録点：(1) `write_transformed_source_file`（`emitSourceFileWorker`）の入口で scope を push し、
root 宣言名（`TransformationContext::root_declaration_generated_identifiers`、
`generateNames(sourceFile)` に対応）をその scope に記録する（bundle は source ごと）；
(2) identifier の emit（`note_generated_identifier`）：metadata の domain で printer 全体の
`generated`（numbered / file-wide / file-level）、`node_names`（`nodeIdToGeneratedName`、arena id
+ original node）、`binding_names`（`autoGeneratedIdToGeneratedName`、arena id + binding id、全 domain）
と、最内 scope の `reserved`（scoped optimistic / nested 予約の temp / private temp）・`temp_count`
（binding ごとに 1、以前の失敗で命名済みなら 0）に振り分ける；(3) `emit_transformed_node_worker`
は function-like / class / static block / module block / object・type literal / interface に scope を
push し、emit が完了したときだけ pop する（`ReuseTempVariableScope` は push しない）。
`finish_print` は Err なら `carried` に merge：`generated` / `node_names` / `binding_names` は和、
`reserved` は open な全 scope の和（tsc の `isReservedName` は stack 全体を見る；閉じた scope の
予約は消える）、`temp_ordinal` は失敗時の最内 scope の `temp_count`（tsc の stale な `tempFlags`）。
Ok なら `carried = None`（`reset()`）。

次の print：standalone node は `finalize_generated_names_for_print(root, oracle, carried)` →
`GeneratedBindingScopes` に `seed_generated_names` / `seed_root_reserved_names` /
`seed_root_temp_ordinal` を適用し、割当の前に `binding_names`（同じ binding は同じ綴り、集合・
ordinal を進めない）、numbered arm では続けて `node_names` を引く。source file / bundle の print は
`carried_names_for_source_file_print`（`temp_ordinal = 0`）で seed する：`emitSourceFileWorker` が
新しい scope を push するので temp は `_a` から始まり、`generatedNames` と stale な `reservedNames`
stack は続く。carried がある print は eager 名を必ず再確定する（決定 14：base から決め直す）。

| 上流の表 | 決まる時点 | Rust 側 | 実装 |
| --- | --- | --- | --- |
| `generatedNames`（numbered / file-wide / file-level） | identifier の emit または scope 入口の `generateNames` | `seed_generated_names` | 決定 4 |
| `reservedNames` stack（失敗時に open な scope の分） | 同上 | `seed_root_reserved_names`（和） | 決定 4 → 17（閉じた scope の分は除く） |
| `tempFlags`（失敗時の最内 scope） | 同上 | `seed_root_temp_ordinal`（standalone）/ 0（source file） | 決定 4 → 17 |
| `nodeIdToGeneratedName` | 同上 | `CarriedGeneratedNames::node_names`（arena id + node） | 決定 4 → 16 |
| `autoGeneratedIdToGeneratedName` | 同上 | `CarriedGeneratedNames::binding_names`（arena id + binding） | 決定 16 |
| scope stack（失敗時に push されたまま） | 失敗の深さ | `GeneratedNamesThisPrint::scopes` | 決定 17 |

対照：direct `lifecycle/failure/*`（14）、`reprint-after-failure/*`（12）、`cross-arena/*`（8）、
`double-failure/*`（5）、`scope-fault/<kind>/<inside-nested | after-nested | after-tail>`（15）、
`file-after-failure/<kind>`（5）。scope-fault の上流観測：root の temp 2 つと関数内の temp 1 つ、
関数の後の temp 1 つ（scope 入口の `generateNames` で root の 3 つが先に `_a` `_b` `_c`）に対し、
関数内で失敗 → 次の standalone temp は `_b`（最内 = 関数 scope の 1）、関数の後で失敗 → `_d`
（pop 済み、root の 3）、末尾の後で失敗 → `_d`；scoped は関数内失敗で `_s_4`、関数の後 / 末尾で
`_s_3`（関数 scope の `_s_3` は pop で消える）。決定 4 の近似（root に集約）はこの 6 row で
外れていた。bundle の source ごとの記録は worker 入口に移し、bundle 開始時の一括記録をやめた。

### 6.5 到達前提が未成立の面

`printNode(hint, node, sourceFile)` の `sourceFile` は `currentSourceFile` を差し替え、
`isFileLevelUniqueNameInCurrentFile` が**その** file の census を見る。Rust の
`PrintRequest::StandaloneNode { node, writer }` は node の source を使い、別 source を受け取る
引数が無い。API1.1a の到達面として記録し、本 slice では観測しない。

dispose 後の print：上流は synthetic node の `emitNode`（`autoGenerate`）を残すので
fresh printer が同じ綴りを出す。Rust は `TransformationResult::dispose` 後の print を
`InvalidLifecycle { operation: "query substitution", state: Disposed }` で拒否する（session model、
C01 lifetime の記録と同じ境界）。typed refusal として記録し、exact に数えない。

### 6.6 補足：ESNext × useDefineForClassFields

`getScriptTransformers`（`_tsc.js:115921` 付近）は
`!experimentalDecorators && (languageVersion < ESNext || !useDefineForClassFields)` のときだけ
`transformESDecorators` を積む。ESNext × define では decorator が native のまま印字されるので、
その組合せの行は生成名を持たず、対照としては自明に一致する（SUPER の runtime control が
esnext/define を skip するのと同じ理由）。

