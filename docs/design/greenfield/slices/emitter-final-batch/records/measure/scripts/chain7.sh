#!/bin/zsh
R=/private/tmp/claude-501/-Users-hiramatsu-dev-tsc-rs/01907deb-0413-43b0-9dc0-4f5a56bb8a5f/scratchpad/runlog.sh
D=/Users/hiramatsu/dev/tsc-rs-emitter-final/docs/design/greenfield/slices/emitter-final-batch/records
S=/private/tmp/claude-501/-Users-hiramatsu-dev-tsc-rs/01907deb-0413-43b0-9dc0-4f5a56bb8a5f/scratchpad
cd /Users/hiramatsu/dev/tsc-rs-emitter-final
step() { name="$1"; shift; "$R" "$@" ; echo "STEP $name exit=$?" ; }
step build-cli "$D/measure" build-cli-r6 -- cargo build -p tsc-rs-compiler --bin tsc-rs
for probe in probe2/B probe3/legacy-bound-this probe3/escaped probe3/nested-computed probe3/concise-arrow probe3/direct-escaped probe6/dec probe7/read-comment probe7/async-gen-super; do
  d="$S/$probe/rs"; rm -rf "$d/out"; (cd "$d" && /Users/hiramatsu/dev/tsc-rs-emitter-final/target/debug/tsc-rs -p tsconfig.json > rs.log 2>&1; echo "rs exit $?" >> rs.log)
  echo "=== $probe ==="; diff "$S/$probe/ts/out/main.js" "$d/out/main.js" > "$d/diff-r6.txt" && echo IDENTICAL || head -14 "$d/diff-r6.txt"
done
for c in ef2-ef3-r6 ef4-ef5-r6 ef6-r6; do rm -rf "$S/capture/$c"; mkdir -p "$S/capture/$c"; done
step ef2-ef3-r6 "$D/measure" ef2-ef3-rows-20-r6 TSC_RS_EMITTER_FINAL_CAPTURE_DIR=$S/capture/ef2-ef3-r6 -- cargo test -p tsc-rs-compiler --test emitter_final_rows -- --nocapture --test-threads=1
step ef4-ef5-r6 "$D/measure" ef4-ef5-class-40-r6 TSC_RS_EMITTER_FINAL_CAPTURE_DIR=$S/capture/ef4-ef5-r6 -- cargo test -p tsc-rs-compiler --test emitter_final_batch ef4_ef5_historical_class_rows_match_complete_typescript_observations -- --exact --nocapture --test-threads=1
step ef6-r6 "$D/measure" ef6-global-14-r6 TSC_RS_EMITTER_FINAL_CAPTURE_DIR=$S/capture/ef6-r6 -- cargo test -p tsc-rs-compiler --test emitter_final_batch ef6_historical_global_rows_match_complete_production_commands -- --exact --nocapture --test-threads=1
step emitter-lib "$D/measure" emitter-lib-r6 -- cargo test -p tsc-rs-emitter --lib
step emitter-contracts "$D/measure" emitter-contracts-r6 -- cargo test -p tsc-rs-emitter --test contracts
step printer "$D/measure" printer-all-r6 -- python3 scripts/witness.py printer --all
step compact-body "$D/measure" compact-body-comments-all-r6 -- python3 scripts/witness.py compact-body-comments --all
step class-header "$D/measure" class-header-token-metadata-all-r6 -- python3 scripts/witness.py class-header-token-metadata --all
step declaration-comments "$D/measure" declaration-comments-all-r6 -- python3 scripts/witness.py declaration-comments --all
step jsdoc-return "$D/measure" jsdoc-return-all-r6 -- python3 scripts/witness.py jsdoc-return --all
step direct "$D/measure" decorator-binding-all-r6 -- python3 scripts/witness.py decorator-binding --all
step checker-lib "$D/measure" checker-lib-r6 -- cargo test -p tsc-rs-checker --lib
echo CHAIN7 DONE
