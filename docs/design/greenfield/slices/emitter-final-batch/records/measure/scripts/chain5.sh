#!/bin/zsh
R=/private/tmp/claude-501/-Users-hiramatsu-dev-tsc-rs/01907deb-0413-43b0-9dc0-4f5a56bb8a5f/scratchpad/runlog.sh
D=/Users/hiramatsu/dev/tsc-rs-emitter-final/docs/design/greenfield/slices/emitter-final-batch/records
S=/private/tmp/claude-501/-Users-hiramatsu-dev-tsc-rs/01907deb-0413-43b0-9dc0-4f5a56bb8a5f/scratchpad
cd /Users/hiramatsu/dev/tsc-rs-emitter-final
step() { name="$1"; shift; "$R" "$@" ; echo "STEP $name exit=$?" ; }
step build-cli "$D/measure" build-cli-r4 -- cargo build -p tsc-rs-compiler --bin tsc-rs
for probe in probe2/B probe3/legacy-bound-this probe7/read-comment probe7/arrow-paren probe7/async-gen-super; do
  d="$S/$probe/rs"; rm -rf "$d/out"; (cd "$d" && /Users/hiramatsu/dev/tsc-rs-emitter-final/target/debug/tsc-rs -p tsconfig.json > rs.log 2>&1; echo "rs exit $?" >> rs.log)
  echo "=== $probe ==="; diff "$S/$probe/ts/out/main.js" "$d/out/main.js" > "$d/diff-r4.txt" && echo IDENTICAL || head -16 "$d/diff-r4.txt"
done
rm -rf "$S/capture/ef2-ef3-r4"; mkdir -p "$S/capture/ef2-ef3-r4"
step ef2-ef3-r4 "$D/measure" ef2-ef3-rows-20-r4 TSC_RS_EMITTER_FINAL_CAPTURE_DIR=$S/capture/ef2-ef3-r4 -- cargo test -p tsc-rs-compiler --test emitter_final_rows -- --nocapture --test-threads=1
step ef4-ef5-r4 "$D/measure" ef4-ef5-class-40-r4 -- cargo test -p tsc-rs-compiler --test emitter_final_batch ef4_ef5_historical_class_rows_match_complete_typescript_observations -- --exact --nocapture --test-threads=1
step emitter-lib "$D/measure" emitter-lib-r4 -- cargo test -p tsc-rs-emitter --lib
step emitter-contracts "$D/measure" emitter-contracts-r4 -- cargo test -p tsc-rs-emitter --test contracts
echo CHAIN5 DONE
