# Claude 実装依頼③：A40-PRINT-FAILURE — printer の失敗順序と再利用

作成日：2026-09-14。親：H2.8a / A6-40。状態：隔離候補の research。


**2026-09-15 更新**：開始点と検証分担は[共通手順](claude-high-difficulty-handoffs.md)の最新版に従います。
SUPER 統合後の main の SHA を固定し、ローカルは新規失敗・関連 owner の focused set、
重い全件 replay は hosted で実行します。以下の技術要件は現行実装と照合し、既実装部分を再実装しません。

## 依頼

printer が comment/list/map/hook の途中で失敗した場合の、観測可能な出力 prefix、
callback 順序、保持される状態、次の print への影響を固定 TypeScript と比較し、
必要な Rust 修正を隔離候補として提出してください。正常系の再実行だけで終了せず、
失敗点を選択できる observer、before/after、再利用 control、設計と patch をそろえてください。

[共通手順](claude-high-difficulty-handoffs.md)の開始点で
`draft/h2-8a-printer-failure` / `../tsc-rs-printer-failure` を作り、source/input manifest を保存します。
UTF-16 と SUPER の統合後の printer/writer を基準にします。root integration は依頼に含めません。

## 開始状態と重要な制約

[A6-40 記録](h2-8a-retained-lexical-owners.md)には、list cursor の再利用 controls と
正常時の pipeline 観測があり、comment worker の failure-order、writer/map fault、
metadata annotation failure は未解決事項として残っています。
[emit_pipeline_phases_contract.rs](../../../../crates/emitter/tests/emit_pipeline_phases_contract.rs)
の既存 fixture は成功時の hook 順序を対象にしています。

upstream `writeNode` / `writeFile2` / `writeBundle` は通常の処理後に `reset()` を呼びます。
source のこの入口に unconditional な `finally` はありません。
したがって、**「例外後は必ず初期状態へ戻る」を期待値にしてはいけません**。
entry/失敗位置ごとに source の到達した操作と次の呼出を測定します。
Rust が戻す `Result::Err` に固有の安全性は、source exception 互換性と分けて設計します。

対象は既存 printer とその writer/map/comment/hook 境界です。
公開 custom transform API 全体、全 sink の atomic write 改修、新 printer の再実装は含めません。
source が外へ公開しない内部状態は probe で観測できますが、その観測に production parity の
加点はしません。source で再利用不能なら、その根拠と境界を明記します。

## Source/Rust の入口

固定 `_tsc.js` の入口と、調査する Rust owner：

| Source | 意味 | Rust |
| --- | --- | --- |
| `createPrinter` 116912、`printNode` 116985、`printFile` 117019 | owned writer と public entry の寿命 | [printer.rs](../../../../crates/emitter/src/printer.rs) の Printer / PrintRequest |
| `writeNode` 117028、`writeBundle` 117058、`writeFile2`、`reset` 117117 | previous writer、source switch、正常終了・例外時の実行順 | 同 printer と [printer/bundle.rs](../../../../crates/emitter/src/printer/bundle.rs) |
| `pipelineEmitWithNotification` 117219、`pipelineEmitWithSubstitution` 117712 | before/emit/after、代替 node、callback が投げる位置 | `emit_node_with_hint_and_source_comments`、[transform.rs](../../../../crates/emitter/src/transform.rs) の通知契約 |
| `emitList` 120015 と list/comment callee | delimiter、separator、次 item の comment、cursor 更新 | `emit_node_array`、`emit_list_item_position_comments`、[comment_cursor.rs](../../../../crates/emitter/src/comment_cursor.rs) |
| source-map emit worker と writer callee（今回 pin を追加） | map callback の前後、既に発行した bytes/mapping | [source_map.rs](../../../../crates/emitter/src/source_map.rs)、[writer.rs](../../../../crates/emitter/src/writer.rs) |

実装前に全 callee の範囲/hash と「失敗し得るか」を一覧にします。
Rust `TextWriter` の通常 write は fallible callback API ではありません。
source の外部 writer 例外に対応する入口がない場合、既存 handler/StandaloneWriter の
実装を調査し、test-only adapter でどこまで観測できるかを定義します。
全 write を安易に `Result` 化する大改修は、この差の存在だけを根拠に開始しません。

