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

## 最終候補の記録

[PR #557](https://github.com/kazhiramatsu/tsc-rs/pull/557)の最終sourceは
`bc4fa2c295dc20e45814f44ce8f460a6d959be0f`。
[local receipt](local/receipt.v1.json)は、このclean headでの77 planner/runner tests、
policy検証、2 policy tests、台帳v24、diff、production/oracle/ratchet無変更の確認を記録する。
各コマンドのexit・経過秒・圧縮logのhashを保持する。

[変更前後の配置比較](partition.v1.json)は開始mainと最終sourceの実装を読み、
acceptance3 groupsとwitness65 suitesが同一で、重複・追加・削除がないことを確認した。
compiler-directの24 suiteはcatalog・suite別observer argv・suite別Cargo argvも同一。
配置またはjob名が変わるのはmodule-outputの4 suiteだけで、witness7 jobsを維持する。

[収集・検証script](collect-hosted.py)はPR head、各jobのcheckout、tree、ログのhashを固定する。
実際のcompiler replay3 jobsのstart/finishをargv・owner・順序で突き合わせ、
失敗のない完了と契約どおりのtest件数を要求する。
planner unit testが意図的に出す失敗記録は、実際のreplayの失敗として数えない。
前回と今回のcontrolsとmap/module-outputを合算し、同じ23 suites・81 testsの保持も検査する。

## Hostedの実測

| job | PR #555 | PR #557 |
| --- | ---: | ---: |
| controls | 39m57s | **25m21s** |
| declaration-maps → module-output | 6m00s | **20m42s** |
| 上記2 jobsの合計runner時間 | 45m57s | 46m03s |

controlsのjob時間はこの実行で14m36s短くなり、両jobとも45分の分割検討目安を下回った。
合計runner時間はほぼ同じであり、削減したテストや比較面はない。
これは各1回のhosted実測で、host/cacheの違いを含む。処理自体の速度向上や、
次のemitter修復を追加した後の所要時間を保証する値ではない。

compiler-directの3 jobsで、48 observerと13 Cargo batchesの計61実行について
start/finishの組、argv・owner・順序、passedと経過秒を確認した。
controlsは19 suites/64 tests、module-outputは4 suites/17 tests、POST-T1は1 suite/2 tests。
module-outputのobserver計209.460s、Cargo build/replay計1015.236s。
controlsはobserver計320.673s、Cargo build/replay計635.197s。
この内訳は他のwitness処理、checkout、setupを含むjob全体の時間と区別する。

[hosted receipt](hosted/receipt.v1.json)に全14 checksの成功と全logを保存した。
10 replay jobsの最長はacceptance wideの**28m25s**、合計runner時間は**141m48s**。
前回の最長39m57s、合計144m34sと区別し、plans・gates・main pushはこの合計に含めない。
全jobがsynthetic merge `f733713d44ff82110f7935431113323235b69c94`を実行し、
tree `9e47b211450b0ebab2411f9d173ae192345c4772`は最終sourceと一致した。

H2.5gは8511 exactを2回、SUPER primaryは670 exact、pipelineは767 exact/0 known。
T1は18 complete/15 packetがexact、POST-T1は96 exact/5 known/0 failedと79 exact packetを保持する。
5 knownのprinter所有者、過去の限定qualificationとadmissionはこのCI整備で変更しない。
