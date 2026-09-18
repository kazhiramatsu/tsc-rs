# Cross-review decisions

2026-09-18、ユーザーの依頼に基づきClaude Codeへread-onlyの独立調査を依頼し、Codexの再現・原因分析と照合した。依頼と回答はround1/round2に保存。実装と最終検証・統合の責任はCodex。

- **Arrow**: 両者が同じvisitor更新順序を原因と特定。raw child visit→lexical environment終了→bodyをblockへ変換→originalへ一回のfactory updateとする。括弧を後から剥がす案はparsed parenを壊すため採らない。無関係なfactory/printer partial-wrapper変更は撤回。parsed-comma/object-body/no-temporariesの18対照を追加し、既存primary/followup全群と合わせて検証する。
- **Resolution**: Node module＋Bundlerでimplied formatが無いことはupstreamの正当な状態。moduleへのfallbackとstatic Unspecifiedを採用。Some(Unspecified)という不正な公開factの拒否は保持。15原本commandsと40 request-plan testsに加え、authoritative format優先の対照を確認する。
- **Fresh Program option**: assumeChangesOnlyAffectDirectDependenciesの一律拒否を撤去。builderに入らない経路では意味を持たず、143 commands（flag/declaration 4対照を含む）がexact×2。
- **Comment guard**: raw text走査によるliteral/comment本文の誤拒否をCLIと48 complete commandsで再現。隔離候補でguardを撤去し、実際の構文を含めた完全観測で挙動を検証する。差分が残ればそのproducer/printerを直すか、実際の未対応境界だけをtyped AST/triviaで表現する。拒否が消えただけでは成功としない。sourceMapはobserver defaultsで全48行すでにtrueであり、JSとmapを比較する。追加のline-comment/JSDoc/non-null 18対照も含める。
- **Numeric target**: upstreamは未知の数値を保持する。i32全域の比較とdefault-lib fallbackが同じであることを両者で確認し、API固有の値正規化は採らず上限guardを撤去する案を採用。99/100/1234・declaration・default library対照を実行する。未検証段階では互換完了を主張しない。
- **CLI named values**: configの既存option catalogを再利用し、実装済みmodule/targetをCLIでも選択可能にする案に同意。config経路との一致とTypeScriptの凍結出力を検証する。CLI全体の完成とは別の主張。
- **Parse recovery / stableTypeOrdering**: 一律guard撤去は採らない。前者はparser recovery factsとmissing-nodeのprinter/map契約、後者はcheckerのunion/symbol/type-display順序の実装が必要。通常commandの未解決差分として理由を残し、別ownerだから解決済みとは数えない。

ここでの設計一致と、最終候補での実測成功は別に記録する。最終判断はhosted receiptと修復後のlocal recordsに従う。

- **Comment round3**: 当初mapだけとした推論を撤回。assertion順序でcallback比較前に停止していた。両者がchild trailing phase不発によるJS comment欠落を確認した。既存nested phaseへ接続し66 controls exact×2。source writer/map記録は変更しない。
- **Numeric round4**: 100は内部JSON値。source-kindがfilename判定に依存する現基盤では、JSONでない拡張子をJSON parse/emitする共有factsが足りない。明示typed拒否を保持し、通常未知値1234の解決とは分ける案で一致。transpileだけ数値を見てupstream例外を偽装する案は採らない。
- **Pending comment round4→5**: Claudeの追加所有権変更案について、提案された7つの普通のsource形状をCLIで直接比較したところ全てJS/map/診断/exitが一致した。追加の変更は適用せず、到達条件を再確認し、各形状を3 targetsの完全観測へ追加する。`comment-context-cli-probes.json.gz`は初回比較の記録。

- **Round5合意**: LeadingOnly Pendingはno-ASIの到達条件から通常入力では到達不能。Claudeは再container化提案を撤回。既存semanticsを保持し、debug assertionとnested return/throw/yield対照を追加する。
- **Full-contract回帰**: child leading phaseを戻したことで旧before側の補助leading出力と重複した2テストを検出。upstreamと同じbefore-trailing / child通常phase / after-leadingに整理する。完了は再測定で判断する。

