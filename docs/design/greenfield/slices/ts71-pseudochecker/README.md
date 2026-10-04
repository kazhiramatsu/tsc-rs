# 宣言の型をtsgoのpseudocheckerで作る

状態：**PC1–PC4 完了**（2026-10-04）。ユーザー決定（2026-10-04）：「pseudocheckerを移植（推奨）」。前段：
[JavaScriptのd.ts](../ts71-js-declarations/README.md)（完了）、[TypeScript 7.1への切替](../ts71-cutover/README.md)のP3-5ac。

## 背景

- **tsc-rsの現状。**
  - 宣言の型（`crates/checker/src/node_builder/serialize.rs`の`serialize_type_for_declaration_in_context`）と
    signatureの戻り値の型（`serialize_return_type_for_signature_in_context`）は、tsc 6.0の`expressionToTypeNode`の移植
    （`crates/checker/src/syntactic_type_node_builder.rs`、4,397行）を先に呼び、結果が無ければcheckerの型を直列化する。
  - 6.0の`typeFromObjectLiteral`と`typeFromArrayLiteral`は型literalやtupleを組み立てても`notImplemented`を返す
    （_tsc.js:134146-134221）。そのため`as const`のobject literalと配列の初期化子は、常にcheckerの型の直列化
    （`typeToTypeNode`）になる。
- **tsgoの方式**（`target/typescript7/upstream/tsc/internal`、19dadef88）。
  - **pseudochecker**（`pseudochecker/`、1,126行）：宣言・式・signatureから、型を使わずに構文だけで「pseudo type」を作る。
    種類はDirect（書かれたtype node）、Inferred（構文で扱えない式。error nodeを持てる）、NoResult、
    MaybeConstLocation（const文脈かどうかで変わるliteral）、Union、primitive、literal（元のnodeを持つ）、
    SingleCallSignature、Tuple、ObjectLiteral（property・method・accessor）。入口は`GetTypeOfDeclaration`、
    `GetTypeOfAccessor`、`GetReturnTypeOfSignature`、`GetTypeOfExpression`（lookup.go）。
  - **node builder**（`checker/pseudotypenodebuilder.go`、765行）：
    - `pseudoTypeEquivalentToType`がpseudo typeとcheckerの型の等価を確かめ、等価なときだけ
      `pseudoTypeToNodeWithCheckerFallback`でpseudo typeからnodeを作る。等価でなければcheckerの型を直列化する
      （`serializeTypeForDeclaration`、nodebuilderimpl.go:2253-2370；`serializeReturnTypeForSignature`、2100-2140）。
    - literalは元のnodeを再利用する（`NewLiteralTypeNode(b.reuseNode(source))`、pseudotypenodebuilder.go:318-320）
      ので、引用符・template literal・escapeが書いたまま残る。property名も書いたままの名前を使う（`reuseName`）。
    - isolatedDeclarationsの診断（`ReportInferenceFallback`）は、等価判定とpseudo typeのnode化から出る。
- **差の例**（P3-5ac後のconformance）：
  - literalの書き方：`isolatedDeclarationsLiterals`（`readonly oneStrSingleQuote: '1'`、`` readonly oneStrTemplate: `1` ``）、
    `constAssertions`（`` declare let v2: `abc`; ``）、`correlatedUnions`（`readonly name: 'a'`）、
    `declarationEmitNonAsciiStringLiteralType`、`unicodeSurrogatesInStringLiterals`。
  - object literalのaccessor：`declarationEmitObjectLiteralAccessors1`と`…Js1`（型の付いたget/setの組を
    accessorのまま書く）。
  - 実際のcodeでも、単一引用符で書いた`as const`のobjectのd.tsが、tsgoは`'x'`、tsc-rsは`"x"`になる。

## 方針

- **pseudocheckerを移植する**（`crates/checker/src/pseudochecker.rs`）。binderのparse tree（`NodeId`）だけを読み、
  型を使わない。JavaScriptのJSDocの型は、J1bのhostedの欄（`Type`・`FullSignature`）から読む。
- **node builder**（`crates/checker/src/node_builder/`に新しいmodule）：`pseudoTypeToNode`、
  `pseudoTypeToNodeWithCheckerFallback`、`pseudoTypeEquivalentToType`、`pseudoParametersEquivalentToParameters`、
  `pseudoReturnTypeMatchesPredicate`、`pseudoTypeToType`を移植する。
- **宣言の型とsignatureの戻り値の型**を、tsgoの`serializeTypeForDeclaration`と`serializeReturnTypeForSignature`の
  構造に置き換える。
