# JavaScriptのd.tsをtsgoの方式で作る

状態：**実装中**（J1・J1b・J2 2026-10-04）。ユーザー決定（2026-10-04）：「再設計して進める」。前段：
[TypeScript 7.1への切替](../ts71-cutover/README.md)のP3-5ab。

## 背景

P3-5ab後、lane Aでemitが不一致の434構成のうち、約270構成はJavaScriptから作るd.tsである。

- **tsc-rsの現状。**
  - `crates/emitter/src/declarations/root.rs`の`transform_root`は、JavaScriptのsource fileを
    `transform_declarations_for_js`に送る。
  - そこからresolverの`get_declaration_statements_for_source_file`を経て、checkerのsymbol直列化
    （`node_builder/statements.rs`の`symbol_table_to_declaration_statements`）が宣言を作る。
    これはtsc 6.0のtransformDeclarationsForJSの移植である。
- **tsgoの方式**（`target/typescript7/upstream/tsc/internal`、19dadef88）。
  - d.tsのためのsymbol直列化は無い。`nodebuilder_hover.go:20-24`のcommentに名前が残るだけである。
    JavaScriptもTypeScriptと同じdeclaration transform（`transformers/declarations/transform.go`）を通る。
  - JSDocはparserが再解析して木に入れる（`parser/reparser.go`）。
    - 新しいnodeの種類は2つだけで、どちらもhostの文の前に挿入される（`parser.go:613-643`）：
      `KindJSTypeAliasDeclaration`（`@typedef`・`@callback`）と`KindJSImportDeclaration`（`@import`）。
    - `@type`・`@param`・`@returns`・`@template`・`@this`・修飾子のtag・`@implements`は、hostの既存の欄
      （`Type`、`FullSignature`、`?`、型引数、修飾子、heritage clause）を埋める。
    - `@overload`は本体の無い宣言を足し、`@satisfies`は式を包む。
  - CommonJSは再解析しない。
    - `module.exports =`・`exports.x =`・`Object.defineProperty(exports, …)`・`require`は普通の式と宣言の
      ままで、declaration transformがd.tsの宣言に変える（`transform.go:327-371`、`1192-1534`、`875-904`、
      `2228-2266`）。
    - `this.x =`のmemberもtransformが集める（`2084-2192`）。
  - 宣言の型：
    - JSDocから再解析した型は`TryJSTypeNodeToTypeNode`（`checker/nodecopy.go`）でTypeScriptの型nodeに直す。
    - 直せないときはcheckerの型を直列化する（`transform.go:1650-1662`）。
- **実験**（2026-10-04、main `21978bb76`、JavaScriptのd.tsの288 case・406構成）。
  - ES moduleのJavaScriptを、JavaScript特有の処理を足さずに今のTypeScriptの経路へ通した：
    emit full 78→155（+79）、下がった2構成。下がったのは`@overload`と、拡張子だけでES moduleになる`.cjs`。
  - JavaScriptをすべて通した：+87、下がった15構成。主にCommonJS、scriptの`@implements`、JavaScriptの
    unique symbol。

## 方針

- **経路。** JavaScriptのsource fileも、TypeScriptと同じdeclaration transform（`declarations/statements.rs`・
  `subtree.rs`）に通し、tsgoの`transform.go`のJavaScript特有の処理を移植する。
- **JSDocの再解析はtree nodeにしない。** P3-5oの`JsDocHosted`（`tsc_syntax::jsdoc_hosted`、binderの
  `hosted.rs`）を使う。
  - tsgoがhostの欄を読むところでは、hostedの表を読む。
  - `@typedef`・`@callback`・`@import`の宣言は、transformがhostの文を訪れる前にtagの位置で合成する。
    位置はtsgoの挿入位置に合わせ、commentもその位置で扱う。
  - 理由：parse treeの形とnode identity（incrementalな再解析、checker、JavaScriptのemit）を変えずに済む。
    P3-5oのside tableの判断とも一貫し、変更はemitterとresolverの照会に閉じる。
- **JSDocの型。** tsgoの`TryJSTypeNodeToTypeNode`を移植する。
  - JSDocの型を書き換える：`*`→`any`、`?T`→`T | null`、`T=`→`T | undefined`など。
  - `String`・`Object`などの参照と、`exports`を含む名前は再利用せず、checkerの型に戻す。
