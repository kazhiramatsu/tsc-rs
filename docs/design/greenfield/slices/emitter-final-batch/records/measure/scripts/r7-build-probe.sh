#!/bin/zsh
S=/private/tmp/claude-501/-Users-hiramatsu-dev-tsc-rs/01907deb-0413-43b0-9dc0-4f5a56bb8a5f/scratchpad
D=/Users/hiramatsu/dev/tsc-rs-emitter-final/docs/design/greenfield/slices/emitter-final-batch/records/measure
W=/Users/hiramatsu/dev/tsc-rs-emitter-final
cd $W
$S/runlog.sh $D check-r7e -- cargo check -p tsc-rs-emitter -p tsc-rs-checker; c=$?; echo "STEP check exit=$c"
grep -nE "^(error|warning)" $D/check-r7e.log | head -10
[ $c -ne 0 ] && exit 1
$S/runlog.sh $D build-cli-r7e -- cargo build -p tsc-rs-compiler --bin tsc-rs; b=$?; echo "STEP build exit=$b"
[ $b -ne 0 ] && exit 1
run() { d="$1"; f="${2:-main.js}"; rm -rf $S/$d/rs/out; (cd $S/$d/rs && $W/target/debug/tsc-rs -p tsconfig.json > rs.log 2>&1; echo "rs exit $?" >> rs.log); printf "%-30s " "$d"; if diff -q $S/$d/ts/out/$f $S/$d/rs/out/$f >/dev/null 2>&1; then echo IDENTICAL; else echo DIFF; diff $S/$d/ts/out/$f $S/$d/rs/out/$f | head -10; fi; }
for d in probe11/P5 probe11/P6 probe11/P7 probe11/P1 probe11/P3 probe3/concise-arrow probe9/field-arrow probe9/legacy-static-block probe3/direct-escaped probe3/escaped probe6/dec probe7/async-gen-super probe7/read-comment probe2/B probe3/nested-computed probe3/legacy-bound-this; do run $d; done
run probe8/imported-promise test.js
echo "=== probe10 iterable (rs diagnostics) ==="; (cd $S/probe10/iterable-es5/rs && $W/target/debug/tsc-rs -p tsconfig.json; echo "rs exit $?")
echo R7-PROBES DONE
