# C01 / A40-LITERAL-UPDATE：結果報告（before / after、focused 隣接回帰、提出物）

提出時点の記録。現在の取り込み・CI登録・検証状況は [統合記録](integration/README.md) を参照。

作成日：2026-09-16。base `6c41a03888b66bb6781e5ef39c253b6200ff44c6`（origin/main）。
設計は [DESIGN.md](DESIGN.md)、統合仕様は [INTEGRATION.md](INTEGRATION.md)。
本報告は隔離候補の提出であり、production admission・accepted profile の変更を報告しない。
commit / PR / hosted 登録は統合担当が行う（本 worktree は未 commit）。

## 1. 提出物

| 種別 | path |
| --- | --- |
| candidate patch | [records/candidate.patch](records/candidate.patch)（`git diff` ＋ untracked、SHA-256 は [records/candidate.patch.sha256](records/candidate.patch.sha256)） |
| production 変更 | `crates/emitter/src/factory.rs`（typed constructor / typed update / generic `update_node` の property 整合）、`crates/emitter/src/metadata.rs`（`clear_raw_template_text`）、`crates/emitter/src/builtins/tagged_template.rs`（getRawLiteral の source-slice fallback）、`crates/emitter/src/builtins/relative_imports.rs`（`update_string_literal` へ移行） |
| observer | `scripts/observe-literal-update.mjs`（`<group> --list\|--write\|--check`） |
| 入力 manifest | [inputs.v1.json](inputs.v1.json)（1,418 ID、group 別件数、observer SHA-256） |
| 凍結 expected | `crates/emitter/tests/fixtures/literal-update-factory.json`（987）、`…-transform.json`（399）、`…-lifetime.json`（10）、`crates/compiler/tests/fixtures/literal-update-pipeline.json`（22） |
| Rust target | `crates/emitter/tests/literal_update_contract.rs`（3 tests）、`crates/compiler/tests/literal_update_pipeline_contract.rs`（1 test） |
| before / after 記録 | [records/before/](records/before/)、[records/after/](records/after/)（route 別 JSON、log、argv/env/exit、binary SHA-256） |
| observer 記録 | [records/observer/](records/observer/)（各 group の `--write` と別 process `--check` の stdout） |
| 開始記録 | [records/start.txt](records/start.txt) |

focused 入口（0 tests 不可。`TSC_RS_LITERAL_UPDATE_REPORT_DIR` を与えると route 別 JSON を書く）：

```sh
for g in factory transform lifetime pipeline; do node scripts/observe-literal-update.mjs $g --check; done
taskpolicy -b nice -n 15 env CARGO_BUILD_JOBS=2 cargo test --offline \
  --manifest-path crates/emitter/Cargo.toml --test literal_update_contract -- --test-threads=1
taskpolicy -b nice -n 15 env CARGO_BUILD_JOBS=2 cargo test --offline \
  --manifest-path crates/compiler/Cargo.toml --test literal_update_pipeline_contract -- --test-threads=1
```

## 2. before → after（同一 fixture、同一 binary 条件、各 row 2 回実行）

Rust の route：`generic` = `NodeFactory::update_node`（payload 差し替え）、`typed` = 本 slice の
`update_template_literal_like_node` / `update_string_literal`。`n/a` は route が表現できない row
（generic：template flags、quote、孤立 surrogate を含む raw）で、一致には数えない。

| group | route | before exact | before 差 / error / n/a | after exact | after 差 / error / n/a |
| --- | --- | ---: | --- | ---: | --- |
| factory（987） | generic | 605 | 177 差 / 0 / 205 | **782** | 0 / 0 / 205 |
| factory（987） | typed | API 不在 | — | **987** | 0 / 0 / 0 |
| transform（399） | generic | 303 | 0 / 12 error / 84 | **315** | 0 / 0 / 84 |
| transform（399） | typed | API 不在 | — | **399** | 0 / 0 / 0 |
| lifetime（10） | — | 10（parsed 5 行の emit flags 一致、property 生存 10/10） | synthetic 5 行の dispose 後 emit flags は既知差（§4） | 同じ | 同じ |
| pipeline（22 complete command） | — | **22** | 0 | **22** | 0 |

before の 177 差の内訳（[records/before/factory-generic.json](records/before/factory-generic.json)）：

