# H2.8b-CFG1b config外の探索開始点 実装報告

2026-09-11 UTC。config directory外へ広がるincludeの取りこぼしを修正した。
準備24件は修正前5一致/19不一致×2から、修正後24/24一致×2になった。
通常emitの追加8件も全command tupleとordered Program factsが各2回一致した。

[設計](h2-8b-config-discovery.md)、
[baseline受領証](../../../../ratchets/h2-8b-config-discovery-baseline.v1.json)、
[最終受領証](../../../../ratchets/h2-8b-config-discovery-final.v1.json)を保存した。
この完了範囲はCFG1bであり、CFG1/B全体の完了やprofile activationは含まない。

## 開始点と原因

- 開始HEAD：`e195cdf59e9073233d69e30f85a586499eadc4d6`（CFG1a最終）。
- branch：`work/h2-8b-config-discovery`。
- worktree：`/Users/hiramatsu/dev/tsc-rs-h2-8b-lr1`を継続利用。
- production最終・最終計測HEAD：`3208260fde21bd2b762f359cbfaa55552a3d99d4`。
- production変更は`crates/program/src/config_host.rs`と`config.rs`の2ファイル。

| 原因 | 修正と上流の根拠 | コミット |
| --- | --- | --- |
| config directoryのみから再帰していた | `getBasePaths/getIncludeBasePath`に合わせ、includeから外部開始点を追加。UTF-16候補順、component単位の包含、全開始点でvisited/bucketを共有 | `b1614aba2` |
| filesがあると明示includeのwatcher rootsも消した | `getWildcardDirectories`に合わせ明示includeを優先。filesは暗黙includeの生成だけを抑止 | `635daa485` |
| 開始点の末尾separatorを共有helperが除去した | `normalizePath`の末尾保持と`getBaseFileName`の1 separator除去を反映 | `3208260fd` |

`include: ["../shared/**/*.ts"]`は、以前はsharedのrootを読み込まず無入力診断になった。
修正後は外部fileを読み込み、ファイル順、include bucket順、重複除去、診断、watcher rootsが上流と一致する。
親directoryを追加する場合も最初のconfig directoryは保持する。

## 入力とbaseline

24件は兄弟・親・絶対path、直接file、暗黙directory、wildcard/query、相対dot/backslash、
重複と親子、prefix境界、exclude、files併記、extends、configDir、include順、欠落、内部探索。
UTF-16順がUTF-8順と異なる2つのfile名も含む。
raw、fileNames、wildcardDirectories、extended source列とtext、20 typed options、
root parse / parsed errors / compiler-visible config diagnosticsを全比較する。

- `925ef9c80`：48 fresh parsesをすべて実行し、5一致/19不一致×2、実exit101。
- `b1614aba2`：23一致/1不一致×2。最後の差はfiles併記時のwildcardDirectoriesだけ。
- `fe074e0c6`：追加spelling 6件の純粋計算は3一致/3不一致×2、実exit101。

最初のcargo実行はpackage名の指定ミスでtest開始前に終了したため、計測件数に含めていない。
実際のbaselineは正しい`-p tsc-rs-program`で再実行したもの。両方のログを保存した。

## 最終検証

すべて最終計測HEADで通常実行exit0。fresh比較は各caseを2回構築した。

| 比較またはsuite | 結果 |
| --- | --- |
| CFG1b config plan | 24/24 ×2、48 fresh parses |
| CFG1a既存config plan | 28/28 ×2、期待値不変 |
| 開始点の純粋計算 | 16 + 6 = 22/22 ×2、44 calls |
| CFG1b追加通常command | 8/8 complete tuple ×2、Program facts ×2 |
| CFG1a/LR2既存通常command | 26/26 complete tuple ×2、Program facts ×2 |
| compiler `h2_8b_` | 8 tests通過、合計136 fresh PreparedPrograms |
| program `config_` contracts | 145通過、失敗0、既存ignore 1 |
| program lib | 28通過、失敗0 |
| library loader contracts | 20通過、失敗0 |
| fmt、diff check、今回4 observerとCFG1a 2 observerの再観測 | すべてexit0 |

通常commandは出力bytes・順序・BOM・metadata、診断、source maps、emit result、status、exitを、
変更していない共有comparatorで照合した。独立のnative tuple JSON captureは追加していない。
Program factsとconfig/pure-path actual列は別captureへ保存した。
新規command 8件は最初の探索/watch修正後に上流観測したcontrolであり、修正前の不一致8件とは主張しない。

既存fixtureと既存integration testの期待値は変更していない。共有comparatorも不変。
既存ignoreは`missing_library_config_related_information_matches_vendored_typescript`の
pinned Node audit注記によるもので、新しいignoreは追加していない。

## 記録と残り

run dir：`target/config-discovery-runs/20260911T143948Z/`。
受領証は実HEAD/tree、実exit、開始時と終了時の入力SHA、実行直後のbinary SHA、log/capture SHA、
上流source範囲、入力・observer・compiler SHA、toolchainを含む。
専用target、taskpolicy -b nice -n 19、CARGO_BUILD_JOBS=1、1 test thread、重い実行は1つずつ。

開始点の22件は純粋計算であり、case-insensitive・Windows・UNC・URLのhost lookup表記全体を
観測したものではない。host callbackの順序・回数・fault、cached config、watch/typeAcquisition、
Programのoption relationship diagnosticsとその出所は後続単位に残る。
次の候補はCFG1cとしてoption relationship diagnosticsの優先順位とconfig由来の位置情報を観測すること。

新規hosted acceptance、walk、chain-walk、full cargo xtask ci、emitter全体suiteは実行していない。
