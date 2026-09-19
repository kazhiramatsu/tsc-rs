# Emitter final r11 integration

追加監査で通常emitの残差・回帰を確認した。提出REPORTの「emitter owner未解決0」を
そのまま完了判定には採用しない。[追加監査と修復状況](residual-audit.md)を参照。

2026-09-18。producer canonical `~/dev/tsc-rs-emitter-final` の未commit r11を、
main `3b1f5fe87fd31e3b303bb44bd257342735452ed9` に基づく
`work/emitter-final-integration` へ受領した。提出worktreeのsourceと元workspaceの未commit作業は変更していない。
提出時の検証と統合候補の検証は以下で区別する。hosted完了前に全体完了を主張しない。

## r157 統合候補の検証中（2026-09-19）

統合は未完了。`add70fc29e3c553f36c9b78e23733877379e6a28` は、確認済みのproject/noEmit観測経路と
ES5変数修復を組み合わせた候補であり、r157/r158の実行中はsourceとHEADを固定する。
下表の過去の成功を、この候補の全体成功へ読み替えない。

| 検証 | 確認済みの範囲 |
| --- | --- |
| 元のcensus r78 | 14,329 ID = 14,219 loaded＋110明示的load failure。16,994 parse inputsを保存。元snapshotは変更しない |
| parser replay r131/r151 | 元入力のAST・診断・順序付き回復記録は不変。許可判定の差から44 commandを選択し、従来43件をすべて含む。完全command比較はr158待ち |
| project r144 | 108件のsource/order/options、診断、全callback bytes、map、status/exitを各2回完全一致。最終候補でも再確認する |
| noEmit r153 | 元censusのemit loaderで拒否された2件を既存の通常checking commandで各2回完全一致。4観測を保存。元loader成功やemitter呼び出しとは数えない |
| variable r150 | emitter全1,015 tests・printer147 cases成功。268 complete commands中263 exact×2、追加ES5 controlsの5件が失敗 |
| comma/neighbours r152 | missing-comma 145 complete commandsが各2回一致、関連7 Rust tests成功 |
| transpile r152 | 独立291件は289 exact×2・2 known、unexpected差分0。6件のKNOWN退役要求だけでraw exit101。live KNOWNは全8件を保持中 |
| ES5 producer候補 r154/r156 | 全404 TypeScript complete commandsが各2回一致し、元268/中間380 recordsは構造同一。native比較はr157実行中 |

r150の5件は、CommonJS変数名変更後のresolver参照2件、Systemの消えた型に続くコメント1件、
for-of変換後のsemicolonをまたぐコメント2件だった。実際のOpus136/137と、semantic originalが
コメントmetadataを複製しないこと、置換前の型末尾はinitializer cursorだけが参照すること、
for-ofの片側コメント範囲を区別して検討した。候補修復は共有comment/map collectorを変更しない。

証拠は[parser replay](records/parser-replays-r131-complete/manifest.json)、
[project108](records/project-supplement-r144-complete/archive-manifest.json)、
[noEmit2](records/noemit-r153-complete/manifest.json)、
[選択44件のTS観測](records/selected-corpus-oracle-r155/manifest.json)に保存する。
r150/r152とES5候補の記録は候補worktreeで固定し、検証後に統合する。
KNOWN36と独立transpile KNOWNの退役、全体chain walk、unsplit local CI、hosted確認、mergeは未完了。

## r120–r124 の固定候補に対する検証履歴（2026-09-19）

統合は未完了。検証対象の Rust・fixture・HEAD を固定し、修復候補の準備は別 worktree で行う。
次の成功はそれぞれ記載した候補の証拠であり、現在の最終候補の成功へ読み替えない。