| 差 | 行数 | 原因（owner） | 修復 |
| --- | ---: | --- | --- |
| `updated.raw_text_utf16` ＋ printed（raw / raw-absent / raw-empty on synthetic・clone・set-original・cross-source・synthetic-raw-empty） | 144 | `update_node` の clone が旧 owned raw を運び、projection だけ更新する | `TransformArena::reconcile_literal_properties`：projection 変更で owned raw を追従（None は消去） |
| `updated.raw_text_utf16` のみ | 12 | 同上（印字が偶然一致する row） | 同上 |
| `updated.has_extended_unicode_escape`（StringLiteral parsed cooked） | 14 | generic payload が原 node の marker を運ぶ（旧 `rewrite_literal` と同じ形）。upstream の `createStringLiteral(after, node.singleQuote)` は undefined | 対照の generic payload を constructor 引数どおり `None` にし、production caller は typed update へ移行 |
| `updated.text_source` ＋ printed（text-source cooked） | 7 | clone が textSourceNode を運び、新しい値ではなく identifier の綴りを印字 | `reconcile_literal_properties`：値変更で textSourceNode を消す |

before の transform 12 error：`RequiredChildRemoved { field: "template literal raw text" }`
（tagged-spans / tagged-nosub × raw-absent × es5 全 5 値 ＋ es2015 invalid）。upstream は `setTextRange`
済み fresh fragment の raw を source slice から復元して `__makeTemplateObject` を出す。
`raw_from_source_range` で一致（after 0 error）。

n/a の内訳（after も同数）：factory generic 205 = raw 144（孤立 surrogate を含む after 値）＋ flags 48 ＋ quote 13；
transform generic 84 = flags 66 ＋ raw 18。すべて typed route が 100% 一致。

## 3. after の観測一致（route 別、2 回一致）

- factory typed 987/987：origin 別 exact = synthetic 217×4 kind、parsed、clone、set-original、synthetic-raw-absent/empty、cross-source（Rust seam、cloneNode semantics と比較）、text-source、node-no-ascii policy を含む。
  identity は `same` 全行 true、他は false。`updated.original` は "node"、`pos/end` は原 node の range、NodeFlags 16、transform flags は
  `createTemplateLiteralLikeNode` / `createStringLiteral` の規則、emit flags は setOriginalNode の merge。
- transform typed 399/399：ES5（All lowering、cooked/raw 配列、`void 0`、CRLF→LF、templateObject temp）、ES2015（LiftRestriction：invalid のみ lowering）、
  ESNext（printer：raw verbatim、raw absent は escaped cooked）、children（`z` 置換、head/literal identity）、module 48 行。
- pipeline 22/22：ES2020 `??` の span 内 lowering（updateTemplateSpan / updateTemplateExpression / updateTaggedTemplateExpression の子のみ更新）、
  nested tag、invalid escape、CRLF raw、rewriteRelativeImportExtensions（`'./x.ts'`、`"./y\u{79}.ts"`、`export *`、`import()`、ESNext / CommonJS × ES5 / ES2015）。
  JS / d.ts / map の bytes・path・順序、callback metadata、diagnostics（ES5 の TS5107 を含む）、emit result、status、exit を比較。

## 4. 変わらないことが正しい経路（根拠と対照）

| 経路 | 根拠 | 対照 |
| --- | --- | --- |
| printer `emit_template_literal_token`（owned raw → projection → escaped cooked） | getLiteralText 13647-13688 | factory 全 row の printed、transform esnext |
| `create_template_cooked` / `get_raw_literal` の IsInvalid・CRLF | 94019-94032 | transform es5 / es2015、pipeline |
| es2015 `visit_template_literal` / `visit_template_expression`（cooked のみ） | 107912-107952 | transform untagged es5、pipeline es5 |
| `clone_node` / `set_original_node` | 24436-24466 / 25208-25277 | factory clone / set-original / node-no-ascii |
| 子のみ更新の identity | 22840-22842、23037-23039、22653-22655 | pipeline children、transform same/children |
| `set_literal_value`（Rust-only in-place） | upstream に対応物なし | 変更せず。factory `cooked` は unchecked constructor で対照 |
| dispose 後の synthetic emitNode | upstream `disposeEmitNodes` は parse-tree node のみ消去；Rust は session metadata を全消去 | lifetime：parsed 5 行一致、synthetic 5 行は記録（Rust-only session model、修復対象外）。dispose 後の print は Rust では typed refusal `InvalidLifecycle` |

