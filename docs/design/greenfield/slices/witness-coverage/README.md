# OPS-COVER-1：PR CI のテスト入口台帳

2026-09-16。統合担当：Codex。**棚卸し完了。追加実行入口の実装は OPS-COVER-2〜4。**
対象は `.github/workflows/ci.yml` と `witness.yml` の PR gate。
[固定台帳](inventory.v1.json)の `source_commit` と `source_sha256` が調査した source を定める。

## 分かったこと

| Cargo の入口 | 個数 | 設定された PR CI の呼び方 |
| --- | ---: | --- |
| standalone target（filter なし） | 7 | printer job の7 target。ignored/cfg-disabled test の実行までは意味しない |
| standalone target（名前で filter） | 5 | compiler3 / emitter2。現在1 testしかない targetでも、将来の追加を自動では実行しない |
| standalone target の直接呼出しなし | 52 | compiler22 / emitter10 / その他20 |
| lib/bin の test harness | 16 | この2 workflowからの `cargo test` による直接実行なし |

**64 standalone target を列挙した。52件を「挙動が未検証」とは数えない。**
acceptance が同じ比較 helper を Rust の `#[path]` で取り込み、関数を直接呼ぶ場合がある。
台帳は source の共有関係10 target、fixture の literal 参照、明示的な関数呼出名を別に記録する。
helper の共有から、その target の全テスト・新しい入力集合の実行まで推論しない。

現在、直接入口のない52 targetの source を単独変更すると、planner は unknown input として
**全 acceptance / witness グループを選択するが、その standalone target 自身は追加しない**。
`literal_value_provenance_contract.rs` なども該当する。これは共有 production の安全策としての
全体 replay と、変更した専用 contract の実行が別物であることを示す。
解決には target / fixture の所有関係と実行コマンドを同時に登録する必要がある。

### 実例：shared helper と target 全体は違う

- `h2_7d_original_corpus` は shared `assert_original_corpus` を使い、hosted H2.7d/e も同じ
  関数を呼ぶ。standalone entry がなくても、その比較関数は acceptance にある。
- `h2_7e_original_corpus` は shared command 比較の後に **`assert_original_cli_corpus`** を呼ぶ。
  hosted helper の実行だけで、standalone target の追加 CLI 比較まで実行済みとは扱えない。
- `h2_8a_declaration_specifiers` / `h2_8a_require_rewrite` は既存 corpus/helper を再利用するが、
  各 target の専用 fixture が全てその helper 経由で replay される保証にはならない。
- `decorator_super_contract` と acceptance は `support/witness_libraries.rs` を共有する。
  この補助 source の共有自体は SUPER の実行証明にならない。SUPER は別 witness job が担う。

## 次に実装するスライス

| ID / 担当 | 対象 | 入れる順序・終了条件 |
| --- | --- | --- |
| OPS-COVER-2 / 統合担当 | 未登録の emitter direct 10 target | literal4 / metadata6を分けて現行 baselineと fixture 分母を確認。必要な owner group に実コマンド・fixture/target 選択・0 test拒否を一緒に登録。新しい producer 修復PRの検証範囲と合わせ、無関係な full chain を増やさない |
| OPS-COVER-3 / 統合担当 | compiler22 target と filtered3 target の未選択部分 | 先に既存 acceptance の case ID / helper 呼出しとの重複を照合。H2.7e の追加CLI比較、UTF-16/literal、declaration/map、parameter の順に具体的な未収載集合を確定。530などの既存全体を再度追加しない |
| OPS-COVER-4 / 統合担当、各製品 owner | その他20 standalone と16 lib/bin harness | syntax/binder/types、host/program、checker/API、harness/fuzz に分割。単独file変更と共有変更の依存表を持ち、該当製品 slice の公開契約・取消・error・文字列境界を実測して登録 |
| OPS-BUDGET / 統合担当 | 新規 group の build / replay / merge後の重複 | 2 workers、45分で分割検討、60分hard limit。PRとmain pushの実行時間を別集計。entryを追加してから恒常的な時間超過を発見する順序にしない |

