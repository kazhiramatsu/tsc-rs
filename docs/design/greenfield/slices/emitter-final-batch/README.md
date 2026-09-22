# Claude 一括依頼：通常compiler emitterの完了まで

統合候補・追加修正・最終検証は[統合記録](integration/README.md)を参照。以下は依頼時の固定scope。

2026-09-17。ユーザー指示「Claudeに渡す残りのタスクを一気に」に対応する依頼資料。
親ID `H2.8a-A-RES-EMITTER-FINAL`。**通常compiler emitterの残りを、調査・実装・
完了範囲の証明までまとめて担当してください。EF1〜EF8は開始時点の入口です。**
原因別の設計・commit・証拠を保持し、一つの依頼・worktreeで合成候補を作ります。
追加の必要修復が見つかったら同じ依頼の子スライスを増やして実装し、
既知の5件や20 IDだけ直して依頼完了にはしません。子ごとの再依頼を待たずに進めてください。
PR / merge、shared CI登録、profile admission、最終qualificationは統合担当が持ちます。

次の一区切りはemitter。LSP、一般公開API、TS7へのpin移行・Go移植・追従自動化、
Program再利用・build/watch、CLI表示全般は別フェーズです。
今回は採用中の**TypeScript 6.0.3**で通常のcompiler emitを閉じるための作業をまとめます。
その完了に不可欠なcompiler/host/checkerとの接続も、既存ownerと設計を確認して扱います。
単にemitter crateの外にあることを理由に、必要な通常emit修復を残さないでください。
TS7での廃止予定を、現在の6.0.3の一致や修復完了に置き換えません。

これは「残り8不具合」「この候補だけでemitter全体が完成」という判定ではありません。
EF1〜3はguarded known、EF4〜6は現在値の再測定から、EF7〜8は取りこぼしと完了条件の照合です。
新しい未解決producerも同じ台帳で追い、設計・実装・検証まで続けてください。
採用通常emitの未解決行が残る場合は継続中と明記し、別製品との境界を証明した項目だけを後続へ分けます。

## 1. 一括で依頼する8子スライス

| 子 | 内容 | 固定した入口 | 終了条件 |
| --- | --- | --- | --- |
| EF1 | bound decorator targetの末尾コメントとmap | POST-T1のknown **5 commands** | 5件exact ×2、既存96 positivesと79 packetを維持し101 exactへ |
| EF2 | ES5 loweringの既知差分 | H2.5h **12 IDs** | 現在の差とproducerを確定し、原因別に修復。JS以外の完全観測も一致 |
| EF3 | source-map・出力path・emit結果・診断・option拒否 | H2.6c **8 IDs**、うち1 IDがH2.6aにも所属 | owner別に修復または必要な前提を具体化。H2.6aとの両envelopeを保護 |
| EF4 | class fieldのalias / this / computed name / map | 旧classの未再測定 **28 commands**（C2 24＋C3 4） | 現在のexactと再現差を分離し、残るproducerを修復 |
| EF5 | class header token / export / escaped nameのrange・comment | 旧classの未再測定 **12 commands**（C4 8＋C5 4） | 現在のexactを保護し、残るtoken・range所有を修復 |
| EF6 | globalのmodule / declaration / JSDoc / JSX残差 | 旧global **14 IDs**（7件には後続修復記録） | 全14の現在値を照合。修復済みを再実装せず、残る原因だけ修復 |
| EF7 | 通常emitの未観測・deferred・不足producerの棚卸しと実装 | PLAN-BASEの全所属と、そのうち旧未観測 **217 IDs** | 最新証拠で観測単位のownerを確定し、必要な追加子スライスを同依頼内で設計・実装 |
| EF8 | 仕上げの出力軸監査・合成検証・A-CLOSE候補 | EF1〜7と現在のoutput-path一覧 | 採用emitの全経路をdispositionし、全件hosted計画と未解決一覧を提出 |

EF2とEF3は合計**20 unique IDs /21 profile所属**です。EF3の共有1 IDを二度数えません。
EF4＋EF5は旧class40件。既に同じfixtureでhosted exactとなった88件は退行対照です。
global14、known20、EF7の217等は重複し得るため、表の件数を足して残件数にしません。

[inventory.v1.json](inventory.v1.json)に全ID、状態、source hashを固定しました。
これはsource台帳で、新しいRust replayの結果ではありません。歴史的記録の基準は
PLAN-BASE `f9ef828a5`、現在のsource照合はPOST-T1候補`2883e3c79`です。
開始SHAで再測定し、古いdispositionを現在値としてコピーしないでください。

