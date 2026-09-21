# H2.8a-A-RES-EMITTER-FINAL — 設計（EF1〜EF8：source trace、gap、architecture 判定、修復単位）

2026-09-17（r8 追補・最終計測 2026-09-18）。担当：Claude（調査・実装候補）。統合担当：shared CI 登録・hosted・最終 qualification・PR/merge。
本書は [依頼書](README.md) §4 の設計契約に従い、子ごとに **production 編集前の** 固定 upstream span/hash、
実 trace、Rust producer/consumer、gap matrix、変更ファイル、対照、runtime 対照要否、architecture 行を記録する。
実行結果（before → after、件数、logs）は [REPORT.md](REPORT.md) と [records/](records/)。
新台帳（EF7）は [ledger/](ledger/)。

## 1. Identity / purpose / boundary

- 親 ID `H2.8a-A-RES-EMITTER-FINAL`。開始点：main `c35e00ccb006e3b4e3e2643491e8fe3207595097`（PR #556、PR #555 `ce39261ac` を含む）、
  worktree `../tsc-rs-emitter-final`、branch `draft/h2-8a-emitter-final`（[records/start.v1.json](records/start.v1.json)）。
- semantics / byte 期待値：vendored TypeScript 6.0.3（`_tsc.js` `1c59e77a…`、`typescript.js` `56917765…`）。
- 範囲：採用中の通常 compiler emit（JS / d.ts / map / d.ts.map、module/target、outFile/outDir、診断・exit・callback metadata）。
  LSP、一般公開 API、TS7 移行、Program 再利用、CLI 表示は別フェーズ（依頼書冒頭）。
- 件数は重複し得る（依頼書 §1）。本書の子 ID は EF1〜EF8 とその下の原因別 ID（`EF<n>-<cause>`）。

## 2. Required-reference table

| 参照 | 用途 |
| --- | --- |
| [design index](../../../README.md) → [emitter architecture](../../emitter-architecture.md) §4 `E-COMMENT-SCOPE-H` / `E-POSITIONS` / `E-NAMES-CLASS-G` / `E-METADATA-G-CLASS`、§6 | 現行 owner と open constraint |
| [schedule](../../post-h1-completion-slices.md) 冒頭（live profile = H2.8a） | readiness / close 条件 |
| [依頼書](README.md)、[inventory.v1.json](inventory.v1.json)（`inventory.py --check` exit 0） | 入口 ID・状態・source hash |
| [共通 handoff 手順](../claude-high-difficulty-handoffs.md)、[witness 方針](../../../../witness-testing.md) | 観測契約、focused/hosted 分担 |
| [POST-T1 DESIGN §8](../h2-8a-post-t1-residuals/DESIGN.md) / [REPORT §3.5](../h2-8a-post-t1-residuals/REPORT.md) / [統合記録](../h2-8a-post-t1-residuals/integration/README.md) | EF1 の凍結 5 行と owner |
| [PLAN-BASE](../plan-base/README.md) | EF4〜EF7 の履歴 disposition |

## 3. 固定 upstream span（vendored `_tsc.js`、SHA-256 先頭 16 hex は行範囲のバイト列）

| symbol | lines | sha256[:16] | 使う子 |
| --- | --- | --- | --- |
| `emitPropertyAccessExpression` | 118223-118249 | `16bbe4828853fce2` | EF1 |
| `emitElementAccessExpression` | 118268-118274 | `19be0bb4ca3c146a` | EF1 |
| `emitCallExpression` | 118275-118290 | `0c5b07dd72b5883e` | EF1 |
| `emitTokenWithComment` | 118731-118752 | `0074985c553cb844` | EF1 |
| `emitNodeListItems` | 120068-120158 | `9046ff5ab901c6c8` | EF1 |
| `pipelineEmitWithComments` | 120978-120986 | `263af5299b06aaec` | EF1 |
| `emitCommentsBeforeNode` | 120987-120994 | `dc59a0901c0b5aaa` | EF1 |
| `emitCommentsAfterNode` | 120995-121006 | `f0baac32a6d9fcf8` | EF1 |
| `emitLeadingCommentsOfNode` | 121007-121032 | `ce6bf342a94094cc` | EF1 |
| `emitTrailingCommentsOfNode` | 121033-121046 | `e5c99d84eeab2c12` | EF1 |
| `emitTrailingComments` | 121176-121190 | `5aefffe5fbff9050` | EF1 |
| `forEachLeadingCommentToEmit` | 121219-121233 | `2e1fb613c9b9bb29` | EF1 |
| `forEachTrailingCommentToEmit` | 121234-121238 | `bd6612ac9040b10e` | EF1 |
| `createCallBinding` | 24691-24740 | `dd40a5b622ee1bd3` | EF1 |
| `createFunctionBindCall` | 24577-24579 | `be58956cd65c50b4` | EF1 |
| `transformDecorator`（esDecorators） | 100554-100569 | `d59f7ca48434d473` | EF1 |

（EF2〜EF8 の span は各節に追記する。）

## 4. EF1 — bound decorator target の末尾コメント（printer、`E-COMMENT-SCOPE-H`）

### 4.1 上流の経路（実 trace：`post-t1-residuals/decorator-comments/es2022/set-property-access`）

入力 `@/* a */ ns./* b */dec /* c */ class B …`、`@ns.dec /* d */ m()`。
`transformDecorator`（100554-100569）：visited 式（parse node `ns./* b */dec`）に `NoComments`、
`createCallBinding(expression, hoist, languageVersion, cacheIdentifiers=true)`（24691-24740）→ `thisArg = _a`、
`target = createPropertyAccessExpression(paren(_a = ns), callee.name)` + `setTextRange(target, callee)`（**fresh node、original 無し、flags 無し、range = `ns./* b */dec`**）、
`createFunctionBindCall(target, thisArg, [])`（24577）→ `target.bind(_a)`（**synthesized** PropertyAccess + Call）。
印字：array literal の要素 → `emitCallExpression`（118275）の `emitExpression(node.expression)` → `.bind` access（synthesized、pos/end = -1、comments phase は extent gate で何も claim しない）→
`emitPropertyAccessExpression`（118223）の `emitExpression(node.expression)` → **target の `pipelineEmitWithComments`（120978）**：
`emitLeadingCommentsOfNode`（121007）は `containerPos = target.pos`、`containerEnd = target.end` を claim、`emitLeadingComments(target.pos)` は同一行の `/* a */` を収集しない（`iterateCommentRanges` の leading は改行後にのみ collecting）；
worker：`(_a = ns)` → `emitTokenWithComment(DotToken, expression.end, …, target)`（118731）は `getParseTreeNode(target)` が無いので **`/* b */` を出さない**；`emit(name)`：`dec` の trailing は `end === containerEnd` で skip；
**`emitCommentsAfterNode`（120995）→ `emitTrailingCommentsOfNode`（121033）が container を復元してから `emitTrailingComments(target.end)`（121176）で `/* c */` を出す**（`forEachTrailingCommentToEmit`（121234）：外側 container の end と異なる）。
結果 `[(_a = ns).dec /* c */.bind(_a)]`。`set-parenthesized`（`@(/* r */ ns.dec /* s */)`）は `restoreOuterExpressions` の updated paren（original = parse paren、`NoComments` merge）が `( /* r */` を token の trailing-of-position で出し、target の phase が `/* s */` を出す。

### 4.2 現行 Rust（開始 SHA）

| 段階 | symbol | 状態 |
| --- | --- | --- |
| producer | `builtins/standard_decorators.rs` `bind_decorator_expression`（POST-T1 決定 2：fresh target + `set_text_range`、NoComments） | already-exact（node 構成は上流と同じ、DESIGN §8.4 POST-T1） |
| 汎用 node pipeline | `printer.rs` `emit_node_with_hint_and_source_comments_worker`：source comment phase は **親が deferred request を渡した node だけ**で走る（owner = その node、leading at pos / trailing at end、container claim は `active_expression_comment_scope`） | premise |
| Call arm | `NodeData::CallExpression`：callee に `DeferredExpressionSourceComments::nested(…, LeadingAndTrailing)` を渡し、callee 自身の phase を走らせる（118275 の `emitExpression`） | already-exact |
| **PropertyAccess / ElementAccess arm** | `NodeData::PropertyAccessExpression` / `ElementAccessExpression`：expression 子を `emit_node_id_with_forwarded_source_comments` で印字。pending request（no-ASI 転送 lane）があればそれを転送し、**無ければ子は source phase 無し**で印字。子の末尾境界は access token の `emit_token_with_comments(owner = access node)` の `BoundaryUnion`（`node_has_source_token_shape(owner)` = 「similar node」gate）に委ねる | **gap**：owner（`.bind` access）が synthesized だと token phase は source comment を出さず、target 自身の phase も走らないので `/* c */` が消える。parse-tree の親（`a.b /* t */;`、`a.d /* t4 */()`、`[a.b /* t6 */]`、`a.b /* m3 */.c /* t3 */`：records/probes/probe1）は token/terminator 境界が claim するため一致 |
| consumer（token anchor） | `deferred_source_comments.visited_trailing_anchor_at(token_cursor)`：転送 lane で子が末尾 phase を visit した場合、access token はその anchor（`CommentResume`）を使って二重出力を避ける | already-exact（転送 lane 限定） |

### 4.3 Gap matrix

| 観測 | 上流 | Rust（開始 SHA） | 分類 |
| --- | --- | --- | --- |
| `[(_a = ns).dec /* c */.bind(_a)]` 5 行（es2015/es2022/esnext × set-property-access、es2022/esnext × set-parenthesized） | target 自身の trailing phase | phase 無し、synthesized 親は claim しない | **gap（本子）** |
| `[( /* p */dec /* q */)]`、`( /* r */…` | token trailing-of-position（updated paren は original を持つ） | exact | already-exact |
| parse-tree access（probe1 の 10 形、es2022）、transform 由来の synthesized 親（probe2-A：class field 代入、export 代入、enum、namespace、optional chain） | 各 node の phase | exact（token / terminator 境界が claim） | already-exact（回帰対照） |
| `/* a */`（`@` 直後の同一行 comment）、`/* b */`（`.` 直後） | 出さない | 出さない | already-exact |

### 4.4 設計決定（EF1-ACCESS-TARGET-PHASE）

1. **access の target 子に通常の comments phase を与える**（`emit_access_target_with_source_comments`）：pending request があれば従来どおり転送、inherited `NoNested` extent なら phase 無し、それ以外は Call arm と同じ `nested(expression_context.comments(), LeadingAndTrailing)`。
   `expression_context.comments()` は access node 自身の claim を含む active scope なので、parse-tree access では子の leading（同 pos）は container-owned prefix で skip、子の末尾は access 末尾と異なるので visit される（上流の containerPos/containerEnd と同じ判定）。
2. **token anchor の共有**：子の outcome `Complete { trailing: VisitedHere { anchor } }` の cursor が access token の cursor と一致する場合はその anchor を token emission に渡す（転送 lane と同じ規約 `access_target_trailing_anchor_at`）。parse-tree access で token phase が同じ境界を二度 claim しない。
3. **owner が synthesized の場合**：token phase は従来どおり何も出さない（similar-node gate は変えない）。target の phase が上流と同じ位置に `/* c */` を出す。
4. ElementAccess も同じ（`open_anchor`）。Conditional の条件・TaggedTemplate の tag・PartiallyEmitted・Binary 左辺の転送 arm は本子では変更しない（Binary は `separator_anchor_after_child` が子境界を claim 済み。Tagged template の tag は EF7 で照合）。
5. flag 整数値や producer 側の細工で見かけを合わせない。runtime 対照は不要（JS 式の意味・順序は変わらず、コメント/map のみ）。

### 4.5 変更ファイル

