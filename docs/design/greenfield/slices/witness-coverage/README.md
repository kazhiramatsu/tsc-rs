# OPS-COVER：PR CI のテスト入口台帳

2026-09-17。統合担当：Codex。**宣言map出力/APIまでPR #547で検証・統合済み。残りは OPS-COVER-3残部〜4。**
対象は `.github/workflows/ci.yml` と `witness.yml` の PR gate。
[現在の固定台帳](inventory.v20.json)の `source_commit` と `source_sha256` が調査した source を定める。

[最初の台帳 v1](inventory.v1.json) は #528 の merge を調べた履歴として保持する。
[OPS-COVER-2](emitter-direct/README.md) で10 targetを追加した [v2](inventory.v2.json) も保持する。
[OPS-COVER-3A](declaration-map-cli/README.md) のCLI追加を記録した [v3](inventory.v3.json)も保持する。
[OPS-COVER-3B](compiler-utf16/README.md) は3つのUTF-16 targetを追加し、以下をv4に更新した。
3Bは[PR #532](https://github.com/kazhiramatsu/tsc-rs/pull/532)で統合済み。全7 replay jobと両gateが成功。
[OPS-COVER-3C](compiler-literals/README.md) は64専用入力と4原本の追加比較を接続し、v5に更新した。
C04 [transpile統合](../h2-8c-transpile/INTEGRATION.md) は新設1 targetを登録した（v6）。
初回hostedでNode version不一致を検出し、transpileを含むcontrolsにNode25.2.1を設定したv7へ更新。入口の件数はv6と同じ。
[A-PC1](../h2-8a-compact-body-comments.md) が新設printer targetと既存parameter targetを登録したv8へ更新。
[C05](../l2-3-resolution-cache/integration/README.md) がprogram contractと全program libを登録したv9、path表記の追加回帰を反映したv10へ更新。
[OPS-COVER-3D](compiler-declarations/README.md) はdeclaration specifier / comment / JSDocの3 target・129専用入力を登録したv11へ更新。[PR #540](https://github.com/kazhiramatsu/tsc-rs/pull/540)で統合済み。全7 replay job・両gate成功、controls22分14秒。
[C01](../h2-8a-literal-update/integration/README.md)の新規2 targetと[OPS-COVER-3E](compiler-require-rewrite/README.md)のrequire-rewrite専用74入力を登録したv12へ更新。[PR #542](https://github.com/kazhiramatsu/tsc-rs/pull/542)で複数スライスをまとめて検証・統合済み。全7 hosted jobと両gateが成功。
[OPS-COVER-3F / 3G](compiler-config-prologue/README.md) はconfig/libraryの24 exact test・96入力とprologueの1 target・8入力を追加したv13。[PR #544](https://github.com/kazhiramatsu/tsc-rs/pull/544)で限定printer修復と一括検証・統合済み。全7 replay job・両gate成功、controls34分54秒。
設定された入口と実行した比較面・件数は各スライスの記録で区別する。

[OPS-COVER-3H / 3I](compiler-recovery-map/README.md) はrecovery50入力とmap-option31専用入力・原本5 IDの2 targetを追加したv14。PR #545で一括検証・統合済み（全7 replay job・両gate成功、controls35分45秒）。個別の比較面と重複は同記録に保持する。

[OPS-COVER-3J / 3K](compiler-bundles/README.md) はbundle Programとdeclaration/mapの2 targetを登録したv15。古いnoEmit拒否検査を固定tuple比較へ更新し、PR #546で最終7 testsと両observerを含む全7 hosted replay job・両gateが成功。controls37m51s。

[OPS-COVER-3L / 3M](compiler-declaration-maps/README.md) は宣言map出力とstateful APIの2 target・11 testsを登録したv16。実測6m57sを受け、controlsの37m51sに余裕を残すため専用jobへ分けたv17。PR #547で全8 replay job・両gateが成功し統合済み。新jobは5m38s、controlsは38m11s。共有fixtureとreferenceの重複は個別記録に保持する。

C02 generated-binding はPR #549で統合済み。 direct 156入力と pipeline 768入力（767 complete + 1 upstream exception）を追加した v18。pipeline は独立 job、共有 observer は両 suite を選択する。採取時の詳細は [統合記録](../h2-8a-generated-binding/integration/revised/README.md) を参照。

[OPS-COVER-3N/3O](compiler-module-facets/README.md) はmodule identityの3 testsと原本JavaScript bundle recorderの1 testをcontrolsへ追加したv20（実測81秒を受けてビルドを共有）。nativeの比較面は38＋4入力、各2回。追加のAPI/path参照はRust実行件数へ加算しない。hostedは同記録で追跡する。

## 現在の入口

| Cargo の入口 | 個数 | 設定された PR CI の呼び方 |
| --- | ---: | --- |
| standalone target（filter なし） | 34 | controlsのmodule identity target、printer job の7 targetとdirect13、controls jobのcompiler UTF-16/literalの4 targetとtranspile・parameter・literal-update-pipeline・prologue-comments・recovery-corpus・bundle-program・resolution cache contractと専用declaration-maps jobの2 target。ignored/cfg-disabled test の実行までは意味しない |
| standalone target（名前で filter） | 14 | compiler12 / emitter2。現在1 testしかない targetでも、将来の追加を自動では実行しない |
| standalone target の直接呼出しなし | 23 | compiler3 / その他20 |
| lib/bin の test harness | 16 | Program lib 1件に直接入口、残り15件は直接実行なし |

**71 standalone target を列挙した。23件を「挙動が未検証」とは数えない。**
acceptance が同じ比較 helper を Rust の `#[path]` で取り込み、関数を直接呼ぶ場合がある。
台帳は source の共有関係11 target、fixture の literal 参照、明示的な関数呼出名を別に記録する。
helper の共有から、その target の全テスト・新しい入力集合の実行まで推論しない。

現在、直接入口のない23 targetの source を単独変更すると、planner は unknown input として
**全 acceptance / witness グループを選択するが、その standalone target 自身は追加しない**。
v1 では `literal_value_provenance_contract.rs` も該当したが、v2 では専用 target のみを選ぶ。共有 production の安全策としての
全体 replay と、変更した専用 contract の実行が別物であることを示す。
残る23件も target / fixture の所有関係と実行コマンドを同時に登録する必要がある。

### 実例：shared helper と target 全体は違う

- `h2_7d_original_corpus` は shared `assert_original_corpus` を使い、hosted H2.7d/e も同じ
  関数を呼ぶ。standalone entry がなくても、その比較関数は acceptance にある。
- `h2_7e_original_corpus` は元のtestで shared command の後にCLIを呼んでいた。
  v3ではCLIを別testに切り出して `--exact` で登録し、Programは既存acceptanceが担う。
  同じ8 IDでも比較経路が異なるため、CLI追加は原本exact総数の増分として数えない。
- `h2_8a_declaration_specifiers` / `h2_8a_require_rewrite` は既存 corpus/helper を再利用するが、
  各 target の専用 fixture が全てその helper 経由で replay される保証にはならない。
  v11は前者の専用30入力、v12は後者の専用74入力を明示登録した。原本wrapperは別管理する。
- `decorator_super_contract` と acceptance は `support/witness_libraries.rs` を共有する。
  この補助 source の共有自体は SUPER の実行証明にならない。SUPER は別 witness job が担う。

## 次に実装するスライス

| ID / 担当 | 対象 | 入れる順序・終了条件 |
| --- | --- | --- |
| OPS-COVER-2 / 統合担当 | emitter direct 10 target：入口追加完了 | [検証記録](emitter-direct/README.md)。literal4 / metadata6、2239 row ×2、13 tests。既存 printer job の20分枠・2 workersでbuildを共有し、専用入力はtarget単位で選択 |
| OPS-COVER-3 / 統合担当 | [3AのCLI8件](declaration-map-cli/README.md)と[3BのUTF-16 3 target](compiler-utf16/README.md)、[3Cのliteral2 target](compiler-literals/README.md)を登録。[3Dのdeclaration3 target](compiler-declarations/README.md)も登録。[3Eのrequire-rewrite74入力](compiler-require-rewrite/README.md)も登録。[3F/3Gのconfig/library96入力とprologue8入力](compiler-config-prologue/README.md)も登録。[3H/3I](compiler-recovery-map/README.md)でrecovery50とmap-optionの2 targetを登録。[3J/3K](compiler-bundles/README.md)でbundle Programとdeclaration/mapを登録。[3N/3O](compiler-module-facets/README.md)でmodule identityと原本JavaScript recorderも登録。残りcompiler3 targetと名前filterの未選択部分 | 続いて旧literal rowsの重複、declaration/map、parameterの未収載集合を既存acceptanceのID／比較面と照合。530などの既存全体を再度追加しない |
| OPS-COVER-4 / 統合担当、各製品 owner | その他20 standalone と残15 lib/bin harness | syntax/binder/types、host/program、checker/API、harness/fuzz に分割。単独file変更と共有変更の依存表を持ち、該当製品 slice の公開契約・取消・error・文字列境界を実測して登録 |
| OPS-BUDGET / 統合担当 | 新規 group の build / replay / merge後の重複 | 2 workers、45分で分割検討、60分hard limit。PRとmain pushの実行時間を別集計。entryを追加してから恒常的な時間超過を発見する順序にしない |

emitter literal4：`literal_parent_provenance_contract`、`literal_value_provenance_contract`、
`string_literal_identifier_source_contract`、`utf16_literal_escaping_contract`。
metadata6：`class_header_token_metadata_contract`、`comma_argument_factory_contract`、
`ellipsis_comment_metadata_contract`、`import_type_attributes_contract`、
`mapped_type_members_contract`、`token_comment_phase_metadata_contract`。

これらは Claude の新しい6件目の大規模依頼にはしない。C01/C02/C04 等の提出時に必要な対照を
照合し、登録と本番統合は統合担当が行う。C01はPR #542で統合済み。C02もPR #549で統合済み。次の追加依頼は[T1](../h2-8a-bundle-metadata-t1-claude-handoff.md)。

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

## 全 standalone target（v15の履歴表）

現行の全件はv20 JSONを参照。以下は過去の表であり現在の入口判定には使わない。

下表の「共有」は acceptance と明示的な source参照が重なる数で、実行したテスト数ではない。
filter名、command owner、source/fixture path、driverの関数名はJSON台帳で参照できる。

| crate | target | PRの直接入口 | 共有source数 |
| --- | --- | --- | ---: |
| binder | [owned_symbol_names](../../../../../crates/binder/tests/owned_symbol_names.rs) | なし | 0 |
| checker | [authoritative_external_fact](../../../../../crates/checker/tests/authoritative_external_fact.rs) | なし | 0 |
| compiler | [contracts](../../../../../crates/compiler/tests/contracts.rs) | test名でfilter | 7 |
| compiler | [decorator_super_contract](../../../../../crates/compiler/tests/decorator_super_contract.rs) | test名でfilter | 1 |
| compiler | [h2_5h_parameter_temporaries](../../../../../crates/compiler/tests/h2_5h_parameter_temporaries.rs) | target指定・filterなし | 0 |
| compiler | [h2_5h_utf16_literal_rows](../../../../../crates/compiler/tests/h2_5h_utf16_literal_rows.rs) | なし | 0 |
| compiler | [h2_5h_utf16_literal_witnesses](../../../../../crates/compiler/tests/h2_5h_utf16_literal_witnesses.rs) | test名でfilter | 3 |
| compiler | [h2_5h_utf16_original_rows_complete](../../../../../crates/compiler/tests/h2_5h_utf16_original_rows_complete.rs) | target指定・filterなし | 0 |
| compiler | [h2_6a_map_option_projection](../../../../../crates/compiler/tests/h2_6a_map_option_projection.rs) | test名でfilter | 0 |
| compiler | [h2_7d_bundle_program](../../../../../crates/compiler/tests/h2_7d_bundle_program.rs) | target指定・filterなし | 0 |
| compiler | [h2_7d_bundle_sinks](../../../../../crates/compiler/tests/h2_7d_bundle_sinks.rs) | test名でfilter | 0 |
| compiler | [h2_7d_declaration_bundles](../../../../../crates/compiler/tests/h2_7d_declaration_bundles.rs) | test名でfilter | 0 |
| compiler | [h2_7d_module_identities](../../../../../crates/compiler/tests/h2_7d_module_identities.rs) | なし | 0 |
| compiler | [h2_7d_original_corpus](../../../../../crates/compiler/tests/h2_7d_original_corpus.rs) | なし | 1 |
| compiler | [h2_7e_declaration_map_apis](../../../../../crates/compiler/tests/h2_7e_declaration_map_apis.rs) | なし | 0 |
| compiler | [h2_7e_declaration_maps](../../../../../crates/compiler/tests/h2_7e_declaration_maps.rs) | なし | 0 |
| compiler | [h2_7e_original_corpus](../../../../../crates/compiler/tests/h2_7e_original_corpus.rs) | test名でfilter | 1 |
| compiler | [h2_8a_declaration_comment_ranges](../../../../../crates/compiler/tests/h2_8a_declaration_comment_ranges.rs) | test名でfilter | 4 |
| compiler | [h2_8a_declaration_specifiers](../../../../../crates/compiler/tests/h2_8a_declaration_specifiers.rs) | test名でfilter | 2 |
| compiler | [h2_8a_jsdoc_return](../../../../../crates/compiler/tests/h2_8a_jsdoc_return.rs) | test名でfilter | 1 |
| compiler | [h2_8a_original_corpus](../../../../../crates/compiler/tests/h2_8a_original_corpus.rs) | なし | 1 |
| compiler | [h2_8a_prologue_only_detached_comments](../../../../../crates/compiler/tests/h2_8a_prologue_only_detached_comments.rs) | target指定・filterなし | 0 |
| compiler | [h2_8a_require_rewrite](../../../../../crates/compiler/tests/h2_8a_require_rewrite.rs) | test名でfilter | 4 |
| compiler | [h2_8a_utf16_identity_recovery_controls](../../../../../crates/compiler/tests/h2_8a_utf16_identity_recovery_controls.rs) | target指定・filterなし | 0 |
| compiler | [h2_8a_utf16_literal_recovery_corpus](../../../../../crates/compiler/tests/h2_8a_utf16_literal_recovery_corpus.rs) | target指定・filterなし | 0 |
| compiler | [h2_8a_utf16_review_fix_controls](../../../../../crates/compiler/tests/h2_8a_utf16_review_fix_controls.rs) | target指定・filterなし | 0 |
| compiler | [h2_8a_utf16_tagged_template_controls](../../../../../crates/compiler/tests/h2_8a_utf16_tagged_template_controls.rs) | target指定・filterなし | 0 |
| compiler | [literal_update_pipeline_contract](../../../../../crates/compiler/tests/literal_update_pipeline_contract.rs) | target指定・filterなし | 0 |
| compiler | [transpile_routes_contract](../../../../../crates/compiler/tests/transpile_routes_contract.rs) | target指定・filterなし | 0 |
| emitter | [class_header_token_metadata_contract](../../../../../crates/emitter/tests/class_header_token_metadata_contract.rs) | target指定・filterなし | 0 |
| emitter | [comma_argument_factory_contract](../../../../../crates/emitter/tests/comma_argument_factory_contract.rs) | target指定・filterなし | 0 |
| emitter | [comma_list_printer_contract](../../../../../crates/emitter/tests/comma_list_printer_contract.rs) | target指定・filterなし | 0 |
| emitter | [compact_body_comments_contract](../../../../../crates/emitter/tests/compact_body_comments_contract.rs) | target指定・filterなし | 0 |
| emitter | [contracts](../../../../../crates/emitter/tests/contracts.rs) | test名でfilter | 0 |
| emitter | [decorator_super_direct_contract](../../../../../crates/emitter/tests/decorator_super_direct_contract.rs) | test名でfilter | 0 |
| emitter | [ellipsis_comment_metadata_contract](../../../../../crates/emitter/tests/ellipsis_comment_metadata_contract.rs) | target指定・filterなし | 0 |
| emitter | [emit_pipeline_phases_contract](../../../../../crates/emitter/tests/emit_pipeline_phases_contract.rs) | target指定・filterなし | 0 |
| emitter | [import_type_attributes_contract](../../../../../crates/emitter/tests/import_type_attributes_contract.rs) | target指定・filterなし | 0 |
| emitter | [list_comment_flags_contract](../../../../../crates/emitter/tests/list_comment_flags_contract.rs) | target指定・filterなし | 0 |
| emitter | [list_format_flags_contract](../../../../../crates/emitter/tests/list_format_flags_contract.rs) | target指定・filterなし | 0 |
| emitter | [literal_parent_provenance_contract](../../../../../crates/emitter/tests/literal_parent_provenance_contract.rs) | target指定・filterなし | 0 |
| emitter | [literal_update_contract](../../../../../crates/emitter/tests/literal_update_contract.rs) | target指定・filterなし | 0 |
| emitter | [literal_value_provenance_contract](../../../../../crates/emitter/tests/literal_value_provenance_contract.rs) | target指定・filterなし | 0 |
| emitter | [mapped_type_members_contract](../../../../../crates/emitter/tests/mapped_type_members_contract.rs) | target指定・filterなし | 0 |
| emitter | [printer_failure_contract](../../../../../crates/emitter/tests/printer_failure_contract.rs) | target指定・filterなし | 0 |
| emitter | [source_comment_topology_contract](../../../../../crates/emitter/tests/source_comment_topology_contract.rs) | target指定・filterなし | 0 |
| emitter | [string_literal_identifier_source_contract](../../../../../crates/emitter/tests/string_literal_identifier_source_contract.rs) | target指定・filterなし | 0 |
| emitter | [token_comment_phase_metadata_contract](../../../../../crates/emitter/tests/token_comment_phase_metadata_contract.rs) | target指定・filterなし | 0 |
| emitter | [utf16_literal_escaping_contract](../../../../../crates/emitter/tests/utf16_literal_escaping_contract.rs) | target指定・filterなし | 0 |
| emitter | [utf16_writer_contract](../../../../../crates/emitter/tests/utf16_writer_contract.rs) | target指定・filterなし | 0 |
| fuzz | [contracts](../../../../../crates/fuzz/tests/contracts.rs) | なし | 0 |
| harness | [contracts](../../../../../crates/harness/tests/contracts.rs) | なし | 0 |
| host | [compiler_host_contract](../../../../../crates/host/tests/compiler_host_contract.rs) | なし | 0 |
| host | [filesystem_host_contract](../../../../../crates/host/tests/filesystem_host_contract.rs) | なし | 0 |
| program | [contracts](../../../../../crates/program/tests/contracts.rs) | なし | 0 |
| program | [h2_7d_bundle_source_facts](../../../../../crates/program/tests/h2_7d_bundle_source_facts.rs) | なし | 0 |
| program | [host_platform_smoke_contract](../../../../../crates/program/tests/host_platform_smoke_contract.rs) | なし | 0 |
| program | [resolution_cache_contract](../../../../../crates/program/tests/resolution_cache_contract.rs) | target指定・filterなし | 0 |
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