emitter literal4：`literal_parent_provenance_contract`、`literal_value_provenance_contract`、
`string_literal_identifier_source_contract`、`utf16_literal_escaping_contract`。
metadata6：`class_header_token_metadata_contract`、`comma_argument_factory_contract`、
`ellipsis_comment_metadata_contract`、`import_type_attributes_contract`、
`mapped_type_members_contract`、`token_comment_phase_metadata_contract`。

これらは Claude の新しい6件目の大規模依頼にはしない。C01/C02/C04 等の提出時に必要な対照を
照合し、登録と本番統合は統合担当が行う。現在の Claude 推奨順は④→⑤→①→②のまま。

`ci.yml` は PRに加えてmain pushでも動く。たとえば #527 の PR acceptance の後、
merge `526c2b37a` に [main push run](https://github.com/kazhiramatsu/tsc-rs/actions/runs/34987714601)
が発生している。PR jobの最大時間と、merge後を含む総runner時間を混同しない。
本棚卸しはmain検証を省略する設定変更や、既存 gate の成功履歴流用を行っていない。

## 再生成・検証と限界

```sh
python3 docs/design/greenfield/slices/witness-coverage/inventory.py --check
# 新しい source の調査結果は、新しい保存先へ明示的に採取する
python3 docs/design/greenfield/slices/witness-coverage/inventory.py --write --output /tmp/coverage-candidate.json
```

generator が実行する Cargo コマンドは `cargo metadata --offline --no-deps` のみ。
Rustのビルド・テスト・oracle、既存の全件 replay は実行しない。
`witness.invocation` から argv を取得し、printer の宣言 target と literal owner command を読む。
workflow の認識していない shell entry が追加された場合はエラーにする。

- source module の探索は literal `#[path]` edge。macro / generated / default-path module の
  完全な Rust reachability 解析ではない。
- acceptance の共有 source は file単位の保守的な参照。関数 bodyの到達、filter/env、
  dynamic fixtureの展開、caseごとの比較項目は各追加スライスで照合する。
- fixture literalの参照は入力一覧の網羅性や実行結果ではない。動的・解決不能な綴りは別欄に残す。
- lib/bin の一覧はテスト harness の直接入口だけを対象にする。同じ関数の通常実行を否定しない。
- `--check` は metadata / command / source hash の drift を検知する。台帳の1行を「全体実行」に
  改変した negative control も拒否を確認した。これは互換テストのpass件数には加算しない。

## 全 standalone target

下表の「共有」は acceptance と明示的な source参照が重なる数で、実行したテスト数ではない。
filter名、command owner、source/fixture path、driverの関数名はJSON台帳で参照できる。

| crate | target | PRの直接入口 | 共有source数 |
| --- | --- | --- | ---: |
| binder | [owned_symbol_names](../../../../../crates/binder/tests/owned_symbol_names.rs) | なし | 0 |
| checker | [authoritative_external_fact](../../../../../crates/checker/tests/authoritative_external_fact.rs) | なし | 0 |
| compiler | [contracts](../../../../../crates/compiler/tests/contracts.rs) | test名でfilter | 7 |
| compiler | [decorator_super_contract](../../../../../crates/compiler/tests/decorator_super_contract.rs) | test名でfilter | 1 |
| compiler | [h2_5h_parameter_temporaries](../../../../../crates/compiler/tests/h2_5h_parameter_temporaries.rs) | なし | 0 |
| compiler | [h2_5h_utf16_literal_rows](../../../../../crates/compiler/tests/h2_5h_utf16_literal_rows.rs) | なし | 0 |
| compiler | [h2_5h_utf16_literal_witnesses](../../../../../crates/compiler/tests/h2_5h_utf16_literal_witnesses.rs) | なし | 3 |
| compiler | [h2_5h_utf16_original_rows_complete](../../../../../crates/compiler/tests/h2_5h_utf16_original_rows_complete.rs) | なし | 0 |
| compiler | [h2_6a_map_option_projection](../../../../../crates/compiler/tests/h2_6a_map_option_projection.rs) | なし | 0 |
| compiler | [h2_7d_bundle_program](../../../../../crates/compiler/tests/h2_7d_bundle_program.rs) | なし | 0 |
| compiler | [h2_7d_bundle_sinks](../../../../../crates/compiler/tests/h2_7d_bundle_sinks.rs) | test名でfilter | 0 |
| compiler | [h2_7d_declaration_bundles](../../../../../crates/compiler/tests/h2_7d_declaration_bundles.rs) | なし | 0 |
| compiler | [h2_7d_module_identities](../../../../../crates/compiler/tests/h2_7d_module_identities.rs) | なし | 0 |
| compiler | [h2_7d_original_corpus](../../../../../crates/compiler/tests/h2_7d_original_corpus.rs) | なし | 1 |
| compiler | [h2_7e_declaration_map_apis](../../../../../crates/compiler/tests/h2_7e_declaration_map_apis.rs) | なし | 0 |
| compiler | [h2_7e_declaration_maps](../../../../../crates/compiler/tests/h2_7e_declaration_maps.rs) | なし | 0 |
| compiler | [h2_7e_original_corpus](../../../../../crates/compiler/tests/h2_7e_original_corpus.rs) | なし | 1 |
| compiler | [h2_8a_declaration_comment_ranges](../../../../../crates/compiler/tests/h2_8a_declaration_comment_ranges.rs) | なし | 4 |
| compiler | [h2_8a_declaration_specifiers](../../../../../crates/compiler/tests/h2_8a_declaration_specifiers.rs) | なし | 2 |
| compiler | [h2_8a_jsdoc_return](../../../../../crates/compiler/tests/h2_8a_jsdoc_return.rs) | なし | 1 |
| compiler | [h2_8a_original_corpus](../../../../../crates/compiler/tests/h2_8a_original_corpus.rs) | なし | 1 |
| compiler | [h2_8a_prologue_only_detached_comments](../../../../../crates/compiler/tests/h2_8a_prologue_only_detached_comments.rs) | なし | 0 |
| compiler | [h2_8a_require_rewrite](../../../../../crates/compiler/tests/h2_8a_require_rewrite.rs) | なし | 4 |
| compiler | [h2_8a_utf16_identity_recovery_controls](../../../../../crates/compiler/tests/h2_8a_utf16_identity_recovery_controls.rs) | なし | 0 |
| compiler | [h2_8a_utf16_literal_recovery_corpus](../../../../../crates/compiler/tests/h2_8a_utf16_literal_recovery_corpus.rs) | なし | 0 |
| compiler | [h2_8a_utf16_review_fix_controls](../../../../../crates/compiler/tests/h2_8a_utf16_review_fix_controls.rs) | なし | 0 |
| compiler | [h2_8a_utf16_tagged_template_controls](../../../../../crates/compiler/tests/h2_8a_utf16_tagged_template_controls.rs) | なし | 0 |
| emitter | [class_header_token_metadata_contract](../../../../../crates/emitter/tests/class_header_token_metadata_contract.rs) | なし | 0 |
| emitter | [comma_argument_factory_contract](../../../../../crates/emitter/tests/comma_argument_factory_contract.rs) | なし | 0 |
| emitter | [comma_list_printer_contract](../../../../../crates/emitter/tests/comma_list_printer_contract.rs) | target指定・filterなし | 0 |
| emitter | [contracts](../../../../../crates/emitter/tests/contracts.rs) | test名でfilter | 0 |
| emitter | [decorator_super_direct_contract](../../../../../crates/emitter/tests/decorator_super_direct_contract.rs) | test名でfilter | 0 |
| emitter | [ellipsis_comment_metadata_contract](../../../../../crates/emitter/tests/ellipsis_comment_metadata_contract.rs) | なし | 0 |
| emitter | [emit_pipeline_phases_contract](../../../../../crates/emitter/tests/emit_pipeline_phases_contract.rs) | target指定・filterなし | 0 |
| emitter | [import_type_attributes_contract](../../../../../crates/emitter/tests/import_type_attributes_contract.rs) | なし | 0 |
| emitter | [list_comment_flags_contract](../../../../../crates/emitter/tests/list_comment_flags_contract.rs) | target指定・filterなし | 0 |
| emitter | [list_format_flags_contract](../../../../../crates/emitter/tests/list_format_flags_contract.rs) | target指定・filterなし | 0 |
| emitter | [literal_parent_provenance_contract](../../../../../crates/emitter/tests/literal_parent_provenance_contract.rs) | なし | 0 |
| emitter | [literal_value_provenance_contract](../../../../../crates/emitter/tests/literal_value_provenance_contract.rs) | なし | 0 |
| emitter | [mapped_type_members_contract](../../../../../crates/emitter/tests/mapped_type_members_contract.rs) | なし | 0 |
| emitter | [printer_failure_contract](../../../../../crates/emitter/tests/printer_failure_contract.rs) | target指定・filterなし | 0 |
| emitter | [source_comment_topology_contract](../../../../../crates/emitter/tests/source_comment_topology_contract.rs) | target指定・filterなし | 0 |
| emitter | [string_literal_identifier_source_contract](../../../../../crates/emitter/tests/string_literal_identifier_source_contract.rs) | なし | 0 |
| emitter | [token_comment_phase_metadata_contract](../../../../../crates/emitter/tests/token_comment_phase_metadata_contract.rs) | なし | 0 |
| emitter | [utf16_literal_escaping_contract](../../../../../crates/emitter/tests/utf16_literal_escaping_contract.rs) | なし | 0 |
| emitter | [utf16_writer_contract](../../../../../crates/emitter/tests/utf16_writer_contract.rs) | target指定・filterなし | 0 |
| fuzz | [contracts](../../../../../crates/fuzz/tests/contracts.rs) | なし | 0 |
| harness | [contracts](../../../../../crates/harness/tests/contracts.rs) | なし | 0 |
| host | [compiler_host_contract](../../../../../crates/host/tests/compiler_host_contract.rs) | なし | 0 |
| host | [filesystem_host_contract](../../../../../crates/host/tests/filesystem_host_contract.rs) | なし | 0 |
| program | [contracts](../../../../../crates/program/tests/contracts.rs) | なし | 0 |
| program | [h2_7d_bundle_source_facts](../../../../../crates/program/tests/h2_7d_bundle_source_facts.rs) | なし | 0 |
| program | [host_platform_smoke_contract](../../../../../crates/program/tests/host_platform_smoke_contract.rs) | なし | 0 |
| program | [utf16_config_paths](../../../../../crates/program/tests/utf16_config_paths.rs) | なし | 0 |
| program | [utf16_module_paths](../../../../../crates/program/tests/utf16_module_paths.rs) | なし | 0 |
| program | [utf16_raw_source_boundary](../../../../../crates/program/tests/utf16_raw_source_boundary.rs) | なし | 0 |
| syntax | [entity_names](../../../../../crates/syntax/tests/entity_names.rs) | なし | 0 |
| syntax | [new_meta_property_name](../../../../../crates/syntax/tests/new_meta_property_name.rs) | なし | 0 |
| syntax | [owned_literal_values](../../../../../crates/syntax/tests/owned_literal_values.rs) | なし | 0 |
| syntax | [recovery_provenance](../../../../../crates/syntax/tests/recovery_provenance.rs) | なし | 0 |
| syntax | [scanner_escape_diagnostics](../../../../../crates/syntax/tests/scanner_escape_diagnostics.rs) | なし | 0 |
| syntax | [template_escape_flags](../../../../../crates/syntax/tests/template_escape_flags.rs) | なし | 0 |
| syntax | [template_flags](../../../../../crates/syntax/tests/template_flags.rs) | なし | 0 |
| types | [compiler_option_number_contract](../../../../../crates/types/tests/compiler_option_number_contract.rs) | なし | 0 |