| file | 変更 |
| --- | --- |
| `crates/emitter/src/printer.rs` | `emit_access_target_with_source_comments` / `access_target_trailing_anchor_at`（新規）、PropertyAccess / ElementAccess arm の子印字と token anchor |
| `crates/compiler/tests/fixtures/post-t1-residuals-known-native.json` | 修復確認後に 5 行を retire（元 projection は [records/ef1/](records/ef1/) に保存） |

### 4.6 対照

- 主：`post-t1-residuals` の `decorator-comments/` 27 行（before / after：records/ef1/）、最終は `witness.py post-t1-residuals --all`（101）と `bundle-metadata-t1 --all`。
- 回帰：records/probes/probe1（10 形）・probe2（12 形 × es2022/es5）の tsc 直接比較、`printer --all`、`compact-body-comments --all`、`prologue-comments --all`、`declaration-comments --all`、emitter `comment_scope_witness_contract` を含む emitter integration suite、`retained` の decorator/accessor 群、`decorator-binding-pipeline` の lifecycle/global/nested 群。
- 追加候補（EF7 で確定）：synthesized 親内の parse-tree access に同一行 trailing comment を持つ native 対照（tagged template の tag、no-ASI 転送 lane、`?.` chain）。

## 5. EF2 — H2.5h の 12 known（ES5 lowering）

入口：[emitter_final_rows](../../../../../crates/compiler/tests/emitter_final_rows.rs)（12 行を `ratchets/h2-5h-qualification.v1.json` の凍結観測へ
qualified-vfs / `Established` floor で 2 回 replay、typed diff、`KNOWN` に retire 規約）。開始 SHA の再測定は 12/12 DIVERGING（records/measure/ef2-ef3-rows-20.meta.json）。
差分を原因別に分割した結果（probe2-B / probe7 の直接比較で再現）：

| 子 ID | 行 | 差 | 上流 | Rust（開始 SHA） | 決定 |
| --- | --- | --- | --- | --- | --- |
| EF2-PROMISE-CTOR | `asyncAwait_es5`、`asyncImportedPromise_es5` | `__awaiter(this, void 0, MyPromise, …)` の第 3 引数が `void 0`；import された Promise 型の `require` が消える | `transformAsyncFunctionBody` 101315-101320 → `getPromiseConstructor(nodeType)` 101432-101441（`getEntityNameFromTypeNode` 14623-14635、`resolver.getTypeReferenceSerializationKind` が `TypeWithConstructSignatureAndValue`/`Unknown` のとき entity name）、`createAwaiterHelper` 25824-25850 の `createExpressionFromEntityName` 27330-27338 | `es2017.rs create_awaiter_call` は常に `create_void_zero`；checker 側の `mark_async_function_alias_referenced` は存在 | **実装**：`get_promise_constructor(original)`（原本 function の `type` → entity name → serialization kind）、`create_expression_from_entity_name`（clone + `set_text_range`、QualifiedName → PropertyAccess）、`Es2017Visitor` に `target` を伝搬 |
| EF2-ARROW-PARENS | `emitAccessExpressionOfCastedObjectLiteralExpressionInArrowFunctionES5` | `return ({…}[x]);` の括弧が無い | `updateArrowFunction` → `createArrowFunction` 22701-22735 → `parenthesizeConciseBodyOfArrowFunction` 20514-20523（leftmost が ObjectLiteral または comma sequence）| `factory.rs create_arrow_function` は適用するが、TypeScript pass の構造 update（`update_node` → `apply_parenthesizer_rules`）に arrow の concise body 規則が無い | **実装**：`parenthesize_updated_arrow_concise_body` を `apply_parenthesizer_rules` に追加 |
| EF2-ASYNC-SUPER | `emitter.asyncGenerators.classMethods.es5`（C9） | Rust が `var _super_1 = Object.create(null, {g: …})` を出し `_super_1.g.call(this)`；上流は ES2015 lowering の `_super.prototype.g.call(this)` | `emitSuperHelpers` 101243 / 101378 / 102657：`languageVersion >= ES2015` かつ（ES2017 の method では）async generator でない場合のみ `_super` access object | `es2017.rs plan_async_super_capture` と `es2018.rs transform_function` に target gate が無い | **実装**：es2017 は `target < ES2015 || is_async_generator(function)` で capture 無し；es2018 は capture 構造（boundary の balance）を保ったまま ES5 では `owns_access=false`（`super_capture_is_active` は `owns_access` を見る）。r5 の第 1 案（boundary ごと外す）は `RequiredChildRemoved(async-generator super capture)` で失敗 |
| EF2-CLASS-NAME | probe2-B `D_1`、`decoratedBlockScopedClass2/3` の一部（class alias 番号は別原因） | ES-decorators が作る無名 class expression の ES5 constructor 名が `class_1`（上流 `D_1`）| `generateName` → `getNodeForGeneratedName` 28084-28102（original chain の根）→ `generateNameForNode` 120876-120942 / `generateNameCached` 120633-120637 | `es2015.rs get_generated_name_for_node` は要求 node 自身の kind で base を決める | **実装**：base と cache key を `get_original_node(node)` の根で決める |
| EF2-BLOCK-SCOPED-DECORATED | `decoratedBlockScopedClass2/3` | try block 内の legacy-decorated class の宣言名が `Foo_1` に rename されない（alias `Foo_2` も `Foo_1` のまま） | `substituteIdentifier` 108020-108029（`isNameOfDeclarationWithCollidingName` → `resolver.isDeclarationWithCollidingName`）、alias は `createUniqueName` の print 順 | probe6：decorator 無しは一致 → checker flag と ES2015 substitution は動く；legacy-decorators が作る `let Foo` 宣言名に substitution が届かない | **調査継続**（owner：`legacy_decorators.rs` 宣言名 identity / `es2015.rs substitute_identifier`）。alias は `allocate_class_alias` の TargetBinding なので rename が TargetBinding になれば finalizer が `Foo_2` を与える見込み |
| EF2-READ-COMMENT | `destructuringVariableDeclaration1ES5iterable` | `__read([1, "string"]` の直後に次行の `//` コメントが混入 | `emitNodeListItems` 120068-120158：`previousSibling.end !== parentNode.end` のときだけ `emitLeadingCommentsOfPosition`；`createReadHelper` の call は `setTextRange(…, location)` 93538-93546 で end が一致 | `flatten_destructuring.rs` は `set_text_range(read, location)` 済み；printer `emit_comma_element_end_comments` も `end == parent.end` を見る → 差の位置は未確定（call arguments の別経路の可能性） | **調査継続**（probe7/read-comment で再現） |
| EF2-FOR-OF-COMMENTS | `ES5For-of37`、probe2-B `/* t5 */` | converted loop の `_c = a.e /* t5 */; … /* t5 */; … /* t5 */` と try/finally 内の先頭コメント位置 | `convertForOfStatementForArray` 106666-106725 は各 node に `setTextRange(node.expression)`；printer は各 node 自身の trailing phase（`emitTrailingCommentsOfNode`）で出す | `ForStatement` arm は initializer/condition/incrementor を `LeadingOnly` phase で印字し末尾を `;`/`)` token の BoundaryUnion に委ねる → owner が synthesized（original は ForOfStatement、similar でない）だと出ない | **設計 EF2-COMMENT-BOUNDARY**（§5.1） |
| EF2-DESTRUCTURING-DEFAULT-COMMENT | probe2-B `/* t4 */` ×2 | `p = _b === void 0 ? a.b /* t4 */ : _b /* t4 */`（上流は前者を出さない） | conditional の whenTrue `a.b` は `emitTrailingCommentsOfNode` で containerEnd（宣言 range = location の end）と一致し skip、宣言自身の phase が末尾で出す | `ConditionalExpression` arm の `separator_anchor_between_child_and_token` は親 node の range だけで container を判定（inherited scope の claim を見ない） | **設計 EF2-COMMENT-BOUNDARY**（§5.1） |
| EF2-FOR-AWAIT-USING | `awaitUsingDeclarationsInForAwaitOf.3` | `for await (await using …)` が配列 for へ落ち `__asyncValues` protocol が消える | ESNext using → ES2018 for-await の順序と `AwaitContext`/downlevelIteration | 未 trace | **調査継続**（owner：es2018 for-await / esnext using） |
| EF2-TEMP-NAMES | `awaitUsingDeclarationsInForAwaitOf`（`e_2_1`）、`awaitUsingDeclarationsInForOf.1`（`_i`/`_a`）、`.5`（`_a_1`） | 生成名の番号・family | `createUniqueName` / `createLoopVariable` / `getGeneratedNameForNode` の print 順 | 未 trace | **調査継続**（owner：E-NAMES finalizer と esnext using の temp family） |

### 5.1 設計 EF2-COMMENT-BOUNDARY（printer、EF1 の一般化）

上流は全 node に `pipelineEmitWithComments` を走らせ、fixed token（`;` `)` `:` `in` `of` `while`）は
`emitTokenWithComment` で「位置の leading comment」だけを出す（118731-118752）。Rust は子の phase を
「親が deferred request を渡した node」に限り、子の末尾を親 token の `BoundaryUnion` に委ねる。
親 token の owner が synthesized（similar-node gate 不成立）または container 判定が inherited scope を
見ない場合に差が出る（EF1、EF2-FOR-OF-COMMENTS、EF2-DESTRUCTURING-DEFAULT-COMMENT、既知の
inherited red `compact_private_function_body_emits_inter_statement_comment_once`）。

決定（実装は EF2-FOR-OF / DESTRUCTURING の修復と同じ子で行い、対照で保護する）：
1. 「親 token が続く子式」（For 3、ForIn 2、ForOf 2、With 1、while clause 1、If 1、Conditional whenTrue 1）は
   `LeadingAndTrailing` の完全 phase を子自身が持ち（`emit_child_after_token_with_context_and_source_extent`）、
   続く token は子の visited anchor（`CommentResume`）を受け取って `SourceLeading` phase だけを行う
   （`emit_token_with_source_leading_comments` 系）。BoundaryUnion の「前の子の同一行 trailing」半分は子の phase に移る。
2. container 判定は子の phase 内の `retains_end`（inherited scope + 親 claim）に一本化する
   （`separator_anchor_after_child` の親 range だけの判定は使わない）。
3. Binary 左辺・Call callee・PropertyAccess/ElementAccess target は EF1 で導入した子 phase + anchor 規約に揃える。
4. 保護：records/probes（probe1/2/7）、`printer --all`、`compact-body-comments --all`、`prologue-comments`、`declaration-comments`、
   emitter `comment_scope_witness_contract`、`retained` の decorator/accessor 群、`decorator-binding-pipeline` 群。

**実装（r9、EF2-COMMENT-BOUNDARY、`crates/emitter/src/printer.rs`）**：レビュー指摘「probe2/B は実際に未修復（分割代入 `/* t4 */` の二重出力、for-of `/* t5 */` の欠落）」への修復。

