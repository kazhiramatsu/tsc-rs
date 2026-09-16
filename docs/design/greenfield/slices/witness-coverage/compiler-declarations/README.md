# OPS-COVER-3D：declaration の専用比較を hosted に接続

2026-09-16。統合担当 Codex。base は main `6c41a0388` の full SHA を
[local.v1.json](local.v1.json) に記録した。専用 worktree / branch は
`tsc-rs-compiler-declaration-witness-ci` / `work/compiler-declaration-witness-ci`。

## 追加する入口と比較範囲

| suite | Rust target | 入力 | 実行 / filter する test数 |
| --- | --- | ---: | ---: |
| `declaration-specifiers` | `h2_8a_declaration_specifiers` | focused24＋composition6＝30 | 2 / 9 |
| `declaration-comments` | `h2_8a_declaration_comment_ranges` | range17＋detached12＋parameter12＝41 | 3 / 12 |
| `jsdoc-return` | `h2_8a_jsdoc_return` | R1〜R8 の58 | 1 / 1 |

129専用入力を既存 controls job に追加し、各 command を fresh Program で2回比較する。
callback / materialized bytes、BOM、pathとwrite順序・metadata、reported / emit diagnostics、
related information、emit result、source maps、status、exitを既存の比較式で検証する。
specifierはProgramのsource/library/root順序も比較する。JSDocはcallback UTF-16と
UTF-8 sinkを区別し、noEmitケースは既存のゼロemitter活動対照も維持する。
Rust source、既存fixture、比較式、既存observerは変更しない。

3 targetに取り込まれた共有17 testsと原本wrapper5 testsをfilterする。
原本wrapper5件にはG5cの重複もあり、今回の129専用入力へ合算しない。
これらの未選択入口は引き続きOPS-COVER-3のfilter残部。共有helperがacceptanceにあることだけで
wrapper全体の実行を主張しない。新しい129件も既存corpusのexact総数へ加算しない。

## Observer と実行契約

specifierの2 observerとJSDoc observerは既存の `--check` を使用する。
コメントの41件には新しい `observe-declaration-comment-commands.mjs --check` を追加した。
元の[観測notebook](../../h2-8a-declaration-comment-range-observations.md)と同じ
`observe-jsdoc-block-scope-container.mjs` のordinary complete-command関数を、同じ抽出境界で使う。
TypeScript、共有observer、VFS overlay、fixture3個のSHA-256を検査し、固定入力から毎回生成した
完全なcommandを既存期待値と2回照合する。期待値をobserverに渡さず、ファイルも書き換えない。
内部の `prefix_trace` / `parameter_trace` は今回の再実行範囲に含めない。
JSDocの既存observerは従来どおりreturn traceも照合する。

複数の専用test名はlibtestの `--exact` に和集合として渡し、targetごとに1回起動する。
Cargoの結果で2/3/1 passed、0 ignored、9/12/1 filteredを厳密に要求する。
observer失敗はCargoより前に停止し、Rustの非0 exit・0件・missing / ignored / filter driftは失敗となる。
内部case filterとcapture先の環境変数を消し、同じtest件数のまま入力だけが減ることを防ぐ。

```sh
python3 scripts/witness.py declaration-specifiers --list
python3 scripts/witness.py declaration-comments --all --dry-run
python3 scripts/witness.py jsdoc-return --all
# 3 suiteを同じhosted経路で実行するローカル入口
WITNESS_SUITES='["declaration-specifiers","declaration-comments","jsdoc-return"]' \
  taskpolicy -b nice -n 15 python3 .github/ci/replay.py witnesses
```

専用target / fixture / input / observerの変更は該当suiteのみを選択する。
`h2_7c_declaration_blocking.rs` と `witness_libraries.rs` の変更は従来のacceptance / witnessに
`declaration-comments` を加える。共有w4a、original comparator、VFS overlay、共有observer、
production等は全体選択を維持する。新しいworkflow/jobは不要で既存controlsのbuildを共有する。
2 workers、45分で分割検討、60分hard limitを維持する。直前のC05統合でcontrolsは
13分51秒だった（[実測](../../l2-3-resolution-cache/integration/README.md)）。追加分のローカル実測と
hosted全体の時間は分けて記録し、今回のhosted所要時間を過去値から推定して成功扱いしない。

## 検証と残り

ローカルの結果・実command・exit・時間・source/input/binary hashは[local.v1.json](local.v1.json)。
変更前のbaselineと新runnerを同じRust/fixture/binaryで比較し、入力を変更せず検証した。

- baselineは129入力が各2回一致し、6 tests成功。specifier2 / comment3 / JSDoc1、filter9 / 12 / 1。
  cold buildを含む実行は587.560＋40.654＋55.600＝683.814秒。
  テスト本体は281.53＋36.82＋52.53＝370.88秒で、初回buildは約305秒。
- 新runnerも129入力が各2回一致、6 tests成功。内部filterを不一致値、capture先を古いパスに
  設定しても全入力を実行した。4 observerも固定期待値と一致した。
  合計537.798秒、observer 133.094秒、Cargo build/replay 404.471秒。
  baselineとafterの3 binaryはSHA-256が同一。Rust/build source768 filesも不変で、
  production / fixture / vendorはbaseと同一。最終runner bytesで検証した。
- planner42 tests、関連policy境界3 tests、policy check、台帳v11再生成が成功。
  exact-nameの和集合、0件 / missing / ignored / filter drift / 非0 exit、observer先行失敗、
  内部filterとcapture変数の除去、台帳に全test名が残ることを検証した。
- 新しいcomment observerは `--write` と読取時のfixture改変注入を各exit1で拒否した。
  実際のfixtureは変更していない。negative controlは互換成功件数へ加算しない。

台帳v11は67 standalone中25 unfiltered / 10 filtered / 32直接入口なし。
残る直接入口なしはcompiler12・その他20、lib/binは15。既存filtered targetの残部も別管理する。

hostedは未実行。PR head、run URL、controls全体とbuild/oracle/replayの実測、両gateの結果は
統合時に追記する。入口の実装・ローカル比較の成功とhosted qualificationを区別する。
