#!/bin/zsh
# EF1 adjacent regression + first measurement of the new EF2-EF6 targets (sequential, demoted).
R=/private/tmp/claude-501/-Users-hiramatsu-dev-tsc-rs/01907deb-0413-43b0-9dc0-4f5a56bb8a5f/scratchpad/runlog.sh
D=/Users/hiramatsu/dev/tsc-rs-emitter-final/docs/design/greenfield/slices/emitter-final-batch/records
cd /Users/hiramatsu/dev/tsc-rs-emitter-final
step() { name="$1"; shift; "$R" "$@" ; echo "STEP $name exit=$?" ; }
step printer            "$D/ef1/adjacent" printer-all -- python3 scripts/witness.py printer --all
step compact-body       "$D/ef1/adjacent" compact-body-comments-all -- python3 scripts/witness.py compact-body-comments --all
step prologue           "$D/ef1/adjacent" prologue-comments-all -- python3 scripts/witness.py prologue-comments --all
step declaration-comm   "$D/ef1/adjacent" declaration-comments-all -- python3 scripts/witness.py declaration-comments --all
step emitter-lib        "$D/ef1/adjacent" emitter-lib -- cargo test -p tsc-rs-emitter --lib
step emitter-contracts  "$D/ef1/adjacent" emitter-contracts -- cargo test -p tsc-rs-emitter --test contracts
step bundle-t1          "$D/ef1/adjacent" bundle-metadata-t1-all -- python3 scripts/witness.py bundle-metadata-t1 --all
step build-new          "$D/measure" build-new-targets -- cargo test -p tsc-rs-compiler --test emitter_final_batch --test emitter_final_rows --no-run
step ef4-ef5            "$D/measure" ef4-ef5-class-40 -- cargo test -p tsc-rs-compiler --test emitter_final_batch ef4_ef5_historical_class_rows_match_complete_typescript_observations -- --exact --nocapture --test-threads=1
step ef6                "$D/measure" ef6-global-14 -- cargo test -p tsc-rs-compiler --test emitter_final_batch ef6_historical_global_rows_match_complete_production_commands -- --exact --nocapture --test-threads=1
step ef2-ef3            "$D/measure" ef2-ef3-rows-20 TSC_RS_EMITTER_FINAL_CAPTURE_DIR=/private/tmp/claude-501/-Users-hiramatsu-dev-tsc-rs/01907deb-0413-43b0-9dc0-4f5a56bb8a5f/scratchpad/capture/ef2-ef3 -- cargo test -p tsc-rs-compiler --test emitter_final_rows -- --nocapture --test-threads=1
echo CHAIN1 DONE