| 単位 | 変更 | 上流対応 |
| --- | --- | --- |
| `emit_child_with_trailing_phase_before_token`（新規） | 「親 token が続く子」に `LeadingAndTrailing` の完全 phase（`emit_child_after_token_with_context_and_source_extent`）を与え、visited trailing anchor（cursor が子の original end と一致するもの）を返す。RetainedByParent / NoSourceRange は素の end cursor | `pipelineEmitWithComments`（120978-121046）＋ `emitTokenWithComment`（118731-118764）の順序 |
| `visited_trailing_anchor_at_cursor`、`separator_anchor_after_child_phase`（新規） | 子 phase の outcome から token anchor を選ぶ（parsed separator が別位置なら separator 自身の start＋`child_owned_trailing_resume_at_cursor`）。子の trailing を再出力する経路を持たない | `emitConditionalExpression` の `emitTokenWithComment(ColonToken, node.whenTrue.end)` |
| For（3 子）/ ForIn（2）/ ForOf（2）/ With / `emit_while_clause`（While・Do）/ If / Switch | 子の phase → 続く `;` `)` `in` `of` は `emit_token_with_source_leading_comments` / 新規 `emit_for_binding_keyword_with_source_leading_comments`（`SourceLeading` phase＝位置の leading comment だけ。旧 `BoundaryUnion` の「前の子の同一行 trailing」半分は子の phase へ移動）。子が無い For 節（`for (;;)`）だけ従来の BoundaryUnion を保つ。If / Switch は旧 `token_owned_child_prefix`＋`emit_leading_comments_for_node_worker`＋phase 無し印字から同じ deferred phase へ統一 | `emitForStatement` 118613-118624 ほか各 `emitTokenWithComment(…, child.end, …, node)`：similar でない owner は何も出さず、子自身の trailing phase だけが `/* t5 */` を出す |
| Conditional `whenTrue` | 完全 phase＋`separator_anchor_after_child_phase` | `whenTrue` の `emitTrailingCommentsOfNode`：`end === containerEnd`（flattener の宣言 range）で skip → `/* t4 */` は宣言 container が末尾で 1 回出す |
| `separator_anchor_after_child` / `separator_anchor_between_child_and_token`（Conditional condition、Binary 左辺） | `active_scope: CommentEmissionScope` を受け取り `child_trailing_comments_escape_active_container`（inherited scope の `retains_end` を先に見る；active container が無いときだけ親 range）で判定 | `forEachTrailingCommentToEmit` の `containerEnd` guard（決定 2） |
| 旧 `emit_for_binding_keyword_with_comments` | 削除（呼び出し元なし） | — |

結果：probe2/A・B とも tsc と byte 一致、records/probes 33 形すべて出力ファイル単位で一致（`records/measure/probes-r9a`）、emitter `--lib` 508、`--test contracts` 452/452（§4.7）、witness 18 suites（REPORT §8.1 r9）。

### 4.7 補足 — 「inherited red」`compact_private_function_body_emits_inter_statement_comment_once` の正体（r9）

レビュー指摘「private method 内のコメント二重出力は通常 emit の未解決不具合」を上流で再検証した結果、**二重出力は TypeScript 6.0.3 自身の出力**である
（[records/compact-private-body/](records/compact-private-body/)：`_tsc.js` の CLI と `transpileModule`、target ES2015 / ES2022、関数宣言・メソッド・private method・object method の 4 形すべてで
`{ first(); /* c */ /* c */ second(); }`）。原因は `emitNodeListItems`（120068-120158、`SingleLineFunctionBodyStatements`）が 2 文目に対して
`emitTrailingCommentsOfPosition(child.pos)` を呼び、1 文目の `emitTrailingCommentsOfNode` が既に出した同じ range を再度出すこと。
`compact-body-comments` witness suite（240 行、tsc 採取）の `middle/*` 行は parsed body でこの重複を凍結済みで、tsc-rs の printer は一致している。
tsc-rs CLI の出力は同入力で tsc と byte 一致（sha256 `27725962…`）。したがって赤だったのは期待値（Aug 15 の commit 315746b77 が観測なしに `1` と書いた）であり、
test を `compact_private_function_body_duplicates_inter_statement_comment_like_tsc`（count 2、完全な行を照合）に修正した。producer 側の修復対象ではない。

## 6. EF3 — H2.6a/6c の 8 unique IDs

入口：同 [emitter_final_rows](../../../../../crates/compiler/tests/emitter_final_rows.rs)（6c 8 行 = `MapFamilyWithDeclarationOnly`、
6a 共有 1 行 = `SourceMapWithOptions`、recorded-compiler-plan / qualified-vfs、typed refusal を別報告）。

| 子 ID | 行 | 差（開始 SHA） | 原因 | 決定 / 結果 |
| --- | --- | --- | --- | --- |
| EF3-HARNESS-FLOOR | `jsFileCompilationWithMapFileAsJsWithOutDir`、`requireOfJsonFileWithSourceMap`、`sourceMapValidationVarInDownLevelGenerator`、`sourceMapWithCaseSensitiveFileNamesAndOutDir` | 出力 path が `/.src/` のまま（`out/` でない）、TS5055 上書き診断、helper が inline（`noEmitHelpers`） | `crates/harness/src/upstream_suites/execution.rs apply_compiler_setting` が `outdir` / `noemithelpers` を全 floor で drop していた（上流 harness は適用して観測） | **実装**：map-family 2 floor（`MapFamily`、`MapFamilyWithDeclarationOnly`）で admit。他 floor の凍結 admission は不変。r2 で **4 行 exact ×2** |
| EF3-ISOLATED | `isolatedModulesSourceMap` | typed refusal `isolatedModules` | `execute.rs validate_emit_options` の route gate。TypeScript pass は `should_preserve_const_enums`（宣言保持）と `try_substitute_constant_value` の `isolated_modules` 早期 return（`tryGetConstEnumValue` 相当）を route 非依存で実装済み；checker は isolatedModules 診断を保持 | **実装**：`isolatedModules` の gate 除去（`verbatimModuleSyntax` は transpile 限定のまま）。r2 で **exact ×2** |
| EF3-CASE-CANONICAL | `sourceMapWithNonCaseSensitiveFileNamesAndOutDir`（差は map `sources` の `app.ts` vs `../testFiles/app.ts` のみ）、`sourceMapWithNonCaseSensitiveFileNames`（refusal `useCaseSensitiveFileNames`＋outFile） | 大小無視 host での相対 path | 凍結観測は oracle host `getCanonicalFileName: identity`（`crates/oracle/h2-6c-qualification.mjs:724-731`）で採取。実 tsc を大小無視 host（canonical = lowercase）で実行すると `app.ts`（Rust と同じ）：`getRelativePathToDirectoryOrUrl` は canonical 化した成分を比較する（records/probes 参照：node 直接実行） | **方針 1 採用（integrator 指示）**：oracle host が directive を尊重する（`ts.createGetCanonicalFileName`、VFS lookup も同じ canonical、元の綴りは保持）。修正案 patch・新旧観測差分（mirror は凍結 6c 書き込みを byte 一致で再現）・再採取範囲・検証手順は [records/oracle/case-canonical-proposal.md](records/oracle/case-canonical-proposal.md)。`…AndOutDir` は tsc-rs の最終バイトが提案観測と 4 write すべて一致（診断なし・exit 0）。`outFile` 行の refusal guard は候補で再採取観測（mirror）との完全一致を確認済み（capture `ef2-ef3-r8c`：writes 2＝`c509c0df0dc2…`/`083fcce15b19…`・map `sources` `app.ts`/`app2.ts`・診断 TS5101・exit 2）→ `execute.rs` の guard を候補で解除。integrator の再採取と Rust 比較の完了まで 2 行は未解決 |
| EF3-ITERABLE-2318 | `sourceMapValidationDestructuringForArrayBindingPattern`（6a/6c 共有） | 診断 TS2318 `Cannot find global type 'Iterable'`（lib=es5、target es2015 の配列 destructuring）が出ず exit 0 | checker：`getGlobalIterableType(reportErrors)` の報告経路 | **checker owner**（通常 emit の complete command に必要な診断）。調査継続 |

## 7. EF4 / EF5 — 旧 class 40 commands

入口：[emitter_final_batch](../../../../../crates/compiler/tests/emitter_final_batch.rs) `ef4_ef5_…`（40 行を共有 comparator で 2 回、
`TSC_RS_EMITTER_FINAL_CAPTURE_DIR` で writes を保存）。開始 SHA：**8 exact / 32 failed**（class-header-token 8 行は既に exact）。
JS diff（records/measure の capture + probe3）で 5 原因に分割：

| 子 ID | 行 | 差 | 上流 | Rust | 決定 |
| --- | --- | --- | --- | --- | --- |
| EF4-DEFAULT-NAME | `legacy-bound-this` ×4 | `function default_1()` vs `class_1`（EF2-CLASS-NAME 修正後は `default_2`） | `getName` → `getGeneratedNameForNode(classExpr)` → `generateNameCached` は宣言 node あたり 1 slot（transformTypeScript の `default_1` と同じ） | ES2015 `get_generated_name_for_node` は pass ごとの cache で **新しい** numbered binding を割り当てる；transformTypeScript は `default_1` の生成 identifier を中間 ClassDeclaration の `name` に残す | **実装（v2）**：origin cache miss 時に original chain を辿り、中間の Class/Function 宣言・式の `name` が generated identifier ならその `TargetBinding` を再利用（`generated_declaration_name_binding_on_chain`）。v1（assigned name 追跡）は class_fields の comma 展開で initializer が class expression でなくなるため不十分 |
| EF4-NESTED-THIS | `nested-computed-name` ×8（es5/es2015 × cjs/esnext × define/set） | 入れ子 class の computed method 名 `[this.key]` を `_a.key` に置換 | `visitThisExpression` 97136-97150 は **現在の** class lexical environment で置換し、`visitInNewClassLexicalEnvironment` は入れ子 class 進入時にそれを undefined にする（print 時の `lexicalEnvironment.previous` switch は onEmitNode だけ） | `visit_computed_property_expression` が class element name のとき `enclosing_class_evaluation()`（1 つ外の class の static frame）へ transform 時に横断して置換 | **実装**：transform 時の横断を廃止（現在 frame = 入れ子 class の `ClassBoundary` → 置換なし）。print 時 frame（`before_emit_node` の ComputedPropertyName）は維持 |
| EF4-FILE-THIS-CAPTURE | `concise-arrow` ×4、`legacy-static-block` ×4（es5） | 先頭の `var _this = this;` が無い | ES2015 `visitArrowFunction` は arrow の `ContainsLexicalThis` flag（class-fields は `this` を tree に残し print 時に `_a` へ置換）から `CapturedLexicalThis` を立て、SourceFile で `addCaptureThisForNodeIfNeeded` | Rust は arrow 内の `this` を visit した場合だけ capture する（class-fields が置換済み） | **調査・実装**（owner `EA-GAP-FLAGS` / `es2015.rs` hierarchy facts） |
| EF4-ARROW-CRASH | `field-arrow` ×4、`static-block-arrow` ×4（es5） | `transform removed required child generated binding identifier from Block` で command 失敗 | `mergeEmitNode` は `autoGenerate` を複写しない | class_fields が `this` を generated identifier `_a` に置換した後、ES2015 `transform_function_body` が concise body を Block に変換し `set_original(block, body)`；`EmitMetadata::merge_from` が generated-binding fields を Block に複写し finalizer が非 identifier を検出 | **実装**：`set_original_node` は identifier/private identifier 以外の node へ generated-binding fields を継承しない（`clear_generated_binding`） |
| EF5-ESCAPED | `hoisted-export/es5/{amd,commonjs}/class/{escaped,direct-escaped}` | `Foo` を `Foo` と印字（class 名、`.prototype`、`exports.Bar = …`） | `getTextOfNode` 120442-120465：clone（`setTextRange` 済み）は source text | ES2015 `get_name` の clone は位置を持たず（byte-exact 適応）、printer の `transformed_identifier_can_reuse_source_spelling` は位置一致を要求 | **実装**：`clone_node` が identifier clone に `cloned_identifier_spelling` を記録し、printer は original の source slice を使う |

### 7.1 r7 — 残り原因の分割（DESIGN 追補）

r6 計測（26/40）後の残り 14 行 + EF2 残 8 行を probe で再分割した結果（records/measure `*-r7*`、scratchpad probe8–11）：

