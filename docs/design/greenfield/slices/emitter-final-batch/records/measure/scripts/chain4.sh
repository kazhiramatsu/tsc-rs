#!/bin/zsh
R=/private/tmp/claude-501/-Users-hiramatsu-dev-tsc-rs/01907deb-0413-43b0-9dc0-4f5a56bb8a5f/scratchpad/runlog.sh
D=/Users/hiramatsu/dev/tsc-rs-emitter-final/docs/design/greenfield/slices/emitter-final-batch/records
S=/private/tmp/claude-501/-Users-hiramatsu-dev-tsc-rs/01907deb-0413-43b0-9dc0-4f5a56bb8a5f/scratchpad
cd /Users/hiramatsu/dev/tsc-rs-emitter-final
step() { name="$1"; shift; "$R" "$@" ; echo "STEP $name exit=$?" ; }
step build-cli "$D/measure" build-cli-r3 -- cargo build -p tsc-rs-compiler --bin tsc-rs
for v in rs v1 v6; do d="$S/probe4/$v"; rm -rf "$d/build"; (cd "$d" && /Users/hiramatsu/dev/tsc-rs-emitter-final/target/debug/tsc-rs -p tsconfig.json > rs-r3.log 2>&1; echo "rs exit $?" >> rs-r3.log); echo "=== probe4/$v ==="; tail -2 "$d/rs-r3.log"; [ -f "$d/build/src/modules/navigation/NavigationView.js" ] && diff "$S/probe4/ts/build/src/modules/navigation/NavigationView.js" "$d/build/src/modules/navigation/NavigationView.js" && echo IDENTICAL; done
step ef6-r3 "$D/measure" ef6-global-14-r3 -- cargo test -p tsc-rs-compiler --test emitter_final_batch ef6_historical_global_rows_match_complete_production_commands -- --exact --nocapture --test-threads=1
step ef2-ef3-r3 "$D/measure" ef2-ef3-rows-20-r3 -- cargo test -p tsc-rs-compiler --test emitter_final_rows -- --nocapture --test-threads=1
step emitter-lib "$D/measure" emitter-lib-r3 -- cargo test -p tsc-rs-emitter --lib
step harness-contracts "$D/measure" harness-contracts-r3 -- cargo test -p tsc-rs-harness --test contracts
echo CHAIN4 DONE