- **CommonJS。** tsgoのtransformどおりd.tsの宣言に変える。
- **退役。** 最後に`get_declaration_statements_for_source_file`と、JavaScriptにしか使わないsymbol直列化を
  退役させる。

## 段階

| slice | 内容 | 主な対象 | 終了条件 |
| --- | --- | --- | --- |
| J1 | CommonJSの印の無いES moduleのJavaScriptをdeclaration transformに通す。追加するもの：`this.x =`のmember、`@typedef`・`@callback`・`@import`の宣言、`@overload`、`TryJSTypeNodeToTypeNode`、修飾子とJSDoc commentの扱い | ES moduleのJavaScriptのd.ts（実験で+79、残り約59構成） | 0 regressions。ES moduleのJavaScriptは6.0の経路を使わない |
| J2 | script（ES moduleでもCommonJSでもないJavaScript）を通す。`@implements`、JavaScriptのunique symbol、expandoのclass hostを足す | scriptの約61構成 | 同上 |
| J3 | CommonJSのfileを通す：`module.exports =`、`exports.x =`、`Object.defineProperty`、`require`からのimport、`_exports`、文の順序（`appendCjsExports`） | CommonJSの約71構成 | 同上 |
| J4 | 退役：`transform_declarations_for_js`、resolverの`get_declaration_statements_for_source_file`、JavaScriptにしか使わないsymbol直列化 | — | JavaScriptのd.tsはすべてdeclaration transformから出る |

各sliceの検証は切替packetと同じ：
- tsgoの出力にpinしたunit test。
- full conformanceを1回（0 regressions、同じreportからratchetを更新）。
- hosted。
- README corporaのperf（`--noEmit`と`tsconfig.bench-full.json`）。

## 注意点

- **commentの位置。**
  - tsgoの再解析nodeはtagの位置を持つので、d.tsではhostの宣言の上に元のJSDoc blockが付く。
  - 型alias自体はcomment無しで出る（baseline `jsDeclarationsTypeAliases.js`）。合成するnodeの範囲も同じにする。
- **`this.x =`のmember。** 残す修飾子は`static`だけで、`@readonly`・`@protected`は落とす
  （`jsdocAccessibilityTagsDeclarations.js`、`jsdocReadonlyDeclarations.js`）。
- **CommonJSの判定。** binderの`common_js_module_indicator`を使う。`.cjs`は拡張子でES moduleの印も持つので、
  emitterにはresolverの照会で渡す。
- **d.tsの診断。** TS4023・TS9006などの診断とemitを止める条件は、TypeScriptの経路に従う。
- **出力の順序。** CommonJSの`module.exports`がclassか関数なら`export =`を先に、それ以外は`const`を先に書く
  （`transform.go:1241-1295`）。

## J1 ES moduleのJavaScriptをdeclaration transformに通す（2026-10-04）

- `declarations/root.rs`の`transform_root`は、次の条件を満たすJavaScriptのfileを、TypeScriptと同じ
  `visit_declaration_statement`／`transform_and_replace_late_painted_statements`に通す：
  - ES moduleの印がある。
  - binderのCommonJSの印が無い（resolverの`is_common_js_module`。`.cjs`は拡張子でES moduleの印も持つため）。
  - `@overload` tagが無い（再解析される宣言は後のsliceで扱う）。
  これを満たさないscript・CommonJS・`@overload`のfileは、従来どおりtsc 6.0のsymbol直列化を使う。
- 文の列の範囲：この経路のJavaScriptも、TypeScriptと同じく元の文の列の範囲を持つ（detached commentの扱い）。
- 結果として、tsgoと同じく次のようになる：
  - exportした宣言の`declare`。
  - JSDocの型からの引数と戻り値の型。
  - JSDoc commentを宣言の上に残す。
  - literalの`const`の初期化子。
  - 文の順序はsourceのまま。
- unit test（CLI、tsgoの出力にpin）：class、JSDocで型を付けた関数（省略可能な引数を含む）、`@type`の`const`、
  literalの`const`、default exportの関数を持つES moduleのJavaScriptのd.ts。
- conformance：
  - 15,228 configuration、lane A 13,467（変化なし）、441 s。
  - errorsは変化なし（full 13,325、描いたbaselineのdigestもすべて同じ）。
  - emit full 13,003→13,086（+83）、emit mismatch 434→351。上がった83構成はすべてJavaScriptのd.tsで、下がった
    構成は無く、それまでfullだったemitのdigestもすべて同じ。