| 子 ID | 行 | 上流 | Rust（r6） | 決定 |
| --- | --- | --- | --- | --- |
| EF2-ASYNC-ALIAS-MARK | `asyncImportedPromise_es5` | `checkAsyncFunctionReturnType` 82513：ES5 分岐は check 時に `markLinkedReferences(node, AsyncFunction)` → import を referenced に | `check_async_function_return_type` は「markLinkedReferences は emit 専用」として省略；unchecked file の emit walk しか到達しない | **実装**：`mark_linked_references_async_function` 前置き（verbatimModuleSyntax / ambient ガード）を ES5 分岐先頭で呼ぶ。probe8 exact |
| EF2-ASYNC-GEN-BODY-FLAG | `emitter.asyncGenerators.classMethods.es5` C9 | `createAsyncGeneratorHelper` は generator 関数式に `AsyncFunctionBody | ReuseTempVariableScope`；ES2015 は `AsyncFunctionBodyExcludes` で入り `NonStaticClassElement` を保つ → `_super.prototype.g` | `create_async_generator_outer_body` は flag 無し → `FunctionExcludes` で `_super.g` | **実装**：flag 付与。probe7/async-gen-super exact |
| EF2-TOP-LEVEL-FOR-AWAIT | `awaitUsingDeclarationsInForAwaitOf.3`（部分） | ES2018 `visitForOfStatement` は `awaitModifier` があれば常に `transformForAwaitOfStatement`（module top-level でも） | `current_function_is_async()` ゲートで async 関数外は ES2015 配列 loop | **実装**：ゲート撤去（labeled 版も）＋ `plan_for_await_lowering` の「enclosing async function 必須」を撤去（`createDownlevelAwait`：async generator 以外は素の `await`；chain8 で `transform removed required child enclosing async function` を検出して修正）。行は `await using` の欠落 binding 名（下記）が残る |
| EF5-BARE-CLONE-SPELLING | `hoisted-export/es5/{amd,cjs}/class/direct-escaped` | `cloneNode(name)` 単体は synthesized・parent 無し → `getTextOfNode` は `idText`（`exports.Foo`）；`getName` 系だけ `setTextRange`+`setParent` で source spelling | r5 の `cloned_identifier_spelling` を `clone_node` 全体で立てていた | **実装（最終）**：`clone_node` は flag を立てず、`clone_node_with_source_spelling`（ES2015 `get_name` の適応 site）が立てる；`EmitMetadata::merge_from` の継承（`|=`）は **維持**（chain8 の probe3/escaped で `return \u0046oo` / `exports.Bar = \u0046oo` が退行：この port の transform は上流が parse 名を保持する箇所で位置無し clone を後続 pass に渡すため、getName 由来の clone chain は mark を運ぶ必要がある）；上流の bare clone site `createExportExpression` だけ `create_export_access_from_module_name` で mark を明示的に落とす（printer trace `build-cli-r7-debug4c`）。generators の hoisted 名は上流も bare clone。r7b：escaped/direct-escaped ×4 exact ×2 |
| EF2-ARROW-BLOCK-ORDER | `concise-arrow` ×4（括弧側） | `visitFunctionBody` は `convertToFunctionBlock` を `updateArrowFunction`（concise-body parenthesizer）より前に適用 → `return _a = …, _a;` | `update_generic` が先に括弧を付け、`install_function_bindings` が `return (…)` にする | **実装**：`strip_update_introduced_concise_parentheses`（元の arrow body が括弧でない時だけ剥がす）。probe11/P7 exact |
| EF2-BLOCK-SCOPED-DECORATED | `decoratedBlockScopedClass2/3` | `declName = getInternalName(node)`（ES5、`LocalName|InternalName`）は colliding-name 置換をスキップ；wrapper の `var` は `getLocalName(node)`（clone、original あり）→ `Foo_1` に改名 | `let Foo = …` の name は parse identifier そのもの（改名される）、wrapper の var は text から新規 identifier（改名されない） | **実装**：`create_declaration_head_name` / `create_wrapper_local_name`。残差：alias の採番（下記 EF2-ALIAS-NUMBERING） |
| EF2-ALIAS-NUMBERING | 同上（残差） | printer の `makeUniqueName` は emit 順で `Foo_1`（var）→ `Foo_2`（alias） | ES2015 の colliding 改名 binding は print 時 substitution で生成され finalize walk の event に現れず、legacy ClassAlias（同じ numbered family）が先に `Foo_1` を取る | **実装（r8）**：ES2015 が colliding 宣言名（parse identifier、InternalName 以外）を visit 時に `getGeneratedNameForNode` の同じ slot の generated identifier に置き換える（`colliding_declaration_name_substitute`、`visit_variable_declaration` と `let` list 経路の両方；trace で wrapper の var は `let` list 経路だけを通ると判明）。置換 identifier は `generated_binding_print_order` を持ち、finalize walk の宣言名 pre-pass（`generateNames` 相当）と numbered scope pass から除外して traversal 位置で採番。visit 時の resolver 問い合わせは tsc の `substituteIdentifier` と同じく `enabledSubstitutions & BlockScopedBindings` で gate する（有効化点＝block-scoped 宣言 list／`for` initializer／名前付き class expression は対象名の visit より前；gate 無しでは stub resolver の contract test 3 本が `Resolver(Unavailable)` で落ちた、r8-patches-8）。さらに finalize walk の numbered scope pass を「moment ごとの group を traversal がその moment に入った時点で採番」する形に再構成（root group は traversal 前）：tsc は `generateNames(body)` を各 body の emit 開始時に走らせるため、print 時置換名（block の `var Foo_1`）→ その IIFE の hoisted alias（`Foo_2`）→ 次の block（`Foo_3`…）の順になる（decoratedBlockScopedClass2/3 の両方に必要） |
| EF4-FILE-THIS-CAPTURE | `concise-arrow`/`field-arrow`/`legacy-static-block` ×12 | class-fields `visitThisExpression` 97136 は static **block** だけ transform 時置換；property initializer の `this` は tree に残り print 時 `substituteThisExpression`。ES2015 は `this` token（`ContainsES2015|ContainsLexicalThis`）を visit し `CapturedLexicalThis` → SourceFile の `var _this = this` | class-fields は `StaticThisSubstitution::Emit` でも transform 時に identifier へ置換（original/range 付き）；ES2015 pass は `this` を見ない | **実装**：置換 identifier に `ContainsES2015|ContainsLexicalThis` を付与し、ES2015 `visit_identifier` が original=ThisKeyword の identifier に `visitThisKeyword` の facts を適用（`note_lexical_this_use`）。debug trace（records/measure `build-cli-r7-debug*`）で判明した真因：class-fields 後の全木 `compute_transform_flags`（EA-GAP-FLAGS 分類器）が identifier を 0 に再分類していた → `static_this_substitute_flags` で `this` token 由来の 2 bit を保持。class declaration（wrapper 内、StaticInitializer facts）は上流も capture しない（probe11/P3）。r7b：class 40 = **40/40** |
| EF2-DETACHED-COMMENT | `ES5For-of37` | `detachedCommentsInfo` は nodePos に一致する **最初の** node が消費（入れ子深さ非依存） | `PendingDetachedComments` は statement list ローカル；synthesized `try` 配下の `for` が先頭コメントを再印字 | **実装（r8）**：printer に `carried_source_detached`（`Cell<Option<CommentResume>>`）を追加。source file の先頭 statement が synthesized で resume を消費できなかった場合に carry し、`emit_leading_comments_for_node` が nodePos 一致の最初の node で消費（tsc の `detachedCommentsInfo` pop と同じ 1 回性）。**r8e 退行と修正（r8-patches-7）**：最初の chain10（`*-r8e`）で emitter lib の `hoisted_exports_leave_detached_comments_for_the_function_declaration` が赤（CommonJS の `// detached header` が 2 回）。source file の statement loop は先頭の synthesized statement で pending を carry に移すが、carry を見るのは入れ子 node の経路だけで、後続の top-level statement（hoisted `exports.x = x` の後ろに残る宣言、nodePos 一致）は pos 0 から全走査していた → statement loop でも `take_carried_source_detached_for_node` を試す（`hasDetachedComments(pos)` は深さに依らず最初の一致 node） |
| EF2-AWAIT-USING-MISSING-NAME | `awaitUsingDeclarationsInFor*` ×4 | `for (await using of x)` の欠落 binding は temp（`_e`/`_e_1`）；temp 順序 `_i,_a`… | Rust は `value_1` 系の名前・順序 | **実装（r8）**：parser は上流どおり空の declaration list を返す（vendored `typescript.js` で確認：`decls=[]`、diagnostics 無し）。es_next `visit_for_of_statement` が空 list に対し `createVariableDeclaration(createTempVariable(undefined))`（temp `_e`）を合成し、loop binding を `getGeneratedNameForNode(temp)`（`_e_1`、`allocate_numbered_derived`）にする（`value` 採番を廃止）。finalize walk：ordinary temp を親に持つ derived binding は scope pass で保留し、traversal で先に現れた時点で親 temp を採番してから派生名を付ける（tsc の `getTextOfNode(generated)` → `generateName` と同じ順序；r8 中間バイトでは親の provisional `_tmp_1` が漏れて `_tmp_1_1` になった）。numbered 親（ES2018 の `e_4` → `e_4_1`）も同様に、派生 entry の処理時に未採番なら親 entry を先に採番する（従来は planned `e_1` で `e_1_2` になっていた既存差分） |
| EF2-LOOP-VARIABLE-POLICY | `awaitUsingDeclarationsInForOf.1/.5`（r8 中間バイトで露出：`var _i, _a, …` が `var _a, _b, …`）；probe16 A–E（generator / async 関数内の配列 for-of は全て同型：`function* g() { for (const x of [1, 2]) yield x; }` でも再現）| `createLoopVariable` の identifier は `cloneNode` でも `autoGenerate`（Loop kind）を保ち、`generateNames(body)` が hoisted `var _i` を `makeTempVariableName(TempFlags._i)` で `_i` にする | ES2015 は `_i` を LoopVariable policy で作るが、generators pass が hoisted 宣言名／`_i = 0` の代入先として **再生成** した identifier は binding id だけを引き継ぎ policy を落とす（`TargetBinding::from_existing` の 8 caller は loop flag を渡さず、`EmitMetadata::merge_from` も `generated_binding_loop_variable` を伝播しない）→ print 時 finalize walk の最初の event（hoisted `var`）が FinalizerTraversal で `_a` を取る | **実装（r8）**：`from_existing` に `loop_variable` を追加（8 caller が `generated_binding_is_loop_variable()` を渡す）、`merge_from` が loop flag を `|=` で伝播。既存の非 generator 行は for 文自身の宣言（元 node）が最初の event だったため露出しなかった（潜在差分：generator/async 内の ES5 for-of 全般） |
| EF3-ITERABLE-2318 | `sourceMapValidationDestructuringForArrayBindingPattern` es2015 | `@lib: es5` + `let [...x] = [array literal]` → contextual `getTypeFromArrayBindingPattern`（rest-only）→ `createIterableType` → `getGlobalIterableType(true)` → TS2318（global） | probe10：`let it: Iterable<number>` は TS2304 を出す（lib は効いている）が TS2318 は出ない；`get_type_from_array_binding_pattern`/`create_iterable_type`/`get_global_symbol` の port は一致 | **実装（r8）**：trace（`build-cli-r8-debug`）で rest-only 分岐と `get_global_iterable_type(true)` の到達を確認；差は診断の可視化：tsc の `getDiagnosticsWorker` は file check 中に増えた global 診断（`deferredGlobalDiagnostics`）をその file の semantic 診断として返すが、この port は provenance-checked getter だけが `publish_visible_global_diagnostics_since` で公開する（`get_global_iterable_iterator_type` は公開、`get_global_iterable_type` は未公開）→ `get_global_iterable_type` に公開を追加。probe10/nolib で global 診断経路自体は動作していることを確認。同族 getter（iterator / generator / async 系）は同じ未公開状態（follow-up、owner checker） |
| EF6-IMPORT-TYPE-SELF | `…AnonymousWithSub` ×2 | `getSymbolChain` の parents loop：`parent.exports.get("export=")` が symbol と同参照なら `accessibleSymbolChain = parentChain`（chain = [module] → `import(".")`） | `chains.rs` は `alias_for_symbol_in_module` に同等分岐を持つが `getSymbolChain` 相当の parents loop が無い | **実装（r8）**：trace（`TSC_RS_DEBUG_CHAIN`、`build-cli-r8b`）で `symbol_chain_slice` の export= 短絡は存在するが `containers_of_symbol_slice(__class)` が `[]` を返すことを確認 → `getContainersOfSymbol` 49999-50007 の class-expression candidate（`<access> = class` の左辺が `module.exports`/`exports.x` なら source file symbol、それ以外は左辺 receiver の resolved symbol）を `class_expression_assignment_container` として追加 |

