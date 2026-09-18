# Emitter residual audit

2026-09-18。提出r11を受領した統合候補 `ed71c45343b2063414d5563c9b5455631d7c5c68`
に対する追加監査。**提出REPORTの「emitter ownerの未解決行0」は、通常emitの残作業0を
意味しない。実際に通常のProgram/commandで残差と回帰を確認した。**
本書の修復は検証中であり、最終hosted receiptが完成するまで統合完了とはしない。

## 確認した残差と修復

| 原因 | 提出候補での証拠 | 修復方針・検証 |
| --- | --- | --- |
| ESM helper名衝突 | EF7 PLAN-BASEの `importHelpersWithLocalCollisions` 1行がH2.5h ownerとして残っていた。実装は `insert_external_helpers_import_declaration` の拒否 | 同じsource/helperに同じ生成bindingを割り当て、import aliasとhelper参照に確定名を共有。既存111 commandに、3 modules × 2 targets × 4衝突・source分離形状の24 controlsを追加 |
| async arguments / superの宣言順 | H2.8cの3行。うち `control-checked/es2015-flags` は通常checked Program | TypeScriptと同じprologue挿入順。3行exact×2を確認しKNOWNから退役 |
| namespace `export import` のmap | H2.8cの4行。うち `control-checked/namespace-source-map` は通常checked Program | propertyのcloneは位置をsyntheticに保ち、access全体がname rangeを所有。4行exact×2を確認しKNOWNから退役 |
| ES5 anonymous class/functionのassigned name | 新設のfull class CIが24行の不一致を検出。3つの既存fixtureに各8行 | 変換でparentを失った式に、visitorの囲む変数名を無条件に補う処理を撤去。generated declarationの名前は既存original chainから得る。24行は固定rosterでもexact×2を確認 |
| concise arrowの余分なmap境界 | 既存decorator-super primary 5行、followup 5行 | factory/printerのcomma判定修正だけでは直らない。完全captureで、lexical temporariesによりarrowをblockへ変換した後のreturnに余分な括弧が残ることを確認。Claudeと原因を照合し、raw child visit→lexical environment終了→body変換→一回のfactory updateに修復。primary/followupの18選択行と18追加対照がexact×2 |
| synthetic constructorへのresolver query | decorator-super primary 2行とglobal `jsFileCompilationAwaitModifier` | parse-tree identityを持たない合成functionはcheckerのasync-super flagsを持たない。arena内IDをresolverへ渡さない |
| 末尾JSDoc前の改行 | global `jsDeclarationsTypeAliases` のJS callbackが417 bytes、TypeScriptは419 bytes | SourceFileのMultiLine listを閉じてからEOF commentsを出力 |
| outFile + importHelpers | 既存 `import-helpers/bundle/control/import-helpers` は通常commandだが一律のoption拒否が残っていた | bundleの既存module/helper経路でexact×2を確認。alias24・通常source4を含む139 commands、filesystem24、class24、global2のfocused4 testsが成功 |

既知差分7行の退役前native outcomeは
[retired-transpile-emitter.v1.json](records/retired-transpile-emitter.v1.json)に保存した。
`audit-transpile` は287入力＋14 API factsを含む9 testsを実行し、新規差分0、
7行のstale-KNOWNのみでexit 101となった。台帳は15→8へ縮小し、行そのものと
TypeScript側の期待値は削除していない。後続の最終候補では全testの成功を要求する。

## 検証の穴と統合上の不具合

提出の217＋1798行はstrictなnative KNOWN比較を含めて再現できたが、
従来のdecorator-superとfull class/globalには別の失敗があった。
最初のhosted runは23 checks中13成功、10失敗（gate2を含む）。
[全jobの記録](hosted/progress-ed71c45343b2.json)とraw logを保存している。
PLAN-BASEの4 shards、217、class-1、retained、printer、foundations、
decorator-binding pipeline、TypeScript universe oracleは成功した。

acceptance3群は共通の圧縮fixture readerをxtaskがincludeしたことによる
`zstd` dependency宣言不足でbuild失敗した。依存を明記する。
case-insensitive oracleの修復後、D/Eのcurrent artifactsを再観測した際の
新しいprovenance hashをcurrent consumersへ伝播させる。歴史的before/after
packetは再署名しない。D/E qualificationとdirectory23は公式writerで再生成し、
すべての既存case observationsが不変であることを確認する。

