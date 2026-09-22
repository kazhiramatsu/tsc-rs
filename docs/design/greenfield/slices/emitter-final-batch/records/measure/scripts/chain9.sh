#!/bin/zsh
# chain9: final-bytes measurement, resumable (markers in $S/chain9-done). Single-job cargo.
R=/private/tmp/claude-501/-Users-hiramatsu-dev-tsc-rs/01907deb-0413-43b0-9dc0-4f5a56bb8a5f/scratchpad/runlog.sh
D=/Users/hiramatsu/dev/tsc-rs-emitter-final/docs/design/greenfield/slices/emitter-final-batch/records
S=/private/tmp/claude-501/-Users-hiramatsu-dev-tsc-rs/01907deb-0413-43b0-9dc0-4f5a56bb8a5f/scratchpad
W=/Users/hiramatsu/dev/tsc-rs-emitter-final
cd $W; mkdir -p $S/chain9-done
if grep -rn "TSC_RS_DEBUG" crates/emitter/src >/dev/null; then echo "DEBUG HOOKS PRESENT — abort"; exit 2; fi
step() { name="$1"; shift; if [ -f $S/chain9-done/$name ]; then echo "STEP $name skipped(done)"; return 0; fi; "$R" "$@"; c=$?; echo "STEP $name exit=$c"; [ $c -eq 0 ] && touch $S/chain9-done/$name; [ $c -eq 101 ] && touch $S/chain9-done/$name; return $c; }
step build-cli "$D/measure" build-cli-r7b CARGO_BUILD_JOBS=1 -- cargo build -p tsc-rs-compiler --bin tsc-rs
if [ ! -f $S/chain9-done/probes ]; then
run() { d="$1"; f="${2:-main.js}"; rm -rf $S/$d/rs/out; (cd $S/$d/rs && $W/target/debug/tsc-rs -p tsconfig.json > rs.log 2>&1; echo "rs exit $?" >> rs.log); printf "PROBE %-30s " "$d"; if diff -q $S/$d/ts/out/$f $S/$d/rs/out/$f >/dev/null 2>&1; then echo IDENTICAL; else echo DIFF; diff $S/$d/ts/out/$f $S/$d/rs/out/$f > $S/$d/rs/diff-r7.txt; head -8 $S/$d/rs/diff-r7.txt; fi; }
for d in probe2/B probe3/legacy-bound-this probe3/escaped probe3/nested-computed probe3/concise-arrow probe3/direct-escaped probe6/dec probe7/read-comment probe7/async-gen-super probe9/field-arrow probe9/legacy-static-block probe11/P1 probe11/P3 probe11/P5 probe11/P6 probe11/P7; do run $d; done
run probe8/imported-promise test.js
touch $S/chain9-done/probes
fi
for c in ef2-ef3-r7b ef4-ef5-r7b ef6-r7b; do [ -d "$S/capture/$c" ] || mkdir -p "$S/capture/$c"; done
step ef2-ef3-r7b "$D/measure" ef2-ef3-rows-20-r7b CARGO_BUILD_JOBS=1 TSC_RS_EMITTER_FINAL_CAPTURE_DIR=$S/capture/ef2-ef3-r7b -- cargo test -p tsc-rs-compiler --test emitter_final_rows -- --nocapture --test-threads=1
step ef4-ef5-r7b "$D/measure" ef4-ef5-class-40-r7b CARGO_BUILD_JOBS=1 TSC_RS_EMITTER_FINAL_CAPTURE_DIR=$S/capture/ef4-ef5-r7b -- cargo test -p tsc-rs-compiler --test emitter_final_batch ef4_ef5_historical_class_rows_match_complete_typescript_observations -- --exact --nocapture --test-threads=1
step ef6-r7b "$D/measure" ef6-global-14-r7b CARGO_BUILD_JOBS=1 TSC_RS_H2_8A_FAILURE_DIR=$S/capture/ef6-r7b -- cargo test -p tsc-rs-compiler --test emitter_final_batch ef6_historical_global_rows_match_complete_production_commands -- --exact --nocapture --test-threads=1
step emitter-lib "$D/measure" emitter-lib-r7b CARGO_BUILD_JOBS=1 -- cargo test -p tsc-rs-emitter --lib
step emitter-contracts "$D/measure" emitter-contracts-r7b CARGO_BUILD_JOBS=1 -- cargo test -p tsc-rs-emitter --test contracts
step checker-lib "$D/measure" checker-lib-r7 CARGO_BUILD_JOBS=1 -- cargo test -p tsc-rs-checker --lib
step harness-contracts "$D/measure" harness-contracts-r7 CARGO_BUILD_JOBS=1 -- cargo test -p tsc-rs-harness --test contracts
for suite in printer compact-body-comments class-header-token-metadata decorator-binding utf16-literal-witnesses utf16-identity-recovery utf16-review-fix utf16-original-commands string-literal-identifier-source utf16-literal-escaping literal-update prologue-comments parameter-temporaries transpile-routes declaration-comments jsdoc-return post-t1-residuals bundle-metadata-t1; do
  step "witness-$suite" "$D/measure" "witness-$suite-all-r7" CARGO_BUILD_JOBS=1 -- python3 scripts/witness.py $suite --all
done
echo CHAIN9 DONE