- ratchet：0 regressions、83行raise。
- local：
  - formatとworkspace全体のclippy。emitter・checker・compiler・conformanceのtest（35 targets、2,623 passed）。
  - 2 workerのfull run（441 s）。
  - 試行（devのrunner、JavaScriptのd.tsの288 case）：CommonJSの判定を`external_module_indicator`で近似すると
    `.cjs`の1構成が、`@overload`のfileを通すと1構成が下がったので、それぞれ除いた。
- 残り：
  - ES moduleのJavaScriptのd.tsで残る不一致：`this.x =`のmember、`@typedef`・`@callback`・`@import`の宣言、
    `@overload`（J1bで扱う）。
  - scriptとCommonJSのfileは従来の経路（J2、J3）。
- hosted：PR #646（head `63c01a24a`、merge `65d0baca5`）、run 37160522503 — `plan` 29s、`rust` 10m23s、`conformance (TypeScript 7.1)` 19m11s、`gates` 12s。
- perf（README corpora、`--noEmit`、3 rounds、nice 20、main `fd6b1aac3`（P3-5abのcode `8a3cc8986`と同じcodeのrelease build）と本branchのrelease build対tsgo 7.1.0-dev、median wall ms main→本branch）：hono 134→145、zod 540→521、Playwright 369→345、TypeScript `src/compiler` 338→341、Next.js 745→754、Effect 491→530、VS Code 3,510→3,469。tsc-rs÷tsgoは0.59〜0.97、peak memory（MB main→本branch）：318→315、1,285→1,284、822→740、289→289、1,300→1,318、1,014→1,047、5,448→5,434。J1は`--noEmit`の経路を変えないので、hono・Effectの差をA/Bで確かめた：Effect 10 roundsはmain／本branch 498／482 ms、hono 5 roundsは133／124 ms、単一checker（`TSRS_CHECKERS=1`）の命令数（5回のmedian）はEffect 27.896／27.897 G、hono 4.298／4.299 Gで同じ。診断の出力と読み込んだdocument数は7 corpusともmainと同一。`tsconfig.bench-full.json`（JS・d.ts・source map、3 rounds）：hono 159→155、zod 633→642、Playwright 520→505、TypeScript `src/compiler` 504→507、Next.js 1,026→1,021、Effect 825→819（tsgo比0.54〜0.79）で、出力と診断は6 corpusともmainと同一。退行なし。

## J1b JSDocから再解析される宣言と欄をdeclaration transformで読む（2026-10-04）

J1でdeclaration transformに通したES moduleのJavaScriptについて、tsgoのparserがJSDocから作る宣言と欄を、
`tsc_binder::jsdoc_hosted`の表とJSDocのtagから合成する（`crates/emitter/src/declarations/javascript.rs`）。
emitterは`tsc-binder`に依存するようになった。

- **`this.x =`のmember**（`collectThisPropertyAssignments`、transform.go:2084-2192）：
  - classのmemberの中の`this.x = …`から、classが宣言していないmemberを集める。`this`が別の関数に
    束縛される所では探さない。残す修飾子は`static`だけ。
  - resolverの照会を2つ足した：memberの値の宣言（`GetReferencedMemberValueDeclaration`）と、`extends`の
    基底型がすでに持つmemberか（`IsThisPropertyAssignmentDeclarationRedundant`）。
- **hostの欄**：
  - `@private`・`@protected`・`@public`・`@readonly`・`@override`の修飾子（再解析した修飾子は使い回さず、
    flagから作り直す。`canReuseModifierNodes`）。
  - `@implements`の型（最初の`implements`句に足すか、新しい句を最後に足す）。
  - `@augments`の型引数。
  - `@template`の型引数。診断では型引数の親をhostとする。
  - `@this`の`this`引数（型はtagの型、無ければ`any`）。