| 検証 | 確認済みの結果 |
| --- | --- |
| r120 recovery 境界 | 1,930 complete commands が各2回一致。3,860回分の完全観測archiveもr106とbyte同一 |
| r106 program / checker | program 558成功・5 ignored、checker lib 1,739成功 |
| r120 emitter | 23 binaries、1,015 tests成功・ignored0 |
| r112 post-child metadata | 既存96＋追加24の120 controlsが各2回一致 |
| r120 追加境界 | 110 commandsが各2回一致。r116で残った3差分も解消 |
| r120 元のSystem/recovery | 4,088 commandsが各2回一致、11 Rust tests成功。全8,176観測を保存 |
| r122 parse KNOWN | 残る36行すべて各2回一致。2 testsは退役要求assertionのみでexit101。全corpusの影響確認・退役は未完了 |
| r123 config/library | 現在の固定compiler binaryで24 tests成功・ignored0・filtered459。r121 planner全84 testsも成功 |
| r124 EF7 / PLAN-BASE全件 | 217 / 1,798行すべて各2回一致・新規差分0。両testsは既存KNOWNの退役要求のみでexit101。比較器／shard guard3 testsは成功 |
| r110b ledger | 固定済み旧xtask実行ファイルで4,113 entries、stale0・undispositioned0。最終toolによる再確認は未実施 |

r116の3差分は、System変数名の型コメントと、クラス／引数プロパティの宣言コメントだった。
[全tupleと結果](cross-review/r116-native-results.md)を保存した。
Opus116–119と照合した限定修復、および追加12＋24 controlsを統合し、
`b451489e4a18abbff42651d8eb814537f4c5f800` に対するr120検証は完了した。
110 complete commands、emitter全1,015 tests、境界1,930、元の4,088 commandsがすべて成功した。
[r120の全結果と観測archive](cross-review/r120-native-results.md)を保存した。
失敗時は後続を進めず、current bytesでの成功を要求する。

[r122の36行の一致と退役要求](cross-review/r122-known36-results.md)は元fixture・比較器で確認した。
live KNOWNはまだ変更していない。過去の拒否観測をbyte同一で保存し、parser/corpusの証明が
完了した後に比較ガードをarchiveへ接続して退役させる。単独36行の一致で共有変更をqualifyしない。
[r124の全件比較](cross-review/r124-full-universe-results.md)でも元の217＋1,798行の集合を
変えずに全件一致した。raw exit101と全実行IDを保存し、KNOWN退役前の成功扱いはしない。

censusは7,908 recorded plansと、過去のqualification/candidate入力にのみある6,421 IDの
計14,329 IDを対象にする。これは入力IDの棚卸しであり、実行成功件数ではない。
固定したCensus HEADの入力hashと全IDは `records/census-r78-claimed-id-inventory.json.gz` に保存した。
census完了時には各IDがloaded rowsまたは明示的なload failureへ残っていることも確認する。
この時点で進行中だったcensusとparser replayは、その後r131/r151で完了した。
選択された元commandのnative比較と最終統合は、上段の現状を参照する。
censusは固定sourceと実行ファイルで採取し、停止を含む経過時間や機能比較を併走させた時間は
性能qualificationに使用しない。
Fableはround54でlimitに達したため、以降は実際のClaude Opusを1 CLIずつ使用している。

共通処理の変更が複数出力へ及ぶ設計は、[統合後の設計調査](cross-review/post-integration-dependency-boundaries.md)
へ記録した。checkerの診断用symbol表示もprinterを呼ぶ実経路を確認している。
このtrainでは互換差分の修復と検証を完了させ、境界の再設計は別の設計判断とする。

## 受領とレビュー

[受領台帳](records/received.v1.json)はproducerの37 tracked filesと557 untracked files、
各byte hash、基点とpatch hashを保持する。`candidate-tracked.diff` のSHA-256は
`241f13aa19a15ca76ad85db8392a19d741d2f5bda3389e790bf3baaf7e4c2d53`。
mainとのproduction競合はなかった。[原因とcommitの対応](records/producer-commits.v1.json)により
16 commitsへ分割した。共通hunkを持つbinding/class/async等は、提出の最終bytesを保つ単位にまとめた。
元のpatch scriptsとCOMMIT-PLANも保存した。

305本のignored raw logsはlossless gzipとして隣接保存し、
[archive台帳](records/producer-log-archive.v1.json)に展開後のhashを記録した。
提出metaの `.log` は同名 `.log.gz` を展開して照合できる。履歴の失敗logも保持する。

レビュー対象は、checkerのsource identity・enum/async/JSDoc facts、factoryのoriginalとgenerated binding、
名前確定のscope/print順、async superとPromise constructor、class/decoratorのreceiverとassigned name、
System/moduleのhelper/import/export、printerのcommentとsource spelling、option admissionとharness floor。
実装は既存typed producer/consumerへ接続し、test IDや期待outputをproductionの分岐に用いない。
全共有経路の回帰確認は既存acceptance/witnessと追加global/classで行う。

