# 宣言の型をtsgoのpseudocheckerで作る

状態：**設計**（2026-10-04）。ユーザー決定（2026-10-04）：「pseudocheckerを移植（推奨）」。前段：
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
