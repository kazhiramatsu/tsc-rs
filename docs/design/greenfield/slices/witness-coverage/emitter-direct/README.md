# OPS-COVER-2：emitter direct の個別実行入口

2026-09-16。統合担当：Codex。main `78b186190` の未登録10 targetを追加した。
本番 Rust と既存 fixture の変更はない。Claude の④→⑤→①→②の作業順に影響しない。

[PR #530](https://github.com/kazhiramatsu/tsc-rs/pull/530) として merge `92e323587` に着地。
全7 replay job・両必須gateが成功。printer jobは2分17秒、追加direct分はobserver7.336秒＋
Cargo build/replay12.019秒。最長job27分36秒、7 replay job合計87分04秒（plan/gate/main push除外）。
これはrunner変動を含む実測であり、単独の性能比較ではない。

## 実行単位と検証した母集団

次の suite を `python3 scripts/witness.py <suite> --all` で実行できる。
全て emitter の専用 target を filter なしで実行する。`--list` / `--dry-run` は実行しない。

| suite | target（末尾 `_contract`） | fixture row | Rust tests |
| --- | --- | ---: | ---: |
| literal-parent-provenance | literal_parent_provenance | 128 | 1 |
| literal-value-provenance | literal_value_provenance | 480 + 60 | 2 |
| string-literal-identifier-source | string_literal_identifier_source | 72 | 1 |
| utf16-literal-escaping | utf16_literal_escaping | 288 + 8 | 2 |
| class-header-token-metadata | class_header_token_metadata | 32 | 1 |
| comma-argument-factory | comma_argument_factory | 44 + 96 + 104 + 264 + 11 | 2 |
| ellipsis-comment-metadata | ellipsis_comment_metadata | 144 | 1 |
| import-type-attributes | import_type_attributes | 84 | 1 |
| mapped-type-members | mapped_type_members | 328 | 1 |
| token-comment-phase-metadata | token_comment_phase_metadata | 96 | 1 |
| 合計 | 10 target | **2239 ×2** | **13** |

16 fixture の direct factory/printer 観測との比較。全 compiler command の exact 件数には加算しない。
Rust は各 fixture の件数を固定し、全 row を2回比較する。runner も空・重複ID・件数の変更を検知する。
ID の表示は fixture 名で名前空間を分ける。fixture 内に細かい filter を追加せず、この小さい target を
ローカルの focused set とする。TS 6.0.3 の observer は16本を直接、literal parent の元 observer を
子プロセスから1本実行する。親の元128 rowと追加 setup failure 4件は2239へ重複加算しない。

`ratchets/h2-8a-list-cursor-lifecycle.v1.json` の11 rowも実行対象に含む。
この不変の元観測にある過去の `status` は現行 Rust の未実装判定に転用しない。

## 選択と hosted の予算

- 専用 target / fixture / observer の変更は、その suite のみ選択する。複数変更は所有者の和集合。
  literal parent の元 JSON / 元 observer、list cursor ratchet も登録した。
- 新しい10 suiteは既存 `printer` jobに同居する。選択した direct targetだけを1回のCargo呼出しで実行。
  共通変更時は既存のprinter suiteとbuildを共有する。専用入力だけの変更ではprinter failure suiteを実行しない。
- 同じ20分上限、2 workers、同時に重いローカルreplayを走らせない運用を維持する。
  job追加による同一依存crateのcold build重複を増やさない。
- 0 test、missing target、ignored、filtered、非0 exit、observer失敗はgate失敗にする。
  plannerの23件の軽量テストも両workflowのplanで実行し、テストファイルをpolicyのsource hashに追加した。
- 共通本番source・未知の入力は従来通り全関連groupを選ぶ。CIの選択処理自体を変える今回のPRも該当する。

Rust側の参照と全observerの読込を確認した。新規16 fixtureは、この10 target以外のacceptance比較から
参照されていない。Node標準ライブラリ以外の共通依存はvendored TypeScriptで、変更時は全groupを維持する。
これは任意の将来の共有を推定するルールではない。新しい共有consumerの追加時はowner表も更新する。

## ローカル検証と実測

[実行記録](local.v1.json)に source / input / log hash、コマンドと分母を固定した。

- main baseline：この10 targetのみ、13 tests pass。build 3分40秒、各binaryのtest時間合計0.51秒。
- 新しい hosted entry を選択10 suiteで実行：13 tests pass、2239 row ×2。
  observer 19.075 秒、Cargo build済みのbuild＋replay 0.891 秒。
- planner23件、policyとpolicy境界test、inventoryの新旧差分、`git diff --check` を確認。
- build済みのローカル結果はcold hosted jobの時間保証ではない。hosted最終結果・job時間・合計runner時間は
  PR本文に追記する。今回のdirect追加分はobserver時間とCargo build＋replay時間をjob logにも出す。

Rustの本番codeを変更しないため、無関係なlib/contracts全体や530/672のローカルreplayを追加していない。
既存strict Clippy警告の解消はこの入口追加の成果に含めない。

## 残り

[台帳v2](../inventory.v2.json)：64 target中、filterなし17、filterあり5、直接入口なし42。
次は OPS-COVER-3 のcompiler22とfilter残部、OPS-COVER-4のその他20と16 lib/bin。
shared helperの重複を引いた具体的な追加集合を決めてから、同じ選択・分母・予算の単位で登録する。