## 統合時の追加修正

### KNOWNを新しい不具合の免除にしない

提出のKNOWNはIDと原因を保持していたが、比較では「そのIDが何らかの差分を持つ」ことしか要求しなかった。
`emitter-final-known-native.json` は受領時のr11の68行のnative outcomeを固定した。追加監査で解決した32行を退役archiveへ移し、現在のKNOWN36行にのみ残す。
受領時の27 checker行はwrite/diagnostics/source order/emit result/exit等の全観測を、41拒否行は正確な拒否理由を比較した。checker全27行は完全一致を確認して退役した。live KNOWNに残る36行は受領時のparse-recovery拒否観測を保持しているが、現在の候補では全36行がexactとなり退役ガードが発火した。全corpusへの影響確認が終わるまで台帳の退役を保留している。
command構築時の拒否はpartial callbackのpath/hashも既存messageに含む。
2回の独立観測が一致し、ID集合・owner/cause・native outcomeがすべて一致したときだけKNOWNとして認める。
exactになったKNOWNと、新しく差分が出た行は引き続き失敗する。
[由来](records/known-native-provenance.v1.json)と `local/known-native-68.log.gz` を参照。

受領時68行はH2.9 parse recovery 36、module resolution request plan 4、H2.5h helper collision 1、checker 27。追加監査後はparse recovery36行のみ。helper1、resolution4、checker27は各2回の完全一致を確認して退役した。
受領時のparse refusal自体は互換成功に数えない。r122の36行の一致は現在の限定観測として記録し、共有parser変更の全体qualificationとは区別する。受領時の「emitter owner 0」は残差全体の解決済みを意味しない。

### Case-insensitive oracleと修復済み台帳

H2.6cのcompiler-runner hostに `@useCaseSensitiveFileNames` を反映し、lookup keyのみcanonical化した。
出力pathやsource spellingを正規化して比較する変更ではない。project descriptor経路にはこのdirectiveがなく、
当該2件はcompiler経路だけなのでproject/config hostは変更しない。
2件の再採取とcase-sensitive対照2件を各2回検証した。643 case recordsのうち641件は不変。
[旧新の完全観測](records/oracle-host-correction.v1.json)では変更はmap write/emit-result mapとそのfingerprintに限られる。
診断、JS callback、exitは不変。採取cacheのidentityもこのhost変更を含む。

RustでEF2/EF3の21 profile memberships（20 unique IDs）がすべてexact×2となったため、
H2.5h 12、H2.6a 1、H2.6c 8の既知差分台帳を空にした。
[退役前の全レコード](records/retired-known.v2.json)を保存し、固定した21行のreplayは残す。
最初のrunがstale-KNOWNを検出したexit 101も保存する。
後続のD/E・globalのcurrent oracle artifactsは公式writerで再生成し、全case/input/observationの不変を
[機械検証](refresh-provenance.py)する。歴史的before/after測定packetのhashを書き換えない。

## Hosted入口と予算

既存65 witness suitesを保持し、次の10 suitesを追加する。全jobは既存の2 workers・60分制限、
45分で分割再検討の方針を維持する。

| suite | 固定した観測範囲 |
| --- | --- |
| emitter-final | oracle対照4、EF2/EF3 21所属、EF2–EF6 batch11 tests、EF8 22＋4、filesystem24、helpers/controls551（539 exact＋12 typed boundary）、CLI58、class24、empty-block72、session4、source-map70、global2、EF7 217＋guard3 tests、request-plan40 tests |
| emitter-universe-oracle | TypeScript 6.0.3で217＋1798を各2回再観測 |
| emitter-plan-base-0..3 | sorted IDのmodulo 4、450/450/449/449行を各2回 |
| emitter-global | 既存global output-only 769行を各2回 |
| emitter-comment-controls | export/destructuring1130＋元JSDoc1＋async arrow372の完全commandを各2回。emitter-globalと同じjobで実行 |
| emitter-class-0 / -1 | 既存8 class bandsを700/528行に分割、各2回 |

重複する観測所属なので件数を足して互換ケース総数にしない。PLAN-BASEの約70分のローカル直列測定は
hosted一jobへ持ち込まない。専用fixtureは担当suiteを選び、shared sourceは全既存群を含める。
editing selectorsを除去し、zero/ignored testsと欠けたshard summaryを拒否する。
入口台帳v35はcomposite runnerを実Cargo commandsへ展開する。

