#!/bin/zsh
# chain8: final-bytes measurement (r7). Run only after the debug hooks are removed.
R=/private/tmp/claude-501/-Users-hiramatsu-dev-tsc-rs/01907deb-0413-43b0-9dc0-4f5a56bb8a5f/scratchpad/runlog.sh
D=/Users/hiramatsu/dev/tsc-rs-emitter-final/docs/design/greenfield/slices/emitter-final-batch/records
S=/private/tmp/claude-501/-Users-hiramatsu-dev-tsc-rs/01907deb-0413-43b0-9dc0-4f5a56bb8a5f/scratchpad
W=/Users/hiramatsu/dev/tsc-rs-emitter-final
cd $W
if grep -rn "TSC_RS_DEBUG" crates/emitter/src >/dev/null; then echo "DEBUG HOOKS PRESENT — abort"; exit 2; fi
step() { name="$1"; shift; "$R" "$@" ; echo "STEP $name exit=$?" ; }
step build-cli "$D/measure" build-cli-r7-final CARGO_BUILD_JOBS=1 -- cargo build -p tsc-rs-compiler --bin tsc-rs
run() { d="$1"; f="${2:-main.js}"; rm -rf $S/$d/rs/out; (cd $S/$d/rs && $W/target/debug/tsc-rs -p tsconfig.json > rs.log 2>&1; echo "rs exit $?" >> rs.log); printf "PROBE %-30s " "$d"; if diff -q $S/$d/ts/out/$f $S/$d/rs/out/$f >/dev/null 2>&1; then echo IDENTICAL; else echo DIFF; diff $S/$d/ts/out/$f $S/$d/rs/out/$f > $S/$d/rs/diff-r7.txt; head -8 $S/$d/rs/diff-r7.txt; fi; }
for d in probe2/B probe3/legacy-bound-this probe3/escaped probe3/nested-computed probe3/concise-arrow probe3/direct-escaped probe6/dec probe7/read-comment probe7/async-gen-super probe9/field-arrow probe9/legacy-static-block probe11/P1 probe11/P3 probe11/P5 probe11/P6 probe11/P7; do run $d; done
run probe8/imported-promise test.js
for c in ef2-ef3-r7 ef4-ef5-r7 ef6-r7; do rm -rf "$S/capture/$c"; mkdir -p "$S/capture/$c"; done
step ef2-ef3-r7 "$D/measure" ef2-ef3-rows-20-r7 TSC_RS_EMITTER_FINAL_CAPTURE_DIR=$S/capture/ef2-ef3-r7 CARGO_BUILD_JOBS=1 -- cargo test -p tsc-rs-compiler --test emitter_final_rows -- --nocapture --test-threads=1
step ef4-ef5-r7 "$D/measure" ef4-ef5-class-40-r7 TSC_RS_EMITTER_FINAL_CAPTURE_DIR=$S/capture/ef4-ef5-r7 CARGO_BUILD_JOBS=1 -- cargo test -p tsc-rs-compiler --test emitter_final_batch ef4_ef5_historical_class_rows_match_complete_typescript_observations -- --exact --nocapture --test-threads=1
step ef6-r7 "$D/measure" ef6-global-14-r7 TSC_RS_H2_8A_FAILURE_DIR=$S/capture/ef6-r7 CARGO_BUILD_JOBS=1 -- cargo test -p tsc-rs-compiler --test emitter_final_batch ef6_historical_global_rows_match_complete_production_commands -- --exact --nocapture --test-threads=1
step emitter-lib "$D/measure" emitter-lib-r7 CARGO_BUILD_JOBS=1 -- cargo test -p tsc-rs-emitter --lib
step emitter-contracts "$D/measure" emitter-contracts-r7 CARGO_BUILD_JOBS=1 -- cargo test -p tsc-rs-emitter --test contracts
step checker-lib "$D/measure" checker-lib-r7 CARGO_BUILD_JOBS=1 -- cargo test -p tsc-rs-checker --lib
step harness-contracts "$D/measure" harness-contracts-r7 CARGO_BUILD_JOBS=1 -- cargo test -p tsc-rs-harness --test contracts
for suite in printer compact-body-comments class-header-token-metadata declaration-comments jsdoc-return decorator-binding utf16-literal-witnesses utf16-identity-recovery utf16-review-fix utf16-original-commands string-literal-identifier-source utf16-literal-escaping literal-update prologue-comments parameter-temporaries transpile-routes post-t1-residuals bundle-metadata-t1; do
  step "witness-$suite" "$D/measure" "witness-$suite-all-r7" -- python3 scripts/witness.py $suite --all
done
echo CHAIN8 DONE
