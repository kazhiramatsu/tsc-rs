# H2.8a-A-RES-POST-T1 — 設計：R9 / R12 / RECEIVER-MAP / PRIVATE-SET-COMMENTS / DECORATOR-COMMENTS

作成日：2026-09-17。依頼書：[h2-8a-post-t1-residuals-claude-handoff.md](../h2-8a-post-t1-residuals-claude-handoff.md)。
結果と実行記録：[REPORT.md](REPORT.md)。記録：[records/](records/)。
本書は隔離候補の設計であり、production readiness、accepted profile、H2.8 全体の admission を主張しない。

## 1. Identity / purpose / boundary

| 項目 | 値 |
| --- | --- |
| slice ID / kind | `H2.8a-A-RES-POST-T1` / `runtime`（原因別の限定修復候補、5 子スライス） |
| 目的 | C02 / T1 の後に残る 7 complete commands（R9 ×2、R12 ×4、RECEIVER-MAP ×1）を complete-command 契約で exact ×2 にし、3 packet probes（PRIVATE-SET-COMMENTS ×1、DECORATOR-COMMENTS ×2）の内部 flag 差を公開観測（コメントを含む新規対照）で確認して、必要な修復または根拠付きの実装差 disposition を出す |
| 非目標 | 独立した新 subsystem、dispose 後 print の session contract、同一 Program の API 再 emit、TS7 移行、transpile / cache の再実装、T1 の packet source identity / atomic restore / typed refusal の変更、profile / STAGE / accepted-state ratchet / 共有 CI policy の変更、PR / merge / admission |
| 前提 | PR #549（C02）と PR #550（T1）を含む main |
| trusted base（開始 SHA） | `eb6dc2c7872b18442657f8eefde5efc9e8fb4cf7`（origin/main = PR #550 merge、[records/before/start-state.txt](records/before/start-state.txt)：rustc 1.93.0 / cargo 1.93.0 / Node v25.2.1、`_tsc.js` sha256 `1c59e77a…`、`typescript.js` sha256 `56917765…`；dirty は複製した依頼書 .md 1 件のみ） |
| worktree / branch | `~/dev/tsc-rs-post-t1-residuals` / `draft/h2-8a-post-t1-residuals` |
| activation 前 / 後 | 変更する architecture 行は §2。いずれも `active-qualified` → 候補では `modified-requalify`（最終 validation ref での再 qualify は統合担当） |
| 次の owner | 統合担当（合成 PR・hosted・admission）。未解決項目は REPORT §6 |
| authority artifact hash | 上流 span 46 本は §3（[records/upstream-spans.tsv](records/upstream-spans.tsv)、`records/span-hash.py`：関数開始行から brace 対応で終端を求め、行を `\n` 連結し末尾改行を付けた sha256。T1 と同じ規約） |

処理順は依頼書どおり R9 → R12 → RECEIVER-MAP → PRIVATE-SET-COMMENTS → DECORATOR-COMMENTS。
子ごとに原因別 commit を残し、最終状態で組合せを検証する（REPORT §5）。

## 2. Required-reference table

