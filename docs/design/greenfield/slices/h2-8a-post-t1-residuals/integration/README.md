# POST-T1の統合と限定した再qualification

2026-09-17。提出候補`8529a4326`（基点`eb6dc2c78`、9 commits）を、
main `3de6ab9bc7ecbee096699e927dd3e0f35f2c923e`へmerge commitで取り込んだ。
統合branchは`work/post-t1-residuals-integration`。提出worktreeのsourceは変更していない。

## 受領とレビュー

[受領記録](records/received.v1.json)で46 upstream spanのhash、9 commits、
提出と同一のproduction5 files、変更のない既存pipeline/T1の入力・期待値4 artifactsを確認した。
競合は`docs/witness-testing.md`への追記だけで、foundationとPOST-T1の両記録を保持した。
提出元にだけ残っていたobserver採取logをgzipで保存し、REPORTのリンクを修正した。

レビューした境界：

- R9：TypeScript transformで生成したdefault binding identityをSystemとCommonJS/AMD/UMDの
  hoist・代入・exportへ引き継ぐ。provisional spellingはparsed censusを避け、最終名は既存finalizerが決める。
- R12：Systemのclass expression/statementはtext rangeだけを持ち、元宣言のmap抑制flagを継承しない。
- RECEIVER-MAP：private compound/updateのreceiverはsynthesized cloneをtemp初期化へ渡し、
  updateのcomma括弧はupdate範囲を持つ。評価順序やJS式の意味は変えない。
- PRIVATE-SET：RHSに置かれていたRust固有のtrailing-comment抑制を除去する。
  既存H2.5g・retained・SUPERのhosted成功を採用条件とする。
- DECORATOR：visited式のNoComments、cached receiverのfresh target、native decoratorの
  comment phaseとclassInfoに限定したmember-name flagを確認した。コメントscopeの表現は維持する。

