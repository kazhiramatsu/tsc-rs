# H2.8b-LR1 library replacement baseline 実装報告

2026-09-11。LR1の比較test追加・baseline採取・owner分類を完了した。
**通常emitは12/12完全一致×2。順序を含むProgram比較は5/12一致×2、7件不一致、通常test exit101。**
7件をqualifiedにはしていない。修正は[LR2引継ぎ](h2-8b-library-replacement-order-handoff.md)に残す。

開始設計は[h2-8b-library-replacement-baseline.md](h2-8b-library-replacement-baseline.md)。
開始時のregistryと観測は不変の記録として保持する。その`native_executions:0`は準備時点の値であり、
今回の実行結果は[baseline受領証](../../../../ratchets/h2-8b-library-replacement-baseline.v1.json)に記録した。

## 実装と基準

- worktree：`/Users/hiramatsu/dev/tsc-rs-h2-8b-lr1`。
- branch：`work/h2-8b-library-replacement`。
- 開始HEAD：`f3e7d612dd4824d5b347dca74ded90893fe1c115`（設計branch）。
- 実測HEAD：`f01ab82293dd4b7c33fa696bc8f2a6ad192282aa`、測定時clean。
- production基準：`5134bb0187f9f3f6ac2a0218f8d8e98859cf4fea`。
- 新規`crates/compiler/tests/integration/h2_8b_library_replacement.rs`に独立した2 testsを追加し、
  `contracts.rs`へmodule登録した。共有comparator・production・入力・上流期待値の変更は0。

完全command比較は既存`assert_cases_with_inspection(..., true, record_attempt)`から
`assert_command_observation`を実行する。もう一方は同じhelperのinspectionでsource順・library順・
root名順を照合する。各prepared Program到達と実際のmembershipをJSON行としてログへ出した。
後者の不一致が前者の実行を妨げない構成である。

## 結果

| 比較対象 | 一致 | 不一致・実行範囲 |
| --- | --- | --- |
| 通常command完全タプル | 12/12 ×2 | 0。JS・declaration・両map、write metadata、診断、status、exitを含む |
| source/libraryの集合とroot名順 | 12/12 | 全caseに到達。失敗した7件は最初のmembership比較まで |
| source/libraryの順序を含む全facts | 5/12 ×2 | 7件は最初の比較で失敗。helperがそのcaseの2回目を省略 |
| libtest | 1 test pass / 1 test fail | 実exit101、418 filtered out |

新規12件を24件として数えない。nativeのprepared Programは41回
（完全command側24回＋membership側17回）。失敗した7件を「×2失敗」とは主張しない。
TypeScriptはこのworktreeでも12件×2を再観測し、凍結artifactと完全一致、exit0だった。

config-directory-anchorはTypeScriptのcommand exit2、診断6059→5011も含めてRustと一致した。
そのcaseのmembership順序は別に失敗しているため、全体一致には含めない。

不一致はreplacementを実際に選ぶ以下の7件に限定された。

- enabled-package
- duplicate-lib-reference
- config-directory-anchor
- package-subpath
- module-suffixes-isolated
- package-exports-isolated
- root-promoted-to-library

共通する順序差（source末尾にはmainが続く）：

| 実装 | library順 |
| --- | --- |
| TypeScript | es5 → decorators → decorators.legacy → replacement |
| Rust | es5 → replacement → decorators → decorators.legacy |

## 原因とowner

上流`getDefaultLibFilePriority`（`_tsc.js:123124–123138`）は解決後の`SourceFile.fileName`を参照する。
defaultLibraryPathの外にあるreplacementは`libs.length + 2`となり、catalog内の既知libより後に並ぶ。
`compareDefaultLibFiles`を使う`toSorted(processingDefaultLibFiles, ...)`がその順序を適用する。

Rustの`load_selected_libraries`と`process_lib_references`は、解決後も論理basenameを
`LibraryCatalog::file_name_priority`へ渡している。その順位が`StagedSource.library_priority`に保存され、
`StagedGraph::finish`のstable sortに使われるため、replacementも元のdom/dom.iterable順位のままになる。
`library_replacement`のboolは分類・promotionにも使われており、順位そのものの代替にはならない。
ownerはH2.8b-LR2 / program loader-library。今回の12件では出力の差は観測していないが、
他の入力で順序が出力に影響しないとは主張しない。

## 検査と運用

開始時のdesign checker、observer syntax、`cargo fmt --all -- --check`、diff whitespace検査は通過。
134 pinsのうち、実装後に変わったのは許可された`contracts.rs`の登録だけで、残る133 pinsは一致。
上流・nativeの各実行前後も全対象SHAは不変だった。通常testの失敗をignore/expected-failureへ変えていない。

実行は専用target、`taskpolicy -b nice -n 15`、`CARGO_BUILD_JOBS=2`、`--test-threads=1`。
開始時に他の重い処理がないことを確認し、その後別のビルドが始まったため、こちらのprocess groupを
11:17:10–11:21:08 UTCに一時停止して再開した。この待ち時間を含むビルド時間を性能値として扱わない。

ログとfacts captureは`target/library-replacement-runs/20260911T110757Z/`。
受領証に実HEAD/tree・環境・command・実exit・case別到達・binary SHA・log/capture SHAを保存した。
バイナリSHA256：`8e086a4462066a3b4cc64dfaddb42d315f64a67a99d2748056a2de2003a47e80`。
完全commandは既存comparatorで照合しており、別のnative command tuple JSONは作成していない。

LR1のための新規hosted acceptanceは実行していない。別途、マージ済みmain `9806a4cb9`の
[run 34591181671](https://github.com/kazhiramatsu/tsc-rs/actions/runs/34591181671)は
10:50:51–11:20:28 UTCに成功した。これは今回の新規testを含むbranchのCI結果ではない。
