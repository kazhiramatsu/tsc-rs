# tsgo の bug と思われる現象のメモ

利用者の指示（2026-10-17）：「tsgo のバグと思われる現象はメモしておいてくださいね」。
tsgo（TypeScript 7.1、vendored commit aa814927）と比べていて、tsgo の側が怪しいと
判断した現象をここに積む。本家（microsoft/TypeScript）への報告は利用者が行う
（AI 利用の開示が必要。本家の test 追加 PR の材料は `target/claude-tools/handover/upstream-tests-pr.md`、
`target/` 以下で未 commit）。tsc-rs 側の扱いは原則「tsgo に合わせる」。例外は
利用者の決定があるものだけ（下表の「tsc-rs の扱い」）。

書式：見つけた日 / 見つけた場所 / 現象 / 根拠（再現と tsgo の該当 code）/ tsc-rs の扱い /
本家への報告の状態。新しく見つけたら末尾に足し、報告したら状態を更新する。

## 一覧

### 1. auto-import が、export を augment するだけの file を specifier に選ぶ（run ごとに違う）

- 見つけた日・場所：2026-10-16、hono・vue・effect・playwright の LSP session（PR #827）。
- 現象：auto-import の module specifier が session ごとに変わり、ときに symbol を export して
  いない file（augment するだけの file：hono の `../..`、effect の `./http-api/HttpApiSchema.ts`、
  `@types/codemirror` の addon）を指す。その import は書いても解決しない。
- 根拠：`tsc/internal/ls/autoimport` の group は Go の map 順で処理され、specifier の cache は
  importing file ごとに export の file で引かれる。先に処理された group の答えが後の group にも
  使われるため、augment 側が先に来ると無効な specifier になる。再現
  `lsp-compare/repro/{vue-mini,hono-mini,cm-mini}`、tsgo を 3〜11 回走らせた記録
  （`out-hono-*`、`out-vue-tsgo*`）。本家に該当 issue は無い（2026-10-16 時点。#64821 は別件）。
- tsc-rs の扱い：利用者の決定（2026-10-16）で、tsgo の答えに無効な import が混じるときは宣言
  file（有効な方）を返す。tsgo が有効な答えの間で揺れるときは最頻の答え（PR #827 の
  `fill_shared_specifiers`）。
- 報告：下書き `target/claude-tools/handover/tsgo-autoimport-issue.md`、未投稿（利用者の判断待ち）。

### 2. 未 build の参照先出力に redirect される root に、file も診断も出ない

- 見つけた日・場所：2026-10-17、astro の `markdoc` の tsconfig（PR #828、`compare-all.sh`）。
- 現象：参照 project が持つ file を `include` が含むと、その root は参照先の出力 `.d.ts` に
  redirect される。出力が未 build だと tsgo は file を program に入れず、診断も出さない。
  TypeScript 6.0.3 は TS6305（「出力 '…' は source '…' から build されていない」）を報告する。
- 根拠：再現 `~/dev/real-projects.noindex/lsp-compare/repro/astro-refs`（`pkg/tsconfig.json`、`${configDir}` を
  使う `extends`、`references: [./tsconfig.test.json]`、test 側の `include` が `components/**/*` も含む）を `-p` で
  build すると、tsc 6.0.3 は exit 2 で `error TS6305: Output file '.../out/components/a.d.ts' has not been built
  from source file '.../components/a.ts'.`（`The file is in the program because: Matched by include pattern
  './components/'`）、tsgo aa814927 は exit 0・診断無し・`a.ts` は `--listFiles` に出ず、出力は `tsbuildinfo` だけ。
  astro 本体では `packages/markdoc` などがこの形。
- tsc-rs の扱い：tsgo に合わせる（`loader.rs resolve_root_sources` が redirect 先の無い root を落とす）。
- 報告：本家の test 追加 PR の材料（`upstream-tests-pr.md`）に「test で固定するより issue 向き」と
  して判断材料を置いた。未投稿。