EF8がunhostedとした `output-filesystem` 24 commands（all-writes failureを含む）と
`import-helpers` の全commandsを `emitter-final` に登録する。
後者にはenum、namespace、parameter property、legacy decoratorの通常source controlsを
各1行追加した。TypeScript transformの前段を通る対照であり、custom transformerを
注入して結果を通常emitの証明に流用するものではない。
[再採取記録](records/audit-oracles.v1.json)は既存111＋23観測のbyte不変を検証している。

## 完了範囲の限界

KNOWNは互換成功ではない。受領時のparse-recovery36行とchecker27行も通常commandの未解決差分として監査した。checker27行は修復後に元観測へ各2回一致し、退役した。parse-recovery36行は未解決のままである。
helper修復は元のPLAN-BASE 4 module variantsでもexact×2となり、退役前recordを保存して68→67行へ縮小した。resolution 4行も周辺を含む15 commandsがexact×2、request-plan 40 testsが成功した。退役前nativeを保存してさらに67→63行へ縮小した。checker27行の退役により最終36行となった。全体の回帰replayは継続する。
H2.8cは受領後の修復で15→8行。旧target1234の拒否もexactになった一方、追加target100で内部JSON source-kind境界を発見した。最終台帳はparse recovery 7とJSON target 1を区別する。
これらの集合は観測単位とrouteが異なるため、単純に足して製品の残件数としない。

公開APIにはdisposed TransformationResultの再print 2、custom-beforeで同じsuper式nodeを
2箇所に共有する4の既知差分がある。NodeList印字、standalone nodeのmap記録、custom
declaration transformers、targeted JS selectionにもtypedな未対応境界がある。
これらは通常のfresh commandとは別の公開API作業として残す。

`validate_emit_options` は `incremental` / `composite` / `tsBuildInfoFile`、
`stableTypeOrdering` も拒否している。
前者群はbuild/watchとの接続、後者はcheckerのtype orderingに関わる。
TypeScript 6.0.3の正規optionなので「存在しない入力」とは扱わない。
`assumeChangesOnlyAffectDirectDependencies` はfresh Programで無効なbuilder用flagのため、一律拒否を撤去した。flag/declarationの4対照を含む143 commandsがexact×2。これらを含む製品全体のemit完成は、このPRの成功からは導けない。

`Unsupported` の文字列検索だけで残件数は決めない。単一SourceFileを受け取る
transformのBundle拒否、hostを持たないdirect factory、旧H1形状検証、
不正なresolver callbackのguardは、通常commandで到達するかを呼出経路と併せて確認する。
isolated declarationのaccessor/parameter diagnosticsもtracker専用effectで処理される
経路があり、fallbackの拒否枝だけで機能欠落と数えない。

有限の回帰集合を通したという主張と、全TypeScript入力で差分が存在しないという
主張は区別する。最終的なexact/known/failed数とcommitはhosted receiptで固定する。

## 追加のcross-review対象

ユーザーの指示により、影響が大きい・未知の修復はClaude Codeと独立検討して照合する。
[依頼](cross-review/round1-request.md)と[Codex側のarrow原因分析](cross-review/codex-arrow-finding.md)を保存する。
parse-recovery/stable type orderingの一律guardは変更していない。

`has_advanced_comment_placement` のprivate-name/in間とoptional-chain/type-assertionの
raw text拒否を撤去し、下記の実際のcomment phase欠落を修復した。literal/comment本文も
通常commandの完全比較へ追加した。AstDepth 256は別の資源上限である。

H1歴史的omission writerの `--check` は `emitter-artifact-protocol` anchorで失敗する。
参照するartifact.rsとwriterはtrusted main `3b1f5fe`からbyte不変であり、この統合の
新規失敗ではない。現在の入口台帳は別に再生成し、古い全Rust棚卸しを互換証拠として再署名しない。

## Comment / CLI / numeric target の追加監査

raw textによるadvanced-comment guardは、正当なliteral/comment本文まで拒否していた。
撤去後227 commands中221 exact、6差分を検出した。map assertionがcallback bytesの比較より
先に失敗していたため、当初のmapだけという仮説を直接CLI出力で訂正した。
実際にはerased assertion/non-nullのchild trailing phaseが呼ばれず、comment自体が欠落していた。
Claudeも独立に同じ原因を特定した（round3）。既存のnested comment phaseに接続し、
66 comment controlsがJS/mapを含めてexact×2になった。satisfies・連続block・line・own-lineの
24 controlsも追加し、最終480 commands（468通常比較＋12 typed boundary）と既存printer topologyで確認する。