| 参照 | 種別 | 状態 / ref | 現行 Rust symbol（開始 SHA） | 本候補での扱い |
| --- | --- | --- | --- | --- |
| [`E-NAMES-BASE`](../../emitter-architecture.md) | architecture | `active-qualified` @0653e10d | `GeneratedBindingId`、`target_bindings::{TargetBinding, finalize_generated_binding_names}`、`GeneratedBindingScopes` | `modified-requalify`（R9）：transformTypeScript が付ける匿名 default class 名を、生成時点から typed numbered binding（family `default`）として運ぶ。finalizer / oracle / bundle 名前表は不変 |
| [`E-NAMES-CLASS-G`](../../emitter-architecture.md) | architecture | `active-qualified` @0653e10d | `class_fields::downlevel::ClassBinding`（`Existing` / `Generated`）、`class_this_binding` | `premise-unchanged`：class 名 identifier が binding を持てば `Generated` へ投影する既存経路（`downlevel.rs:2826`）をそのまま使う |
| [`E-METADATA-BASE`](../../emitter-architecture.md) | architecture | `active-qualified`（T1 部分は `modified-requalify`） | `TransformArena::{set_original_node, metadata}`、`EmitMetadata::merge_from`（`factory.rs:836`、`metadata.rs:826`） | `premise-unchanged`：`set_original_node` の metadata merge（tsc `mergeEmitNode`）は変えない。R12 は「original を結ばない」上流の node 構成に合わせる。T1 の packet（`comment_range` の source identity / atomic restore / typed refusal）は不変 |
| [`E-METADATA-G-CLASS`](../../emitter-architecture.md) | architecture | `active-qualified` @0653e10d | `class_this` / `assigned_name` / `class_expression_declaration_origin`（producer `builtins.rs` / `es_next.rs` / `standard_decorators.rs`、consumer `class_fields*.rs`） | `premise-unchanged`：System の hoist は module transform（consumer より後）で走り、上流も class expression にこれらを複写しない |
| [`E-COMMENTS-G`](../../emitter-architecture.md) / [`E-COMMENT-SCOPE-H`](../../emitter-architecture.md) | architecture | `active-qualified` | printer `comment_range_for_node`（`printer.rs:16079`）、`CommentEmissionScope`、`emit_list_element_end_comments_in_container`（`printer.rs:17543`）、`emit_child_boundary_comments_before_parent_end`（`printer.rs:8447`） | PRIVATE-SET-COMMENTS / DECORATOR-COMMENTS の consumer。§7 / §8 で観測に基づき disposition を決める（printer の container 規則は変更しない） |
| printer の map 境界 | architecture（`E-POSITIONS` 系） | `active-qualified` | `record_node_map_boundary`（`printer.rs:17690`：`NO_LEADING_SOURCE_MAP` / `NO_TRAILING_SOURCE_MAP`、metadata の `source_map_range` else node range） | `premise-unchanged`：R12 / RECEIVER-MAP は producer 側の node 構成 / range 所有を上流に合わせる。printer は変えない |
| C02 [REPORT §3](../h2-8a-generated-binding/REPORT.md)、[DESIGN 決定 8 / 9 / 11](../h2-8a-generated-binding/DESIGN.md) | frozen evidence / rationale | PR #549 | System `collect_declaration_list_hoists` の binding 登録、CJS `export default` の range-only 複製、`stabilize_receiver` の clone 位置 | rationale：同じ機構（text→binding の登録、`setTextRange` のみの複製、clone の位置所有）を R9 / R12 / RECEIVER-MAP で使う |
| T1 [DESIGN](../h2-8a-bundle-metadata-t1/DESIGN.md) / [REPORT §3.2](../h2-8a-bundle-metadata-t1/REPORT.md) | frozen evidence | PR #550 | `bundle-metadata-t1-known-native.json`（3 行）、`bundle-metadata-t1-known-packet.json`（3 行）、`decorator-binding-known-native.json`（R9 / R12 の 4 行） | 修復が確認できた ID だけを候補側で retire（§9） |
| [`docs/witness-testing.md`](../../../../witness-testing.md) | process | — | `scripts/witness.py` | 新 suite `post-t1-residuals` を local runner に登録（hosted job / planner / CI の登録は統合担当） |

歴史的文書（M-stage guide、h1-emit.md）は本候補の実装事実として引用しない。

## 3. Pinned upstream map（vendored TypeScript 6.0.3 `_tsc.js`）

hash は [records/upstream-spans.tsv](records/upstream-spans.tsv)（46 span）。以下は子ごとの入口。

