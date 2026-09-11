# H2.8b-LR2 library replacement 順序修正報告

2026-09-11。LR1で残った7件の順序不一致を修正した。
元の12件と追加6件は、通常commandの完全タプルと順序を含むProgram factsがすべて各2回一致した。
最終compiler testは4 passed / 0 failed、実exit0。

[修正設計](h2-8b-library-order-fix.md)と
[最終受領証](../../../../ratchets/h2-8b-library-order-final.v1.json)を参照。
[LR1 baseline](h2-8b-library-replacement-report.md)は修正前の記録として保存する。

## 原因と変更

production変更は`crates/program/src/library.rs`と`crates/program/src/loader.rs`の2ファイル。
`getDefaultLibFilePriority`（TypeScript 6.0.3 `_tsc.js:123124–123138`）に合わせ、
論理catalog名ではなく、解決後のsourceの実パスでライブラリ順位を決める。
既定ライブラリディレクトリの外にある置換先は、catalog内の既知ライブラリより後に並ぶ。
包含判定はdisplay pathを使い、rootだけcase-insensitive、後続成分はcase-sensitiveに比較する。
包含確認後のbasename処理も、上流のoptionalなprefix/suffix除去に合わせた。

もう一つは最初の読込時の分類を保持する変更。
上流は最初にdefault libraryとして読んだファイルだけをstable sortし、後からlibrary membershipが
付いたrootをそのsortへ移さない。追加の複数置換・root promotionケースで観測し、
`StagedSource.initially_library`により、昇格したrootの元のpostorderを保持した。

| コミット | 内容 |
| --- | --- |
| `3478b824d` | 解決後の実パスによる順位、15 path対の上流probe、通常commandの追加6件 |
| `bf304ce48` | basenameのoptional affix処理と2 controls |
| `d2ad11cbf` | root promotion時の最初の分類とpostorder保持（production最終） |
| `96796f4f0` | 既存loader契約の手書き期待順序を同一入力のTypeScript観測へ置換 |

## 最終結果

| 対象 | 結果 |
| --- | --- |
| 元の12件：通常command完全タプル | 12/12一致 ×2、修正前の一致を維持 |
| 元の12件：source/library/rootの順序を含むfacts | 12/12一致 ×2、7件を修正 |
| 追加6件：通常command完全タプルとProgram facts | 6/6一致 ×2 |
| compiler tests | 4 passed / 0 failed、実exit0 |
| library unit tests | 5 passed、実exit0 |
| library loader contracts | 20 passed、実exit0 |
| TypeScript観測の再検査 | 元12件、追加6件、順位probe15件、既存loader観測の4 commandsがexit0 |
| 整形・差分検査 | `cargo fmt --all -- --check`、`git diff --check`ともexit0 |

元の不一致はenabled-package、duplicate-lib-reference、config-directory-anchor、package-subpath、
module-suffixes-isolated、package-exports-isolated、root-promoted-to-libraryの7件。
すべて元の期待値のまま通過した。元12入力・上流artifact・observer・baseline受領証は不変。
元の上流artifact SHA256は`32ee0d8204de1def87d953c685c448467bb9d403080881b7e93c5ca72e48de2d`。

追加6件は2置換の正逆順、複数rootのpromotionと重複lib参照、catalog外のcatalog風basename、
catalog内の別basename、`/lib-extra`の包含境界を含む。
順位unitは別途15 path対を各2回比較し、optional affixの2 pathも検査した。
順位probeはProgram構築を行わず、18件のProgramケース数には加算しない。

最終compiler実行のprepared Programは72回（18件 ×2回 ×2 tests）。
完全commandは共有comparatorを変更せず、JS・declaration・両map、write metadata、診断、status、exitを照合。
config-directory-anchorのcommand exit2・診断6059/5011も期待どおりに一致している。
Program factsは実際のsource/library/root順を別途captureした。

## 既存loader契約の訂正

最初の修正後実行では、library loader contractsが19 passed / 1 failed、実exit101だった。
`lib_replacement_uses_the_config_directory_for_package_subpaths`の旧手書き期待値は
es6 → DOM → iterableだったが、同じ入力・optionのTypeScript観測はes6 → iterable → DOM → mainとなる。
rootのtriple-slash lib参照でiterableが先に発見され、catalog外の置換先どうしは同順位なのでその順を保つ。

この同一入力をfresh Programで2回観測し、
`crates/program/tests/fixtures/h2-8b-library-loader-order.json`に保存して当該assertだけを訂正した。
通常testのignore化・expected-failure化はしていない。残る19 testsと元のLR1期待値は変更していない。
最初の失敗ログとsource SHAは受領証に残す。最初の失敗binaryは再ビルド前のSHAを採取しておらず、
再実行binaryのSHAをその代用にはしていない。

## 再実行と証拠

worktreeは`/Users/hiramatsu/dev/tsc-rs-h2-8b-lr1`、branchは`work/h2-8b-library-replacement`。
実測HEADは`96796f4f0dfcc105cc0d29aab7b94c9cc6431e92`。
library unitsと最初のloader実行は`d2ad11cbf`、最終loaderとcompilerは上記HEADで測定した。
全実行のproduction 2ファイルのSHAは同一で、各実行の前後に入力SHAが不変であることも確認した。

```sh
export CARGO_TARGET_DIR=/Users/hiramatsu/dev/tsc-rs-h2-8b-lr1/target/h2-8b-lr1
export CARGO_BUILD_JOBS=1
taskpolicy -b nice -n 19 cargo test -p tsc-rs-program --lib library::tests -- --nocapture --test-threads=1
taskpolicy -b nice -n 19 cargo test -p tsc-rs-program --test contracts library_program_loader_contract -- --nocapture --test-threads=1
taskpolicy -b nice -n 19 cargo test -p tsc-rs-compiler --test contracts h2_8b_library_replacement -- --nocapture --test-threads=1
```

上流の再観測は`scripts/observe-h2-8b-library-{replacement,order,priority,loader}.mjs --check`。
実際の4 commandsと実exitは受領証に記録した。
ログ・facts・実行前後のSHAは`target/library-order-runs/20260911T121700Z/`。
最終成功binaryのSHA、各log/captureのSHA、環境、実HEAD/tree、実exitは受領証を参照。
完全commandの別形式native tuple JSONは作成していない。

運用は専用target、1 build job、1 test thread、background I/O、nice19。
別worktreeのacceptanceが長時間継続していたため、10 CPU cores・32 GiB RAM・開始前54%空きを確認し、
他のacceptanceとの重なりを許容する方針へ変更して実行した。他のprocessは停止・変更していない。
全重処理が直列だったとは主張せず、待機込みの所要時間を性能値にもしない。

## 範囲

今回の完了はLR1の7件修正と追加観測の一致。
H2.8b全体の完了やprofile activationは含めない。
任意の非library rootと交差する非prefixのlibrary membership layout、未観測のhost hookは残る。
この修正branchで新たなhosted acceptanceは実行していない。