CLIはconfigの既存named-value catalogを共有し、15 module spellings×3 targetsと
ES5非抑制・ES3削除・mixed-caseを含む48 commandsを比較する。
最初のobserverは存在しないtsc.jsを指定したため無効だった。起動bundleを実在する
_tsc.jsに直し、Node stderrとstatusを厳密に検査する。診断位置を保つため、configの
入力bytesもfreezeした。修正版48 commandsはRustのexit/stdout/stderr/全outputと
成功時config移行の対照が各2回一致した。CLI全オプションの互換完成は主張しない。

未知の数値targetはupstream同様その値を保持する。1234の旧拒否はexact化したが、
100は内部のScriptTarget.JSONであり、単なる未知値ではない。追加対照でJSON parse routeと
通常TS parseの違いを検出した。parser/source-kindへの影響をClaudeとcross-reviewし、
この境界をguard撤去だけで解決したとは数えない。

emitter contractのinherited clippy 4件も、重複moduleの共通化・不要closureとborrowの除去で
修復する。checker全体の145件とは区別し、emitter all-targetsとcompiler lib/binを再測定する。

既存emitter全体の初回再検証は508 unit成功・contracts450成功/2失敗。
普通のchild leading phaseを有効化すると、PEE before側の旧補助leading collectorと
重複していた。upstreamと同じくbeforeはtrailingのみとし、childがleading/trailingを
所有し、afterはleadingのみとする。round6に原因・最終差分を照合する。

Pendingの再container化案は採らない。Claudeもround5で、LeadingOnlyがPEE Pendingへ
到達しないことを確認し、提案を撤回した。現phaseを保つ小さい修復と、到達性assertion・
return/throw/yieldのnested no-ASIを含む33 commandsを採る。


## Checker残件の追加修復候補

ownerをcheckerへ分類しただけでは残件を閉じない。Claude round7/8の調査とCodexの
upstream/Rust比較を照合し、以下を修正した。最終的な退役数は完全比較で確定する。

| 範囲 | 最小変更と負の対照 |
| --- | --- |
| import-equals診断・JS constructor span・tslib alias / rest (元4行) | JS/TS/module/interop分岐、constructor keyword終端、既存Value lookup経由のalias解決、target別helper要求。27対照 |
| isolatedModules (元8行) | 2865/1269/1280/1281/2866の欠落診断分岐。既存last_locationを成功callbackへ渡し、新しい名前解決状態は設けない。verbatim-only、型利用、same-file enum、非実体namespace等20対照 |
| 隣接helper要求 | 旧ES2025 floorを理由に省略していたES5 `__assign` とbinding/assignmentの `__read` をtarget/downlevelIteration/importHelpersでgate。22対照 |
| relatedInformation (元4行) | index/propertyの2箇所で既存ProgramFileFactsのdefault-library情報を読む。普通のuser root、lib風filename、lib reference、noLib明示rootを区別 |
| JS (元4行、3関数) | 既存is_optional_declaration、JS base constructorの型引数補完、expression statement先頭 `(` によるJSDoc host判定。relatedと合わせ20対照 |

追加checker対照178を含むhelper/ordinary command corpusは、round17で480行（468通常比較＋12 typed boundary）とした。元の111行は
公式TypeScript observerで観測値が不変であることを検査する。JS2行のmodule augmentationと
late member再統合、型深さ・型表示5行の修正案はround9–11で上流の該当分岐と照合した。
対象は既存のSymbol/type factsであり、診断文の置換や比較予算の引き上げは行わない。

| 追加修復 | 根拠と対照 |
| --- | --- |
| CommonJS augmentation / late member | 異なるfileのvalue宣言のduplicate診断と、既に解決済みの元symbolのlate tablesを合成cloneへ継承。checkJs、type-only、computed member、static、追加export、root順の16対照 |
| JSX literal表示 | 属性initializerを文脈型で再検査し、specific型と元property型で上流順にelaborate。引用string、number、as string、as const、unionの5対照 |
| homomorphic mapped / intersection | Substitutionから実type variableを取得。array/tupleだけのintersectionに限りmember単位で写像。8対照 |
| mapped recursion identity | InstantiatedMappedをsymbol付きmodifier targetまで剥がす。fallible化し、持続stack/flags/maybe stateは失敗時にも復元。reverse stackは再入時の状態を保つsnapshotを使う |
| conditional error identity | 共通error intrinsicとper-alias error型を区別。循環alias・未解決名の負の対照 |
| relation origin / cached overflow | 既に記録されるunion originによる上流shortcutと、cached overflowの診断再報告を移植。budgetは不変。小unionの3対照と元のlarge fixtureで確認 |

