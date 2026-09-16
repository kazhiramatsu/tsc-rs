# OPS-COVER-3F / 3G: config/library commands and prologue comments

2026-09-16。Codex／統合担当。基点は main
`ccb6661c16f75cd6824e9fedb9281be68d01e542`。
3Fと3Gを同じbranchで実装し、個別のfocused検証後、最終sourceをまとめてhostedへ提出する。
これは既存テストのCI入口追加であり、H2.8bのruntime admissionではない。

## 範囲と観測

| Slice / suite | 既存入力とRustの選択 | 比較する範囲 |
| --- | --- | --- |
| OPS-COVER-3F / `config-library` | `contracts` 内の11 module・24 exact test、12 fixture・96入力 | library18 + config78。94 complete commandsと96 ordered Program factsをそれぞれ2回。残り2入力は `isolatedModules` / `verbatimModuleSyntax` のtyped refusal、無出力、membershipを確認し、command exactへ数えない |
| OPS-COVER-3G / `prologue-comments` | `h2_8a_prologue_only_detached_comments` の1 test、8入力 | JS text、write件数、exit code、diagnostic codesを2回。完全なcommand tupleを比較するテストとは区別する |

3FにはCFG completion receiptの88入力に加え、初期のconfig-commands8入力を含む。
case IDはfixture名で名前空間を分ける。同じ入力のcommandとmembershipは別の観測であり、
新規入力数を2倍にしない。3Gの8入力も既存の凍結対照である。

実装は `scripts/witness.py`、`.github/ci/replay.py`、plannerのtests、変更した3ファイルのexecution policy pin、入口台帳と本記録に限定する。
既存fixture/input、observer、Rust test、profile、ratchet、vendorは変更しない。
変更前に検出したforced-emptyの既存printer差は、別の[限定packet](empty-source.md)と
readiness checkでOPS-DEBT-EMPTY-SOURCEとして修復する。CI接続とこの修復を同じ候補で検証する。
3Fの20 config testsと4 library testsは名前の和集合で選ぶ。
登録先の `contracts.rs` 全体をこのsuiteの専用sourceにしない。

## CI選択と失敗の伝播

両suiteは既存controls jobのcompiler buildを共有する。3Fの専用11 module、12 fixture、
12 input、12 observerは `config-library`、3Gのtarget/fixture/observerは
`prologue-comments` を選択する。`contracts.rs`、共有w4a、VFS overlay、production変更は
全体選択を維持する。共有declaration comparatorとlibrary snapshotの変更は既存のlate
acceptance／witnessに `config-library` を追加する。

3Fは12 observer、3Gは1 observerを `--check` で実行してからRustへ進む。
観測の再生成・上書きはしない。fixtureの件数・空ID・重複、testの欠落・0件・ignored・
filter件数変更・非0 exitを拒否する。observer失敗はCargo前に伝播する。
選択の境界、24 exact nameの保持、共有sourceのcoverageはplanner testsで確認する。

```sh
python3 scripts/witness.py config-library --list
python3 scripts/witness.py config-library --all --dry-run
python3 scripts/witness.py prologue-comments --all --dry-run
WITNESS_SUITES='["config-library","prologue-comments"]' \
  taskpolicy -b nice -n 15 python3 .github/ci/replay.py witnesses
```

2 workers、45分で分割検討、60分hard limitを維持する。最終候補のobserver時間、
Cargo build/replay時間とcontrols全体を別に記録する。過去のcontrols時間は今回の成功証拠へ転用しない。

## 検証状態

[local.v1.json](local.v1.json)に実command・exit・時間・source/input/binary hashを保存した。
基点のRust/build source770 filesのうち、変更はprinterの改行1箇所のみ。
既存fixture・observer・Rust test・profileは基点と同一。

- 変更前はconfig/library22 pass /2 fail（1つのforced-empty差）、prologue1 pass。
  新runnerでも同じ不一致を非0 exitで検出し、13 observerは凍結値と一致した。
- 修復後は24＋1 testsが成功。94 complete commands、2 typed refusals、96 ordered
  Program factsが各2回。ログのpreparedイベント384件をcase ID／観測種別ごとに照合した。
  prologue8入力も各2回成功。compiler全447 testsのうち423を意図的にfilterする。
- 最終runnerは185.141秒（observer28.878秒、Cargo build/replay156.091秒）。
  buildを含まないRust test時間はprologue11.80秒、config/library44.15秒。
  変更前初回build込み372.027秒との比率を性能改善として扱わない。
- planner47 tests、変更したCI policyの3 tests、fmt、readiness、台帳v13再生成が成功。
  全体qualification checkerは基点と候補が同じ循環 `$ref` 非対応でexit1。
  [基点](policy-baseline.log.gz)と[候補](policy-candidate.log.gz)のログを保存し、全体greenとはしない。

隣接printer8 target・35 testsも成功（初回build込み226.738秒）。
失敗／再利用、hook、list、comment topology、UTF-16 writer、compact body240対照を含む。
hostedは未実行。
CFGのProgram単体projection/cache/host、filesystem CLI、他のcompiler contractはこの追加の対象外。