## 8. EF6 — 旧 global 14 IDs

入口：`ef6_…`（`assert_output_matrix_projection` の 14 行、`TSC_RS_H2_8A_FAILURE_DIR` で actual/expected 保存）。
開始 SHA：**10 exact / 4 failed**（後続修復記録 7 行は全て exact；未確認 7 行のうち `jsDeclarationsParameterTagReusesInputNodeInEmit1/2`、`bundlerImportTsExtensions` も exact）。

| 子 ID | 行 | 差 | 上流 | Rust | 決定 / 結果 |
| --- | --- | --- | --- | --- | --- |
| EF6-UMD-FACTORY | `reactImportDropped` | `Emit(Transform(UnknownNode(0:140856)))`：`.js`/`.tsx` の JSX と `export = React; export as namespace React` | `getReferencedImportDeclaration` 87900-87918 は他 file の NamespaceExportDeclaration を返し得る；consumer は ImportClause/ImportSpecifier 以外を無視 | `checker/src/emit.rs` の wrapper が返り値 node を **参照側の source** で `EmitResolverNode::new` していたため、jsx.rs の same-source guard が他 file の node id を現 arena に引いて失敗 | **実装**：5 wrapper を `project_resolver_node(state, …)` に、CJS/System の import-binding lookup に same-source filter。r3 で **exact ×2**（probe4 も一致） |
| EF6-IMPORT-TYPE-SELF | `jsDeclarationsExportAssignedClassExpressionAnonymousWithSub` ×2 | `.d.ts` の `instance: import(".")` が `exports` | `symbolToTypeNode` 53114-53182：chain[0] が external module symbol なら `import(specifier)`（`module.exports = class` の自己参照は chain 長 1） | `chains.rs chains_symbol_to_type_node` は同構造；`lookup_symbol_chain` が JS `module.exports` の class に対し module symbol ではなく `exports` 名の symbol を根にする疑い | **調査継続**（owner：checker node builder `lookup_symbol_chain` の JS export= 扱い） |
| EF6-JSDOC-LINK | `linkTagEmit1` | `.d.ts` の `@property` コメントから `{@link NS.R}` が消える | JS declaration emit は `getTextOfJSDocComment` 11773-11775（link part は `formatJSDocLink` 11776-11781）で合成 comment を作る | checker node builder `type_nodes.rs preserve_comments_on` は `JSDocText` part だけ連結（typedef alias 側 `statements.rs` は link を扱う） | **実装**：共有 `js_doc_comment_text`（両 site） |

## 9. EF7 — 未観測範囲と不足 producer の台帳

[ledger/](ledger/)（`build_ledger.py --check` exit 0）：PLAN-BASE 6,045 ID（＋EF1 5）を **6,806 行 / 24,875 観測単位** に展開し、
state を CE / RP / RD / UP / RM / OB / UX で付与（件数非加算）。要点：旧 217 ID は全て RM（どの profile も選択していない；
noEmit route 106、noEmitHelpers 89、allowJs 15、verbatimModuleSyntax 5、jsx 3、isolatedModules 2 …）、global 769 は hosted 入口なし（RP）、
class 40 は local 入口のみ、7c/7de/8a の later references は件数のみ。追加子の提案は [ledger/README.md](ledger/README.md)（EF7-a..h）。
本依頼で採用・着手した追加子：EF3-HARNESS-FLOOR（runner）、EF3-ISOLATED（option gate）、EF6-UMD-FACTORY（resolver bridge）。

### 9.1 EF7-UNIVERSE — 未観測範囲の実観測（r9）

レビュー指摘「未観測範囲の扱いが調査止まり」への対応。台帳の RM 行を **fresh TS 観測＋Rust complete-command replay** で CE / RD に確定する。

| 単位 | 内容 |
| --- | --- |
| ID 集合 | [ef7/universe-217.v1.json](ef7/universe-217.v1.json)（旧未観測 217：compiler 61 / conformance 156）、[ef7/universe-plan-base.v1.json](ef7/universe-plan-base.v1.json)（PLAN-BASE 未観測 1,817 のうち compiler/conformance 1,809；project 8 行は `skipped` に理由付きで残す） |
| observer | `scripts/observe-emitter-final-universe.mjs`（`--write [--set 217|plan-base]`、`--check`、`--self-check`）。入力 builder は `crates/oracle/h2-8a-candidates.mjs` の `directiveInput` 一式を verbatim に持ち、起動時に凍結 `h2-8a-candidate-inputs.v1.json` の compiler/conformance 621 行を byte 一致で再生成することを自己検証する。観測は `h2-8a-observations.mjs` の `observe` と同じ（fresh Program、`emitFilesAndReportErrorsAndGetExitStatus`、2 回一致）。差分は 1 点：標準 lib に位置する診断の `file` / message 内の vendored lib path を Rust memory host と同じ `/lib/<name>` に射影する（809 候補には無かった形） |
| fixture | `crates/compiler/tests/fixtures/emitter-final-universe.json`（217、1.7 MB）、`emitter-final-universe-plan-base.json`（1,809） |
| Rust entry | `crates/compiler/tests/emitter_final_universe.rs` → `h2_7d_original_corpus_shared::replay_universe_fixture`（769 候補と同じ comparator・memory host・2 回 replay、`TSC_RS_EMITTER_FINAL_FAILURE_DIR` に diff 保存）。`KNOWN` = attributed open rows、retire assertion 付き。option projection に `experimentalDecorators` / `emitDecoratorMetadata` / `noUnusedLocals` / `noUnusedParameters` / `strictBuiltinIteratorReturn` を追加（217 行が到達する 26 option 名の残り 5） |
| 結果 | REPORT §9（r9）と [ledger/DELTA-r9.md](ledger/DELTA-r9.md) |

#### 9.1.1 217 行の初回 replay（r9b、開始 SHA の producer）と原因分割

`universe-r9b`：**exact 88 / failed 129**。129 行を panic / diff 種別で分割し、原因別に修復した（r9c で再計測）。

| 子 ID | 行 | 差 | 上流 | Rust（r9b） | 決定 |
| --- | --- | --- | --- | --- | --- |
| EF7-NOEMIT-ROUTE | 106（`route:noEmit=true` 行すべて；期待 = 診断＋exit、writes 0、`emit_skipped:false`、`emitted_files`/`source_maps` null） | `load_emitting_program: InvalidInput { ValidateOptions … noEmit=true }` | `handleNoEmitOptions` → `program.emitBuildInfo()` の空結果（build-info 無し）＋ `emitFilesAndReportErrorsAndGetExitStatus` の順序付き診断 | H0 loader は emitting program として `noEmit` を拒否する設計；CLI の `execute_explicit_files` は `noEmit` で `load_program`（checking program）へ分岐し `emit_command_for_harness` に NoEmit 分岐がある | **実装（replay 側）**：`h2_7d_original_corpus_shared::prepared` が CLI と同じ分岐（`no_emit == Some(true)` → `load_program`）を採る。producer 変更なし |
| EF7-SYSTEM-USING-EXPORT | 12（`usingDeclarationsWithESClassDecorators.*#module=system,target=esnext`） | `using before_1 = before = null;` が `before = null;` | `transformSystemModule.visitVariableStatement` 112639-112657：`isVarUsing`/`isVarAwaitUsing` の list は宣言を `getGeneratedNameForNode(name)` に改名し `transformInitializedVariable(variable, false)` で初期化した using 文として残す | `system.rs transform_hoisted_variable_statement` に using 分岐が無く、通常の式文へ落としていた | **実装**：`transform_hoisted_using_statement`（`allocate_numbered_name` の numbered binding、既存 flatten plan、export は従来どおり後置） |
| EF7-MEMBER-NAME-LEADING-COMMENT | 3（`esDecorators-classDeclaration-commentPreservation` ×2 module、`classExpression-commentPreservation`） | decorator/modifier と member 名の間の独立行コメント（`@dec\n/*6*/\nmethod() {}`）が消える | `emit(node.name)`（pipelineEmitWithComments）の leading phase が `/*6*/` を出す（name.pos = 最後の decorator/modifier の end） | Method/Property/GetAccessor/SetAccessor の名前は `emit_required_identifier_name_with_context`（phase 無し） | **実装**：`emit_member_name_leading_comments`（container prefix で name.pos == member.pos を除外、own-line のみ；Parameter は既に `emit_optional_ordinary_child` の完全 phase） |
| EF7-DECORATOR-PARSE-PARENS | 1（`esDecorators-decoratorExpression.1`） | `@new x` / `@x?.y` が `@(new x)` / `@(x?.y)` | `emitDecorator` 117872-117875 は `parenthesizeLeftSideOfAccess` を渡すが、`pipelineEmit` は substitution された node にしか rule を適用しない；parsed decorator は原文どおり | Decorator arm が常に `left_side_of_access(false)` context（factory 時の括弧付けを print 時に再現） | **実装**：`parse_tree_node(node) == node` の decorator は `NORMAL`、synthesized（`createDecorator` 経由）は従来どおり |
| EF7-VERBATIM-GATE | 4（`verbatimModuleSyntax*`） | typed refusal `UnsupportedCompilerOption("verbatimModuleSyntax")` | 通常 emit（診断付き） | `validate_emit_options` の transpile 限定 admission | **実装**：refusal 撤去（TS pass は `verbatim_module_syntax` を route 非依存で読む） |
| EF7-METADATA-INERT | 1（`esDecorators-emitDecoratorMetadata#target=esnext`） | typed refusal `UnsupportedCompilerOption("emitDecoratorMetadata")` | TS5052（experimentalDecorators 無し）を報告して通常 emit（metadata は legacy decorators 限定） | `emit_decorator_metadata && !experimental_decorators` を拒否 | **実装**：refusal 撤去。TS5052 の報告は program option 診断の owner（r9c で照合） |
| EF7-ENUM-18055 | 1（`computedEnumMemberSyntacticallyString2#isolatedmodules=true`） | TS18055 欠落 | `computeConstantEnumMemberValue` 85646-85651：`getIsolatedModules && typeof value === "string" && !isSyntacticallyString` | `evaluate.rs` が「isolatedModules は拒否されるため elide」と記録 | **実装**（checker）：arm を追加（`isolatedModules || verbatimModuleSyntax`） |
| EF7-PARSE-DIAGNOSTICS-BOUNDARY | 1（`esDecorators-decoratorExpression.3`、TS1146 ×2 の recovery 入力） | typed refusal `ParseDiagnosticsDeferred { owner_slice: "H2.9" }` | recovery 後の木を emit | 入力 syntax boundary（`remainingOwners`：parse diagnostic units → H2.9） | **KNOWN（UP、owner H2.9）**：本依頼の通常 emit 範囲外の typed boundary。行は `KNOWN` に owner 付きで残す |

r9 最終バイト（chain15 `universe-r9`）：217 = **exact 214 / failed 3**（parse-diagnostics boundary 1 ＋ `verbatimModuleSyntax*CJS` 2 = 下表 EF7-VERBATIM-EXPORT-ASSIGNMENT）。