| 子 | 関数 | span | sha256 | 役割 / 分岐 |
| --- | --- | --- | --- | --- |
| R9 | `getClassFacts` | 94410-94427 | `81b81924…` | `HasStaticInitializedProperties`（static + initializer）、`HasMemberDecorators`（`childIsDecorated`） |
| R9 | `isClassLikeDeclarationWithTypeScriptSyntax` | 94431-94433 | `f99b7077…` | decorators / typeParameters / `ContainsTypeScriptClassSyntax`（static initializer を含む） |
| R9 | `visitClassDeclaration`（transformTypeScript） | 94434-94548 | `b4f4c7bb…` | `needsName = moveModifiers && !node.name \|\| HasMemberDecorators \|\| HasStaticInitializedProperties` → `name = node.name ?? getGeneratedNameForNode(node)`（94455-94456）。`HasStaticInitializedProperties` → 更新した宣言に `NoTrailingSourceMap`（94464-94467） |
| R9 | `getGeneratedNameForNode` | 21652-21666 | `7aeec7c8…` | `GeneratedIdentifierFlags.Node` + `original = node`：綴りは printer が決める |
| R9 | `getName` / `getLocalName` / `getDeclarationName` | 24788-24799 / 24803-24805 / 24809-24811 | `9734f557…` / `db85ef71…` / `2774ac86…` | parsed 名なら clone、generated 名なら `getGeneratedNameForNode(node)`（同じ node → `generateNameCached` で同じ綴り） |
| R9 | `generateNameForNode` → `generateNameForExportDefault` | 120876-120942 / 120831-120846 | `80b51fff…` / `d2ea6c08…` | `ClassDeclaration` で name 無し / generated → `makeUniqueName("default", isUniqueName, optimistic=false, scoped=false)` |
| R9 | `makeUniqueName` / `isUniqueName` / `isFileLevelUniqueNameInCurrentFile` / `isFileLevelUniqueName` | 120741-120779 / 120638-120640 / 120665-120667 / 12907-12909 | `24cb46aa…` / `5394b599…` / `ff107c74…` / `a001ca50…` | `default_1`, `default_2`, … を `hasGlobalName`（checker globals）、`sourceFile.identifiers`（parse census）、`reservedNames`、bundle 全体の `generatedNames` で判定 |
| R9 | `visitClassDeclaration`（System） / `visitFunctionDeclaration`（System） / `appendExportsOfHoistedDeclaration`（System） | 112605-112633 / 112576-112604 / 112795-112809 | `de6afa6b…` / `c9849240…` / `99fef8ae…` | `getLocalName(node)` を hoist / 代入先 / `exports_1("default", …)` の全部に使う（同一 generated 名） |
| R9 | `visitClassDeclaration`（CommonJS/AMD/UMD） / `appendExportsOfHoistedDeclaration`（同） | 111505-111536 / 111722-111742 | `6f7feb4a…` / `bfbaa381…` | `getDeclarationName(node)` を宣言名に、`getLocalName(decl)` を `exports.default = …` に使う |
| R12 | `visitClassDeclaration`（System） | 112605-112633 | `de6afa6b…` | `setTextRange(createExpressionStatement(createAssignment(name, setTextRange(createClassExpression(...), node))), node)`：class expression と statement は **`setOriginalNode` を持たない** fresh node |
| R12 | `setTextRange` / `setOriginalNode` / `mergeEmitNode` | 28256-28258 / 25208-25217 / 25218-25277 | `b4484231…` / `8ef5d40b…` / `6d9f4af1…` | `setTextRange` は pos/end のみ。`setOriginalNode` は emitNode（flags を含む）を複写 |
| R12 | `emitSourceMapsBeforeNode` / `emitSourceMapsAfterNode` | 121283-121293 / 121294-121303 | `ac346b41…` / `6ca767d4…` | `NoTrailingSourceMap` が無く `end >= 0` なら node 終端で `emitSourcePos`（class expression の `}` 直後と statement の `;` 直後） |
| RECEIVER-MAP | `createPrivateIdentifierAssignment` | 96795-96840 | `50aca93f…` | compound なら `createCopiableReceiverExpr(receiver)`：`receiver = initializeExpression \|\| readExpression` |
| RECEIVER-MAP | `createCopiableReceiverExpr` / `cloneNode` / `isSimpleInlineableExpression` / `isSimpleCopiableExpression` | 96567-96578 / 24436-24466 / 93030-93032 / 93027-93029 | `4e366a50…` / `d223dcea…` / `75411b58…` / `388e8823…` | `clone = cloneNode(receiver)`（synthesized：pos/end = -1、`setOriginalNode(clone, receiver)`）。identifier は inlineable でないので `_b = clone` の temp。`this` / literal は clone をそのまま読む |
| RECEIVER-MAP | `visitPreOrPostfixUnaryExpression` / `visitBinaryExpression`（class fields） | 96486-96551 / 96694-96785 | `05f34f10…` / `193d3a00…` | update 式も同じ `createCopiableReceiverExpr`；結果は `setOriginalNode` + `setTextRange(node)` |
| PRIVATE-SET | `createClassPrivateFieldSetHelper` / `createClassPrivateFieldGetHelper` | 25971-25985 / 25956-25970 | `24690a69…` / `bef17701…` | helper call は range 無し（`visitBinaryExpression` が `setTextRange(setOriginalNode(call, node), node)`） |
| PRIVATE-SET | `emitNodeListItems` | 120068-120155 | `ebeb65a7…` | 区切りの前：`previousSibling.end !== parentNode.end` かつ `!NoTrailingComments` のとき `emitLeadingCommentsOfPosition(previousSibling.end)`（120090-120095）；末尾：同条件で `emitLeadingCommentsOfPosition` |
| PRIVATE-SET | `emitCommentsBeforeNode` / `emitCommentsAfterNode` / `emitLeadingCommentsOfNode` / `emitTrailingCommentsOfNode` / `emitLeadingCommentsOfPosition` / `forEachTrailingCommentToEmit` | 120987-120994 / 120995-121006 / 121007-121032 / 121033-121046 / 121166-121175 / 121234-121238 | `dc59a090…` / `f0baac32…` / `ce6bf342…` / `e5c99d84…` / `fa23b688…` / `bd6612ac…` | `containerPos` / `containerEnd` の claim と、`end === containerEnd` の子の trailing skip |
| DECORATOR | `transformAllDecoratorsOfDeclaration` / `transformDecorator` | 100546-100553 / 100554-100569 | `1b5a7f8e…` / `d59f7ca4…` | `expression = visitNode(decorator.expression)`；`setEmitFlags(expression, NoComments)`；access 式は `createCallBinding(expression, hoist, languageVersion, cacheIdentifiers=true)` → `restoreOuterExpressions(expression, createFunctionBindCall(target, thisArg, []))` |
| DECORATOR | `createCallBinding` / `restoreOuterExpressions` | 24691-24753 / 24646-24654 | `445f6a35…` / `954f25c4…` | identifier receiver は temp（`_a = ns`）へ、target は新 PropertyAccess（range = callee）；outer expression（paren 等）を復元 |
| DECORATOR | `emitDecoratorsAndModifiers` / `emitDecorator` | 119846-119902 / 117872-117875 | `8fb50c70…` / `aa7b18f7…` | native 経路（ESNext × define）は parsed decorator をそのまま印字 |
| 共通 | `getEmitFlags` | 13054-13057 | `6d8c5eec…` | `node.emitNode?.flags ?? 0` |