- **`@typedef`・`@callback`の型alias**（`reparseUnhosted`、reparser.go:74-123）：
  - top-levelの文の前に置く。その文のJSDocと、blockの中を除く入れ子のnodeのJSDocのtagを、tsgoがhostを
    閉じる順（後順）に並べる。end-of-fileのJSDocのtagは文の後ろに置く（parser.go:445-448、614-643）。
  - moduleでは`export`を付ける（`IsImplicitlyExportedJSDocDeclaration`）。
  - `@property`のtagは型literalにし、tagのcommentをpropertyの上に残す（`preservePartialJsDoc`）。
  - `@callback`は関数型にする。commentの`@template`は型引数にする。
  - 点で区切った名前はnamespaceで包む。入れ子のnamespaceは`export`を外し、型aliasは`export`を残す。
- **`@import`のimport**（reparser.go:124-137）：
  - tagをlate paintingの文として列に置き、`import type`の宣言にする。
  - checkerは、top-levelのtagのbindingを、source fileのimport宣言と同じく描く。
- **`@overload`のoverload**（reparser.go:138-142、146-242）：
  - 関数・method・constructorの実装を省く所に、tagの署名からoverloadを書く。`@returns`が無ければ`any`、
    constructorには書かない。
  - overloadは実装の書かれた修飾子と名前を位置ごと写す（`DeepCloneReparseModifiers`）。そのため、修飾子を
    使い回すときと、修飾子の無いmethodでは、実装の前のcommentがoverloadの前に出る。
  - `@overload`のあるES moduleのfileも、declaration transformに通す（J1の除外をやめた）。
- **JavaScriptの型**：
  - `ensureType`は、まずnode builderの再利用（`TryJSTypeNodeToTypeNode`、transform.go:1650-1666）を通す。
    そのためのresolverの照会を足した。
  - transformが訪れるJSDocの型はTypeScriptに書き換える（`*`→`any`、`?T`→`T | null`、`!T`→`T`、
    `T=`→`T | undefined`、`...T`→`T[]`。transform.go:2584-2638）。
- **checkerの可視性**（emitresolver.go:131-227）：
  - top-levelのJSDocのalias、およびその点付きの名前のnamespaceを可視とする。
  - module指定子の無い`export {X}`のExportSpecifierを可視の宣言とする。
- unit test（CLI、tsgoの出力にpin）：
  - `this.x =`のmemberと修飾子・heritage・`@template`。
  - `@typedef`・`@callback`と、commentの位置。
  - 再exportした値と点付きの`@typedef`名。
  - `@import`。
  - `@overload`。
  - `@this`。
- conformance：
  - 15,228 configuration、lane A 13,467（変化なし）、461 s。
  - errorsは変化なし。描いたbaselineのdigestが変わったのは`intersectionConstructorReductionCrash`だけで、
    これはmemoryの上限（3,072 MiB）付近でharness errorと比較とを行き来するstressのcaseである（full 13,325→
    13,326、harness errors 22→21はこのcase）。
  - emit full 13,086→13,127（その上のcaseを除いて+40）、emit mismatch 351→311。上がった40構成は、JavaScriptの
    d.ts 38構成と、TypeScriptのtupleに書いた後置のJSDocの`?`（`[...string?]`→`[...string | null]`）の2構成
    （`restTupleElements1`、`namedTupleMembersErrors`）。下がった構成は無く、それまでfullだったemitのdigestも
    すべて同じ。
- ratchet：0 regressions、40行raise（emit none→js）。`intersectionConstructorReductionCrash`は従来どおり
  ratchetに入れない。
- local：
  - formatとworkspace全体のclippy。emitter・checker・compiler・conformanceのtest（35 targets、2,629 passed、
    `9ca4d191e`。その後のclippyの修正は意味を変えない書き換えで、JavaScriptのCLI test 12件を再実行）。
  - 2 workerのfull run（461 s、`4c15abfbc`のrelease build）。
  - 試行（devのrunner、JavaScriptのd.tsの288 case）：J1から38構成上がり、下がった構成は無い。途中で
    `jsDeclarationsImportAliasExposedWithinNamespace`のerrorsが下がった（TS4081）のを、tsgoのcheckerの可視性
    （JSDocのnamespaceとExportSpecifier）を移して直した。