#### 9.1.2 PLAN-BASE 1,798 行の初回 replay（r9 最終バイト、chain15 `universe-plan-base-r9`）と r10 の原因分割

途中集計（先頭 345 行：317 exact / 28 failed）から分割した原因。全行の確定値は REPORT §9.6。

| 子 ID | 行（途中集計） | 差 | 上流 | Rust（r9） | 決定 |
| --- | --- | --- | --- | --- | --- |
| EF7-YIELD-PARENS | `es5-asyncFunctionBinaryExpressions` ×2 target | `Math.pow(x, (yield y))`、`x, (yield y)`、ES5 では `[(_d.sent())]` | `updateBinaryExpression` の parenthesizer（`binaryOperandNeedsParentheses`）：yield（優先順位 2）は右結合演算子の右辺（`=`、`**`）と comma では括弧なし | `es2017.rs yield_requires_parentheses` が BinaryExpression 親を代入右辺以外すべて括弧付け（Rust 独自規則） | **実装（r10）**：親 BinaryExpression は演算子優先順位・結合性で判定（`factory::binary_operator_precedence` を pub(crate) 化）、`typeof`/`void`/`delete`/`await` 親は prefix-unary 規則で括弧 |
| EF7-ASYNC-ARROW-LEXICAL-THIS | `asyncArrowInClassES5` ×2 | `var _a; _a = Test;` と `__generator(_a, …)` が無い | `createArrowFunction` 22709-22710：async arrow は `ContainsES2017 | ContainsLexicalThis`（`__awaiter(this, …)` の this 捕捉）→ class-fields の static initializer facts → class alias | `compute_transform_flags` の ArrowFunction arm は ES2015/TS flag だけ | **実装（r10）**：arm に async → `CONTAINS_ES_2017 | CONTAINS_LEXICAL_THIS` を追加（builtins.rs） |
| EF7-GENERATOR-LOOP-VARIABLE | `es5-asyncFunctionForInStatements` | 2 番目以降の関数の for-in 変換で `_i` が `_j`/`_p`/`_t` | `transformAndEmitForInStatement` の `createLoopVariable()`＝`_i` family、printer の tempFlags は関数ごと | `generators.rs allocate_loop_variable_binding` が `allocate_planned`（綴り固定）で、finalize walk が file 単位の衝突回避で改名 | **実装（r10）**：es2015.rs と同じ `allocate_planned_loop`（LoopVariable policy、printer scope ごとに再採番） |
| EF7-OBJECT-LITERAL-INDENTED | `es5-asyncFunctionObjectLiterals` | generator 内 `_a = {\n a: y\n};` の property/閉じ括弧が 1 段深い | ES2015 `visitObjectLiteralExpression`：`A || (hasComputed = B)` — yield 含み property で短絡すると `hasComputed` は代入されず、`Indented` は付かない | `has_computed = is_computed` を無条件に代入 | **実装（r10）**：短絡どおりに分岐（es2015.rs） |
| EF7-EXPORTED-REST-HOIST | `exportObjectRest` ×5 module | `exports.{ x, ...rest } = …`（CJS）、`export var _a = …, x = _a.x`（ESM） | ES2018 `visitVariableStatement`（export）→ `exportedVariableStatement` → `flattenDestructuringBinding(…, hoistTempVariables=true)`：temp を hoist し `{ x } = (_a = …, _a)` と pending expression を inline | es2018 の `DestructuringPlan` に hoist 形が無く temp を宣言していた | **実装（r10）**：`exported_variable_statement` / `hoist_destructuring_temps` 状態、`DestructuringPlan.pending_expressions`、`plan_push`（pending の inline）、`materialize_binding_plan` の末尾 temp（tsc の `flattenDestructuringBinding` 末尾） |
| EF7-CJS-FLATTENED-EXPORT-NAME | `exportEmptyArrayBindingPattern` / `exportEmptyObjectBindingPattern` ×2 module（`MixedSyntheticRange`）、`exportObjectRest` CJS の `exports.{…}` | CJS が原本宣言の binding pattern を publication name に使い、`set_direct_export_statement_range` が synthesized name の pos で map range を作る | `transformInitializedVariable` は現在の `node.name`（flatten 後の temp）を使う | `publication_name` が `get_original_node(declaration).name` を無条件採用 | **実装（r10）**：identifier 原本だけ綴りを donate、synthesized name は statement の start を採用（builtins.rs） |
| EF7-SYSTEM-IMPORT-HELPERS | `importHelpersSystem` ×2 target | System で helper が inline、`tslib` dependency / setter が無い | `collectExternalModuleInfo` 92875：`createExternalHelpersImportDeclarationIfNeeded` → `externalImports.unshift`、setter `tslib_1 = tslib_1_1`、`substituteExpressionIdentifier` が HelperName を `tslib_1.__extends` に | system.rs は importHelpers を知らない（CJS だけ `collect_external_helpers_import`） | **実装（r10）**：collector を free fn `collect_external_helpers_import_declaration` に共有、System が dependency group 先頭 / hoisted name / print 時 HELPER_NAME 置換を持つ |
| EF7-HELPERS-IMPORT-PROLOGUE | `importHelpersES6` | `import {…} from "tslib"` が hoisted `var _A_x;` より前 | ES module `updateExternalModule`：`copyPrologue` は custom prologue（`EmitFlags.CustomPrologue` の hoisted 宣言）も先に写す | `insert_external_helpers_import_declaration` が directive prologue だけ飛ばす | **実装（r10）**：CUSTOM_PROLOGUE 文も飛ばす |
| EF7-VERBATIM-EXPORT-ASSIGNMENT | `verbatimModuleSyntaxNoElisionCJS`、`verbatimModuleSyntaxRestrictionsCJS`（217 集合）、plan-base の同族 | `module.exports = I` / `exports.default = I`（型のみの export）が消える | TS pass `visitExportAssignment`：`compilerOptions.verbatimModuleSyntax || resolver.isValueAliasDeclaration(node)` | `is_value_alias_declaration` だけ | **実装（r10）**：verbatim を先に見る（builtins.rs） |
| EF7-ENUM-ISOLATED / EF7-ENUM-18056（checker） | `blockScopedEnumVariablesUseBeforeDef_{isolatedModules,verbatimModuleSyntax}`、`enumNoInitializerFollowsNonLiteralInitializer` | TS2450（const enum の used-before-declaration）/ TS18056 欠落 | `checkResolvedBlockScopedVariable` 48468-48470、`computeEnumMemberValue` 85621-85630（`getIsolatedModules` = isolatedModules ‖ verbatimModuleSyntax） | 「isolatedModules は拒否されるため elide」 | **実装（r10）**：両 arm を復元（resolve.rs、evaluate.rs） |
| EF7-JSDOC-CHECK-NODE（checker） | `arrowExpressionBodyJSDoc` | TS2322 の位置（JSDoc cast の括弧 vs 内側） | `getEffectiveCheckNode` 76190-76193：JS file では `ExcludeJSDocTypeAssertion` | 括弧を常に skip | **実装（r10）**：JS file では JSDoc type assertion で停止（functions.rs、EF7 r9 の `checkReturnExpression` と同族） |
| EF7-SYMBOL-WRITTEN-FACE（checker） | `errorInUnnamedClassExpression` | `class '__class'` vs `'Foo'` | `symbolToString(parentSymbol)` = written face（`getAssignedName` → `Foo`） | `symbol_display_name`（escaped name） | **実装（r10）**：`symbol_name_as_written_slice`（access.rs、r9 の TS2855 と同族） |
| 未着手（attributed、KNOWN 候補） | `deeplyNestedMappedTypes`（型表示 / 深さ）、`contextuallyTypedParametersOptionalInJSDoc`（JSDoc optional param）、`genericDefaultsJs`（JS generic defaults）、`excessPropertyCheckIntersectionWithRecursiveType`（related info）、`ctsFileInEsnextHelpers`（`.cts` からの tslib 解決 TS2354）、`importNonExportedMember9`（TS2616 vs TS2597）、`importHelpersWithLocalCollisions#es2015`（ESM helper alias `__extends_1` の typed boundary） | 診断のみ / 型表示 | — | — | REPORT §9.6 の KNOWN 表に owner 付きで残す（checker: 型表示・JSDoc・module 解決；emitter: ESM helper alias） |

#### 9.1.3 PLAN-BASE 全行 replay（chain15 `universe-plan-base-r9`：exact 1,626 / failed 172）から分割した r11 の原因

r10 の probe（28 形）で閉じなかった残りを、失敗 capture（`TSC_RS_EMITTER_FINAL_FAILURE_DIR`）と tsc 直接採取の probe（[records/patches/README.md](records/patches/README.md) r11、scratchpad `r11probes/` 37 形）で分割した。r11 最終バイトの全件 replay：1,798 = **exact 1,731 / KNOWN 67**（emit bytes の divergence 0；REPORT §9.7–§9.8）。