## 4. R9 — 匿名 default class の生成名と global / bundle 名前表

### 4.1 上流の経路

`export default @dec class { @dec static m() {} }`（ESNext × define、native decorator）：

1. transformTypeScript `visitClassDeclaration`：`isClassLikeDeclarationWithTypeScriptSyntax`（decorator あり）で早期 return せず、
   `facts & HasMemberDecorators` → `needsName` → `name = getGeneratedNameForNode(node)`（`GeneratedIdentifierFlags.Node`、original = 宣言）。
   `static f = 1` だけの class も `HasStaticInitializedProperties` で同じ経路（`ContainsTypeScriptClassSyntax` が static initializer に立つ）。
2. esDecorators は走らない（ESNext × define）。class fields も触らない。
3. module transform：System `visitClassDeclaration` → `getLocalName(node)` → `getName` → 名前が generated なので `getGeneratedNameForNode(node)`（同じ宣言 node）を hoist / 代入先 / `exports_1("default", …)` に使う。CommonJS/AMD/UMD は `getDeclarationName(node)` を class 名に、`getLocalName(decl)` を `exports.default = …` に使う。ESNext module は名前に触れない。
4. printer：`generateNameForNode(ClassDeclaration)` → `generateNameForExportDefault` → `makeUniqueName("default", …)`：`default_1` から順に
   `isUniqueName` = `!hasGlobalName(name) && !currentSourceFile.identifiers.has(name) && !isReservedName(name) && !generatedNames.has(name)`。
   同じ node の全参照は `nodeIdToGeneratedName` で同じ綴り。`generatedNames` は bundle の source をまたいで保持（`writeBundle` の末尾で `reset`）。

観測（[records/upstream JS](records/)、`decorator-binding.json.zst`）：`script-let-default_1` は globals.ts（script）の `let default_1` が global なので `class default_2`；
`bundle-default-two-files` は System bundle の 2 番目の source が `default_2 = @dec class default_2 { … }` / `exports_2("default", default_2)`。

### 4.2 現行 Rust の producer / consumer / lifetime（開始 SHA）

| 段階 | symbol | 状態 |
| --- | --- | --- |
| producer（transformTypeScript） | `TypeScriptVisitor` ClassDeclaration arm（`builtins.rs:11276-11292`）：`facts.needs_declaration_name()` → `ensure_generated_declaration_name(id, "default")`（`builtins.rs:12388`：parse census `source_identifier_names` と `generated_namespace_names` だけを見て `default_1` を決める）→ `data.name = create_identifier(&name)`（`builtins.rs:13616`：**plain Identifier、generated-binding metadata 無し**） | gap（typed identity を持たない） |
| lowered decorator consumer | `standard_decorators.rs:5249-5290 decorated_class_declaration_name`：source に名前が無ければ `generated_binding_for_identifier(name)` else `TargetBinding::allocate_numbered("default", text)` を name に書く（`ensure_generated_class_reference_binding` 5398 も同型） | already-exact（C02）：consumer が昇格するので ES2015 / ES2022 / ESNext × set は global / bundle を見る |
| `using` consumer | `es_next.rs:1394-1434 promote_existing_generated_class_binding` | already-exact（同型の昇格） |
| class-fields consumer | `downlevel.rs:2826` `class_this_binding` / `ClassBinding::Generated(from_existing)`、`class_fields.rs:2600-2635`（named class は LOCAL_NAME clone） | already-exact（binding があれば投影） |
| System consumer | `system.rs:815-835 collect_hoisted_names`（class 名の **text** を `push_hoisted_name`）、`system.rs:2125-2195 transform_hoisted_class`（`data.name` の text → `create_identifier(&local)`：`generated_bindings.get(text)` が無ければ plain）、`collect_declaration_list_hoists`（`system.rs:873-940`：var list の名前だけ `generated_binding_of_identifier` で登録 = C02 決定 8） | gap（class 宣言名の binding を登録しない） |
| CommonJS/AMD/UMD consumer | `builtins.rs:5615-5640`（ClassDeclaration arm：`data.name` の text）、`create_identifier`（`builtins.rs:9919`：`generated_module_names.generated_bases` にある text だけ binding を書く；module info は **名前が無い**宣言にだけ `default_1` を割り当てる（`builtins.rs:3272-3286`）ので、transformTypeScript が名前を付けた宣言は bases に無い） | gap（`exports.default = default_1` が plain） |
| ESNext module | `EcmaScriptModuleTransformer`（`builtins.rs:1150`） | 名前に触れない（already-exact） |
| finalizer | `target_bindings.rs:1065-1082 allocate_numbered_name_with_global_oracle`、`finalize_bundle_generated_binding_names_for_print`（bundle 全体の `generated_names`） | already-exact：numbered binding は global oracle と bundle 名前表を通る |
| 名前表の寿命 | `finalize_generated_binding_names_for_print` は print ごと、bundle は `writeBundle` 相当で 1 表 | already-exact |

