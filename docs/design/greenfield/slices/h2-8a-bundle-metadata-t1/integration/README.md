# T1 候補の統合・再検証

2026-09-17。提出候補 `a17009c58b26cf3d98030a16fa32f0153f75179a` を受領した。
基点は PR #549 の main `84da0c0278c296fd295f15e48177ada87810f841`。
統合 branch は `work/bundle-metadata-t1-integration`、最終候補は `4d4ed3c7d42d49aac8eeea3eaf610615ecddb205`（[PR #550](https://github.com/kazhiramatsu/tsc-rs/pull/550)）。PR #550 の
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
declaration-maps/primary/decorator-binding-pipeline）と両gateを一括検証した。
controlsは新suite追加後21 compiler-direct selections / 70 tests。
前回controls38m49sに提出新suite実測約94sを加えた目安は約40m23sで、45分の分割検討閾値内。
実際のbuild/oracle/replay秒数とjob全時間は以下のhosted結果で確認した。60分・2 workerの上限は維持する。

`E-METADATA-BASE` のbundle packet部分は、以下の最終headの実測により `active-qualified`。
検証対象は元5件、新18 complete commands / 15 packet probes、6 unit tests、既存bundle lifetime controlsと
共有productionの全hosted acceptance/witness。成功した最終head・run・log hashを根拠に、この部分だけを
`active-qualified` へ戻した。歴史的qualification、他のmetadata field、H2.8全体のadmissionは更新しない。

実装commit `4d4ed3c7d` の作成時点では全件hostedは未実行だった。以下の後続記録を
検証済みheadを固定した別docs branchへ保存し、PR本文からリンクする。記録追記だけで同じ全件CIを再起動しない。runtime mergeは別途。


## 最終 hosted 結果と bounded 再 qualification

[実測記録](records/hosted.v1.json)はhead、run/job、時刻、ログとSHA-256を保持する。
[acceptance全ログ](records/hosted-ci.logs.zip)、[witness全ログ](records/hosted-witnesses.logs.zip)、
[controls本文](records/hosted-controls.log.gz)、[pipeline本文](records/hosted-pipeline.log.gz)を保存した。

検証headは `4d4ed3c7d42d49aac8eeea3eaf610615ecddb205`。全9 replay jobs・2 planner・2 gate、**13 checksすべて成功**。

| Job | Time | Result |
| --- | ---: | --- |
| [plan](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35171104556/job/105042753803) | 0m26s | success |
| [acceptance (wide)](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35171104556/job/105042844292) | 25m49s | success |
| [acceptance (early)](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35171104556/job/105042844301) | 12m00s | success |
| [acceptance (late)](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35171104556/job/105042844321) | 17m48s | success |
| [gates](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35171104556/job/105047795946) | 0m12s | success |
| [plan](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35171104560/job/105042753892) | 0m25s | success |
| [witnesses (primary)](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35171104560/job/105042839687) | 10m50s | success |
| [witnesses (retained)](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35171104560/job/105042839688) | 8m47s | success |
| [witnesses (decorator-binding-pipeline)](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35171104560/job/105042839693) | 16m32s | success |
| [witnesses (declaration-maps)](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35171104560/job/105042839695) | 6m33s | success |
| [witnesses (printer)](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35171104560/job/105042839696) | 3m16s | success |
| [witnesses (controls)](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35171104560/job/105042839754) | 39m38s | success |
| [witness-gates](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35171104560/job/105050573002) | 0m11s | success |

- 既存pipeline：**763 exact /4 known /767 complete commands**。T1の5 IDすべてをログで `EXACT x2` と照合した。上流例外1件は成功数に含めない。
- 新T1：complete **15 exact /3 known /18**、packet **12 exact /3 known /15**、いずれもfailed0。comment rangeの行は全件exact。
- controls：21 compiler-direct selections /70 tests、全job **39m38s**。oracle 444.621秒、Cargo build+replay 1357.896秒。
- controlsは前回38m49sから49秒増（runner差を含む総job比較）で、45分の分割検討閾値内。
- 全9 replay jobの合計は **141m13s**（planner/gate除外）。

`E-METADATA-BASE` の通常Bundle packetにおけるparsed comment range可搬性だけを、validation ref `4d4ed3c7d42d49aac8eeea3eaf610615ecddb205` で再qualifiedとする。source identity / mount順序 / endpoint状態 / atomic restoreは6 unit testsと15 packet probesで確認し、元5件と全767 complete commands、既存bundle lifetime対照、全hosted回帰が成功した。R9/R12 4件、新規known native3件/packet3件、dispose後print2件、同一ProgramのRust API再emitは各ownerへ残す。H2.8全体のadmissionやmainへのmergeは宣言しない。

統合patch（base `059519e99` → `4d4ed3c7d`）のSHA-256は `28a0f0b52b7146c778dd793db61f50fa46489d94e9a728f3324db923531a36c0`。この後続記録は `docs/bundle-metadata-t1-hosted-550` に保存する。PRの実装headは検証済みの `4d4ed3c7d` に固定し、記録だけのpushで全CIを繰り返さない。