### 3. `@this` の parameter の definition が空の範囲

- 見つけた日・場所：2026-10-17、svelte の LSP session（PR #829）。
- 現象：JavaScript の関数の JSDoc `@this {T}` から作られる hosted parameter の名前が位置 0 の
  node で、`textDocument/definition` の `targetRange`/`targetSelectionRange` が `0:0` になる。
- 根拠：tsgo の reparser が `@this` の parameter 名を Loc 0 で作る。再現は svelte の
  `packages/svelte/src` の該当 file（session の `differing.jsonl`、`out-svelte`）。
- tsc-rs の扱い：正しい範囲を返す（1. と同じく、tsgo の答えが使えないときは有効な答え）。
- 報告：未投稿。

### 4. hover の表示が checker の状態で変わる（session の前の file 次第）

- 見つけた日・場所：2026-10-16〜17、hono `Context<any, any, {}>.header`（tsgo 11 回中 6 回は
  具体化した形、5 回は宣言の型引数の形）、svelte `Value<V = unknown>.equals`（`Value<V>`）。
- 現象：同じ file の同じ位置の hover が run ごとに違う。前に開いた file の check が、対象の
  型の member を型解決前に instantiate していたかどうかで表示が変わる。
- 根拠：単独で record すると両 server とも同じ答え。`lsp-compare/replay.py` で session の
  file 列を二分探索できる。
- tsc-rs の扱い：単独 session の答え（checker の状態に依らない形）を返す。bounded（class B/C）。
- 報告：未投稿（非決定性の報告は 1. と合わせるのがよい）。

### 5. JSDoc の `@returns {x is T}` の `x` に symbol が無い（`asserts x is T` には有る）

- 見つけた日・場所：2026-10-17、svelte の documentHighlight（PR #829）。
- 現象：JavaScript の `@returns {x is T}` の `x` は highlight・references に含まれず、
  `@returns {asserts x is T}` の `x` は含まれる。
- 根拠：tsgo は JSDoc の名前を reparse した clone 経由で解決するが、clone を位置の二分探索で探す
  `ast.GetReparsedNodeForNode` は clone の根と同じ位置から始まる node（型の先頭 token）を
  取りこぼす。checker の表示（述語の型）は clone 経由で正しく解決する。
- tsc-rs の扱い：tsgo に合わせた（`location.rs`、test
  `a_plain_jsdoc_predicate_does_not_name_its_parameter`）。本家で直れば追随する。
- 報告：未投稿。

### 6. LSP test suite の 1 case が run ごとに違う

- 見つけた日・場所：2026-10-15、tsgo の language server test 4,584 件の再生（packet ts71-lsp）。
- 現象：4,584 件中 1 件は tsgo 自身の答えが run ごとに変わる（README の「4,583 of 4,584」）。
- tsc-rs の扱い：記録のみ。
- 報告：未投稿。

### 7. JSDoc の型位置に書いた値（型を持たない symbol）が TS2749 になる

- 見つけた日・場所：2026-10-17、webpack の `lib/javascript/JavascriptParser.js`（`@property {InstanceType<ESMStarExportsList>=}`、
  `ESMStarExportsList` は値だけを export する `@import` の binding）。
- 現象：tsgo は「'X' refers to a value, but is being used as a type here. Did you mean 'typeof X'?」（TS2749）を報告する。
  TypeScript 6.0.3 は JS file の JSDoc では値を型として読む（`getTypeFromJSDocValueReference`）ので報告しない。
- 根拠：tsgo `checker.go` `getTypeReferenceType` の末尾が `// !!! Resolving values as types for JS` のまま `errorType` を返し、
  `getSymbolFromTypeReference` は Type の意味だけで解決する（6.0.3 の Type|Value の再試行が無い）。再現
  `lsp-compare/repro/webpack-mini`（`lib/main.js` の `List`）：tsc 6.0.3 exit 0、tsgo は TS2749。
