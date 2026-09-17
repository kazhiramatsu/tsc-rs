# 最終emitter検証のCI予算と実行記録

2026-09-17。統合担当：Codex。開始main `c35e00ccb006e3b4e3e2643491e8fe3207595097`。
Claudeが通常compiler emitterの修復を進める間に、統合側の2項目をまとめて整備する。
emitter/compiler本体、observer、入力、期待値、比較対象やadmissionは変更しない。

## 1. 既存jobの負荷を分ける

[PR #555の実測](../../h2-8a-post-t1-residuals/integration/hosted/receipt.v1.json)では
controlsが**39m57s**、宣言mapのjobが**6m00s**だった。
[保存したbaseline](baseline.v1.json)は元logのhashとjob URLを持ち、
require-rewrite4 testsが510.03s、declaration-specifiers2 testsが189.28s、
合計**699.31s**だったことをRustの終了行から読み取る。
この時間はobserver/buildを除く。bufferされたlogの表示時刻をtest開始時刻とは扱わない。

既存の宣言map jobを`module-output`へ改称し、次の4 suitesでcompiler buildを共有する。

| suite | native tests | 配置 |
| --- | ---: | --- |
| declaration-map-apis | 3 | 既存map jobから引継ぎ |
| declaration-maps | 8 | 既存map jobから引継ぎ |
| require-rewrite | 4 | controlsから移動 |
| declaration-specifiers | 2 | controlsから移動 |

controlsのcompiler-direct分は21 suites/70 testsから**19 suites/64 tests**へ移る。
追加・削除した比較はなく、全10 replay jobs（acceptance3＋witness7）を維持する。
専用入力だけならそのownerだけを選択し、共有production/未知の入力は既存の全件選択を維持する。
移動した2 suiteを単独選択した場合も`.node-version`のNodeを使う。
Cargo2 workers、45分で分割検討、60分hard limitを保持し、完了後の実測で判断する。
過去の699.31sをそのまま新jobの時間保証や速度向上の実績にはしない。

## 2. Compiler-directの各実行を計測する

`scripts/witness.py`は各observerとCargo batchについて、JSONのstart/finishを即時に出す。
記録項目はphase、suiteの集合、argv、経過秒、passed/failed、失敗時のerror/exit code。
実行前のstartがあるため、長いCIでどの処理を待っているかを確認できる。

共有observerは従来どおり一度だけ実行し、その全ownerを一つの記録へ載せる。
unfiltered targetの共有Cargo呼出しも一つのbatchとして計測する。
Cargoの外側のwall timeはbuildとreplayの合計で、suite別のbuild時間に按分しない。
既存の全体observer_seconds / cargo_build_and_replay_secondsも残す。

observer失敗・Cargo非0 exit・正常終了でもtests/ignored/filteredが契約に合わない場合は、
failedの終了記録を出して元の例外を伝える。成功の全体summaryは出さない。
計測のために比較式、分母、プロセス実行順序、selector環境の除去を変更しない。

## 検証と統合境界

planner/runnerは77 tests。追加4 testsは全suiteの一意な配置、専用入力とNode選択、
共有observerの一度だけの実行、子プロセス開始前のstart記録、失敗伝播とzero-test拒否を確認する。
policyは実行sourceのhashだけを更新し、意味・判定境界は維持する。
入口台帳v24も73 standalone（50 unfiltered /16 filtered /7直接入口なし）、
16 lib/bin中1直接入口を維持する。

Rust productionと期待値は無変更のため、ローカルの重い全件replayは追加しない。
最終候補ではpolicy・planner・台帳・diffを確認し、変更した実行経路はhostedで全件検証する。
最終head、実行log、全jobの時間、module-output/controlsの件数、mergeの同一treeを記録する。
この作業はClaudeのEF1〜8の修復を代行したり、emitterの完了条件を追加したりするものではない。