失敗順序：producer が plain Identifier `default_1` を作る → 全 consumer が text をそのまま複製 → finalizer に binding が無いので global `default_1` も前 source の `default_1` も見ない → `class default_1` / `default_1 = …` / `exports_1("default", default_1)` で一貫して `default_1`（JS bytes と map が上流 `default_2` と異なる）。

### 4.3 Gap matrix

| row | 現行 symbol | 分類 | 証拠 |
| --- | --- | --- | --- |
| transformTypeScript の生成名 identity | `ensure_generated_declaration_name` + `create_identifier` | `missing`（typed binding 無し） | before：pipeline 4 件 known（[records/before](records/before/)）、新対照 `r9/esnext/define-module-*`（REPORT §3） |
| System の hoist / 代入 / export | `collect_hoisted_names` / `transform_hoisted_class` | `missing`（class 名 binding の登録が無い） | `bundle-default-two-files`、新対照 `r9/esnext/define-system-*` |
| CommonJS/AMD/UMD の `exports.default` | `create_identifier`（CJS visitor） | `missing`（bases 経由のみ） | 新対照 `r9/esnext/define-commonjs-*` / `define-amd-*` / `define-umd-*` |
| lowered decorator / using / class-fields consumer | `decorated_class_declaration_name` 等 | `already-exact` | C02 対照、新対照 `r9/*/set-*` |
| module info の名前無し宣言（`export default function () {}` 等） | `generated_declaration_names` + `generated_bases` | `already-exact`（binding は System / CJS の constructor で作られる） | 新対照 `r9/esnext/define-system-function-default-collision` / `define-commonjs-function-default-collision` |
| finalizer / oracle / bundle 名前表 | `finalize_*` | `already-exact` | C02 global 21 variant、lifecycle 7 variant |
| runtime の class name（`B.name`） | — | `already-exact`（`class default_2` の runtime name は上流も `default_2`） | 新対照は `.name` を tail に持つ |

### 4.4 設計決定

1. **producer が typed identity を持つ。** transformTypeScript が匿名宣言に付ける名前は、上流の `getGeneratedNameForNode(node)`（node 由来の generated identifier）に対応する `TargetBinding::allocate_numbered(context, "default", "<provisional>")` を **宣言ごとに 1 つ**割り当て、name identifier に `write_generated_metadata` する。provisional text は既存 `ensure_generated_declaration_name` の綴り（parse census を避けた `default_N`）のまま：finalizer が oracle / 名前表で最終綴りを決める（`E-NAMES-BASE`：identity と印字名の分離）。同じ宣言に対する再要求（namespace の export 代入 `N.default_1 = default_1`）は同じ binding を書く（`TypeScriptVisitor` に `generated_declaration_bindings: BTreeMap<String, TargetBinding>` を text-key で持ち、`create_identifier` が該当 text に metadata を書く。System / CJS の `generated_bindings` と同じ機構。生成 text は parse census を避けるので parsed identifier と衝突しない）。
2. **System は class 宣言名の binding を登録する。** `collect_hoisted_names` の ClassDeclaration arm で `data.name` が generated binding を持てば `generated_bindings.entry(text).or_insert(binding)`（C02 決定 8 の `collect_declaration_list_hoists` と同じ登録）。以後の `create_identifier(text)`（hoisted `var`、`transform_hoisted_class` の代入先、`exports_1("default", …)` の値）は同じ binding metadata を書く。上流の「同じ generated identifier node を hoist する」に対応。
3. **CommonJS/AMD/UMD の `create_identifier` は登録済み binding を優先する。** `generated_module_bindings.get(text)` を先に見て、無ければ従来の `generated_bases` 経路で allocate する。ClassDeclaration arm は `data.name` の binding を `generated_module_bindings` に登録する（System と同じ）。`exports.default = default_N` の値と class 名が同じ identity になる。
4. **consumer の昇格経路は変えない。** `decorated_class_declaration_name` / `promote_existing_generated_class_binding` / `class_this_binding` は `generated_binding_for_identifier(name)` を先に見るので、producer の binding をそのまま使う（二重割当て無し）。
5. **名前無し宣言の module-info 経路は変えない。** `export default function () {}` / 名前を必要としない `export default class {}` は従来どおり module info が `default_1` を割り当て、System / CJS の constructor が numbered binding を作る（対照で退行 0 を確認）。
6. **parsed 名は変えない。** `export default class Named {}` は parsed identifier（binding 無し）のまま。runtime の `class default_2` と印字名は同じ綴り（上流と同じ）。

### 4.5 変更ファイル

