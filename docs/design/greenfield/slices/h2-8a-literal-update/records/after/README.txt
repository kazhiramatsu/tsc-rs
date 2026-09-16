after (final bytes; production tree modified by the candidate patch)
date: 2026-09-16T07:54:39Z
common env: CARGO_BUILD_JOBS=2, taskpolicy -b nice -n 15, --offline

[emitter literal_update_contract] argv: cargo test --offline --manifest-path crates/emitter/Cargo.toml --test literal_update_contract -- --test-threads=1
  env: TSC_RS_LITERAL_UPDATE_REPORT_DIR=<records/after>
  exit: 0 (3 passed) — emitter-test.log; route reports factory-{generic,typed}.json transform-{generic,typed}.json lifetime.json
  binary sha256: 793b09037c20b783d89e5e900ddd29940ee0550bd944ca065e59177c616c3ef5

[compiler literal_update_pipeline_contract] argv: cargo test --offline --manifest-path crates/compiler/Cargo.toml --test literal_update_pipeline_contract -- --test-threads=1
  env: TSC_RS_LITERAL_UPDATE_REPORT_DIR=<records/after>
  exit: 0 (1 passed, 22/22 exact x2) — compiler-pipeline-test.log, pipeline.json
  binary sha256: ebe00cf29df9c630a9638f1e6d8dd90f3ed458f84d41763c34ee6a7bd3990df2

[emitter adjacent] argv: cargo test --offline --manifest-path crates/emitter/Cargo.toml --test literal_value_provenance_contract --test utf16_writer_contract --test utf16_literal_escaping_contract --test literal_parent_provenance_contract --test string_literal_identifier_source_contract --test decorator_super_direct_contract -- --test-threads=1
  exit: 0 (9 passed) — emitter-adjacent.log

[emitter lib] argv: cargo test --offline --manifest-path crates/emitter/Cargo.toml --lib -- --test-threads=2
  exit: 0 (506 passed) — emitter-lib.log

[emitter contracts] argv: cargo test --offline --manifest-path crates/emitter/Cargo.toml --test contracts -- --test-threads=2
  exit: 101 (451 passed, 1 failed: active_transform_contract::compact_private_function_body_emits_inter_statement_comment_once) — emitter-contracts.log.gz
  classification: INHERITED — same failure on unmodified main head 6c41a0388 (worktree ~/dev/tsc-rs-printer-hook-hints, CARGO_TARGET_DIR=<scratch>/baseline-target): baseline-main-contracts-one.log, exit 101

[compiler utf16-tagged-template] argv: python3 scripts/witness.py utf16-tagged-template --all
  exit: 0 — adjacent-utf16-tagged-template.log

[compiler utf16-literal-witnesses] argv: python3 scripts/witness.py utf16-literal-witnesses --all
  exit: 0 — adjacent-utf16-literal-witnesses.log

[compiler h2_8a_require_rewrite] argv: cargo test --offline --manifest-path crates/compiler/Cargo.toml --test h2_8a_require_rewrite -- --test-threads=1
  exit: 0 (14 passed, 822.69s) — adjacent-require-rewrite.log

[clippy] argv: cargo clippy --offline -p tsc-rs-emitter --lib --tests ; cargo clippy --offline -p tsc-rs-compiler --test literal_update_pipeline_contract
  exit: 0 / 0 — clippy-emitter.log.gz (168 warnings, none in changed or new files), clippy-compiler.log.gz (no warnings in the new test)

[fmt] argv: cargo fmt --all -- --check ; git diff --check
  exit: 0 / 0