- **Round6最終comment所有権**: before-trailing / child通常phase / after-leadingで一致。`f(<T>/*c*/x)` と `f((/*c*/x as T))` の余分なprefix空白をdirect CLIで再現し、既存intervening-comment writerへ接続した。3 targetsの対照を追加。writer全体やsource-map境界の再設計は行わない。
- **Round7 checker A**: TS2597/2598のJS arm、constructor keyword終端までのJS grammar span、tslib aliasのValue lookup、object restのhelper要求を、双方でupstream/Rustの欠落として確認。JS/TS×module×interop、UTF-16 comment付きconstructor、direct/reexport/star/type-only helper、restのtarget/importHelpers対照27 commandsを追加した。
- **Round7 checker B**: isolatedModulesの2865/1269/1280/1281/2866を既存owner関数の診断分岐として移植する。Codex独立調査では、2866はwalkに新しいboolean状態を持たせる必要がなく、既存`last_location`を成功callbackへ渡せばupstreamの最終scope判定ができる。名前解決の選択順・alias結果そのものは変更しない。一般名解決への副作用は全checker単体とhosted acceptanceで確認する。
- **Adjacent helper demand**: Claude round7の`__assign`確認提案に従い、CodexがObjectAssignとarray binding/assignmentのReadも同じ旧ES2025 floor前提で省略されていることを確認。既存helper要求関数をupstreamのtarget/downlevelIteration gateから呼ぶだけに限定し、ES5/ES2015、importHelpers on/off、downlevelIteration offの22 controlsを追加する。実測前に解決数へ加えない。
- **途中測定の扱い**: `audit-checker-and-comments-r3` はビルド成功後の広い再生途中で、上記隣接helper修復を先に確定するためCodexがSIGINTで停止した。成功receiptではない。最終359 commandsは改めて最後まで検証する。
- **Round8**: related-info 4行の原因はdefault-library gate欠落で一致 (`codex-library-related-finding.md`)。JSは3関数で4行 (genericDefaultsJsとjsExtendsImplicitAnyは2行) を閉じる案: 既存is_optional_declaration、JS用base constructor型引数補完、JSDoc statement先頭token判定。既存共有関数を利用し、JS/TS・initializer・先頭括弧6形状・default/reference/noLib user rootを含む20対照を追加。残りJS2行は原因を更にtraceするround9へ送った。round8回答の集計「7行」は8行の数え誤りとして扱う。
- **Round9 JS trace**: 実際のTypeScript診断stackは`mergeSymbol`ではなくassignment initializerのcommonJS value mergeを指した。Codexも56391以降と`combine_common_js_export_members`を照合し、両value宣言のfile不一致時の2300×2と相互related-infoが欠落していることを確認。もう1行は57747以降のcjsExportMerged宣言元tableのfold欠落。既存cached lateBindMemberを変更せず、元宣言の既に解決済みtableだけを静的/instance両方へfoldする方針。probe scriptsとinstrumentation差分は`round9-probes/`へ保存。実装・完全比較が完了するまで台帳は退役しない。

## Round 10–12: checker core / module destructuring

Round10の5局所欠落をvendored TypeScriptの関数と独立照合した。diagnostic textの後処理や
深さ/budgetの緩和は採用しない。mapped recursion helpersはfallibleとして移植し、relation frameの
既存Err cleanupを通す。reverse mapped stackはmem::takeよりもsnapshotを採用する。modifiers
解決からの再入が起きてもCheckerState側のstackが空にならないためである。
Round11は追加89対照で見つかった10 emitter差分を独立調査。うち4件のみJS差、6件はmap差と
完全captureで確定した（初期集計のes2018 JS差という分類を訂正）。module専用flattenの不足を
局所修正し、16周辺対照を追加した。noEmit51件はproduction差分ではなくtest adapter入口の誤り。
checking loaderを使い、config/noEmit/oracle期待値は変更しない。
Round12でこれらの実装と、残るparse/JSON source-kind境界の対応可能性を再照合中。

Round12はchecker core/深さガード/CJS合成に意味差なしと独立確認した。module側は
direct exportをaliasより内側に固定する順序、0要素declaration patternのfresh temp不足を追加指摘。
上流 createAllExportExpressions / isDeclarationBindingElement と再照合し、修正と3対照を追加した。
共通source/optionは432 commandsで再採取・検証する。JSX exactOptionalPropertyTypes分岐には
既存欠落の可能性が指摘されたが、元27行の修復とは別の未実測事項である。
parse36はasync32/decorator2/top-level-await2。parser eventにはmessage付きmissing nodeと
skip/reparseの区別がなく、文字列のguard除去で安全に扱える境界ではない。後続は
admission不変のrecovery facts拡張→census→report-only recoveryのadmit→missing nodeの
printer/map契約の順とする。JSON100もfilenameに依存しないscript-kind factsが必要。
これらは未対応の入力であり、互換成功として計上しない。

## Round 13: remaining module maps / noEmit + sourceMap

