# Emitter final r11 integration

追加監査で通常emitの残差・回帰を確認した。提出REPORTの「emitter owner未解決0」を
そのまま完了判定には採用しない。[追加監査と修復状況](residual-audit.md)を参照。

2026-09-18。producer canonical `~/dev/tsc-rs-emitter-final` の未commit r11を、
main `3b1f5fe87fd31e3b303bb44bd257342735452ed9` に基づく
`work/emitter-final-integration` へ受領した。提出worktreeのsourceと元workspaceの未commit作業は変更していない。
提出時の検証と統合候補の検証は以下で区別する。hosted完了前に全体完了を主張しない。

## 現在の統合候補と未完了事項（2026-09-19）

統合は未完了。候補 `d2b29439dd3eed32fcfd50f65317f7cadcd86628` を固定したr194–r196は終了した。
追加controlsで判明したコメント所有境界とconstructor選択を修復中であり、次候補のnative検証は未完了。
以下の過去の成功を現在の候補の全体成功とは数えない。

| 検証 | 実測結果と範囲 |
| --- | --- |
| 元census r78 | 14,329 ID = 14,219 loaded＋110明示的load failure。元16,994 parse inputsは変更しない |
| parser proof r185 | 16,994入力でAST・診断・raw recovery factsはr177と一致、先行5 profiles不変、最終context admissionは単調。syntax treeと依存hashが一致する範囲で再利用する |
| corpus selection r185 | 48元commands。従来44件を保持し、新しいescaped-keyword分を含む。class-body gapが新たに許可する元入力は0件で、この新動作は専用controlsで検証する |
| 元command r186 | 48件中45 complete commands＋2 noEmit commandsが各2回一致。escaped-default後のコメント欠落1件を確認。108 projectsは失敗後に未実行 |
| controls r184 | 176＋280＋232件中682件が各2回一致。Unicode detached comments 4件とES5 missing constructor 2件が不一致。syntax 211 tests成功 |
| controls r194 | 952件中892件が各2回一致。44コメント/map差分（BOM比較条件4件を含む）＋16 constructor差分。raw exit101、全観測を保存 |
| 元commands / project r195 | 46 emit＋2 noEmitの元48 commands、および108 projectsが各2回完全一致。noEmitはemit成功へ数えない |
| planner r200 | 84 tests成功。r199はfixtureの採取完了前に実行して2 FileNotFoundErrorとなったため、その失敗も保存 |
| project r144 | 108件が各2回完全一致した過去の証拠。最終候補はr195で再検証する |
| earlier regression r196 | r194不一致のため前提確認で停止。旧508＋145＋120＋JSDoc1、emitter 1,015 tests、checker lib、独立transpileの再検証は未実行 |

実際のOpus154–160と、Unicode-aware detached discovery、ES2015のconstructor本体判定、
escaped-defaultの末尾コメント所有境界を照合した。コメント修復はsource-mapのphaseまで
[明示的に検証](cross-review/r194-comment-phase-validation.md)する。レビュー意見だけではqualifyしない。
[r184–r190の全失敗とparser証拠](records/corpus-controls-r184-r190-complete/manifest.json)を保存している。

live parse KNOWN36と独立transpile KNOWN8は変更していない。退役は元commandsと周辺修復の
qualification後に行い、過去の拒否／出力を検証するguardを保存する。
最終sourceでのchain walk、unsplit local CI、hosted確認、mergeは未完了。
PR561の旧HEADの失敗は、現在候補の成否とは別に保存する。

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

既存65 witness suitesを保持し、syntax-emitter-recoveryと次の12 suitesを追加する（合計78）。全jobは既存の2 workers・60分制限、
45分で分割再検討の方針を維持する。

| suite | 固定した観測範囲 |
| --- | --- |
| emitter-final | oracle対照4、EF2/EF3 21所属、EF2–EF6 batch11 tests、EF8 22＋4、filesystem24、helpers/controls551（539 exact＋12 typed boundary）、CLI58、class24、empty-block72、session4、source-map70、global2、EF7 217＋guard3 tests、request-plan40 tests |
| emitter-universe-oracle | TypeScript 6.0.3で217＋1798を各2回再観測 |
| emitter-plan-base-0..3 | sorted IDのmodulo 4、450/450/449/449行を各2回 |
| emitter-global | 既存global output-only 769行を各2回 |
| emitter-comment-controls | export/destructuring1130＋元JSDoc1＋async arrow372の完全commandを各2回。emitter-globalと同じjobで実行 |
| emitter-class-0 / -1 | 既存8 class bandsを700/528行に分割、各2回 |
| emitter-system-controls | System・関数・コメント・構文回復の周辺5,549 fixture memberships、23 Rust tests。新1,192 controlsを含む |
| emitter-recovery-controls | bounded parse-recovery 1,224 fixture memberships、8 Rust tests |

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

現在変更中の18 concernsは `active-unqualified` に戻した。
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
