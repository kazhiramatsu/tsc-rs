#!/bin/zsh
R=/private/tmp/claude-501/-Users-hiramatsu-dev-tsc-rs/01907deb-0413-43b0-9dc0-4f5a56bb8a5f/scratchpad/runlog.sh
D=/Users/hiramatsu/dev/tsc-rs-emitter-final/docs/design/greenfield/slices/emitter-final-batch/records
C=/private/tmp/claude-501/-Users-hiramatsu-dev-tsc-rs/01907deb-0413-43b0-9dc0-4f5a56bb8a5f/scratchpad/capture
rm -rf "$C/ef4-ef5" "$C/ef6"; mkdir -p "$C/ef4-ef5" "$C/ef6"
step() { name="$1"; shift; "$R" "$@" ; echo "STEP $name exit=$?" ; }
step ef4-ef5-capture "$D/measure" ef4-ef5-class-40-capture TSC_RS_EMITTER_FINAL_CAPTURE_DIR=$C/ef4-ef5 -- cargo test -p tsc-rs-compiler --test emitter_final_batch ef4_ef5_historical_class_rows_match_complete_typescript_observations -- --exact --nocapture --test-threads=1
step ef6-capture "$D/measure" ef6-global-14-capture TSC_RS_H2_8A_FAILURE_DIR=$C/ef6 -- cargo test -p tsc-rs-compiler --test emitter_final_batch ef6_historical_global_rows_match_complete_production_commands -- --exact --nocapture --test-threads=1
echo CHAIN2 DONE
