# OPS-COVER-3A：既存 E-only8 の CLI 比較を hosted に追加

2026-09-16。統合担当：Codex。OPS-COVER-3 の最初の限定スライス。
対象は `h2_7e_original_corpus` 内で未収載だったCLI wrapperの8件。

## 重複の境界

`h2_7e_original_corpus_preserves_complete_tuples_and_boundaries` は、元の状態では
共有の Program 比較 `assert_original_corpus` を呼んだ後に `assert_original_cli_corpus` を呼ぶ。
acceptance late の H2.7d/e は前者だけを直接呼ぶ。

したがって追加するのは **同じ8 case ID の別経路である CLI 比較**。
Programの8件やD283／directory23の重複replayをhostedに足さない。
Rust testを分離し、suite `declaration-map-cli` は新しいCLI専用test名を `--exact` で呼ぶ。
既存の共有Program関数とそのacceptance入口はそのまま残る。

## 母集団と比較面

固定join：`h2-7de-candidate-inputs.v1.json` の325入力と
`h2-7de-observations.v1.json` の323観測から `required_slices == ["H2.7e"]` の8件。
D/E共有3件、未観測transpile2件はこの集合に含まない。

| 原本（prefix `typescript-6.0.3/`） | CLI case数 |
| --- | ---: |
| compiler/declarationMaps.ts#default | 1 |
| compiler/declarationMapsMultifile.ts#default | 1 |
| compiler/declarationMapsWithoutDeclaration.ts#default | 1 |
| conformance/classes/classStaticBlock/classStaticBlock25.ts（es2022 / esnext） | 2 |
| conformance/esDecorators/classDeclaration/esDecorators-classDeclaration-sourceMap.ts（es2015 / es2022 / esnext） | 3 |
| 合計 | **8 ×2** |

各回で新しい一時ディレクトリを作り、buildした実CLIを起動する。実ファイルの出力集合とbytes、
stdout / stderr、exit codeを比較する。配置を変えても相対map参照をそのまま比較する。
TS5069を返す1件は実configの位置が必要なため固定TS6 CLIとstdout / stderr / exitを追加照合する。
Rustの出力集合とbytesはTSを起動する前に比較し、oracleによる上書きで誤差を隠さない。

CLI専用testには以前Program比較が先に担っていた観測producer・入力・vendored TSのhash検査も置く。
これらの軽量な不変条件を独立して満たしてから起動する。0件・重複ID・未実行testは成功にしない。
この追加はCLI wrapperの証拠であり、acceptanceの原本exact総数に8を足すものではない。

## 選択・予算

- CLI target sourceの変更は `declaration-map-cli` のみ。
- 共通 `h2_7e_original_corpus_shared.rs` は acceptance late とCLIを選ぶ。
- 原本joinはD/E等の共有入力なので、変更時は従来通り全groupを選ぶ。
- hostedは既存 `controls` jobのcompiler buildを共有する。job数・2 workers・60分上限を維持する。
  45分を分割検討の目安にする。CLI8件の追加実行時間とjob全体の時間を分けて記録する。
- ローカル：`python3 scripts/witness.py declaration-map-cli --all`。
  8件を一緒に比較し、曖昧な `--case` と0 test成功を拒否する。

## 検証

[固定したローカル記録](local.v1.json)と隣接baseline / CLI logにコマンド、入力、binary、source hashを保存。

- baseline：元の1 testでProgram8 ×2＋CLI8 ×2が成功。初回build 15分25秒、test 53.25秒。
- 分離後：CLI8 ×2、1 test pass / 1 filtered、test 30.22秒。build＋replay 33.629秒。
  共通Program比較の出力が含まれず、呼出しを重複していないことを確認した。
- 実compiler binaryのSHA-256は前後で同一。`assert_cli` の比較bodyもbyte単位で同一。
- planner25件、policyとpolicy境界test、fmt、inventory checkを確認。
- hostedの最終gate結果、追加CLIとcontrols jobの実測はPR本文に記録する。

低優先度・2 workersのローカル実測であり、hostedの速度保証ではない。
compiler側の依存emitter/checkerは `opt-level=3` でbuildされ、初回compileが大きい。
継続作業では同じworktreeとtarget directoryを使い、必要なsource変更だけをrebuildする。
別worktreeでcompileされたbinaryを直接流用することはtestのworktree guardが拒否する。

本番Rustの変更、fixture再生成、ローカル全acceptanceの追加はない。

## 次の OPS-COVER-3

この入口でcompilerの「直接入口なし」は22から21へ減るが、当該targetはCLI名によるfilter付き入口になる。
そのtarget全体の実行済みとは扱わない。Program部分は共有関数のacceptance呼出しとして別に記録する。

残りはUTF-16/literal、declaration/mapのfocused入力、parameter、既存filtered targetの残部。
各fixtureの重複をIDと呼出し面で照合し、Program／CLI／stateful APIの違いを区別する。
未収載21 targetを一括してcontrolsへ追加することはしない。
