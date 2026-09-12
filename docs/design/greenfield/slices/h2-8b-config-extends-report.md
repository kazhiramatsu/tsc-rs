# H2.8b-CFG1a config extends 実装報告

2026-09-11。configの変換・継承で再現した4件の不一致を、3原因に分けて修正した。
観測対象のconfig planは28/28一致×2。追加の通常command 8件と前作のlibrary 18件も、
完全command・順序を含むProgram factsがそれぞれ各2回一致した。

[設計と修正根拠](h2-8b-config-extends.md)、
[修正前baseline](../../../../ratchets/h2-8b-config-extends-baseline.v1.json)、
[最終受領証](../../../../ratchets/h2-8b-config-extends-final.v1.json)を保存した。
今回の完了範囲はCFG1aであり、CFG1/B全体の完了やprofile activationは含まない。

## 変更

production変更は`crates/program/src/config.rs`と`config_host.rs`の2ファイル。

| 原因 | 修正と結果 |
| --- | --- |
| ファイルとdirectoryを混在した名前順で再帰していた | `CompilerConfigHost::walk_directory`で現在のdirectoryのfileを集めてから子directoryを訪れる。`matchFiles.visitDirectory`に一致し、1件のfileNames順序差を修正 |
| `extends`配列内のnullを診断対象から除外していた | 非string要素としてTS5024を出す。診断の位置と順序を含む1件を修正 |
| 最後のbaseの`compileOnSave: false`をchildのrawへ書き戻していた | 最後の継承値を保持しつつ、childへの追加はtruthyの場合に限定。2件を修正し、own false/nullと複数baseの後勝ちを維持 |

ディレクトリ内の名前順は既存`CompilerHost::read_directory`のUTF-16順契約を使う。
includeごとのbucketは維持するため、明示的なfiles順やinclude優先順位まで並べ替えない。
この点は通常commandの別々の入力でも確認した。

基準は`2b2423041796938091568cea1a756ec1d186b46e`（LR2最終）。
branchは`work/h2-8b-config`、worktreeは`/Users/hiramatsu/dev/tsc-rs-h2-8b-lr1`。

| コミット | 内容 |
| --- | --- |
| `05c2bf451` | 28 config入力・TypeScript観測・native比較・設計 |
| `9fb990dec` | 比較adapterのJSON Number表現差を補正 |
| `aac337508` | production不変のbaselineと3原因の設計記録 |
| `bdc294981` | productionの3原因修正 |
| `baa4579ec` | 通常commandの追加8件・上流観測・native比較 |

## 検証結果

| 対象 | 結果 |
| --- | --- |
| config plan baseline | 24/28一致×2、4件不一致×2、実exit101 |
| 修正後config plan | 28/28一致×2、56 fresh parses |
| program contractsの`config_` filter | 144 passed / 0 failed / 1 ignored、実exit0。新規1 testと既存143 testsが通過 |
| program単体tests | 26 passed / 0 failed、実exit0 |
| library loader contracts | 20 passed / 0 failed、実exit0 |
| 追加config command | 8/8の完全commandとordered Program factsが各2回一致 |
| LR2のlibrary command回帰 | 18/18の完全commandとordered Program factsが各2回一致 |
| compilerの`h2_8b_` filter | 6 passed / 0 failed、実exit0 |
| 上流artifact・整形・空白 | 2 observerの`--check`、`cargo fmt --all -- --check`、`git diff --check`がexit0 |

config planの比較面はraw JSONの値、fileNames、wildcardDirectories、extended sourceの順序とtext、
20種のtyped optionの状態・値、root parse / parsed errors / config diagnosticsの順序である。
診断はcode/category/file/start/length/message/relatedを含む。
この28件ではProgramを生成しておらず、通常emitケースへ合算しない。

初回の比較はserdeが`42`と`42.0`を異なるNumber表現として保持することで1件余計に失敗した。
既存config oracle testと同じJavaScript Number値の照合へ新規adapterだけを補正し、
production・入力・上流期待値を変更せずに再測定した24/4が正規baselineである。
初回23/5のログとbinary SHAも保持した。共有のcommand comparatorは変更していない。

通常commandの追加8件は修正後に独立したgroupとして上流観測した回帰controlである。
それらの修正前emit結果は計測しておらず、8件を追加のrepair数にはしていない。
recursive include、継承include、include bucketの逆順、明示files順、`${configDir}`、
複数baseの出力先、compileOnSaveのfalse継承を含む。すべて上流診断0、16 writes、command exit0。
JS・宣言・両map、write metadata、diagnostics、status、exitは既存の完全command comparatorで比較した。

最終compiler実行は26入力、104 prepared Programs（26件×2回×2 tests）。
config 8件とlibrary 18件をそれぞれ一度だけケース数へ計上する。
libraryのconfig-directory-anchorが返すcommand exit2と診断6059/5011も期待どおり維持している。

ignoredの1件は既存`missing_library_config_related_information_matches_vendored_typescript`に付いた
ローカルNode audit用annotationによる。この変更でignoreを追加・変更していない。
既存config契約の手書き期待値・vendor artifact・LR2観測はすべて維持した。

## 実行と証拠

config契約は`bdc294981`、program単体・library・compiler契約は`baa4579ec`で測定した。
両HEADのproduction 2ファイルは同じSHAであり、各実行中も入力SHAは不変だった。
最終実測HEADは`baa4579ecb99f976eb461381fe3687134e734ddf`。

```sh
export CARGO_TARGET_DIR=/Users/hiramatsu/dev/tsc-rs-h2-8b-lr1/target/h2-8b-lr1
export CARGO_BUILD_JOBS=1
taskpolicy -b nice -n 19 cargo test -p tsc-rs-program --test contracts config_ -- --nocapture --test-threads=1
taskpolicy -b nice -n 19 cargo test -p tsc-rs-program --lib -- --nocapture --test-threads=1
taskpolicy -b nice -n 19 cargo test -p tsc-rs-program --test contracts library_program_loader_contract -- --nocapture --test-threads=1
taskpolicy -b nice -n 19 cargo test -p tsc-rs-compiler --test contracts h2_8b_ -- --nocapture --test-threads=1
```

上流再観測は`node scripts/observe-h2-8b-config-extends.mjs --check`と
`node scripts/observe-h2-8b-config-commands.mjs config-commands --check`。
全source pin・入力・artifact・実行前後のSHA・実exit・log/capture/binary SHAは最終受領証を参照。
ログとactual capturesは`target/config-extends-runs/20260911T135209Z/`。
binary SHAは各実行直後に採取し、再ビルド後のbinaryを過去の実行へ帰属させていない。
完全commandの別形式native tuple JSONは作成していない。

同じ専用target、1 build job、1 test thread、background I/O、nice19で実行した。
別worktreeのacceptanceと重なる方針を開始前に記録し、32 GiB RAM中51%空きを確認した。
他のprocessは変更していない。所要時間を単独実行の性能値にはしない。
このbranchで新規hosted acceptance・walk・chain-walk・full `cargo xtask ci`は実行していない。

## CFG1の残り

この単位は既存config parserから再現した差の修正である。
config directory外へ広がるdiscovery roots、Programのoption relationship diagnosticsとその出所、
watch/typeAcquisitionの全変換、cached config parsing、host callbackの順序・回数・faultは後続で観測する。
CFG1のoutline全体をruntime-readyへ昇格させた扱いにはしない。
