# C01 / A40-LITERAL-UPDATE：統合仕様（A-INT1 向け）

提出時点の記録。現在の取り込み・CI登録・検証状況は [統合記録](integration/README.md) を参照。

作成日：2026-09-16。設計は [DESIGN.md](DESIGN.md)、結果は [REPORT.md](REPORT.md)。
本書は統合担当が candidate patch を main へ取り込み、hosted 入口を登録するための情報を列挙する。
commit / PR / hosted 実行は本 worktree では行っていない。

## 1. patch の取り込み

- base：`6c41a03888b66bb6781e5ef39c253b6200ff44c6`。patch は [records/candidate.patch](records/candidate.patch)
  （`git apply` 可、SHA-256 は [records/candidate.patch.sha256](records/candidate.patch.sha256)）。
- 変更ファイル（production 4、test 2、fixture 4、observer 1、docs）：

| path | 種別 | 内容 |
| --- | --- | --- |
| `crates/emitter/src/factory.rs` | production | `create_template_literal_like_node`（flags）、`update_template_literal_like_node`、`create_string_literal_node`、`update_string_literal`、`TransformArena::reconcile_literal_properties`、`update_node` の literal 整合 |
| `crates/emitter/src/metadata.rs` | production | `LiteralNodeProperties::clear_raw_template_text`（crate 内） |
| `crates/emitter/src/builtins/tagged_template.rs` | production | `raw_from_source_range`（getRawLiteral の source fallback）、typed error の廃止 |
| `crates/emitter/src/builtins/relative_imports.rs` | production | `rewrite_literal` を `update_string_literal(node, updated, node.singleQuote, None)` へ |
| `crates/emitter/tests/literal_update_contract.rs` | test | factory / transform / lifetime（3 tests） |
| `crates/compiler/tests/literal_update_pipeline_contract.rs` | test | 22 complete command ×2（1 test） |
| `crates/emitter/tests/fixtures/literal-update-{factory,transform,lifetime}.json`、`crates/compiler/tests/fixtures/literal-update-pipeline.json` | fixture | 凍結 expected |
| `scripts/observe-literal-update.mjs` | observer | 4 group、`--list`/`--write`/`--check` |
| `docs/design/greenfield/slices/h2-8a-literal-update/` | docs | 本 slice の設計・報告・記録 |

- 公開 API の追加は `NodeFactory` の 4 関数のみ（既存関数の signature 変更なし）。`emitter/src/lib.rs` の re-export は不要。
- 既存の凍結 expected は変更していない。既存 test で挙動が変わるのは、raw channel の無い synthetic fragment を
  tagged-template lowering に通した場合（typed error → upstream の source-slice / ""）だけで、該当する既存 test は無い
  （emitter lib 506、contracts、compiler UTF-16 3 suite で確認、REPORT §5）。

## 2. hosted 入口（未登録：統合担当が `scripts/witness.py` と `.github/ci/replay.py` に追加）

| suite 名（案） | job | target | fixture（rows, id key） | observer | tests | 想定時間 |
| --- | --- | --- | --- | --- | --- | --- |
| `literal-update` | printer（EMITTER_DIRECT） | `literal_update_contract` | `literal-update-factory.json`（987, `case_id`）、`literal-update-transform.json`（399, `case_id`）、`literal-update-lifetime.json`（10, `case_id`） | `scripts/observe-literal-update.mjs factory --check`、`… transform --check`、`… lifetime --check` | 3 | observer 約 35 s（transform は 120 Program 構築を含む）、Cargo replay 約 0.6 s（build 済み） |
| `literal-update-pipeline` | controls（COMPILER_DIRECT、pinned Node 25.2.1） | `literal_update_pipeline_contract` | `literal-update-pipeline.json`（22, `case_id`） | `scripts/observe-literal-update.mjs pipeline --check` | 1（unfiltered） | observer 約 25 s、Cargo replay 約 22 s |

登録上の注意：

- `EMITTER_DIRECT` の observer は現在 `["node", path, "--check"]` 形式（文字列のみ）。本 observer は group 引数を取るので、
  `COMPILER_DIRECT` と同じ `(script, group)` tuple を `run_emitter_direct` / `emitter_inputs` でも受け付けるようにする
  （`compiler_direct_observers` と同じ dedup）。あるいは 3 本の薄い wrapper script を置く。
- 同一 observer が 4 group を持つため、`emitter_inputs` / `compiler_direct_inputs` の所有 path に `scripts/observe-literal-update.mjs` を
  両 suite で含める（planner は両 suite を選択する。想定どおり）。
- `inputs.v1.json`（docs 配下）は実行入力ではない（件数と ID の固定記録）。docs-only skip の対象で問題ない。
- transform group の observer は `ignoreDeprecations: "6.0"` を使う（TS5107 を避けるため。emit は不変）。pipeline group は
  ES5 の TS5107 を含む完全 command をそのまま比較する（既存 utf16-literal-witnesses と同じ扱い）。
- `cargo xtask acceptance` の対象には含めない。

## 3. 統合時の確認項目

1. patch 適用後：`cargo fmt --all -- --check`、emitter clippy の指摘が変更 file・新規 test に増えていない（本 worktree：lib＋tests 168 指摘、すべて既存 file。REPORT §5）。
2. `node scripts/observe-literal-update.mjs <group> --check` ×4（byte 一致）。
3. 新規 4 tests ＋ REPORT §5 の focused 集合。
4. hosted：printer job（literal-update）と controls job（literal-update-pipeline）の登録後、PR head・run URL・件数を記録。
   共通実装（`factory.rs`、`tagged_template.rs`、`relative_imports.rs`）の変更は planner が全関連 coverage を選ぶ（想定どおり）。

## 4. 残件（本 slice の外）

| 項目 | owner |
| --- | --- |
| `set_literal_value` の存廃（Rust-only in-place seam、test 2 件） | A-INT1 |
| dispose 後の synthetic node emit metadata（upstream 保持 / Rust 全消去）の session model | 後続 API 設計（custom transform admission 時） |
| checker node builder の template/literal 生成の対照（差の根拠なし） | H2.7c / 宣言 emit owner |
| templateFlags を持つ synthetic template producer | upstream にも無し。追加 producer が現れたときに `create_template_literal_like_node` を使う |