- **退役**：6.0のsyntactic builderのうち、式から型を推論する部分（`serializeTypeOfDeclaration`・
  `serializeReturnTypeForSignature`・`serializeTypeOfExpression`・`serializeTypeOfAccessor`、`typeFrom*`、
  `inferTypeOf*`）。書かれたtype nodeの再利用（`tryReuseExistingTypeNode`、tsgoのnodecopy.go）は残す。

## 段階

| slice | 内容 | 終了条件 |
| --- | --- | --- |
| PC1 | pseudocheckerの移植（pseudo typeの構築）とunit test | 宣言・式・signatureから作るpseudo typeがtsgoのlookup.goと同じ |
| PC2 | node builder：pseudo typeのnode化・等価判定・`pseudoTypeToType`。宣言の型（変数・property・parameter・accessor・export assignment・expando）をtsgoの経路にする | 0 regressions。宣言の型が6.0の式からの推論を使わない |
| PC3 | signatureの戻り値の型をtsgoの経路にする | 同上 |
| PC4 | 退役：6.0のsyntactic builderの式からの推論 | node builderが6.0の`expressionToTypeNode`の推論を呼ばない |

各sliceの検証は切替packetと同じ：
- tsgoの出力にpinしたunit test。
- full conformanceを1回（0 regressions、同じreportからratchetを更新）。
- hosted。
- README corporaのperf（`--noEmit`と`tsconfig.bench-full.json`）と、fullの出力のtsgoとの比較。

## 注意点

- **isolatedDeclarationsの診断。** 今は3構成を除いてtsgoと一致している。等価判定とnode化が`ReportInferenceFallback`を
  出す位置・順序（`reportErrors`、error nodeの選び方、`suppressReportInferenceFallback`）をそのまま移し、6.0の
  syntactic builderの報告と同時に入れ替える。
- **等価判定はcheckerの型を読む**（`getPropertiesOfType`、`compareTypesIdentical`、`getRegularTypeOfLiteralType`、
  `isConstContext`、`getContextualType`、`getSingleCallSignature`など）。型の作成やcacheの副作用で、checkerの順序に
  依存する出力（unionの並び、型の名前）が変わらないかを見る。
- **型の取り方。** tsgoは宣言の型に記号の型を広げたもの（`getWidenedLiteralType(getTypeOfSymbol)`）を使うので、
  `as const`のobjectでも`RequiresWidening`の関門を通る。tsc-rsの呼び出し元が渡す型を合わせる。
- **`enclosingSymbolTypes`**（`addSymbolTypeToContext`）と、hover用の`isActivelyExpanding`（tsc-rsには無い）。
- **property名とcomment。** pseudo object literalは書いたままの名前（`reuseName`）と、同じfileならpropertyの
  comment範囲を使う（pseudotypenodebuilder.go:300-302）。
- **perf。** 宣言ごとにpseudo typeを作って等価を確かめるので、checkerの型の作成が増えうる。corporaで命令数まで確かめる。

## PC1–PC3 pseudocheckerと宣言・戻り値の型（2026-10-04）

PC1・PC2・PC3を1つのbranchで入れた。PC1だけではpseudocheckerを使う所が無く、戻り値の型（PC3）も同じ等価判定と
node化を使う。isolatedDeclarationsの診断も宣言と戻り値の両方の報告位置で決まるので、同時に入れ替えた。

- **PC1 pseudochecker**（`crates/checker/src/pseudochecker.rs`）：tsgoのtype.goとlookup.goを移植した。
  - binderのparse treeだけを読む。JavaScriptの型はtsgoの再parseが付けるもの（J1bのhostedの欄）を読み、hostedの
    cast（`@type`・`@satisfies`）は、tsgoの木でcastがある位置（keyの式の親）として祖先の探索に入れる。
  - tsgoが作らない`Any`と、呼び出し元の無い`GetTypeOfExpression`は移植しない。
  - tsc-rsが残すJSDocの関数型と`@callback`の`JSDocParameterTag`は、tsgoの再parse後の関数型・parameterとして扱う。
  - `ForEachReturnStatement`はsource順にたどる（既存のworklistは順序が違う）。