- 残り：
  - ES moduleのJavaScriptのd.tsで残る不一致（約20構成）：
    - 関数のexpando（`const f = () => …; f.x = …`）。tsgoはdeclaration transformでexpandoを集め、
      `declare function`とnamespaceに書く（`transformExpandoAssignment`、`createFullExpandoBlock`、
      transform.go:2700-2970）。
    - object literalのaccessor、namespaceの`export { f as self }`、`export default`のliteralの初期化子
      （`declare const _default = 12;`、transform.go:1265-1297）。
    - `import('./a')`の引用符：tsgoのnode builderはimport型を`canReuseTypeNode`無しで再利用する
      （nodecopy.go:614-646）。tsc-rsはtsc 6.0どおり再利用できるかを先に確かめ、直列化に回る。
  - checkerの差：
    - `@callback`の`...T`引数でtsgoが出すTS2370が出ない。
    - `@overload`のcommentにだけある`@template`の型引数で、tsc-rsにだけTS2304が出る。
    - `@returns`の無い無名default exportのoverloadで、tsgoが出すTS7011が出ない。
  - scriptとCommonJSのfileは従来の経路（J2、J3）。
- hosted：PR #647（head `74555efea`、merge `1f4897c29`）、run 37165101376 — `plan` 33s、`rust` 8m02s、`conformance (TypeScript 7.1)` 12m29s、`gates` 11s。
- perf（README corpora、`--noEmit`、3 rounds、nice 20、main `f67a5f294`（J1のcode `821b401f0`と同じcodeのrelease build）と本branch（`4c15abfbc`）のrelease build対tsgo 7.1.0-dev、median wall ms main→本branch）：hono 136→134、zod 500→497、Playwright 369→356、TypeScript `src/compiler` 331→334、Next.js 758→741、Effect 501→474、VS Code 3,391→3,307。tsc-rs÷tsgoは0.58〜0.99、peak memory（MB main→本branch）：320→325、1,299→1,288、813→800、289→289、1,310→1,319、1,031→1,039、5,458→5,453。診断の出力と読み込んだdocument数は7 corpusともmainと同一。`tsconfig.bench-full.json`（JS・d.ts・source map、3 rounds）：hono 145→147、zod 610→606、Playwright 483→476、TypeScript `src/compiler` 496→489、Next.js 1,000→1,008、Effect 798→778（tsgo比0.52〜0.78）で、出力と診断は6 corpusともmainと同一。退行なし。

## J2 scriptのJavaScriptをdeclaration transformに通す（2026-10-04）

- **経路**：`declarations/root.rs`は、CommonJSのJavaScript（resolverの`is_common_js_module`）だけをtsc 6.0のsymbol直列化に
  送り、ES module（J1）とscriptはTypeScriptと同じdeclaration transformに通す。
- 経路を変えて見つかった差を、tsgoに合わせて直した：
  - **`@type`の関数**：JavaScriptの関数が`@type`から取る署名はその関数自身のものなので、overloadの実装として
    落とさない（`IsImplementationOfOverload`、emitresolver.go:494-498）。
  - **JSDocの複数行の型**：JSDocの型の中のnodeのsource textは、commentの各行の先頭の`*`を除く
    （`getTextOfNodeFromSourceText`）。
  - **再利用した文字列literal型の引用符**：tsgoのscannerはliteralの引用符をtoken flags（`TokenFlagsSingleQuote`）に
    残し、node builderの複製もそれを保つ。tsc-rsのnode builderの複製も、元の`'x'`の引用符を保つようにした。
    emitterのtransformは、tsgoが新しいliteralを作る所（CommonJSの`require`など）があるので変えない。
  - **JSDocの型literalの再利用**：tsgoが再解析した型literalと同じく、propertyの型を書かれたとおりに使い
    （tsc 6.0の`typeViaParent`による`| undefined`の上書きをやめた）、identifierでない名前は文字列literal、
    `Object[]`は配列にする。
  - **関数式の`@template`**：syntacticな型の組み立てで、関数式・arrow関数・object literalのmethodに書かれた
    型引数が無ければ、tsgoの再解析と同じく`@template`の型引数を付ける（reparser.go:440-457）。
  - **空のheritage句**：型が無いか、唯一の型が欠けているheritage句は書かない（transform.go:610-612）。
    型の無い`@implements`はこれに当たる。
- unit test（CLI、tsgoの出力にpin）：
  - scriptのJavaScriptのd.ts（大域の宣言、`export`の無い型alias、`@type`の関数、引用符、複数行のJSDocの型）。
  - 関数式とobject literalのmethodの`@template`、型の無い`@implements`。