```sh
python3 docs/design/greenfield/slices/emitter-final-batch/inventory.py --check
```

## 2. 開始点と既に閉じた項目

[POST-T1統合記録](../h2-8a-post-t1-residuals/integration/README.md)の
**PR #555がMERGEDになってから**、それを含むmainで始めてください。
提出済みPOST-T1 worktreeや古い依頼書の保存元checkoutをそのまま使い回しません。

```sh
cd /Users/hiramatsu/dev/tsc-rs
git fetch origin main
EMITTER_FINAL_BASE="$(git rev-parse origin/main)"
git merge-base --is-ancestor ce39261ace254b4aaf9d2923220f4672818b1d8f "$EMITTER_FINAL_BASE" || exit 1
git worktree add -b draft/h2-8a-emitter-final ../tsc-rs-emitter-final "$EMITTER_FINAL_BASE"
```

既存の同名branch/directoryがあれば別名を使います。開始SHA、clean status、toolchain、
Node pin、入力・observer・upstream hashを保存し、本書がmainに未収載ならコピーしてください。

引き継ぐ成功：POST-T1の元7 command差分＋3 packet差分、pipeline767 exact、
T1 complete18/packet15 exact、新controls96 exact/5 known/packet79 exact。
R9/R12/receiver map/private-set suppression/visited decorator flagsの旧修復を再依頼しません。
C01〜C05、A-PC1、printer failure-carry、hook hints、T1も各統合記録を引き継ぎます。

読む順序は[design index](../../../README.md)、[emitter architecture](../../emitter-architecture.md)、
[schedule](../../post-h1-completion-slices.md)、本書、[共通handoff手順](../claude-high-difficulty-handoffs.md)、
[witness方針](../../../../witness-testing.md)。semanticsとbyte期待値はvendored6.0.3です。

```text
_tsc.js: 1c59e77a54b186ec43fa7f3e0d3c4bb15ca5eb5ba43e96b1d3a267139eddd3e3
typescript.js: 569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39
```

## 3. 子ごとの調査と修復

### EF1 — bound accessのcomment phase