`audit-checker-focused` は旧89対照中28 exact、61 failures。
51件はtest adapterがnoEmitにemitting loaderを使った入口不一致で、通常のchecking loaderへ接続する。
残る10件は通常module変換の実差分であり、4件はJS本文とmap、6件はmap。
完全観測を `cross-review/destructuring-residuals.json` に保存した。
ES2015以降のexported arrayをflattenする際のlate __read要求、leafとexport wrapperのrange、
再利用valueのrange、object-rest除外配列のpattern rangeを上流に合わせる。
non-export、default値、computed key、tslib namespace有無、alias、ES5、AMD/UMDの16対照を追加した。

元27 KNOWNのうち25行は、元入力・全callback・全診断を2回比較してexactを確認し、旧nativeを保存して退役した。残るJSXとrelation complexityの2行もround14–16の修復後、元観測へ各2回一致して退役した。checker共有relationへの変更はunit/full conformanceも再検証する。

### Parse / JSON境界の共同判断

round12の分類はasync32、decorator2、top-level-await2。現在のparser recovery eventsは
message付きmissing nodeとskip/reparseを識別できないため、純粋なreport-only recoveryだけを
安全に許可する条件をまだ表現できない。後続でadmissionを変えずにfactsを追加し、censusを
取ってからreport-onlyを許可する。missing nodeの空印字と0幅mapは別に契約を固定する。
JSON100にはfilenameと独立したsource-kindの伝播が必要。両境界は現時点で未対応のまま。
moduleの最終reviewではdirect/alias順と0要素patternのtempを追加修復し、3対照も採取した。
round12時点の追加対照は142、corpus全体432だった。round15時点の追加checker対照は178、corpus全体468。round17で有効コメント対照12を追加して480行となった。これらの件数は観測数であり、製品の欠陥数ではない。


### 最終2 checker行と周辺の追加比較

`audit-checker-known-r2` は37選択中2 exact/35 known/0 failed。
終了101は一致した2行のretire assertionのみ。旧nativeとproofを
[最終2行の退役記録](records/retired-checker-final-two.v1.json)へ保存した。
先の25行は[退役記録](records/retired-checker-known.v1.json)に保持する。
比較器のoutput/diagnostic改変拒否テストはこの実在した旧観測を使用し、
live checker KNOWNが空になってもguard自体は保持する。

JSXではintrinsicとcomponentのattributes型、およびnested initializerの元source型を
elaborationへ渡し、文脈stack終了後の再計算を避ける。literal/式/nested object、
有効値、optional missing/undefined × exactOptional on/off × 2 tag kindsの28対照、
入れ子・array rest・複数alias・local binding × 2 targetsの8対照を追加した。
round16の468行再生は456 exact×2、12 parse refusal。CLI58は全exact×2。追加JSX28・module8を含むchecker対照178も全exact×2だった。構文エラー12行の扱いは下記参照。

noEmit configではplanが保持するoption診断をharness commandへ受け渡す。
実CLIにもexternal option診断があるとsemanticを抑止する既存欠落があり、
upstreamの報告条件へ合わせた。deprecation抑制有無 × noEmit有無の4対照を追加し、
旧54 CLI観測がbyte不変であることを`records/cli-deprecation-oracle.v1.json`で記録した。

### Malformed comment境界を通常emit成功と区別する

`audit-complete-commands-r5`の失敗12行は改行後の `as` に対しTypeScriptも
1005/1434/1128を報告し、recovery emitする入力だった。全入力・完全観測は
[cross-review/comment-parse-boundaries-r17.json](cross-review/comment-parse-boundaries-r17.json)
に保存し、fixtureから削除しない。Claude round17もparserのmissing/skip factsと
comment ownershipの契約が必要と確認した。現在のtyped refusalはcount=3、
recovery_events=4、owner=H2.9、partial writes=[]まで厳密に2回固定する。
新たな拒否や途中出力を許容せず、emit成功時はretire assertionが失敗する。

`as`を改行前へ置いた有効構文12行を別に追加する。最終480選択の期待は
468 exactと12 typed boundariesであり、互換成功を480とは報告しない。
EF7残36とは別の観測集合なので足して製品欠陥数にはしない。
