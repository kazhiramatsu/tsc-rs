#!/bin/zsh
# Measure candidate binaries against a base: retired instructions (serial.py, 1 thread, the 8 bench projects, warm-up 1 + ROUNDS
# rounds; default 3, use ROUNDS=9 for sub-1% deltas and compare the low mode) and output identity (diffout.py, 8 projects).
# usage: [ROUNDS=9] scripts/perf/measure-patches.sh BASE_NAME CANDIDATE_NAME...   (names in $TSRS_PERF_ROOT/bin)
# Roots: TSRS_REPO, TSRS_PERF_ROOT (default $TSRS_REPO/target/perf), TSRS_BENCH_ROOT (default ~/dev/bench-corpora).
set -u
HERE=${0:A:h}
REPO=${TSRS_REPO:-${HERE:h:h}}
PERF=${TSRS_PERF_ROOT:-$REPO/target/perf}
BIN=$PERF/bin
OUT=$PERF/results
BASE=$1; shift
CASES8=vscode/stable,ts/noemit,next/noemit,zod/noemit,effect/noemit,hono/noemit,vue/noemit,playwright/noemit
mkdir -p $OUT
tag=$(date +%m%d-%H%M)
log() { echo "=== $(date +%H:%M:%S) $*" | tee -a $OUT/measure-patches.log; }
python3 $HERE/binprov.py check $BASE "$@" || { log "binprov check FAILED"; exit 1; }
bins=base=$BIN/$BASE
for n in "$@"; do bins=$bins,$n=$BIN/$n; done
log "serial.py $BASE vs $* on $CASES8 (rounds ${ROUNDS:-3})"
python3 $HERE/serial.py $OUT/patches-$tag.jsonl $bins $CASES8 --warmup 1 --rounds ${ROUNDS:-3} 2>&1 | tee $OUT/patches-$tag.txt
log "serial done rc=${pipestatus[1]}"
for n in "$@"; do
  log "diffout $BASE vs $n"
  python3 $HERE/diffout.py $BIN/$BASE $BIN/$n $CASES8 2>&1 | tee $OUT/diffout-$n.txt
  log "diffout $n rc=${pipestatus[1]}"
done
log "measure-patches done (results patches-$tag.txt; summarize with lowmode.py $OUT/patches-$tag.jsonl)"
