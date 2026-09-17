#!/bin/bash
# Start-SHA (before) measurements with the canonical runners.
set -u
cd /Users/hiramatsu/dev/tsc-rs-post-t1-residuals
OUT="$1"
{
  echo "date: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "head: $(git rev-parse HEAD)"
  echo "status:"; git status --short
  echo "toolchain: $(rustc --version) / $(cargo --version) / node $(node --version)"
  echo "vendor:"; shasum -a 256 vendor/typescript-6.0.3/lib/_tsc.js vendor/typescript-6.0.3/lib/typescript.js
  echo "inputs:"; shasum -a 256 crates/compiler/tests/fixtures/decorator-binding-inputs.json crates/compiler/tests/fixtures/decorator-binding.json.zst crates/compiler/tests/fixtures/decorator-binding-known-native.json crates/compiler/tests/fixtures/bundle-metadata-t1-inputs.json crates/compiler/tests/fixtures/bundle-metadata-t1.json crates/compiler/tests/fixtures/bundle-metadata-t1-known-native.json crates/compiler/tests/fixtures/bundle-metadata-t1-known-packet.json
} > "$OUT/meta.txt" 2>&1
export CARGO_BUILD_JOBS=2
run() {
  local name="$1"; shift
  echo "argv: $*" > "$OUT/$name.meta"
  echo "env: CARGO_BUILD_JOBS=2 (taskpolicy -b nice -n 15)" >> "$OUT/$name.meta"
  local start=$(date +%s)
  taskpolicy -b nice -n 15 "$@" > "$OUT/$name.log" 2>&1
  local code=$?
  echo "exit: $code" >> "$OUT/$name.meta"
  echo "seconds: $(( $(date +%s) - start ))" >> "$OUT/$name.meta"
}
run t1-witness python3 scripts/witness.py bundle-metadata-t1 --all
run pipeline-witness python3 scripts/witness.py decorator-binding-pipeline \
  --case decorator-binding/global/esnext/define/script-let-default_1 \
  --case decorator-binding/lifecycle/esnext/define/bundle-default-two-files \
  --case decorator-binding/lifecycle/esnext/define/bundle-computed-temps-two-files \
  --case decorator-binding/lifecycle/esnext/define/bundle-file-level-then-scoped-across-files
echo "binaries:" >> "$OUT/meta.txt"
ls -1 target/debug/deps/bundle_metadata_t1_contract-* target/debug/deps/decorator_binding_pipeline_contract-* 2>/dev/null | grep -v '\.d$' | xargs shasum -a 256 >> "$OUT/meta.txt"
echo "done" >> "$OUT/meta.txt"