- tsc-rs の扱い：tsgo に合わせてある（`annotate.rs` も Type の意味だけ）。本家が実装すれば追随する。
- 報告：未投稿（tsgo の TODO らしく、issue というより実装待ち）。

### 8. JS の setter の `@returns` が TS1095、`for (const /** @type {T} */ x of …)` が TS2483

- 見つけた日・場所：2026-10-17、webpack の `lib/graph/AsyncDependenciesBlock.js`（setter）、`lib/hmr/HotModuleReplacementPlugin.js`（for-of）。
- 現象：tsgo は JS file で `set x(v) {}` に `@returns {void}` があると「A 'set' accessor cannot have a return type annotation.」（TS1095）、
  for-of の変数に `@type` があると「The left-hand side of a 'for...of' statement cannot use a type annotation.」（TS2483）を報告する。
  tsc 6.0.3 はどちらも報告しない（JSDoc の型は注釈ではない）。
- 根拠：tsgo の reparser が `@returns` を setter の戻り値注釈に、`@type` を for-of 変数の型注釈にしてから文法検査が走る。再現
  `lsp-compare/repro/fn-type-tag/a.js`。
- tsc-rs の扱い：tsgo に合わせた（PR #832、`functions.rs check_grammar_accessor`・`statements.rs`、test
  `reparsed_setter_returns_and_for_of_variable_types_are_grammar_errors`）。
- 報告：未投稿。

### 9. `@this` が `@type` より前にある関数宣言では `@type` の signature が効かない

- 見つけた日・場所：2026-10-17、webpack の `lib/module/Generator.js`（`/** @this {ByTypeGenerator} @type {GenerateErrorFn} */ function generateError(error, module, generateContext)`）。
- 現象：tsgo では parameter が全部 `any`（TS7006）。`@type` を先に書けば効く。tsc 6.0.3 はどちらの順でも `@type` の signature を使う。
- 根拠：reparser は tag を順に処理し、`@this` が型付きの `this` parameter を先に足すので、`@type` の「parameter に型が無いとき」条件
  （reparser.go の `noTypedParams`）が偽になる。再現 `lsp-compare/repro/alias-empty/a.js` の `g`。
- tsc-rs の扱い：tsgo に合わせた（PR #832、`hosted.rs reparse_type_tag` の `typed_this`、test
  `a_this_tag_before_a_function_s_type_tag_leaves_its_parameters_untyped`）。
- 報告：未投稿。

### 10. 入れ子の `@type` cast の内側の型参照が references・highlight に出ない

- 見つけた日・場所：2026-10-17、webpack の `lib/javascript/syntax-printer.js`（`/** @type {Node[]} */ (\n /** @type {Node[] | null} */ (null))` が多数）。
- 現象：内側の cast の JSDoc の中にあるものが tsgo の位置探索に届かない：型名の hover・definition・references が無く、型位置の `null` が
  keyword references・documentHighlight に出ず（webpack で references 12 件 +45 ずつ、highlight 3 件）、cast の括弧の中に書いた arrow の
  `@param` 名も highlight に出ない（`lib/index.js:140`）。1 行の入れ子でも同じ（以前の「1 行なら数えられる」は誤り：hover 11:51 は null）。
  TypeScript 6.0.3 は全部届く。
- 根拠：tsgo の reparser は `/** @type {T} */ (e)` の括弧の中身を `e as T` に置き換える（reparser.go 382-389、`parent.SetExpression(makeNewCast(…))`）。
  `as` node は自分の JSDoc を持たず Pos は `e` の full start なので、`astnav.getTokenAtPosition` の `GetTokenPosOfNode(as, includeJSDoc=true)` は
  `e` の先頭 JSDoc を飛ばした `(` になり、`e` 自身の JSDoc の中の位置は「どの子にも含まれない」。外側の cast の JSDoc は括弧 node 自身のものなので
  届く。再現 `lsp-compare/repro/nested-cast`（documentHighlight 9:32：tsgo 3 件・tsc-rs 5 件、hover 11:51：tsgo null）、`repro/w2-arrow`。