## Validationとarchitecture

[run-local.py](run-local.py)はargv、開始HEAD/diff、環境、exit、時間、log hashを保存する。
macOS background priority・Cargo 2 workersで逐次実行し、canonicalのtargetをcacheとして利用する。
68 KNOWNの再比較、planner 83 tests、policy checkとfocused policy 2 testsは成功。
ローカル途中のhash drift失敗は、oracle修正後に旧pinを検出したもので記録から除外しない。
最終候補の成功とhosted receiptsは完了後に追記する。

現在変更中の14 concernsは `active-unqualified` に戻した。
[以前のqualification](records/architecture-before.v1.json)を保存し、最終immutable headの実測後に
変更した範囲だけ再qualifyする。E-PLAN-SCRIPTのoption admission、resolver/checker facts、
metadata/class provenance、async capture、name allocation、helper import、printer/commentが対象。
H2.9/一般checker全体、disposed-transform/API再emit、build/watch、TS7移行の完了には拡張しない。
STAGE・TypeScript 6.0.3 pin・製品全体のadmission policyは変更しない。

## 追加監査候補

最初の統合CIは23 checks中13成功・10失敗。失敗は隠さず全jobを
[保存](hosted/progress-ed71c45343b2.json)し、原因ごとの修復と追加比較を
[残差監査](residual-audit.md)に記録した。helper alias、resolution fallback、匿名class名、
async super identity、arrow factory更新順、namespace map、EOF comment、過剰なoption/comment拒否、
CLI named valuesを修復対象とした。既存範囲を縮めず、current artifact consumersのhashも同期する。

影響が大きい・未知の変更については、実際にClaude Codeへ独立調査を依頼し、Codexの
原因分析・観測と[突き合わせ](cross-review/decisions.md)た。意見が食い違った場合も
実測を優先し、review回答だけで成功判定しない。source-kind JSON / parse recovery /
stableTypeOrderingは共有基盤の未解決境界として残す。数値targetの1234と内部JSON値100は
区別し、後者を別名で互換成功に数えない。


## 次段のconsumer更新（r26）

候補3c1dfa51cでconformance49,024はT0/T1/T2/T3すべて一致し、H2.5hも
888 exact / 44 deferred / known0となった。H2.1aの旧comment拒否2行は元の
fingerprintと2回のTS観測を使う完全比較へ接続し、元qualificationは変更しない。
H2.6cは空KNOWNの正規状態を「manifest不在」とする既存readerにwriterを合わせる。
元21所属の再比較は保持する。これらのnative再検証は継続中。

bundle declaration/mapのcurrent参照は公式writerで再採取し、全inputと全観測が
不変であることを[bundle-provenance-refresh-r26](records/bundle-provenance-refresh-r26.json)
へ固定した。差は依存artifactのhashとobserver identityのみ。
旧退役台帳v1の空before payloadは、元SHA-256を照合した[v2](records/retired-known.v2.json)
で訂正した。旧版も訂正理由とともに保存する。

## r34 時点の実測と CI 再実行

D772候補の全23 checksとlogsを[保存](hosted/progress-d772eb554098.json)した。
19成功、2つの実行job失敗とそれに伴う2 gate失敗。conformance49,024全層、
wide、全PLAN-BASE shards、global/class、emitter-finalは成功した。
新しいempty-block72とsession4は完全commandを各2回一致。過去source-map70も
既存全比較を通過した（全70行を各2回測定したという意味ではない）。
config-libraryの24テストもすべて成功したが、shared contractsに7テストが増えたため
filtered countの旧423 pinが実測430を拒否していた。選択・ignored・filteredの厳密検証を
維持して固定値を修正した。H2.6cは追加した1行の元tuple hashがJSONのkey順序を
誤っていたため停止した。135行のpinを元artifactの順序で全照合し、当該1行だけ修正した。
元inputとTypeScript観測は変更しない。次のCIで再検証し、全成功まではqualificationを行わない。

構文回復は別候補で段階的に検証中。通常候補の36 KNOWNは維持しており、
新しいparser predicateや追加426 controlsのTypeScript観測だけを根拠に退役させない。
Claude Fableとの実際の比較を継続している。現時点でrate limitによるOpus切替はない。