- **PC2 宣言の型**（`node_builder/pseudo.rs`、`serialize.rs`の`serialize_type_for_declaration_in_context`）：
  - tsgoの`serializeTypeForDeclaration`の構造：型の決め方（enclosing symbol types、set accessorのwrite type、
    widened literal type）、`requiresAddingImplicitUndefined`（propertyはreverse mappedの腕、tsgoのまま）、
    `unique symbol`のflagのsave/restore（6.0は戻さなかった）、宣言の無いsymbolはvalue declarationか最初の宣言。
  - `pseudoTypeToNode`・`pseudoTypeToNodeWithCheckerFallback`・`pseudoTypeEquivalentToType`・
    `pseudoParametersEquivalentToParameters`・`pseudoReturnTypeMatchesPredicate`・`pseudoTypeToType`と、
    nodecopy.goの`reuseTypeNode`・`reuseNode`・`reuseName`。型付けの再利用そのものは6.0の訪問を使う。
  - 再利用する文字列literalは書いたままの文字を保つ（`NoAsciiEscaping`、nodecopy.go:810-821）。JavaScriptの
    `@template`の型parameterは、tagの制約を最初の型parameterに付ける（tsgoの再parseと同じ）。
  - signatureのfake scope：展開したparameterの元の名前は元のsymbolを指し（6.0はunknownにした）、binding patternは
    最初の要素だけをlocalにする（tsgoのまま、nodebuilderscopes.go:166-191）。
- **PC3 戻り値の型**：tsgoの`serializeReturnTypeForSignature`（enclosing symbol typesの型、`tryReuse`、推論した
  type predicateの確認）。APIはsignatureのscopeに入ってから直列化する（nodebuilder.go:116-124）。
- **isolatedDeclarationsの診断**（`crates/emitter/src/declarations/tracker.rs`）：tsgoの`ReportInferenceFallback`
  （tracker.go:86-100）と`getIsolatedDeclarationError`の振り分け（diagnostics.go:701-735）。expandoの関数の報告を
  先に行い、bound expandoの代入の中のnodeは報告しない（`isChildOfBoundExpando`）。entity nameはprivate nameの
  診断。parameterの`requiresAddingImplicitUndefined`はenclosing無しで問う。
- **printer**：reuseした文字列literalが文字を保つので、tsgoの`escapeStringWorker`と同じく対になっていない
  surrogateは`NoAsciiEscaping`でも`\uXXXX`にする（printer/utilities.go:84-86）。node builderのtemplate literal型の
  文字も保つ（nodebuilderimpl.go:3576-3586）。tsc 6.0.3から記録したprinterのfixtureのうち、この規則で変わる
  case（8 fixture、139 case）は`tsgo_overrides`に列挙して書き直した。
- **退役した6.0のコード**：syntactic builderの宣言の型と戻り値の型の推論（`serializeTypeOfDeclaration`、
  `serializeTypeOfAccessor`、`serializeReturnTypeForSignature`、`typeFromVariable`・`typeFromProperty`・
  `typeFromPropertyAssignment`・`typeFromExpandoProperty`など）と`getDeclarationWithTypeAnnotation`。残る6.0の
  推論は、classの`extends`の式の型（`CreateTypeOfExpression`）だけ（PC4）。
- unit test：
  - pseudochecker（8件）：lookup.goから導いた期待値（変数・object literal・戻り値・関数式のparameter・classの
    member・const文脈・`undefined`を指しうる型・JavaScriptのJSDoc）。
  - CLI（tsgoの出力にpin、4件）：宣言の型（literalの書き方、accessorの組、引数の`| undefined`など）、
    JavaScriptの宣言の型、isolatedDeclarationsの診断、既存の`@template`のtest。
- conformance：
  - 15,228構成、lane A 13,467（変化なし）、452 s。
  - errors full 13,329→13,332（+3、`isolatedDeclarationsAddUndefined`、`isolatedDeclarationsJsThisPropertyAssignmentInference`、
    `isolatedDeclarationsNonIdentifierAssignmentInference`）、mismatch 83→80。
  - emit full 13,311→13,326（+15）、emit mismatch 126→111：`isolatedDeclarationsLiterals`、`constAssertions`、
    `correlatedUnions`、`declarationEmitNonAsciiStringLiteralType`、`unicodeSurrogatesInStringLiterals`、
    `declarationEmitObjectLiteralAccessors1`／`Js1`、`declarationEmitExactOptionalPropertyTypesNodeNotReused`（2構成）、
    `declarationEmitOptionalParameterUndefined`、`declarationEmitPropertyNumericStringKey`、`letDeclarations`、
    `definiteAssignmentAssertionsWithObjectShortHand`、`jsdocTypeParameterTagConflict`、
    `isolatedDeclarationsJsThisPropertyAssignmentInference`。
  - mapが上がった構成：`declarationEmitNoCrashOnCommentCopiedFromOtherFile`。
  - 下がった構成は無い。不一致のまま出力が変わったのは2構成で、どちらもtsgoに近づいた：
    - `declarationsWithRecursiveInternalTypesProduceUniqueTypeParams`：`testRecFun`の省略の深さがtsgoと同じになった。
      型parameterの制約の別名（tsgoの`Key<T>`、tsc-rsの`keyof T`）はmainと同じ差。
    - `noImplicitThisBigThis`：型の構造がtsgoと同じになった。tsgoの直列化cacheはnodeを複製するときに合成comment
      （`/*elided*/`）を落とし（`copyFrom`はcommentを写さない）、cacheは要求ごとのnode builderにある。tsc-rsは
      commentを保ち、arenaごとにcacheする。
  - 1回目のfull run（`91df4ccd5`、444 s）でこの2つの差と、`declarationEmitNonAsciiStringLiteralType`の
    template literal型、`unicodeSurrogatesInStringLiterals`の対になっていないsurrogateを見つけ、後の2つを直した。