| 子 ID | 行（r9 census） | 差 | 上流 | Rust（r10） | 決定 |
| --- | --- | --- | --- | --- | --- |
| EF7-USING-HOISTED-CLASS-NAME | `usingDeclarationsWithESClassDecorators.{1,2,3,5,6}` × commonjs/esnext/system（10） | `__setFunctionName(_classThis, "default")` が tsc は `"C"` | transformESNext `hoistClassDeclaration` 103601-103640 は NAMED class を `C = class C {}`（`convertToClassExpression` は name を保つ）へ変換し、`isNamedEvaluation` は名前付き class では偽 → transformESDecorators は宣言名を installs | `class_expression_runtime_name` が owner ClassDeclaration の class expression を無条件に `AnonymousDefaultDeclaration`（`"default"`） | **実装**：明示・非 generated 名があれば `Declared(name)`（standard_decorators.rs） |
| EF7-SYSTEM-EXECUTE-ASYNC | `topLevelAwait.1#module=system` ×2 target、`awaitUsingDeclarationsTopLevelOfModule.1#module=system` | `execute: function ()` が tsc は `execute: async function ()` | `createSystemModuleBody` 112209：`node.transformFlags & ContainsAwait ? Async modifier`；`ContainsAwait` は `createAwaitExpression`（22753）だけが立て（`await using` の宣言 list・`for await` の修飾子 token は立てない：records/probes `sysusing` = target esnext の `await using` は `execute: function ()` のまま）、関数境界で止まる | System pass は modifier を作らない | **実装**：`source_contains_top_level_await`（AwaitExpression を関数境界で止めて walk；lowering 後の `await using`/`for await` は生成された await 式で数える）→ `async` token（system.rs） |
| EF7-SYSTEM-HOISTED-DEFAULT-EXPORT | `usingDeclarationsWith{ES,Legacy}ClassDecorators.{3,4}#module=system`、`awaitUsingDeclarationsTopLevelOfModule.1#module=system` | `_default = X` が tsc は `exports_1("default", _default = X)` | transformSystemModule `getExports` 113318-113336：resolver 解決できない FileLevel|Optimistic|ReservedInNestedScopes の generated 名（transformESNext の `_default`）は `moduleInfo.exportSpecifiers.get(name)` で export 名を得る | `exports_for_identifier` が synthesized 原本で早期 return | **実装**：CJS と同じ `file_level_generated_binding_exports.get_for_identifier`（system.rs） |
| EF7-VERBATIM-IMPORT-EXPORT-ELISION | `verbatimModuleSyntaxNoElisionESM`、`verbatimModuleSyntaxInternalImportEquals`、`packageJsonImportsErrors` ×3 module | `import {} from`、`export {}`（原文）、`export { A }`（型のみ）が消え、末尾に合成 `export {}` | TS pass `visitNamedImportBindings` 95548 / `visitNamedExports` 95589：`allowEmpty = verbatimModuleSyntax`；`visitExportSpecifier` 95599：`verbatimModuleSyntax ‖ isValueAliasDeclaration` | r10 の EF7-VERBATIM-EXPORT-ASSIGNMENT は `export default` だけ | **実装**：3 gate を追加（builtins.rs；原文 `export {}` が external-module indicator になるので合成 `export {}` は付かない） |
| EF7-ASYNC-ARROW-SUPER-CAPTURE | `asyncMethodWithSuper_es6` | 非 async method 内の async arrow の `super.x` が `_super` 捕捉されない | transformES2017 `transformMethodBody` 101236-101262 は async でない method/accessor/constructor でも checker の `MethodWithSuperProperty*InAsync`（arrow の async を container に付与、48xxx `checkSuperExpression`）で `_super`/`_superIndex` を挿入；`_superIndex` は emit helper なので body は multi-line（`emitBlockFunctionBodyWorker`） | `transform_function` は async のときだけ `plan_async_super_capture` | **実装**：Method/Get/Set/Constructor は常に plan、非 async body は prologue 直後に挿入（`insert_sync_super_capture_statements`）、element access ありは `set_multi_line`（es2017.rs） |
| EF7-ES5-ANONYMOUS-DEFAULT-CLASS-NAME | `esDecorators-classDeclaration-setFunctionName#target=es5`、`usingDeclarationsWith{ES,Legacy}ClassDecorators.{4,10}#target=es5` ×6 | typed refusal `RequiredChildRemoved { promoted class name }` | TS pass `visitClassDeclaration` 94454-94455：`needsName = moveModifiers && !node.name …`、`name = node.name ?? getGeneratedNameForNode(node)`（`default_1`） | `promote_class_declaration_to_iife` が名前必須 | **実装**：`ensure_generated_declaration_name(original, "default")`（builtins.rs；decorator 側の `TypeScriptGeneratedAnonymousDefault` 経路がそのまま `"default"` を install） |
| EF7-STATIC-ACCESSOR-RECEIVER | `esDecorators-classDeclaration-fields-staticAccessor#target=es2022` | panic `downlevel static auto-accessor owns a class constructor binding`（同一 file の 2 class 目） | classFields `tryGetClassThis` 96252-96255：`classThis ?? classConstructor ?? currentClassContainer.name`；`getClassFacts` 96960-96962 は無名 class かつ classThis 無しの static auto accessor だけ constructor reference を要求 | `static_auto_accessor_bindings` が `class_alias` を必須（selective 転送 file では facts が偽） | **実装**：宣言名 fallback（`PrivateEnvironment.class_name`）＋無名 class 規則を facts に追加（downlevel.rs） |
| EF7-DECORATED-STATIC-FIELD-COMMENT | `esDecorators-classDeclaration-commentPreservation` ×4、`esDecorators-classExpression-commentPreservation` ×2 | ES2022：`/*28*/` が `static {` の前（tsc は block 内の文の前）、`/*31*/`（accessor backing）が出る；ES2015：`/*31*/` が出る | classFields `transformPrivateFieldInitializer` 96299-96308（block は comment range を持たない）、`transformPropertyOrClassStaticBlock` 97444-97466 / `generateInitializedPropertyExpressions…` 97485（文/式が property の comment range、`hasAccessorModifier` は NoComments）；transformESDecorators は descriptor（private）accessor だけを自ら展開し backing を NoComments に | Rust は private static 文の comment を block へ移し、backing の NoComments を見ない；esDecorators 展開の comment source が private accessor にも付く | **実装**：block は Synthesized range、文は原本 range ＋ backing/NoComments 継承（downlevel.rs）；comment source は `descriptor_name.is_none()` の（public）accessor だけ（standard_decorators.rs） |
| EF7-SCOPED-NUMBERED-NAMES | `asyncFunctionDeclarationParameterEvaluation`、`asyncGeneratorParameterEvaluation` ×2 target | `args_2…args_10`、`x_2` が tsc は各関数で `args_1`、`x_1`；`arguments_N` は file 単位 | `makeUniqueName(scoped = ReservedInNestedScopes)` 120741-120779：`reserveNameInNestedScopes` は現在 scope の集合に入れ pop で解放（`createUniqueName("args", ReservedInNestedScopes)` 101296、`getGeneratedNameForNode(parameter.name, ReservedInNestedScopes)` 101307/102622）；`createUniqueName("arguments")` 101326 は flag 無し（generatedNames = file 単位） | `allocate_source_numbered_with_policy` が scoped 名も root scope に予約 | **実装**：scoped は `reserve_in_current`（generated_bindings.rs）、`arguments` は `TargetBinding::allocate_numbered`（es2017.rs） |
| EF7-YIELD-PARENS（追補） | emitter contracts `es2015_await_lowering_restores_prefix_unary_operand_parentheses` | `yield (yield 0)` が tsc は `yield yield 0` | `createYieldExpression` は `parenthesizeExpressionForDisallowedComma` だけ | r10 が Await 親も prefix-unary 扱い | **実装**：Await 親は括弧なし（es2017.rs） |
| EF7-PRESERVE-CJS-HELPERS | `importHelpersVerbatimModuleSyntax`、`modulePreserveImportHelpers` | `.cts`/`.cjs`（`module: preserve`）で `import { __rest } from "tslib"` が tsc は `const tslib_1 = require("tslib")` ＋ `tslib_1.__rest` | `createExternalHelpersImportDeclarationIfNeeded` 27636-27692：`impliedModuleKind === CommonJS` は import-equals 形、transformECMAScriptModule `updateExternalModule` 113392-113402 は宣言を visitor に通す（`const tslib_1 = require(…)`）、helper 名は `tslib_1.<helper>` に置換 | ESM transformer は host を持たず named import 固定、置換なし | **実装**：host を渡し per-file format で分岐、`insert_external_helpers_import_equals_declaration`、Identifier substitution（builtins.rs） |
| EF7-EXPORT-EQUALS-EMPTY-ASSIGNED-NAME | `esDecorators-classExpression-namedEvaluation.9` | `export = class { @dec y }` に `static { __setFunctionName(this, ""); }` が余計 | `visitExportAssignment` → `transformNamedEvaluation(…, canIgnoreEmptyStringLiteralInAssignedName(node.expression))` 100229-100236：class/constructor-parameter decorator の無い無名 class expression では `""` を捨てる | `ExportAssignment` arm が `""` を無条件に記録 | **実装**：`export_assignment_ignores_empty_assigned_name`（standard_decorators.rs） |
| harness projection | `isolatedModulesNoEmitOnError`、`isolatedModulesRequiresPreserveConstEnum` | `unprojected original option noEmitOnError / preserveConstEnums` | — | replay の option projection に無い | **実装（replay 側）**：2 option を射影（h2_7d_original_corpus_shared.rs） |
| KNOWN（owner 付き） | parse-diagnostics boundary（`asyncArrowFunction{6,7,8,9}_*`、`asyncFunctionDeclaration{6,7,9,10}_*`、`esDecorators-decoratorExpression.3`、`topLevelAwaitErrors.1`）、module resolution request plan（`bundlerDirectoryModule` ×3、`bundlerOptionsCompat`：`static-module-request-plan` typed refusal）、`importHelpersWithLocalCollisions#module=es2015`（H2.5h helper alias）、checker 診断行（REPORT §9.6 の表） | typed refusal / 診断 | — | — | `KNOWN_PLAN_BASE` に owner 付きで残す（継続中） |

## 10. EF8 — 出力経路の仕上げと A-CLOSE 候補

[ef8/](ef8/)（`axis-matrix.py --check` exit 0）：25,326 record / 30 軸 / 6 pair 表 / 74 要件行（67 covered-hosted、4 covered-unhosted、3 uncovered）。
未被覆軸 U1–U10 と最小追加入力 P1–P8（25 行、直積なし）、hosted job 対応表、A-CLOSE 骨子（A–F、global 769 / class 1228 分母）、
API 境界（disposed print known 2、custom transformer shared-node SUPER known 4：通常 Program から到達する反例なし、`api-boundary.md`）。
最大の hosting gap：compiler `contracts` の output/CLI/session 系 target と global 769・class 1228 の全件 replay に hosted 入口が無い。

### 10.1 EF8-P1..P8 — 未被覆軸の入力・比較の実装（r9）

レビュー指摘「P1–P8 の入力・比較実装が残っている」への対応。

| 単位 | 内容 |
| --- | --- |
| observer | `scripts/observe-output-matrix.mjs`（`--write` / `--check`、各行 2 回一致）。P1–P7 は `output-directories.json` の shape（memory sink、`use_case_sensitive_file_names` / `roots` 付き、map 書き込みの kind は `javascript-map` / `declaration-map`、`emit_result.source_maps` は h2-8a 形式）、P8 は `output-filesystem.json` の fault-injection shape |
| fixture | `crates/compiler/tests/fixtures/output-matrix.json`（22 行：P1 6 / P2 4 / P3 2 / P4 2 / P5 2 / P6 4 / P7 2）、`output-matrix-filesystem.json`（P8 4 行） |
| Rust entry | `crates/compiler/tests/integration/h2_8a_output_matrix.rs`（`contracts` target；`assert_cases_with_reporting` と `h2_8a_output_filesystem::assert_filesystem_cases`（旧 test 本体を pub(super) 化し declaration / sourceMap / declarationMap を projection に追加）） |
| 観測の要点 | outFile 行（P3/P4）は 6.0 deprecation 診断（TS5101 `outFile`、TS5107 `module=amd` / `target=es5`）を伴って emit する（exit 2）；`layout/outfile-and-outdir` は両方指定でも拒否されず bundle を out に書く；`skip/noemitonerror-outfile-maps` は emitSkipped（exit 1）；`case-insensitive-declaration-dir/collision` は TS5055 で d.ts を skip し JS 1 write（exit 1）；P8 は 8 writes（js/map/d.ts/d.ts.map ×2）で fault ごとの operation trace |
| 軸監査 | `ef8/axis-matrix.py --write` 再生成：要件 74 行 = covered-hosted 67 / covered-unhosted 7 / **uncovered 0**（U1/U2 の es2018・es2021・es2023 と P8 の fault 2 種が unhosted covered へ；hosted は統合担当の job 登録） |
| 結果 | REPORT §9（r9）：`contracts h2_8a_output_matrix` 2 tests |

## 11. Traceability / unresolved / readiness

原因 → 変更ファイル → 直接の証人（focused test / probe）→ 記録。最終数値は REPORT §8（r7 = chain9、r8 = chain10）。