- conformance：
  - 15,228 configuration、lane A 13,467（変化なし）、445 s。
  - errorsは変化なし。描いたbaselineのdigestが変わったのは`intersectionConstructorReductionCrash`だけ（memoryの
    上限付近で比較とharness errorを行き来するstressのcase。今回はharness errorで、full 13,326→13,325、harness
    errors 21→22はこのcase）。
  - emit full 13,127→13,188、emit mismatch 311→249。上がった62構成は、JavaScriptのd.ts 58構成と、再利用した
    文字列literal型の引用符が合ったTypeScriptのd.ts 4構成（`coAndContraVariantInferences`、
    `declarationEmitClassMemberWithComputedPropertyName`、`destructuredDeclarationEmit`、`keyofAndIndexedAccess`）。
    下がった構成は無く、それまでfullだったemitのdigestもすべて同じ（上のcaseを除く）。
- ratchet：0 regressions、62行raise（emit none→js）。`intersectionConstructorReductionCrash`は入れない。
- local：
  - formatとworkspace全体のclippy。emitter・checker・compiler・conformanceのtest（35 targets、2,631 passed、
    `4c54b986f`）。
  - 2 workerのfull run（445 s、`4c54b986f`のrelease build）。
  - 試行（devのrunner、JavaScriptのd.tsの288 case）：scriptの経路の変更だけで46構成上がり、下がった構成は無い。
    node builderの複製で引用符を保つ変更を、最初はemitter全体の`clone_node`に入れたところ、CommonJSの
    `require`の指定子が`'./b'`のままになり3構成が下がった（tsgoはそこで新しいliteralを作る）。そのため
    node builderの複製に限った。
- 残り：
  - expando（`function f() {}`や`const f = () => {}`への`f.x = …`）：tsgoはdeclaration transformの中でexpandoを
    集め、`declare function`とnamespaceに書く（`visitNestedExpression`、`transformExpandoAssignment`、
    `createFullExpandoBlock`、transform.go:2700-2970）。namespaceのmemberは`var`（`NodeFlagsNone`）で、keywordの
    名前は生成した名前と`export { … as … }`にする（PR本文の`let`は誤りで、`let`は従来のsymbol直列化の書き方）。
    tsc-rsはTypeScriptでもtsc 6.0のexpandoの書き方なので、TypeScriptと合わせて次のsliceで移植する。
  - checkerの差（J1bの記録のとおり）。
  - CommonJSのfileは従来の経路（J3）。
- hosted：PR #648（head `ebe28b5ca`、merge `e723d5f6c`）、run 37167233210 — `plan` 27s、`rust` 9m34s、`conformance (TypeScript 7.1)` 19m25s、`gates` 11s。
- perf（README corpora、`--noEmit`、3 rounds、nice 20、main `e38d9b16e`（J1bのcode `4c15abfbc`と同じcodeのrelease build）と本branch（`4c54b986f`）のrelease build対tsgo 7.1.0-dev、median wall ms main→本branch）：hono 136→126、zod 512→512、Playwright 360→356、TypeScript `src/compiler` 340→329、Next.js 781→743、Effect 515→491、VS Code 3,420→3,370。tsc-rs÷tsgoは0.59〜0.97、peak memory（MB main→本branch）：318→315、1,290→1,297、810→808、290→289、1,319→1,322、1,024→1,042、5,446→5,448。診断の出力と読み込んだdocument数は7 corpusともmainと同一。`tsconfig.bench-full.json`（JS・d.ts・source map、3 rounds）：hono 149→146、zod 606→611、Playwright 473→481、TypeScript `src/compiler` 537→489、Next.js 1,003→980、Effect 784→793（tsgo比0.57〜0.78）。zodの差をA/Bで確かめた：5 roundsはmain／本branch 575／578 ms、peak RSS 1,506／1,508 MB、単一checker（`TSRS_CHECKERS=1`）の命令数（5回のmedian）は39.364／39.376 G、peak memory footprint 939.2／938.4 MBで同じ。診断は6 corpusともmainと同一。出力はTypeScript `src/compiler`とEffectで同一、hono 15・zod 1・Playwright 7・Next.js 12 fileの.d.tsが変わり、変わった行は引用符を揃えるとすべて一致する（再利用した文字列literal型が元の`'…'`を保ち、tsgoの出力に近づいた）。退行なし。
