before (start head 6c41a03888b66bb6781e5ef39c253b6200ff44c6, production tree unmodified)
argv: cargo test --offline --manifest-path crates/emitter/Cargo.toml --test literal_update_contract -- --test-threads=1
env: TSC_RS_LITERAL_UPDATE_REPORT_DIR=<records/before> CARGO_BUILD_JOBS=2 (taskpolicy -b nice -n 15)
exit: 101
argv: cargo test --offline --manifest-path crates/compiler/Cargo.toml --test literal_update_pipeline_contract -- --test-threads=1
exit: 0
binary sha256 (emitter test): 99bdd4904f010d545e4a5978ddb20b85d242702e2400ee9451a98d021d9a4acd
binary sha256 (compiler test): 06833531d7c29aefde1b7ccf677ce011daeb39c3de0b7cbb67242cbf942af888
