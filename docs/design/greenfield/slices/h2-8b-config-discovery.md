# H2.8b-CFG1b config外の探索開始点

2026-09-11。CFG1a最終`e195cdf59e9073233d69e30f85a586499eadc4d6`から
`work/h2-8b-config-discovery`で着手する。ユーザーの「次に進んでください」に対応する。

## 対象

TypeScript 6.0.3 `_tsc.js:18510–18596`の`getFileMatcherPatterns`、`matchFiles`、
`getBasePaths`、`getIncludeBasePath`に対し、`CompilerConfigHost`の探索開始点を照合する。
configのディレクトリを最初に訪問し、includeから導出した追加開始点をソートして採用する。
既存開始点に含まれる候補を除き、全開始点でrealpathのvisited集合とinclude別bucketを共有する。
親を後から追加しても最初のconfigディレクトリは残す。

24入力は兄弟・親・絶対path、ファイル指定、暗黙の再帰directory、wildcard/query文字、
相対dot/backslash、重複と親子の重なり、prefix境界、外部exclude、filesとの重複、
extends、configDir置換、include順、UTF-16ファイル順、欠落および既存の内部探索を含む。
比較面はCFG1aと同じraw/fileNames/wildcardDirectories/extended sources/20 typed options/
3診断列。各入力でfresh parseを2回行い、全比較面を照合する。

入力・上流凍結観測は`crates/program/tests/fixtures/h2-8b-config-discovery{,-inputs}.json`、
observerは`scripts/observe-h2-8b-config-discovery.mjs`、nativeは
`crates/program/tests/integration/h2_8b_config_discovery.rs`。
上流は仮想directory表から実際の`ts.matchFiles`を呼び、nativeは同じfilesを
`MemoryCompilerHost`と既存`CompilerConfigHost`で読む。実機filesystemのfallbackはしない。
source/compiler/observer/input SHAはartifactへ固定し、期待値を修正せずbaseline失敗を残す。

通常emitへの接続は別の8入力で、出力bytes、順序、BOM、metadata、diagnostics、
source maps、emit結果、status、exitとordered Program factsを各2回比較する。
共有comparatorと既存CFG1a/LR2期待値を維持する。

## 境界と受入

24/24 config比較×2と8/8通常command/facts×2、既存config/Program/library契約を受入とする。
production候補は`config_host.rs`と既存path helperの利用に限定し、emitter/printer/profileは触らない。
host callback障害、再利用cache、watch/typeAcquisition、option関係診断、全path spellingの
一般化は後続単位。CFG1/B全体の完了・profile activationは主張しない。

重い実行は1つずつ、専用target、taskpolicy -b nice -n 19、CARGO_BUILD_JOBS=1、
--test-threads=1。baselineと最終受領証に実HEAD、実exit、log/input/binary SHAを記録する。
hosted acceptance、walk、chain-walk、full cargo xtask ciは追加しない。

## 実測から確定した修正と追加観測

`925ef9c80`のbaselineは5/24一致×2、19件不一致×2、実exit101。
`getBasePaths`相当を追加した`b1614aba2`では23/24一致になった。
残る`explicit-file-and-wildcard`は`wildcardDirectories`だけの差で、baselineにも同じ差がある。
`getWildcardDirectories`（`_tsc.js:39719–39757`）は明示includeをfilesの有無にかかわらず読む。
そのため実測に基づき`config.rs::derive_wildcard_directories`も編集し、`635daa485`で修正した。
暗黙の`**/*`だけをfiles不在時に生成する（`_tsc.js:39052–39070`）。

追加の純粋な開始点計算16件を`h2-8b-config-discovery-paths{,-inputs}.json`へ固定した。
大小文字比較、UTF-16候補順、drive/UNC/URL、rooted spelling、root wildcardを含む。
これはhost lookupやconfig parseの全経路の資格判定ではない。

レビュー時に`normalizePath`と共有`getNormalizedAbsolutePath` helperの末尾separator差を発見した。
別6件を`h2-8b-config-discovery-spelling{,-inputs}.json`として新規凍結し、
`fe074e0c6`で3一致/3不一致×2、実exit101を記録してから`3208260fd`で修正した。
相対directoryとconfig directoryの末尾`/`を保持し、`getBaseFileName`が取り除く末尾separatorを
1つに限定した。元の24件・追加16件の期待値は変更していない。

最終production/計測headは`3208260fde21bd2b762f359cbfaa55552a3d99d4`。
[完了報告](h2-8b-config-discovery-report.md)と
[最終受領証](../../../../ratchets/h2-8b-config-discovery-final.v1.json)に検証と境界を記録した。