- tsc-rs の扱い：利用者の決定（2026-10-17「入れ子castはtsgoに合わせてください」）で tsgo に合わせる：`ls/astnav.rs start_of_node` が hosted cast の
  operand の自分の JSDoc を start に含めない（次の PR）。
- 報告：未投稿。

### 11. reparse した宣言への definition の targetRange が `0:0`

- 見つけた日・場所：2026-10-17、webpack の `lib/javascript/syntax-parser.js:7493`、`lib/resolve/getResolveOptionsKey.js:30`、`lib/template/ChunkTemplate.js:173`。
- 現象：definition が同じ file の `0:0` を指す。3 件とも `@this {T}` を持つ関数の中の `this`（`syntax-parser.js:7493` の `this.value`、
  `getResolveOptionsKey.js:30` の `this[name]`、`ChunkTemplate.js:173` の `this._outputOptions`）で、3. と同じ（reparse した `this` parameter の
  名前が Loc 0）。preact の `component.js` でも 3 件同じ。
- tsc-rs の扱い：3. と同じ（`@this` tag の `this` の範囲を返す）。
- 報告：未投稿。

### 12. JSDoc cast の括弧の中身は flow 解析の reference にならない

- 見つけた日・場所：2026-10-17、webpack の `lib/javascript/ScopeAnalyzer.js:144`
  （constructor の `this.variableScope = isVarScope ? this : /** @type {Scope} */ (upper).variableScope;`）。
- 現象：tsgo は `(upper).variableScope` を flow 解析の reference にしない（TS2565「used before being assigned」が出ない）。
  TypeScript 6.0.3（webpack の node_modules の tsc）は cast の有無によらず TS2565 を報告しない。tsgo は cast 無しの `upper.depth` だけ報告し、
  tsc-rs もそこは修正前から tsgo と同じ。
- 根拠：tsgo の reparser は cast を `as` 式にし、binder の `isNarrowableReference` に `as` の arm が無い（parenthesized・non-null はある）ので
  flow node が付かず、`getFlowTypeOfReference` は宣言型を返す。再現 `lsp-compare/repro/w2-scope/scope.js`。
- tsc-rs の扱い：tsgo に合わせた（PR #832、binder `node_util::is_narrowable_reference`、test
  `a_property_read_through_a_jsdoc_cast_is_not_a_flow_reference`）。checker 側の `getReferenceCandidate`・`isMatchingReference` は既に cast を通さない。
- 報告：未投稿（6.0.3 との差として報告候補）。

### 13. `@import` の default 名が、file によっては class の references に出ない

- 見つけた日・場所：2026-10-17、webpack の `lib/module/NormalModule.js:170`（`/** @import Generator, { GenerateErrorFn } from "./Generator" */`）、
  同じ形が Compilation.js（ModuleFactory）、NormalModule.js（Parser）、JavascriptGenerator.js（DependencyTemplate）、ConcatenatedModule.js（Hash）。
- 現象：`class Generator`（Generator.js:70）の references に tsgo は `@import` の default 名（170:12）を含めない（named binding や `@type {Generator}` の
  使用は含める）。小さい再現（`repro/w2-import`、`w2-import2`：同じ CommonJS `module.exports = X` + `@import X, { T } from`）では tsgo も含めるので、
  file の中身（多数の多行 `@import`・typedef）に依存する。
- 根拠の候補：memo 5 と同じ `ast.GetReparsedNodeForNode` の位置二分探索（JSDoc の識別子を reparse した clone に写す）が、近くの clone の並びで
  取りこぼし、`getSymbolAtLocation` が nil になる。未確認。
- tsc-rs の扱い：bounded で残す（利用者の決定 2026-10-17「@importの件はboundedで残してOKです」）。合わせるには tsgo の clone 列（file 内の
  reparse 位置の列）を模す必要があり重い。webpack の 5 件（references +1 ずつ）はこの扱い。
- 報告：未投稿。