| file | 変更 |
| --- | --- |
| `crates/emitter/src/builtins.rs` | `TypeScriptVisitor`：`generated_declaration_bindings`（text → `TargetBinding`）；ClassDeclaration arm の生成名を `ensure_generated_declaration_binding` 経由にし、`create_identifier` が登録 text に metadata を書く。`CommonJsVisitor`：ClassDeclaration arm で名前の binding を `generated_module_bindings` に登録、`create_identifier` は登録済み binding を優先 |
| `crates/emitter/src/builtins/system.rs` | `collect_hoisted_names` ClassDeclaration arm：名前 node の binding を `generated_bindings` に登録 |

禁止：finalizer / oracle / `ParsedSourceIdentifierNames` の変更、fixture 文字列 / case ID による分岐、既存期待値の書換え。

### 4.6 対照（新 ID `post-t1-residuals/r9/…`、21 件）

衝突あり / なし（`let default_1` global、`default_1` + `default_2` global、同一 file の parse census）、匿名 / 名前付き、名前を必要としない class、static initializer のみ（decorator 無し）、
単一 / 複数 source（System bundle、順序反転、bundle 内 global script）、function default（module info 経路）、
ESNext define と ES2015 / ES2022 / ESNext set（lowered）、module = ESNext / System / CommonJS / AMD / UMD。

## 5. R12 — System が hoist する class 文の末尾 source map

### 5.1 上流の経路

System `visitClassDeclaration`（112605-112633）は fresh な `ClassExpression` と `ExpressionStatement` を作り、両方に `setTextRange(…, node)` だけを付ける（`setOriginalNode` 無し）。
printer は `emitSourceMapsAfterNode` で `NoTrailingSourceMap` の無い node の `end` を map するので、`};` 行に class 終端の 2 segment（class expression の `}` 直後 = col 13、statement の `;` 直後 = col 14）が出る。
一方 class 宣言自体は transformTypeScript が `HasStaticInitializedProperties` のとき `NoTrailingSourceMap` を付け（94464-94467）、class fields の `updateClassDeclaration` は `setOriginalNode` で flags を引き継ぐ。
これは宣言 node にだけ効き、System が作る fresh node には及ばない。

### 5.2 現行 Rust

`system.rs:2125-2195 transform_hoisted_class`：`set_original_and_range(class_expression, original)` と `set_original_and_range(statement, original)`（`system.rs:3550`：`set_text_range` + `set_original_node`）。
`TransformArena::set_original_node`（`factory.rs:836`）は tsc `mergeEmitNode` と同じく original の metadata（flags を含む）を複写するので、
`finish_typescript_class_declaration`（`builtins.rs:14902`：static initializer で `NO_TRAILING_SOURCE_MAP`）の flag が class expression と statement に移り、
`record_node_map_boundary`（`printer.rs:17711`）が両 node の After 境界を落とす。4 対象すべてに static initializer（`static f = 1`、`static #p = 1`、`@dec static [keys.x] = 1`）がある。

### 5.3 Gap matrix

| row | 現行 symbol | 分類 | 証拠 |
| --- | --- | --- | --- |
| hoisted class expression の original | `transform_hoisted_class` | `partial-or-stale`（上流は range のみ） | T1 residual 2 行の map diff（`records/residual/*.txt`）、pipeline 2 件、新対照 `r12/*` |
| hoisted statement の original | 同上 | 同上 | 同上 |
| printer の After 境界 | `record_node_map_boundary` | `already-exact` | flag が無ければ node range の end を map する |
| `NO_TRAILING_SOURCE_MAP` の producer | `finish_typescript_class_declaration` | `already-exact`（上流 94464-94467 と同じ） | 非 System の対照（AMD / CJS / UMD / ESNext module）が exact |
| static initializer の無い class | — | `already-exact`（flag 無し） | 新対照 `r12/es2015/system-no-static`、`define-system-decorated-no-static` |

### 5.4 設計決定

1. **System の hoisted class expression と statement は range のみ持つ。** `set_original_and_range` を `factory.set_text_range` に置き換える（上流 `setTextRange` と同じ node 構成；C02 決定 9 の CJS `export default` と同じ扱い）。metadata merge の意味論（`set_original_node`）は変えない。
2. **class 名 identifier（`data.name`）は同じ node を再利用する**（上流も `node.name` を再利用）。R9 の binding metadata はそのまま残る。
3. **印字後の `.map` 書換え、行 / column / case 名の特例は使わない。**
4. export の publication（`exports_1("K", K)`）と `hoisted_declaration_exports` は変更しない（original を要求しない：`transform_hoisted_class` は `original` を引数で保持し、`get_original_node(original)` を export 計算に使う）。

確認事項：class expression の comment 境界は node の pos/end（`set_text_range`）で決まり、上流と同じ。`class_this` / `assigned_name` / `class_expression_declaration_origin` は class fields / decorator（module transform より前）の consumer なので System 以降で参照されない。

### 5.5 変更ファイル

| file | 変更 |
| --- | --- |
| `crates/emitter/src/builtins/system.rs` | `transform_hoisted_class`：class expression / statement に `set_text_range` のみ |

### 5.6 対照（新 ID `post-t1-residuals/r12/…`、18 件）

