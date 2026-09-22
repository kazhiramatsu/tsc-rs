#!/bin/zsh
R=/private/tmp/claude-501/-Users-hiramatsu-dev-tsc-rs/01907deb-0413-43b0-9dc0-4f5a56bb8a5f/scratchpad/runlog.sh
D=/Users/hiramatsu/dev/tsc-rs-emitter-final/docs/design/greenfield/slices/emitter-final-batch/records
S=/private/tmp/claude-501/-Users-hiramatsu-dev-tsc-rs/01907deb-0413-43b0-9dc0-4f5a56bb8a5f/scratchpad
cd /Users/hiramatsu/dev/tsc-rs-emitter-final
step() { name="$1"; shift; "$R" "$@" ; echo "STEP $name exit=$?" ; }
step build-cli "$D/measure" build-cli-r5 -- cargo build -p tsc-rs-compiler --bin tsc-rs
for probe in probe2/B probe3/legacy-bound-this probe3/escaped probe3/nested-computed probe3/concise-arrow probe6/dec probe7/read-comment probe7/async-gen-super; do
  d="$S/$probe/rs"; rm -rf "$d/out"; (cd "$d" && /Users/hiramatsu/dev/tsc-rs-emitter-final/target/debug/tsc-rs -p tsconfig.json > rs.log 2>&1; echo "rs exit $?" >> rs.log)
  echo "=== $probe ==="; diff "$S/$probe/ts/out/main.js" "$d/out/main.js" > "$d/diff-r5.txt" && echo IDENTICAL || head -14 "$d/diff-r5.txt"
done
rm -rf "$S/capture/ef2-ef3-r5"; mkdir -p "$S/capture/ef2-ef3-r5"
step ef2-ef3-r5 "$D/measure" ef2-ef3-rows-20-r5 TSC_RS_EMITTER_FINAL_CAPTURE_DIR=$S/capture/ef2-ef3-r5 -- cargo test -p tsc-rs-compiler --test emitter_final_rows -- --nocapture --test-threads=1
step ef4-ef5-r5 "$D/measure" ef4-ef5-class-40-r5 -- cargo test -p tsc-rs-compiler --test emitter_final_batch ef4_ef5_historical_class_rows_match_complete_typescript_observations -- --exact --nocapture --test-threads=1
step emitter-lib "$D/measure" emitter-lib-r5 -- cargo test -p tsc-rs-emitter --lib
step emitter-contracts "$D/measure" emitter-contracts-r5 -- cargo test -p tsc-rs-emitter --test contracts
step printer "$D/measure" printer-all-r5 -- python3 scripts/witness.py printer --all
step compact-body "$D/measure" compact-body-comments-all-r5 -- python3 scripts/witness.py compact-body-comments --all
step direct "$D/measure" decorator-binding-all-r5 -- python3 scripts/witness.py decorator-binding --all
echo CHAIN6 DONE