編集候補は `printer.rs`、`printer/bundle.rs`、`comment_cursor.rs`、`source_map.rs`、
`writer.rs`、`transform.rs`。factory/metadata が必要なら失敗 source owner と consumer を
示して追加します。writer の UTF-16 値の意味を変更する①とは独立に差分を分けます。

## 実装手順

1. 正常 trace を採取し、event を `phase / node-kind / occurrence / write-category` 等で
   識別する。実行順が変わっても意図した失敗点を指定できる manifest にする。
2. upstream の public writer/handler から実際に到達する失敗点を作る。
   before/substitute/emit/after、comment 出力、map recording、list の前半/後半を区別する。
   公開 hook で到達しない点は別の instrumented probe として記録する。
3. `seed → faulting operation → recovery operation → probe` を同じ printer で実行し、
   fresh printer の対照を取る。source の処理を一律 rollback と仮定しない。
4. state ごとに寿命を整理する：printer 持続、1 print call、source file、node、list、
   writer 持続。復元すべきもの、失敗以前の更新を保持するもの、破棄するものを表にする。
5. source に対応する観測を保つ Rust の状態遷移を実装する。
   borrow/identity の安全性と source の失敗結果を両立させる。
   source が cleanup を呼ばないところで after hook を勝手に追加しない。
6. native-only invalid handle / annotation failure / typed map error を別 battery で検証する。
   復元 guard を入れる場合は適用 scope と保護する不変条件を記録する。

## 必須 witness

ID は `printer-failure/<entry>/<phase>/<site>/<sequence>`。
各表を具体的 ID に展開して件数を固定し、成功/失敗どちらの観測も二度採取します。

| 軸 | 必須対照 |
| --- | --- |
| entry | printNode / printFile / writeNode / writeList / writeFile / writeBundle。実 Rust counterpart の有無も記録 |
| list | empty/single/multiple、trailing comma、comment before/after separator、detached prefix、nested list |
| state | generated names、last-list-item position、comment cursor/container、current source、indent、UTF-16 column、map source |
| hook | before、substitute、nested emit、after。first occurrence / later occurrence と代替 node |
| writer/map | comment/keyword/string/line write、map source 切替と segment、最初の write 前/部分出力後 |
| reuse | 成功→成功、成功→失敗→成功、失敗→別 source、同一 node、clone、bundle の二つ目の source |
| text | LF/CRLF、非 BMP、孤立 surrogate の direct writer、NoComments/NoNestedComments/removeComments |
| compiler | source-map artifact の write failure と `noEmitOnError`。内部 printer fault と sink fault を別 ID にする |

各失敗 row は実例外/typed error、全 event prefix、実際に callback へ渡った text/code units、
その時点で取得可能な map/position、次 operation の結果を保存します。
失敗時に result が返らなかったことと、空 output が返ったことを区別します。
native の before/after callback を source の around-emit handler に対応させる場合、
adapter が `try/finally` を追加して期待値を変えていないことを確認してください。

## 検証と提出

新規 target 案：`crates/emitter/tests/printer_failure_contract.rs`、必要なら
`crates/compiler/tests/printer_failure_pipeline_contract.rs`。
observer は `scripts/observe-printer-failures.mjs`、fixture は新規
`crates/emitter/tests/fixtures/printer-failure-*.json` として保存します。

既存 focused 入口（開始 SHA を固定した worktree、共通の低優先度/env を適用）：

```sh
cargo test --offline --manifest-path crates/emitter/Cargo.toml \
  --test emit_pipeline_phases_contract --test comma_list_printer_contract \
  --test list_format_flags_contract --test list_comment_flags_contract \
  --test source_comment_topology_contract -- --test-threads=1
```

ローカルでは新規 failure battery の focused set と、変更した入口の既存正常 trace を検証します。
必要な全件 regression は hosted で実行し、旧 530 / 494 / 452 をローカル終了条件にしません。
外部 writer を変更した場合は UTF16 writer/escaping controls、map owner を変更した場合は
既存 source-map focused tests を追加します。0-test selector は通過と扱いません。

完了条件は source/Rust の failure site 対応、state lifetime 表、全選定 row の一致 × 2、
再利用と正常系 regression 0、patch と全 capture の保存です。
source で観測不能な内部状態や Rust-only error は別記し、全 API failure 互換性を主張しません。
readiness の未解決 owner は明示的に残します。