decorated / undecorated、export 有無、宣言 emit 無し（`declaration: false`）、class 末尾のコメントと空行、非 BMP の source 位置、`export default class { static f = 1 }`、
class expression の変数、関数内 class（hoist されない）、static private を両 source に、ES2015 / ES2022 / ESNext define、他の module 形式（AMD bundle / UMD / CommonJS / ESNext module）。
JS / d.ts / d.ts.map が一致することも complete-command 契約で保護する。

## 6. RECEIVER-MAP — static private の複合代入の receiver temp

### 6.1 上流の経路

`B.#p += 1`：`createPrivateIdentifierAssignment`（96795-96840）は compound のとき `createCopiableReceiverExpr(receiver)`（96567-96578）：
`clone = cloneNode(receiver)`（`cloneNode` 24436-24466 は synthesized node：pos/end = -1、`setOriginalNode(clone, receiver)` で emitNode だけ複写）。
identifier は `isSimpleInlineableExpression` でない（93030-93032：`!isIdentifier && isSimpleCopiableExpression`）ので
`readExpression = createTempVariable(hoist)`（`_b`）、`initializeExpression = createAssignment(_b, clone)`。
set helper の receiver は `_b = clone`、get helper の receiver は `_b`。clone は position を持たないので、印字時に class alias `_a` へ substitute された後も map されない。
`this` / literal（inlineable）は clone をそのまま両 helper に使う（同じく unmapped）。update 式（`visitPreOrPostfixUnaryExpression` 96486-96551）も同じ helper を通る。

### 6.2 現行 Rust

`downlevel.rs:4436-4452`（compound）と `downlevel.rs:1495-1515`（update）は `stabilize_inline_receiver(receiver)`（`downlevel.rs:7772-7789`）を使う：
inlineable なら `clone_node(receiver)`（`factory.rs:5514`：pos/end = u32::MAX、`set_original_node(clone, original)`；上流の `cloneNode` と同じ）、
そうでなければ temp `_b` を作り **`create_assignment(target, receiver)` に visited receiver（parsed `B` node）をそのまま置く**。
上流は clone を置く。parsed `B` は source range を持つので、`_b = _a` の右辺（substitute 後の `_a`）に `B` の leading / trailing map が出る
（T1 の diff：generated (26,60)→(2,20)、(26,62)→(2,21) が Rust だけに存在、JS bytes は一致）。

C02 決定 11 の `stabilize_receiver`（`downlevel.rs:7742`、call binding）は逆に「clone に位置を戻す」修復だった（`createCallBinding` は receiver node 自体を target と `thisArg` に再利用するため）。
両者は上流の別関数に対応し、混同しない：`createCallBinding` = 同一 node の再利用（mapped）、`createCopiableReceiverExpr` = synthesized clone（unmapped）。

### 6.3 Gap matrix

| row | 現行 symbol | 分類 | 証拠 |
| --- | --- | --- | --- |
| temp 初期化の右辺 | `stabilize_inline_receiver`（temp 分岐） | `partial-or-stale`（visited node を置く；上流は clone） | T1 known-native 1 行、新対照 `receiver-map/es2015/static-compound-*` |
| inlineable 分岐 | 同（`clone_node`） | `already-exact` | T1 `static-this` / `instance-this` exact、新対照 `instance-this-compound*` |
| call binding の receiver | `stabilize_receiver` | `already-exact`（C02 決定 11） | 新対照 `static-read` |
| update 式 | `downlevel.rs:1495-1515` | 同じ temp 分岐（同じ修復で閉じる） | 新対照 `static-postfix-*` / `static-prefix-*` |
| temp の comment range | `set_private_receiver_comment_range(initialized)` | `already-exact`（synthesized node の `{-1,-1}`） | T1 packet：`static-compound` の parse node に range 無し |

### 6.4 設計決定

1. **temp 分岐の初期化右辺は `clone_node(receiver)`。** `initialized = create_assignment(target, clone)`：clone は synthesized（unmapped）で original = receiver（上流 `cloneNode` と同じ metadata 複写）。読み取り `read` は temp のまま。
2. **すべての receiver range を一括で消さない。** inlineable 分岐（既に clone）と `stabilize_receiver`（call binding、mapped が正しい）は変更しない。
3. **評価順序・回数・式の値は変わらない**（生成 JS bytes は修復前後で同一：complete-command 契約が保護）。runtime control は不要（式が変わらないため）；対照は value used / discarded、side-effect receiver、update 式を含む。

### 6.5 変更ファイル

| file | 変更 |
| --- | --- |
| `crates/emitter/src/builtins/class_fields/downlevel.rs` | `stabilize_inline_receiver`：temp 分岐で clone を初期化右辺にする |

### 6.6 対照（新 ID `post-t1-residuals/receiver-map/…`、18 件）

read / simple set / compound（`+=`、`*=`）/ update（prefix / postfix、value used / discarded）、static / instance（`this`）、
安定した receiver（identifier / `this`）/ 副作用あり（`B.make().#q`、`B.pick().#p`、括弧付き）、bundle の第 1 / 第 2 source、outDir（packet 無し）、関数内 class、ES2015 / ES2022。

