# T1 候補の統合・再検証

2026-09-17。提出候補 `a17009c58b26cf3d98030a16fa32f0153f75179a` を受領した。
基点は PR #549 の main `84da0c0278c296fd295f15e48177ada87810f841`。
統合 branch は `work/bundle-metadata-t1-integration`。PR #550 の
`059519e997c5f81fd313c8166b9e28fd8fefee79` の上へ取り込み、登録件数の競合を解消した。
PR #550 の hosted 結果も `128c5e273` から取り込んだ。提出 worktree は変更していない。

## 受領とレビュー

[受領記録](records/received.v1.json)で、patch が commit の `git format-patch --binary` と
byte 一致すること、上流26 spanと提出14 file hashの一致を確認した。
patch SHA-256 は `beea6313f1a1eca84b6ea876b714ef17c9f827a825aac4b5b0fe2d146002973d`。
production の2 fileは提出候補と同一。追加修復は不要だった。

- snapshot は `comment_range` 自身の source を記録し、node の source と同一視しない。
- restore は全 source の snapshot / lease / node interval と既存 metadata を検証してから一括書込みする。
- EndOnly / StartOnly / Original / Synthesized の状態と byte endpoint を維持し、source の mount 順序だけを再対応付けする。
- synthetic comments / source-map range は引き続き typed refusal。通常 Bundle emit 以外の寿命は変えない。
- 既存767件の入力・上流期待値は不変。known-native から提出どおりT1の5行だけを除いた。

統合変更は CI の固定 Node 条件、generator の専用 suite 所有、selector / dump 環境の除去テスト、
PR #550 を含む planner 件数、policy の execution hash、入口台帳v21。
新 test の不要な `mut` と reference への no-op `.clone()` を除き、提出レポートに残った旧件数を直した。
上流 observer / 新旧 fixture の bytes は変更していない。

## 検証面と件数

| 集合 | 提出 before | 提出 after / 最終検証の必要値 |
| --- | --- | --- |
| 元のT1 5 complete commands | 0 exact / 5 known / 0 failed、各2回 | 5 exact / 0 known / 0 failed、各2回 |
| 既存 pipeline 全767 complete commands | PR #549: 758 exact / 9 known | 763 exact / 4 known、上流例外1件は別集計 |
| 新18 complete commands | 新規観測 | 15 exact / 3 known / 0 failed、各2回 |
| 新15 bundle packet probes | 新規観測 | 12 exact / 3 known / 0 failed、comment rangeの行は全件exact |
| packet 単体テスト | 既存4件 | 6 passed（既存4 + source/range・atomicity2件） |

known は互換成功に含めない。新 native 3件は System class末尾map2件とprivate compound receiver map1件。
新 packet 3件はprivate-set RHS flag1件とdecorator式flag2件。
元のR9/R12 4件、dispose後printのtyped差分2件、同一ProgramのRust API再emitは各既存ownerへ残る。
[提出結果](../REPORT.md)のbefore/afterは提出sourceの証拠であり、統合後の全件実測とは区別する。

## 統合 source のローカル検証

[実行記録](records/local-replay.v1.json)は argv、環境、exit、秒数、圧縮ログとhashを保持する。
packet単体テストの後、新suiteとbundle Program/declaration、module identity、元JavaScript recorderを
同じ低優先度・2 workerの直列chainで再実行した。[binary hash](records/binaries.sha256.json)も保存する。

| 検証 | 結果 |
| --- | --- |
| packet unit | 6 passed / 0 failed（fresh build込み180.917秒） |
| 新T1 + 隣接4 suites | 13 passed / 0 failed、各targetのfilter件数も一致。oracle118.755秒 / Cargo build+replay511.415秒、合計630.337秒 |
| 新T1の内訳 | complete 15 exact /3 known /18、packet12 exact /3 known /15、いずれもfailed0、各2回 |
| planner / runner | 63 tests passed |
| qualification policy / policy schema | passed |
| 台帳v21再生成 | 72 standalone: unfiltered34 / filtered15 / 直接入口なし23、lib/bin16中1入口 |

最初の統合buildは新testのreferenceへのno-op `.clone()` を1件警告した。これを除去し、変更した2 testsだけを再実行した記録は [warning-cleanup](records/warning-cleanup.v1.json)。上流observerとfixtureは不変なので再採取していない。

## Hosted と architecture 再 qualification

最終 source の全9 replay jobs（acceptance early/wide/late、witness printer/controls/retained/
declaration-maps/primary/decorator-binding-pipeline）と両gateを一括検証する。
controlsは新suite追加後21 compiler-direct selections / 70 tests。
前回controls38m49sに提出新suite実測約94sを加えた目安は約40m23sで、45分の分割検討閾値内。
実際のbuild/oracle/replay秒数とjob全時間はhosted結果で確認する。60分・2 workerの上限は維持する。

`E-METADATA-BASE` のbundle packet部分は `modified-requalify`。
検証対象は元5件、新18 complete commands / 15 packet probes、6 unit tests、既存bundle lifetime controlsと
共有productionの全hosted acceptance/witness。成功した最終head・run・log hashを根拠に、この部分だけを
`active-qualified` へ戻す。歴史的qualification、他のmetadata field、H2.8全体のadmissionは更新しない。

このcommit時点で全件hostedは未実行。後続のhosted記録は検証済みheadを固定した別docs branchへ保存し、
PR本文からリンクする。記録追記だけで同じ全件CIを再起動しない。runtime mergeは別途。