新101 complete commandsのうち、5行はbound decorator targetの末尾コメントとmapに
既知差分が残る。ownerは`E-COMMENT-SCOPE-H`のPropertyAccess/ElementAccess転送境界。
[提出REPORT §3.5](../REPORT.md#35-known-native-として凍結した-5-行printer-所有)の5 IDと
native projectionを固定し、互換成功に含めない。dispose後printの既存typed差分や
同一ProgramのAPI再emitも、この修復の完了対象へ広げない。

## CIへの追加

`post-t1-residuals`を既存`decorator-binding-pipeline` jobへ同乗させる。
controlsは前回の実測が29m08s、先々回が39m38sで、新suiteの提出実測は約7分。
pipeline側のcompiler buildを共有し、controlsの45分の分割検討閾値に余裕を残す。
全jobの60分・2 worker制限を維持し、実際のhosted時間で再確認する。

専用入力だけならpost-T1の2 testsだけを選び、pinned Nodeを使う。
pipelineとの同時選択は一つのjobへまとめ、共有productionでは全既存群を保持する。
selector/dump環境を除去し、fixture101件、test2件、importされたfilter8件を検証する。
plannerに単独/合成選択と環境除去の対照を追加し、policyのexecution hashを更新した。
入口台帳v23は73 standalone：unfiltered50 / filtered16 / 直接入口なし7、lib/bin16中1直接入口。

## 検証計画とarchitecture

ローカルは[run-local.py](run-local.py)でemitter unitと新POST-T1/T1を直列に検証する。
planner73 tests、policyの検証と2 focused testsは成功。全767 pipeline、SUPER、retained、
acceptanceは最終候補のhostedで検証し、提出ログと統合候補の実測を分けて記録する。

| architecture | 本候補での扱い | 再qualificationの根拠 |
| --- | --- | --- |
| E-NAMES-BASE | default宣言のtyped binding拡張は`modified-requalify` | R9元2件と新21対照、pipeline767、既存generated-binding/direct/5g/retained |
| E-METADATA-BASE | range/original/flagsの限定修復は`modified-requalify` | R12とreceiver、T1 complete18/packet15、新complete101/packet79、既存metadata unitと全hosted |
| E-COMMENT-SCOPE-H | native decoratorのcomment入口とprivate-set抑制除去は`modified-requalify` | 新private17/decorator27と既存printer/retained/SUPER/5g。残る5 knownはこの部分のqualify対象外として明記 |
| E-NAMES-CLASS-G / E-METADATA-G-CLASS / E-POSITIONS | 表現・既存責務は`premise-unchanged` | 原本とproducer/consumer照合、既存class/metadata/map観測の回帰検証 |

必要値はpipeline **767 exact /0 known**、T1 complete **18 exact /0 known**、
T1 packet **15 exact**、POST-T1 complete **96 exact /5 known**、POST-T1 packet **79 exact**。
packetは内部観測でありcomplete-command成功へ加算しない。上流例外も別集計する。
成功した最終headとhosted証拠で変更部分だけを再qualifyする。H2.8全体のadmission、
STAGE、既存profile/ratchetは今回変更しない。

hostedの最終head・log・時間・merge証拠は、検証済みsourceを固定した後続記録へ保存する。
記録追記だけで全件CIを繰り返さず、実装PR本文からその記録へリンクする。

## 統合候補の実測と限定した再qualification

[PR #555](https://github.com/kazhiramatsu/tsc-rs/pull/555)の最終候補は
`2883e3c79247b988106c6e4f1f2bdd074ae90e66`。以下は提出候補の結果の転記ではなく、このheadで実行した結果。
mergeの完了と同一treeの証拠は後述のmerge記録で確認する。

### ローカル

[ローカルreceipt](records/local/receipt.v1.json)と隣接gzip logsにargv、環境、
開始head・clean状態、exit、時間、hashを保存した。Cargoは2 workers、macOSはbackground priority。
提出worktreeのtargetをビルドcacheとして再利用し、統合worktreeのsourceを実行した。

| コマンド | 実測 | 結果 |
| --- | --- | --- |
| emitter-units | 249.211s | exit 0 |
| post-t1 | 610.790s | exit 0 |
| t1 | 64.226s | exit 0 |

emitter unit **508 passed**。POST-T1は**96 exact /5 known /0 failed**、
内部packet **79 exact /0 known**。T1はcomplete **18 exact /0 known**、
内部packet **15 exact /0 known**。両suiteのcomplete比較は各2回。
planner73 tests、policy checkとfocused policy2 tests、fmt、inventory v23、diff checkも成功。
全workspace Clippyの成功は主張しない。提出時の既存emitter16指摘・program側指摘は別の負債。

### Hosted

[acceptance run](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35185982007)と
[witness run](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35185981972)の
**14 checks（10 replay job、2 plan、2 gate）がすべて成功**。
[hosted receipt](hosted/receipt.v1.json)に各jobの全logとSHA-256、候補commit/tree、実時間を保存した。
全14 jobのcheckoutはGitHubが作ったmerge commit
`5ae9a7a7265c458f7e7d831333b212401fface41`で、検証候補と同一treeであることも照合した。

| replay job | 実測 | 結果 |
| --- | --- | --- |
| acceptance (early) | 11m19s | success |
| acceptance (late) | 18m04s | success |
| acceptance (wide) | 27m40s | success |
| witnesses (retained) | 9m21s | success |
| witnesses (printer) | 3m06s | success |
| witnesses (primary) | 6m27s | success |
| witnesses (declaration-maps) | 6m00s | success |
| witnesses (controls) | 39m57s | success |
| witnesses (decorator-binding-pipeline) | 20m57s | success |
| witnesses (foundations) | 1m43s | success |

合計runner時間は**144m34s**、最長は
**39m57s**。plan/gateとmain pushは除外する。
2 workers・60分制限を維持する。45分を分割検討の目安とする既存方針は変えない。
whole-job時間であり、suite単体やmachine性能の速度比較ではない。
新POST-T1自体はobserver **118.699s**、
Rust test **115.39s**（Cargo build・setupを除く）。
pipelineのcompiler buildを共有した配置で、controlsへ実行時間を追加せずに収まった。

元pipeline **767 exact /0 known /0 failed**、T1 **18 complete＋15 packet exact**、
POST-T1 **96 exact /5 known＋79 packet exact**を実logで確認した。
H2.5g、SUPER、retained、printer、bundle/declarationの既存回帰も対応jobで成功したため、
private-set suppression除去のhosted確認条件も満たす。
[提出対照の照合](records/submitted-positive-preservation.v1.json)では新101入力の
before exact40件をすべて維持し、56件を新たにexactとした。5 knownへの既存成功例の移動はない。

以上により、上表の`modified-requalify`だった3箇所をこの候補headで`active-qualified`へ戻す。
対象はE-NAMES-BASEのtyped default binding伝播、E-METADATA-BASEの限定producer修復、
E-COMMENT-SCOPE-Hのnative decorator入口・private-set抑制除去だけ。
残るbound decorator targetの末尾コメント5行は同ownerの別経路として未解決のまま保持する。
元handoffの**7 complete-command差分と3 packet差分**は解消した。
emitter全体の完了、H2.8全体のadmission、全API再emitの互換性へは範囲を拡張しない。

## Merge

PR #555は`2026-09-17T06:16:16Z`にmerge commit
`ce39261ace254b4aaf9d2923220f4672818b1d8f`でmainへ統合済み。
[merge証拠](hosted/merge.v1.json)で2 parentsと検証候補の同一Git treeを確認した。
Claudeの9 commitsを保持し、source修復とCI入口を一つのPRで統合した。
本記録の追記は検証済みproduction・fixture・runnerを変更しない。