- ratchet：0 regressions、17行raise。`intersectionConstructorReductionCrash`は今回もharness errorで、ratchetに
  入れない。
- local：
  - formatとworkspace全体のclippy。binder・emitter・checker・compiler・conformanceのtest（38 targets）。
  - 試行（release runner、filter `isolatedDeclaration`・`declarationEmit`・`jsDeclarations`・`onstAssert`・`iteral`・
    `ccessor`・`ymbol`・`xpando`・`eclaration`）で、mainのreportより下がった構成は無かった。
- hosted：PR #653（head `5a90ccc31`、merge `21f09a2e7`）、run 37190701375 — `plan` 20s、`rust` 8m30s、
  `conformance (TypeScript 7.1)` 19m27s、`gates` 12s。
- perf（README corpora、nice 20、main（P3-5acのcode `91fea3f56`）と本branch（`b06ff367b`）のrelease build対tsgo
  7.1.0-dev、median wall ms main→本branch）：
  - `--noEmit`（3 rounds）：hono 144→131、zod 549→538、Playwright 367→384、TypeScript `src/compiler` 349→347、
    Next.js 797→762、Effect 562→527、VS Code 3,655→3,597。tsc-rs÷tsgoは0.60〜0.99。診断の出力と読み込んだ
    document数は7 corpusともmainと同一。
  - `tsconfig.bench-full.json`（3 rounds）：hono 152→154、zod 708→660、Playwright 555→538、TypeScript
    `src/compiler` 560→522、Next.js 1,110→1,103、Effect 870→861（tsgo比0.59〜0.76）。診断はmainと同一。
  - A/B：Playwrightの`--noEmit` 10 roundsは366／363 ms。5 roundsはPlaywright full 487／486、Effect full 812／772、
    Next.js full 1,026／1,017 ms。
  - 単一checker（`TSRS_CHECKERS=1`、5回のmedian）の命令数：Playwrightの`--noEmit` 22.342／22.346 G（peak memory
    footprint 516.2／516.4 MB）、full 34.105／34.133 G、Effect full 54.792／52.966 G（929.4／835.7 MB）。退行なし。
  - fullの出力をtsgoと比べた（6構成）：mainから変わった719 fileのうち591（d.ts 42、declaration map 549）がtsgoと
    byte単位で同じになり、tsgoと同じだったfileで違うようになったものは無い。残る128（map 126、Effectのd.ts 2）は
    mainと同じ差。変わったmapのmapping segmentは11,468がtsgoに近づき、300が離れた（既存のd.tsの差で行がずれた所と、
    上の直列化cacheの複製）。
- 残り（PC4以降）：
  - classの`extends`の式の型（`CreateTypeOfExpression`）をtsgoの`serializeTypeForExpression`にし、6.0の
    syntactic builderの残りの推論を消す（PC4）。
  - tsgoの直列化cacheの範囲と複製（上の`noImplicitThisBigThis`）。
  - JavaScriptの`@this`：tsgoの再parseは関数のparameterの先頭に`this`を加えるが、tsc-rsのparameter一覧には無い。

## PC4 6.0の式からの推論の退役（2026-10-04）

node builderは6.0の`expressionToTypeNode`の推論を呼ばなくなった。書かれたtype nodeの再利用（6.0の訪問、tsgoの
nodecopy.go）だけが`crates/checker/src/syntactic_type_node_builder.rs`に残る（PC4の前は4,089行、今は1,934行）。

