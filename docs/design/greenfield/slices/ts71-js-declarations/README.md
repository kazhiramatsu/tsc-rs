# JavaScriptのd.tsをtsgoの方式で作る

状態：**実装中**（J1 2026-10-04）。ユーザー決定（2026-10-04）：「再設計して進める」。前段：
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