| 子 ID | ファイル | 証人 | 記録 |
| --- | --- | --- | --- |
| EF1-ACCESS-TARGET-PHASE | `emitter/src/printer.rs`（`emit_access_target_with_source_comments`、`access_target_trailing_anchor_at`）、`compiler/tests/fixtures/post-t1-residuals-known-native.json` | `TSC_RS_POST_T1_RESIDUALS_CASE_SET=decorator-comments/ cargo test -p tsc-rs-compiler --test post_t1_residuals_contract`、probe1/probe2 | `records/ef1/` |
| EF3-HARNESS-FLOOR | `harness/src/upstream_suites/execution.rs` | harness `--test contracts`、`emitter_final_rows` | `records/measure/harness-contracts-*` |
| EF3-ISOLATED | `emitter/src/execute.rs` | `emitter_final_rows`（`isolatedModulesSourceMap`） | `ef2-ef3-rows-20-r2` |
| EF2-CLASS-NAME / EF4-DEFAULT-NAME v2 | `emitter/src/builtins/es2015.rs`（`get_generated_name_for_node`、`generated_declaration_name_binding_on_chain`、`get_name`） | probe3/legacy-bound-this、`emitter_final_batch` | `ef4-ef5-class-40-r6` |
| EF2-PROMISE-CTOR / EF2-ASYNC-SUPER | `emitter/src/builtins/es2017.rs`、`es2018.rs` | `emitter_final_rows`（`asyncAwait_es5`、C9）、probe7/async-gen-super；副次：h2-8c transpile-routes の known-open 4 行（`_super_1` accessor 原因）が exact 化 → retire（元 projection `records/ef2/h2_8c-known-open.before-retire.json`） | `ef2-ef3-rows-20-r4/-r6/-r7b`、`witness-transpile-routes-all-r7` |
| EF2-ARROW-PARENS / EF2-ARROW-BLOCK-ORDER | `emitter/src/factory.rs`（`parenthesize_updated_arrow_concise_body`）、`class_fields/downlevel.rs`（`strip_update_introduced_concise_parentheses`） | probe11/P7、probe3/concise-arrow | `ef4-ef5-class-40-r7` |
| EF2-READ-COMMENT | `emitter/src/printer.rs`（`emit_call_arguments` synthesized branch） | probe7/read-comment、`destructuringVariableDeclaration1ES5iterable` | `ef2-ef3-rows-20-r6` |
| EF5-ESCAPED / EF5-BARE-CLONE-SPELLING | `emitter/src/metadata.rs`、`factory.rs`（`clone_node_with_source_spelling`）、`es2015.rs`、`printer.rs` | probe3/escaped、probe3/direct-escaped、utf16 witness suites | `ef4-ef5-class-40-r7`、`witness-utf16-*-r7` |
| EF4-NESTED-THIS | `class_fields/downlevel.rs`（`visit_computed_property_expression`） | probe3/nested-computed | `ef4-ef5-class-40-r6` |
| EF4-ARROW-CRASH | `emitter/src/metadata.rs`（`clear_generated_binding`）、`factory.rs`（`set_original_node`） | `emitter_final_batch`（field-arrow / static-block-arrow が command 完了） | `ef4-ef5-class-40-r6` |
| EF4-FILE-THIS-CAPTURE | `class_fields/downlevel.rs`（`this` arm、`add_lexical_this_flag`）、`builtins.rs`（`static_this_substitute_flags`）、`es2015.rs`（`is_static_this_substitute`、`note_lexical_this_use`） | probe11/P5,P6、probe9、probe3/concise-arrow | `ef4-ef5-class-40-r7`、`build-cli-r7-debug*` |
| EF2-ASYNC-GEN-BODY-FLAG / EF2-TOP-LEVEL-FOR-AWAIT | `emitter/src/builtins/es2018.rs` | probe7/async-gen-super、`emitter_final_rows` | `ef2-ef3-rows-20-r7` |
| EF2-ASYNC-ALIAS-MARK | `checker/src/functions.rs`、`modules.rs`（`mark_linked_references_async_function`） | probe8/imported-promise、checker `--lib` | `ef2-ef3-rows-20-r7`、`checker-lib-r7` |
| EF2-BLOCK-SCOPED-DECORATED | `emitter/src/builtins/legacy_decorators.rs`（`create_declaration_head_name`）、`builtins.rs`（`create_wrapper_local_name`） | probe6/dec（残差 = 採番） | `ef2-ef3-rows-20-r7` |
| EF6-UMD-FACTORY | `checker/src/emit.rs`、`emitter/src/builtins.rs`、`system.rs` | probe4、`emitter_final_batch` EF6 | `ef6-global-14-r3` |
| EF6-JSDOC-LINK | `checker/src/node_builder/type_nodes.rs`（`js_doc_comment_text`） | `linkTagEmit1`、`jsdoc-return --all`、`declaration-comments --all` | `ef6-global-14-r6` |
| EF2-ALIAS-NUMBERING（r8） | `emitter/src/builtins/es2015.rs`（`colliding_declaration_name_substitute`（`enabledSubstitutions & BlockScopedBindings` で gate、r8-patches-8）、`update_variable_declaration_name`、`substitution_identifier_clone` の記録済み綴り）、`metadata.rs`（`generated_binding_print_order`）、`target_bindings.rs`（pre-pass の print-order 除外、numbered scope pass の naming-moment 再構成） | probe6/dec、probe6/dec3、`emitter_final_rows`（class2/3）、emitter contracts `anonymous_class_expression_reuses_the_*_name` ×2・`multiline_array_of_lowered_class_wrappers_breaks_between_elements`（stub resolver：r8f で `Resolver(Unavailable)` → gate で緑） | `ef2-ef3-rows-20-r8c`（class2）、`-r8d`（class3）、`emitter-contracts-r8f`（赤）、`emitter-contracts-focused-r8`、`-r8` |
| EF2-DETACHED-COMMENT（r8） | `emitter/src/printer.rs`（`carried_source_detached`、`take_carried_source_detached_for_node`、statement loop の carry 消費） | probe12/for-of37、`ES5For-of37`、emitter lib `hoisted_exports_leave_detached_comments_for_the_function_declaration`（r8e で赤 → r8 で緑） | `ef2-ef3-rows-20-r8c`、`emitter-lib-r8e`（赤）、`emitter-lib-hoisted-exports-r8`、`-r8` |
| EF2-AWAIT-USING-MISSING-NAME（r8） | `emitter/src/builtins/es_next.rs`（`visit_for_of_statement` の合成 temp）、`target_bindings.rs`（derived-of-temp の保留と親 temp の先行採番、`assign_numbered_entry`） | probe14/using、`emitter_final_rows`（using 4 行） | `ef2-ef3-rows-20-r8c`（ForAwaitOf ×2）、`-r8d`（ForOf ×2）、`-r8` |
| EF2-LOOP-VARIABLE-POLICY（r8） | `emitter/src/builtins/target_bindings.rs`（`from_existing(loop_variable)`）＋ 8 caller、`metadata.rs`（`merge_from` の loop flag 伝播） | probe16/A–E、`awaitUsingDeclarationsInForOf.1/.5` | `build-r8g`（scratchpad logs）、`ef2-ef3-rows-20-r8d`、`-r8` |
| EF3-ITERABLE-2318（r8） | `checker/src/globals.rs`（`get_global_iterable_type` の `publish_visible_global_diagnostics_since`） | probe10/iterable-es5（TS2318 報告）、probe10/nolib、`emitter_final_rows`（destructuring 行 ×2 profile） | `ef2-ef3-rows-20-r8c`、`-r8`、`checker-lib-r8` |
| EF6-IMPORT-TYPE-SELF（r8） | `checker/src/check.rs`（`class_expression_assignment_container`、`containers_of_symbol_slice`） | probe13/self-import（`index.d.ts`）、`emitter_final_batch` EF6（`…AnonymousWithSub` ×2） | `ef6-global-14-r8` |
| EF2-COMMENT-BOUNDARY（r9） | `emitter/src/printer.rs`（`emit_child_with_trailing_phase_before_token`、`visited_trailing_anchor_at_cursor`、`separator_anchor_after_child_phase`、`emit_for_binding_keyword_with_source_leading_comments`、For/ForIn/ForOf/With/While/If/Switch/Conditional arm、`separator_anchor_after_child` の active scope） | probe2/A・B、records/probes 33 形、`printer --all`、`compact-body-comments --all`、emitter contracts 452 | `records/measure/*-r9a` |
| compact private body（r9、期待値修正） | `emitter/tests/integration/active_transform_contract.rs`（`compact_private_function_body_duplicates_inter_statement_comment_like_tsc`） | records/compact-private-body（tsc CLI / transpileModule ×2 target、tsc-rs CLI 一致） | `emitter-contracts-r9a` |
| EF7-UNIVERSE（r9） | `scripts/observe-emitter-final-universe.mjs`、`compiler/tests/fixtures/emitter-final-universe*.json`、`compiler/tests/emitter_final_universe.rs`、`integration/h2_7d_original_corpus_shared.rs`（`replay_universe_fixture`、option projection 5） | `cargo test -p tsc-rs-compiler --test emitter_final_universe` | `universe-r9*`、ledger/DELTA-r9.md |
| EF8-P1..P8（r9） | `scripts/observe-output-matrix.mjs`、`compiler/tests/fixtures/output-matrix*.json`、`integration/h2_8a_output_matrix.rs`、`integration/h2_8a_output_filesystem.rs`（runner の共有）、`contracts.rs`、`ef8/axis-coverage.v1.json` | `cargo test -p tsc-rs-compiler --test contracts h2_8a_output_matrix` | `contracts-output-matrix-r9a` |
| EF3-CASE-CANONICAL（r8、方針 1） | `emitter/src/execute.rs`（`validate_emit_request` の outFile＋大小無視 refusal arm 撤去）；oracle 側は提案のみ（`records/oracle/h2-6c-qualification.case-canonical.patch`） | `records/oracle/remint/`（mirror 新旧観測）、`compare-outfile-candidate.py`（完全一致） | `ef2-ef3-rows-20-r8c` capture、`records/oracle/case-canonical-proposal.md` |
| EF7 r10 causes（DESIGN §9.1.2；`records/patches/r10-patches-{1,2,3}.py`） | `emitter/src/builtins.rs`（arrow facet flags、`collect_external_helpers_import_declaration`、CJS export inline / flattened name、`insert_external_helpers_import_declaration` の CUSTOM_PROLOGUE、verbatim export assignment）、`es2017.rs`（`yield_requires_parentheses`、accessor/shorthand）、`es2018.rs`（hoisted destructuring temps）、`es2015.rs`（object literal `Indented`）、`generators.rs`（loop variable policy）、`system.rs`（import helpers、import-equals exports、namespace alias）、`execute.rs`（noCheck route）、`factory.rs`（precedence tables pub(crate)）；checker `resolve.rs`、`evaluate.rs`、`functions.rs`、`access.rs`、`modules.rs` | scratchpad `r10probes/` 28 形（tsc 直採取、28/28 一致）、`emitter_final_universe`（217 ＋ plan-base） | `records/measure/*-r11`（chain16a/16b） |
| EF7 r11 causes（DESIGN §9.1.3；`records/patches/r11-patches-{1,2,3}.py`） | `standard_decorators.rs`（`class_expression_runtime_name`、accessor comment source gate、`export_assignment_ignores_empty_assigned_name`）、`system.rs`（`source_contains_top_level_await`、`exports_for_identifier`）、`builtins.rs`（verbatim import/export gates、`ensure_generated_declaration_name` promote、ESM transformer host + `insert_external_helpers_import_equals_declaration` + helper-name substitution）、`es2017.rs`（`insert_sync_super_capture_statements`、yield/await parens、`allocate_file_wide_numbered_binding`）、`class_fields/downlevel.rs`（`class_name` fallback、anonymous auto-accessor facts、private static field comments）、`generated_bindings.rs`（scoped numbered reservation）、harness `h2_7d_original_corpus_shared.rs`（2 projections） | scratchpad `r11probes/` 37 形（36/37；残 1 = checker TS2343、KNOWN）、`r10probes/` 28/28、emitter contracts 452（`es2015_await_lowering_restores_prefix_unary_operand_parentheses` 含む） | `records/measure/*-r11` |

未解決（r8 後、ledger/DELTA-r8.md）：大小無視 host 2 行だけ（oracle host 契約 → 方針 1；tsc-rs は提案観測と完全一致、integrator の再採取と比較まで KNOWN）。r7 時点の attributed 5 原因（EF2-ALIAS-NUMBERING、EF2-DETACHED-COMMENT、EF2-AWAIT-USING-MISSING-NAME、EF3-ITERABLE-2318、EF6-IMPORT-TYPE-SELF）と r8 で見つかった EF2-LOOP-VARIABLE-POLICY は上表のとおり修正済み。
未実行（hosted）：`decorator-binding-pipeline --all`、global 769 / class 1228 全件、EF8 P1–P8 の観測（`records/hosted-entry-proposal.md`）。
Readiness：REPORT §8.1 の chain10 表が全て緑（inherited red 1 件を除く）であること、`ratchets/` は無変更、retire 提案は REPORT §8.3（r8 で 9 所属追加）。
