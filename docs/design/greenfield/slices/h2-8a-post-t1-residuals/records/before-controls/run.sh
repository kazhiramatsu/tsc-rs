#!/bin/bash
set -u
cd /Users/hiramatsu/dev/tsc-rs-post-t1-residuals
OUT="$1"; DUMP="$2"
export CARGO_BUILD_JOBS=2
{
  echo "date: $(date -u +%Y-%m-%dT%H:%M:%SZ)"; echo "head: $(git rev-parse HEAD)"; echo "status:"; git status --short
  echo "inputs:"; shasum -a 256 crates/compiler/tests/fixtures/post-t1-residuals-inputs.json crates/compiler/tests/fixtures/post-t1-residuals.json crates/compiler/tests/fixtures/post-t1-residuals-known-native.json crates/compiler/tests/fixtures/post-t1-residuals-known-packet.json scripts/observe-post-t1-residuals.mjs scripts/generate-post-t1-residuals-inputs.mjs
} > "$OUT/meta.txt" 2>&1
run() {
  local name="$1"; shift
  echo "argv: $*" > "$OUT/$name.meta"; echo "env: CARGO_BUILD_JOBS=2 (taskpolicy -b nice -n 15) ${EXTRA_ENV:-}" >> "$OUT/$name.meta"
  local start=$(date +%s)
  taskpolicy -b nice -n 15 "$@" > "$OUT/$name.log" 2>&1
  local code=$?
  echo "exit: $code" >> "$OUT/$name.meta"; echo "seconds: $(( $(date +%s) - start ))" >> "$OUT/$name.meta"
}
# 1. canonical runner (observer --check = second process-level observation, then both tests)
run witness python3 scripts/witness.py post-t1-residuals --all
# 2. direct replay with native dumps (no oracle): argv/env recorded, for analysis only
EXTRA_ENV="TSC_RS_POST_T1_RESIDUALS_DUMP_DIR=$DUMP" run dump env TSC_RS_POST_T1_RESIDUALS_DUMP_DIR="$DUMP" cargo test --manifest-path crates/compiler/Cargo.toml --test post_t1_residuals_contract post_t1_residuals_controls_match_complete_typescript_observations -- --exact post_t1_residuals_parsed_packet_matches_typescript_after_javascript_probe --nocapture --test-threads=1
echo "binaries:" >> "$OUT/meta.txt"; ls -1 target/debug/deps/post_t1_residuals_contract-* | grep -v '\.d$' | xargs shasum -a 256 >> "$OUT/meta.txt"; echo done >> "$OUT/meta.txt"