## 5. focused 隣接回帰（最終 bytes、変更 owner の集合）

| 集合 | コマンド | 結果 |
| --- | --- | --- |
| emitter direct（literal/printer owner） | `cargo test --manifest-path crates/emitter/Cargo.toml --test literal_value_provenance_contract --test utf16_writer_contract --test utf16_literal_escaping_contract --test literal_parent_provenance_contract --test string_literal_identifier_source_contract --test decorator_super_direct_contract` | 9 tests pass（[records/after/emitter-adjacent.log](records/after/emitter-adjacent.log)） |
| emitter lib unit（factory seams、template_flags、tagged template） | `cargo test --manifest-path crates/emitter/Cargo.toml --lib` | 506 pass |
| emitter contracts | `cargo test --manifest-path crates/emitter/Cargo.toml --test contracts` | 451 pass / 1 fail：`active_transform_contract::compact_private_function_body_emits_inter_statement_comment_once`（compact private body の comment 二重出力）は無変更の main head `6c41a0388` でも同一 assert で fail（[records/after/baseline-main-contracts-one.log](records/after/baseline-main-contracts-one.log)、別 target dir でビルド）。本 slice と無関係の inherited failure |
| compiler UTF-16 tagged template（16 complete command ×2） | `python3 scripts/witness.py utf16-tagged-template --all` | pass（1 test、observer 10.4 s、Cargo build＋replay 127.8 s、[records/after/adjacent-utf16-tagged-template.log](records/after/adjacent-utf16-tagged-template.log)） |
| compiler UTF-16 literal witnesses（64 complete command ×2） | `python3 scripts/witness.py utf16-literal-witnesses --all` | pass（observer `--check` 3 group ＋ 1 test：observer 38.5 s、Cargo build＋replay 61.4 s、[records/after/adjacent-utf16-literal-witnesses.log](records/after/adjacent-utf16-literal-witnesses.log)） |
| require / import rewrite（60 + 6 + composition/substitution） | `cargo test --manifest-path crates/compiler/Cargo.toml --test h2_8a_require_rewrite` | 14 tests pass（focused 60、composition、substitution、dynamic 6、original ＋ 共有 `#[path]` module の w4a / declaration-blocking；822.7 s、[records/after/adjacent-require-rewrite.log](records/after/adjacent-require-rewrite.log)） |
| fmt / clippy | `cargo fmt --all -- --check`；`cargo clippy -p tsc-rs-emitter --lib --tests`、`cargo clippy -p tsc-rs-compiler --test literal_update_pipeline_contract` | fmt 差分なし；emitter clippy（lib＋tests）168 指摘はすべて既存 file（変更 4 file・新規 test に指摘なし、[records/after/clippy-emitter.log.gz](records/after/clippy-emitter.log.gz)）；compiler 新規 test に指摘なし（[records/after/clippy-compiler.log.gz](records/after/clippy-compiler.log.gz)） |

530 / 672 等の全件 replay と全 acceptance はローカルで実行していない（[共通手順](../claude-high-difficulty-handoffs.md)の分担）。
hosted への登録・実行は [INTEGRATION.md](INTEGRATION.md) §2。

## 6. Rust-only control と upstream 例外の分離

- Rust-only typed error：dispose 後の standalone print（`InvalidLifecycle`）。TypeScript 互換性には加点しない。
- upstream 例外：本 slice の 1,418 ID に upstream 例外は無い（transform group は pre-emit diagnostics 0 を assert、pipeline は診断込みで比較）。
- 旧 typed error（raw channel 無しの fragment）は廃止し、upstream の定義済み動作（source slice、range 無しは ""）に合わせた。

## 7. 未完了・境界

1. hosted CI 入口の登録と実行（統合担当。[INTEGRATION.md](INTEGRATION.md) §2 に target / path / 件数 / 想定時間）。
2. `set_literal_value` は Rust-only seam のまま（production caller なし、test 2 件）。廃止するかは統合側の判断。
3. dispose 後の synthetic node emit metadata の保持（upstream）と Rust session model の差は記録のみ。
4. checker の node builder（`create_template_head` の projection コピー、raw 無し template type）は差の根拠がなく対象外。
5. templateFlags を持つ synthetic template の production producer は upstream にも無い（typed constructor は typed update の忠実性のために追加）。
