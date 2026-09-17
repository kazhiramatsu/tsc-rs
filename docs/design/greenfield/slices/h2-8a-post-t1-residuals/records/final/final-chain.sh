#!/bin/bash
# Final-bytes verification chain (sequential, one heavy run at a time).
set -u
cd /Users/hiramatsu/dev/tsc-rs-post-t1-residuals
OUT="$1"; mkdir -p "$OUT"
export CARGO_BUILD_JOBS=2
{
  echo "date: $(date -u +%Y-%m-%dT%H:%M:%SZ)"; echo "head: $(git rev-parse HEAD)"; echo "status:"; git status --short
  echo "toolchain: $(rustc --version) / $(cargo --version) / node $(node --version)"
  echo "vendor:"; shasum -a 256 vendor/typescript-6.0.3/lib/_tsc.js vendor/typescript-6.0.3/lib/typescript.js
  echo "inputs:"; shasum -a 256 crates/compiler/tests/fixtures/decorator-binding-inputs.json crates/compiler/tests/fixtures/decorator-binding.json.zst crates/compiler/tests/fixtures/decorator-binding-known-native.json crates/compiler/tests/fixtures/bundle-metadata-t1-inputs.json crates/compiler/tests/fixtures/bundle-metadata-t1.json crates/compiler/tests/fixtures/bundle-metadata-t1-known-native.json crates/compiler/tests/fixtures/bundle-metadata-t1-known-packet.json crates/compiler/tests/fixtures/post-t1-residuals-inputs.json crates/compiler/tests/fixtures/post-t1-residuals.json crates/compiler/tests/fixtures/post-t1-residuals-known-native.json crates/compiler/tests/fixtures/post-t1-residuals-known-packet.json
} > "$OUT/meta.txt" 2>&1
run() {
  local name="$1"; shift
  echo "argv: $*" > "$OUT/$name.meta"; echo "env: CARGO_BUILD_JOBS=2 (taskpolicy -b nice -n 15)" >> "$OUT/$name.meta"
  local start=$(date +%s)
  taskpolicy -b nice -n 15 "$@" > "$OUT/$name.log" 2>&1
  local code=$?
  echo "exit: $code" >> "$OUT/$name.meta"; echo "seconds: $(( $(date +%s) - start ))" >> "$OUT/$name.meta"
  echo "== $name exit $code ($(( $(date +%s) - start ))s)"
}
run fmt-check cargo fmt --all -- --check
run planner python3 -m unittest discover -s .github/ci -p test_replay.py
run post-t1-residuals python3 scripts/witness.py post-t1-residuals --all
run bundle-metadata-t1 python3 scripts/witness.py bundle-metadata-t1 --all
run pipeline-targets python3 scripts/witness.py decorator-binding-pipeline \
  --case decorator-binding/global/esnext/define/script-let-default_1 \
  --case decorator-binding/lifecycle/esnext/define/bundle-default-two-files \
  --case decorator-binding/lifecycle/esnext/define/bundle-computed-temps-two-files \
  --case decorator-binding/lifecycle/esnext/define/bundle-file-level-then-scoped-across-files
run emitter-units cargo test --manifest-path crates/emitter/Cargo.toml --lib
run decorator-binding-direct python3 scripts/witness.py decorator-binding --all
run printer python3 scripts/witness.py printer --all
run compact-body-comments python3 scripts/witness.py compact-body-comments --all
run prologue-comments python3 scripts/witness.py prologue-comments --all
run declaration-comments python3 scripts/witness.py declaration-comments --all
run bundle-declarations python3 scripts/witness.py bundle-declarations --all
run bundle-program python3 scripts/witness.py bundle-program --all
run module-identities python3 scripts/witness.py module-identities --all
run bundle-original-javascript python3 scripts/witness.py bundle-original-javascript --all
run retained-subset python3 scripts/witness.py retained --case decorator-receiver-context/ --case decorator-static-accessor-handoff/ --case retained-accessor-followup/ --case retained-comma-factory/
run pipeline-subset python3 scripts/witness.py decorator-binding-pipeline --case /lifecycle/ --case /global/esnext/ --case /nested/esnext/define/
run clippy-emitter cargo clippy --manifest-path crates/emitter/Cargo.toml --all-targets -- -D warnings
run git-diff-check git diff --check
echo "binaries:" >> "$OUT/meta.txt"; ls -1 target/debug/deps/post_t1_residuals_contract-* target/debug/deps/bundle_metadata_t1_contract-* target/debug/deps/decorator_binding_pipeline_contract-* 2>/dev/null | grep -v '\.d$' | xargs shasum -a 256 >> "$OUT/meta.txt"
echo done >> "$OUT/meta.txt"
