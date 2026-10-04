# 宣言の型をtsgoのpseudocheckerで作る

状態：**PC1–PC3 完了、PC4 未着手**（2026-10-04）。ユーザー決定（2026-10-04）：「pseudocheckerを移植（推奨）」。前段：
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
- 残り（PC4以降）：
  - classの`extends`の式の型（`CreateTypeOfExpression`）をtsgoの`serializeTypeForExpression`にし、6.0の
    syntactic builderの残りの推論を消す（PC4）。
  - tsgoの直列化cacheの範囲と複製（上の`noImplicitThisBigThis`）。
  - JavaScriptの`@this`：tsgoの再parseは関数のparameterの先頭に`this`を加えるが、tsc-rsのparameter一覧には無い。