## 7. PRIVATE-SET-COMMENTS — private setter helper の右辺のコメント所有権

（観測後に §7.2 以降を確定。§7.1 は上流の経路。）

### 7.1 上流の経路

`B.#p = v`：class fields `visitBinaryExpression`（96694-96785）は `setTextRange(setOriginalNode(createPrivateIdentifierAssignment(...), node), node)`：
helper call は代入式 `node` の range と original を持ち、引数 `[receiver, brand, value, kind, …]` のうち `value` は visited parsed node。
printer：call の `emitCommentsBeforeNode` が `containerPos/End = node.pos/end` を claim；引数 list `emitNodeListItems`（120068-120155）は
`previousSibling.end !== parentNode.end` のときだけ区切り前に `emitLeadingCommentsOfPosition(previousSibling.end)` を出す（`value.end === call.end` なので出ない）。
`value` 自身の trailing は `emitTrailingCommentsOfNode` の `end === containerEnd` skip（121033-121046 + 121234-121238）で出ず、call の後で一度だけ出る。
上流は `value` に emitNode を作らない。

### 7.2 現行 Rust（開始 SHA）

`downlevel.rs:7687-7720 create_private_set`：`value` に `NO_TRAILING_COMMENTS` を加える（「helper 引数内と original-linked 外側式の二重印字防止」）。
consumer：`printer.rs:17543 emit_list_element_end_comments_in_container`（`NO_TRAILING_COMMENTS` で skip、else 要素 end の同一行 trailing + leading-of-position）、
`printer.rs:8447 emit_child_boundary_comments_before_parent_end`（同 flag で skip）。

### 7.3 観測と disposition

REPORT §3.4 の対照（前置 / 後置 / 行末 / 括弧 / comma / 引数境界 / 複数行 / removeComments / instance / compound / 値利用 / outDir）で
flag を外した Rust の公開出力が上流と一致するか、一致しない場合にどの consumer が二重印字するかを確定する。決定は §7.4。

## 8. DECORATOR-COMMENTS — decorator 式の `NoComments`

（観測後に §8.2 以降を確定。§8.1 は上流の経路。）

### 8.1 上流の経路

`transformDecorator`（100554-100569）：`expression = visitNode(decorator.expression, visitor)`（identifier / access なら **同じ parse node**）→
`setEmitFlags(expression, NoComments)`（parse node の emitNode に 3072）→ access 式なら `createCallBinding(expression, hoist, languageVersion, cacheIdentifiers=true)`：
identifier receiver は temp に入り（`_a = ns`）、target は新 PropertyAccess（range = callee、flags 無し）→ `restoreOuterExpressions(expression, createFunctionBindCall(target, thisArg, []))`。
結果は `_classDecorators = [dec]` / `_m_decorators = [ns.dec.bind(_a = ns)]` に置かれる。Bundle root では parse node の flag が declaration transform まで残る（T1 probe で観測）。
native 経路（ESNext × define）は transform しない。

### 8.2 現行 Rust（開始 SHA）

`standard_decorators.rs:2819-2833 transform_decorator_expressions`：`visit(decorator)` → `bind_decorator_expression(visited)`。visited 式に `NO_COMMENTS` を書かない。

### 8.3 観測と disposition

REPORT §3.5 の対照（decorator の前後 / 式内部 / computed name 境界のコメント、単一 / 複数、native / lowered、removeComments、bundle / outDir）で確定する。決定は §8.4。

## 9. 検証コマンドと retire

- before（開始 SHA、正規 runner）：[records/before/](records/before/)（`t1-witness`：18 → 15 exact / 3 known / 0 failed、packet 12 / 3；`pipeline-witness`：4 → 0 exact / 4 known / 0 failed）。
- 新対照：`python3 scripts/witness.py post-t1-residuals --all`（observer `--check` → 2 tests；101 complete commands、bundle 行の packet probe）。
- 子ごとの編集ループ：`TSC_RS_POST_T1_RESIDUALS_CASE_SET=<family>/ cargo test --test post_t1_residuals_contract …`（oracle は実行しない：REPORT に argv / env / 件数 / exit を残す）。
- 合成後：依頼書 §5 の `post_t1_pipeline`（4 件）と `bundle-metadata-t1 --all`（18 件）を正規 runner で再実行し、修復が確認できた known 行を retire する
  （`decorator-binding-known-native.json` の 4 行、`bundle-metadata-t1-known-native.json` の 3 行、`bundle-metadata-t1-known-packet.json` は §7 / §8 の決定に従う）。
- 隣接：`witness.py bundle-declarations --all`、`bundle-program --all`、`module-identities --all`、`bundle-original-javascript --all`、`decorator-binding --all`（direct）、
  `decorator-binding-pipeline --case lifecycle/` ほか変更 owner の focused set；全 767 / SUPER / retained / acceptance は hosted（統合担当）。
