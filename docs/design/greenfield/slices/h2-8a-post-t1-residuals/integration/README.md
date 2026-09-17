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