`audit-checker-focused-r3` は142選択中51 exact/91 failure。85はH0 config loaderのsourceMap
admission拒否（入口の修正後に露呈したproduction境界）、6は4 map endと2 alias publication。
後者のCodex独立解析を[codex-destructuring-r3-finding.md](codex-destructuring-r3-finding.md)に
保存し、Claudeも同じsource位置・欠落分岐を確認した。comma rootのrangeのみを除去して
original provenanceを保ち、direct binding patternにも既存alias append経路を接続する。
noEmit/sourceMapはCompilerOptionsへのprojectionとoption診断が既にあり、不足factsなし。
H0許可にsourceMapのみを追加する案で一致した。noEmit/optionをadapterで消す、raw loaderで
config scope gateを飛ばす、といった回避はしない。追加CLI対照とzero-emitter assertionsで確認する。
inlineSourceMap/mapRoot/sourceRoot/build/watchなど他の既存拒否はこの変更からは解決済みとしない。


## Round 14: original checker KNOWN retirement

元62 PLAN KNOWNを再生し、checker25行が元の完全TypeScript観測に各2回一致した。
退役前nativeとproof log hashは`records/retired-checker-known.v1.json`へ保存した。
残るJSX/complexityの2行は引き続き未解決とし、相違したnativeをKNOWNへ上書きしない。
JSXの下層wideningに特例を加える仮説は、両者がupstreamとの一致を確認して撤回した。
真因はintrinsic attributesの文脈型stack終了後の再検査。既存のsource明示elaboration入口へ
元attr_typeを渡す。component/initializerの同じ再検査も隣接対照で確認する。
relationの初回overflowは非報告probeでもcurrentNodeへ診断する上流分岐が欠落していた。
boolean/reportingの双方でcache・診断・captured outputを整合させる。予算変更はしない。
noEmit/sourceMapは後続command projectionとしてscope条件へ明示追加する。
歴史的H0 allowlistとqualificationは変更せず、新CLI6対照と既存85対照で現在の挙動を検証する。


## Round 15–16: diagnostic ownership and ordinary noEmit commands

fresh overflowのboolean入口はcurrentNodeへ直接発行し、reporting入口はowned outputで返す。
後者を先にprogramへpublishすると、consumerのrelated-info追加後の診断と異なるため二重化する。
既存applicabilityはSilentが選択、Reportが失敗後の診断作成であることを独立確認した。
containing chainの実消費をtyped boolで返し、overflowは通常のerrorInfo連結を迂回する。
Claude r15の「shared-chain callerは1箇所」は検索漏れ。calls.rsの2直接callerも確認し、
既存message/related消費のままで処理される。fixtureごとの例外は追加しない。

focused-r4は142中138 exact、4 failure。module/destructuringの全対照はexactとなった。
残る4行はnoEmit loaderがconfig planへ保持するTS5107をtest adapterが渡さない欠落。
planのnonfatal optionsを既存command reportingへ渡す入口を追加する。同じ調査で実CLIも
external options存在時にsemanticを出しており、upstreamのoptions/global gateへ合わせる。
Claude r16の「既存all-report contractは0」は誤り。Codexがcli_contractの明示テストを発見し、
その期待をvendored TypeScriptの直接比較へ置き換えた。旧期待を互換仕様とは扱わない。
noEmit/emitting × deprecation抑制有無の4 CLI対照を追加し、元54の観測不変も検査する。


元checker最後の2行は`audit-checker-known-r2`でexact×2、他35 parse KNOWNは不変。
retire assertionだけのexit101を保存し、2行を台帳から退役した。これにより元68行は
parse36のみとなり、checker27/helper1/resolution4が修復済み。周辺36対照を含む
ordinary corpus468とCLI58の最終native再生、unit/full hostedは別に確認する。

## Round 17: malformed comment inputs

Claude Fable 5.1は1005/1434/1128とmissing/skip recoveryの実装を確認し、
改行後の `as` を含む12行はH2.9 parser factsが必要な境界と判断した。
元の入力・TypeScript完全観測を上書きせず、ParseDiagnosticsDeferredの
count=3 / recovery_events=4 / owner=H2.9 / partial writes空を2回検査する。
正常にemitできればretire assertionで失敗させる。別途 `as` を改行前へ置いた
12行を追加し、480選択は468 exactと12 typed boundariesとして区別する。
EF7残36との集合和を製品残件数にしない。round17-request/response/metadataと
comment-parse-boundaries-r17.jsonに根拠を保存した。

追加のcapture経路にもconfig planのoption診断を渡し、主比較と同じ完全commandを
保存する。relation overflowは上流と同じくincompatible stackを報告してから処理する。