`post-t1-residuals/decorator-comments/`の
`{es2015,es2022,esnext}/set-property-access`と
`{es2022,esnext}/set-parenthesized`が5 IDです。
`(_a = ns).dec /* c */.bind(_a)`で`/* c */`と対応map segmentが欠けます。
入口は`printer.rs`のPropertyAccess/ElementAccess、ownerは`E-COMMENT-SCOPE-H`。
[POST-T1 DESIGN §8](../h2-8a-post-t1-residuals/DESIGN.md)と
[REPORT §3.5](../h2-8a-post-t1-residuals/REPORT.md#35-known-native-として凍結した-5-行printer-所有)を読み、
左端へ転送されたcomment phase、access自身のend、synthesized親のcontainer claimを実traceで追います。
producer側のfresh target / NoCommentsを逆戻りさせて見かけを合わせません。

property/element、cached/uncached、parenthesized/unparenthesized、native/lowered、
same-line/block/line、removeComments、bundleのsource境界を必要な正負対照にします。
同一コメントを二度出さず、既存のsourceをまたぐ失敗時scopeと後続printの状態を保護してください。
最初と最後は正規`witness.py post-t1-residuals --all`とT1を実行できます。

### EF2 — H2.5hの12 known

原本は[H2.5h manifest](../../../../../ratchets/h2-5h-known-divergences.v1.json)。
入口はES2015 lowering、generator/async、destructuring/for-of、using、legacy decoratorの合成です。
全行が同じ旧owner名でも、同じ原因と決めません。temp、this/arguments、helper order、
comma/括弧、original/range/commentのどこで出力が分かれるかを現在のsourceで分割します。

対象にはarrow内casted object access、async/Promise、decorated block-scoped class、
async generator class methods、iterable destructuring、await using/for-ofが含まれます。
実行式を変える場合はreceiver/key/RHS/await/disposeの順序・回数・値・例外もruntime対照で確認し、
完全commandのbyte比較に代えません。upstreamの分岐を飛ばすcase名特例や出力後の置換は禁止です。

### EF3 — H2.6a/6cの8 unique IDs

原本は[6a](../../../../../ratchets/h2-6a-known-divergences.v1.json)と
[6c](../../../../../ratchets/h2-6c-known-divergences.v1.json)。以下を分けて調べます。

- destructuring array binding：6a/6c共有。manifestではwrite差0、診断・emit result差あり。
  mapが原因と決めず、診断schedule/exitまで追います。
- downlevel generator：JS/map/emit resultの所有と順序。
- JSON require、`.map`をJS扱いする入力、outDir、case-sensitive/insensitiveのpath・write集合。
- isolatedModules / case-sensitivityのtyped refusal：guardの採用前提と実producerを調べます。

凍結6c manifestの`sourceMapWithNonCaseSensitiveFileNames`には古い`outFile`表記が残っていますが、
PLAN-BASEで確認した現在のrefusalは`useCaseSensitiveFileNames`です。
履歴は保存し、実装開始点の実際の理由を別記してください。guardを消すだけで対応にせず、
compiler/host/checker側が必要ならそのownerとreadinessを明記します。
通常emitに必要な前提は設計を補って同依頼内で実装します。一般公開APIなど明示した別製品が
前提となる行だけ、その依存と通常emit境界の証拠を提出して後続へ分けます。

### EF4 — class field alias / this / nested computed nameの28件

固定fixtureは`crates/compiler/tests/fixtures/class-field-alias-map-positions.json`。
旧C2の24件とC3の4件は[ID台帳](inventory.v1.json)に列挙しています。
`class_fields/downlevel.rs`、generated binding / target binding、computed-name一時変数、
thisのphaseとsource mapの元nodeを調べます。C02・SUPER・POST-T1によって既に直っている
可能性があるため、最初に現在の28 complete commandsを再測定します。
JSだけでなくdeclaration/maps/diagnostics/write metadataを保持してください。

### EF5 — class headerとhoisted exportの12件

`class-header-token.json`の8件と`hoisted-declaration-export-ranges.json`の4件です。
前者はexport/default後のcommentとtoken、後者はescaped class/export nameとrangeが旧入口です。
現行token-comment/direct metadata対照と同一の比較面かを確かめ、direct成功を
complete-command成功へ読み替えません。必要な修復はproducer・token printer・map ownerごとに分けます。

EF4/5の既存入口は`crates/compiler/tests/integration/`の
`h2_8a_class_field_alias_map_positions.rs`、`h2_8a_class_header_token.rs`、
`h2_8a_hoisted_declaration_export_ranges.rs`です。test名filterだけではfixture全体が走ります。
40件を絞る場合は元のcomparatorを共有する薄いselectorを先に用意し、選択IDと件数を証明します。

### EF6 — global14の現在値と残るowner

[旧A6-37の14 failures](../../../../../ratchets/h2-8a-global-after-a6-37.v1.json)と
[PLAN-BASE](../plan-base/README.md)を、PR #555までの後続修復に突き合わせます。
後続記録が未確認だった7件の探索入口はreactImportDropped、anonymous export-assigned class
のES5/ES2015、parameter JSDoc2件、linkTagEmit1、bundlerImportTsExtensionsです。
これは新しい7失敗の主張ではありません。残り7件の修復記録も現在のcommandで保護します。

module import/exportのelision、declarationのsymbol/accessibility/JSDoc、JSX、
optionと診断・emitSkippedの組合せを、実際に残った原因で分割してください。
require rewrite、declaration specifier、G4a/b/G5c等の統合済み修復を再適用しません。
一つのIDが別profileで成功していても、optionsと比較面が違えば元の観測の完了にはしません。

### EF7 — emitterの未観測範囲と不足producer

PLAN-BASEの6,045 ID /9,004所属は不具合数ではありません。
旧217 IDだけでなく、すべてのsource/future/later-deferredと後続exactを照合し、
採用中の通常emitに必要なものを次の単位で新しい台帳へ整理してください。

`case ID + input hash + options/profile + JS/declaration/map/diagnostic/write観測 + version + owner`。

現在exact、同観測の再測定待ち、再現差、未対応producer、runner不足、明示的な別製品境界、
upstream例外を区別します。H2.7c旧10保留、H2.7d/eの11 later references、
H2.5gのdeferred6/510なども所属を照合し、件数を足さず、単に「later」のまま消しません。
emitterに必要なdeclaration/JSDoc/JSX/module/option経路が見つかれば、source traceと
focused before/afterを持つ原因別修復を同候補へ同乗させてください。
前提の大きい通常emit機能も、型・producer・入力・実装順を揃えた追加子に分けて実装します。
調査報告や次回のスライス提案だけで、この依頼の必要範囲を完了にしません。
TS7採用表は後続であり、この6.0.3の台帳をTS7互換性の完了と呼びません。

### EF8 — 出力経路の仕上げとA-CLOSE候補

EF1〜7を合成し、JS / d.ts / map / d.ts.map、module/target、outFile/outDir、
複数source/順序反転、BOM/LF/CRLF、removeComments、非BMP/escaped name、
生成名衝突、write順・callback metadata・partial failureの適用経路を照合します。
直積を無条件に増やさず、既存対照で覆う軸と不足軸を示して必要な入力だけを追加してください。

通常のfresh commandの反復と、同じProgram/TransformationResultの公開API再利用は別です。
disposed printのtyped known2、custom transformerでのみ作れるshared-node SUPER known4は、
現行sourceとAPI境界を照合して後続API ownerへ明示し、通常emitの失敗数へ混ぜません。
通常Programからその形へ到達できる反例を見つけた場合は、通常emitの残差として戻します。

仕上げとして旧global769・class1228の全件対象表、既存positive保護、新規修復、
未解決、別製品境界、upstream例外を分けた**A-CLOSE候補**を提出してください。
全件hostedの実行と最終認定は統合担当が行います。未計測・未解決を残したままSTAGEを進めません。

## 4. 作業順・設計・検証契約

最初に全8子のsource/owner対応を作り、EF1 → EF2/3 → EF4/5 → EF6 → EF7 → EF8を目安に進めます。
実際にはEF7の読み取り照合を先行でき、重なるprinter/class_fields/generated bindingは
同じworktreeで先の修復の上へ順に載せます。子ごとのPR/full CIには分けません。

各子はproduction編集前に、固定upstream body span/hash、callee/分岐、実trace、
Rust producer/consumer/lifetime、gap matrix、変更ファイル、型・identity・range・所有権、
正負witness、runtime対照が要るか、必要なarchitecture行をDESIGNへ記録してください。
`_tsc.js`の文字列検索だけで終わらず、再現入力で実行して分岐・metadataの変化まで確認します。
今回は6.0.3の調査です。Goの実traceも同じ深さで行うTS7工程は後続依頼に保持します。

before/afterで元入力・比較式・観測分母を固定し、期待値を修復後のRustに合わせません。
新規対照は別ID・別artifactを固定upstreamから2回採取します。
完全commandは出力bytes/path/順序、diagnostics、emit result/status/exit、callback metadata、
partial writesを比較し、direct API / 内部packet / runtime controlを別集計にします。

ローカルはfocusedを先に実行し、最後に必要な隣接集合を一度まとめます。
macOSは`taskpolicy -b nice -n 15`、Cargo2 workers、重い実行は一度に一つ。
実在しない`--case`を仮定せず、まず`--list` / `--dry-run`かselectorの選択表で件数を確認します。
EF2/3/6に既存focused入口がなければ、元comparatorを使う最小の入口を候補として用意し、
正規runnerへの接続案・依存・時間を提出します。丸ごとのacceptanceをローカルの編集ループにしません。

emitter unitと変更ownerの必要対照、POST-T1/T1、generated binding、printer、bundle/declaration
の適用集合を最終bytesで確認します。pipeline767、SUPER、retained530、acceptance、
global/class全件は最終合成候補のhosted担当へ渡し、未実行を成功と報告しません。
追加のhosted入口と45分の分割検討目安・60分制限を計画し、2 workersを維持します。

compiler fixtureのknown-nativeは修復を確認したIDだけ候補でretireし、元projectionとbeforeを保存します。
`ratchets/`のknownやaccepted profile、STAGE、shared CI policyは直接書き換えず、
ID・元hash・根拠・両pass結果を持つretire/admission提案を統合担当へ提出してください。
既存lint16指摘を新規退行と混同せず、逆に古い成功記録を新しい全workspace成功にも転記しません。

## 5. 提出物と担当境界

- `DESIGN.md`：EF1〜8のsource trace、gap、architecture判定、修復単位と依存。
- `REPORT.md`：開始SHAと最終HEAD、各ID/観測のbefore→after、既存positive、残る差、未実行。
- 新台帳：旧台帳・後続PR・現在観測の対応、重複、前提不足、APIへ残す境界、A-CLOSE対象表。
- 専用入力・observer・native比較・正負対照。zero tests / skipped / filteredを成功にしない入口。
- 実argv/env/exit/時間、binary/source/input/hash、logs、complete tuplesと差分。
- 原因別commits/patchesとhash、合成後のclean candidate、必要なhosted入口・group分割案。

Claudeは調査と修復候補を一括提出し、統合担当はsourceレビュー・共有入口登録・hosted・
限定再qualification・PR/merge・admission・残台帳更新を一括で行います。
一部に依存が残っても独立した子を止めず、必要な前提を順に解消してください。
中間の候補提出は可能ですが、最終提出は採用通常emitの未解決producer・未分類・未計測の
扱いが全て明示され、必要な実装が完了した時点です。最終hostedが未実行ならその条件を残し、
最終認定を先取りしません。統合担当が依頼全体の完了を判断します。