- **式の型**（`serialize.rs`の`serialize_type_for_expression`）：tsgoの`serializeTypeForExpression`の仮実装と同じく、
  式のregularな型（`getRegularTypeOfExpression`、qualified nameやproperty accessの右辺なら親）を広げ、contextの
  mapperで具体化して直列化する（nodebuilderimpl.go:1811-1815）。pseudo typeの`pseudoTypeToType`も同じ関数を使う。
- **classの`extends`の式**：declaration transformが型を求める前に`ReportInferenceFallback`を出す（transform.go:2013）。
  emitterのresolverにmember `report_inference_fallback`を加え、checkerはtrackerのaccessを渡す。6.0の推論が
  出していた報告と位置・順序は同じ（TS9021）。
- **computed property名**：再利用する型の中のcomputed名のentity nameがerrorになるとき、tsgoはrecoveryの境界に
  errorを付けて子を訪ねる（nodecopy.go:751-759）。6.0はevaluatorとcheckerから名前を書き換え、そのとき名前を
  trackしてprivate nameとして報告した（関数の中のconst enumのkeyでTS4060。tsgoは報告しない）。
- **消したもの**：sessionの推論の関数（`typeFrom*`・`inferTypeOf*`・`serializeTypeOf*`など）、resolverのmember
  12個（`serializeTypeOfExpression`・`serializeTypeOfDeclaration`・`serializeReturnTypeForSignature`・
  `canReuseTypeNodeAnnotation`・`evaluateEntityNameExpression`・`trackComputedName`など）、`SyntacticResult`・
  `SyntacticSymbol`・`SyntacticAccessorDeclarations`、contextの`noInferenceFallback`。訪問が使う
  `serializeExistingTypeNode`は`addUndefined`の腕を除いて残した（呼び出し元はfalseだけを渡す）。
- **診断の型表示**（`check.rs`の文字列の表示）：
  - signatureの書いた戻り値の再利用は、enclosing declarationがあるときだけにした（tsgo nodebuilderimpl.go:2114）。
    診断がenclosingを渡すのは、文脈に依らない式の型だけ（`getTypeNamesForErrorDisplay`、relater.go:1270-1288）。
    たとえば関数宣言の`return x as number | string`は、tsgoと同じくcheckerのunion（`string | number`）になる。
  - TS2208（`This type parameter might need an extends … constraint`）の型は、tsgoと同じくenclosing無しで表示する
    （relater.go:4777。6.0はtype parameterの宣言を渡した）。
- unit test（tsgoの出力にpin）：
  - CLI：classの`extends`の式（d.tsとisolatedDeclarationsの診断）、再利用する型の中のcomputed名、診断の戻り値の表示。
  - checker：訪問のcomputed名のerrorの腕、TS2208の制約の文字（7.1の既定の`strict`で`| undefined`）、
    6.0の表示にpinしていた`relation_reporting_keeps_union_keyof_and_class_member_failure_levels`の書き直し。
- conformance（release build、`31b313976`、`--workers 2`、442 s）：
  - 15,228構成、lane A 13,467（変化なし）。
  - errors full 13,332→13,337（+5、どれもText→Full）：診断の戻り値の表示で`baseClassImprovedMismatchErrors`、
    TS2208の制約の表示で`tsxNotUsingApparentTypeOfSFC`・`tsxGenericAttributesType5`・`tsxGenericAttributesType6`・
    `subtypingWithOptionalProperties`。text 14→9。
  - emitとmapは変化なし（emit full 13,326、emit mismatch 111）。下がった構成も、tierが同じまま出力が変わった構成も
    無い。
- ratchet：0 regressions、5行raise。`intersectionConstructorReductionCrash`は今回もharness error（memory上限）で、
  ratchetに入れない。
- local：formatとworkspace全体のclippy。binder・emitter・checker・compiler・conformanceのtest（38 targets、2,713件）。
  試行（filter `isolatedDeclaration`・`declarationEmit`・`ixin`・`omputed`・`lassExpression`）で下がった構成は
  無かった。
- 残り：
  - 診断の型表示で、enclosingがある関数式の戻り値：tsgoはpseudo typeを使うので`as const`のliteralを書いたまま
    表示する（`() => { readonly a: 'x'; }`）。tsc-rsは6.0のassertionの再利用だけで、`"x"`になる。
  - tsgoの直列化cacheの範囲と複製（PC1–PC3の`noImplicitThisBigThis`）。
  - JavaScriptの`@this`：tsgoの再parseは関数のparameterの先頭に`this`を加えるが、tsc-rsのparameter一覧には無い。
