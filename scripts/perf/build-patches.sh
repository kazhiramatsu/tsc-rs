#!/bin/zsh
# Build one release binary per lane patch on a common base, sequentially, in ONE reusable worktree (deps compile once).
# usage: scripts/perf/build-patches.sh BASE_COMMIT PREFIX [PATCH_FILE...]
#   -> $TSRS_PERF_ROOT/bin/main-<BASE> and $TSRS_PERF_ROOT/bin/<PREFIX>-<patch stem>, each with a provenance row (binprov.py record).
# Roots: TSRS_REPO (default: the repository this script lives in), TSRS_PERF_ROOT (default: $TSRS_REPO/target/perf).
# Load rules: one cargo at a time, nice -n 20, -j 2, CARGO_INCREMENTAL=0. The only deletion is the worktree this script created.
set -u
HERE=${0:A:h}
REPO=${TSRS_REPO:-${HERE:h:h}}
PERF=${TSRS_PERF_ROOT:-$REPO/target/perf}
BASE=$1; PREFIX=$2; shift 2
WT=/private/tmp/tsc-rs-measure-$PREFIX
mkdir -p $PERF/logs $PERF/bin
log() { echo "=== $(date +%H:%M:%S) $*" | tee -a $PERF/logs/build-$PREFIX.log; }
cd $REPO && git worktree add -q --detach $WT $BASE || { log "worktree add failed"; exit 1; }
build() { # name patch(optional)
  local name=$1 patch=${2:-}
  [ -x $PERF/bin/$name ] && { log "$name exists, skipping"; return; }
  if [ -n "$patch" ]; then
    git -C $WT apply --check "$patch" || { log "$name: patch does not apply"; return; }
    git -C $WT apply "$patch"
  fi
  log "build $name ${patch:+(patch $(basename $patch))}"
  ( cd $WT && CARGO_INCREMENTAL=0 nice -n 20 cargo build --release -p tsc-rs-cli -j 2 ) >> $PERF/logs/build-$PREFIX-$name.log 2>&1
  local rc=$?; log "    rc=$rc"
  if [ $rc = 0 ]; then
    cp $WT/target/release/tsc-rs $PERF/bin/$name
    if [ -n "$patch" ]; then
      python3 $HERE/binprov.py record $name $BASE --patch "$patch" --worktree $WT --note "$PREFIX $(basename $patch)" | tee -a $PERF/logs/build-$PREFIX.log
    else
      python3 $HERE/binprov.py record $name $BASE --worktree $WT --note "$PREFIX base" | tee -a $PERF/logs/build-$PREFIX.log
    fi
  fi
  if [ -n "$patch" ]; then
    git -C $WT apply -R "$patch" || { log "reverse apply FAILED for $patch; resetting tracked files"; git -C $WT checkout -q -- . ; }
  fi
  local dirty=$(git -C $WT status --porcelain --untracked-files=no | wc -l | tr -d ' ')
  [ "$dirty" = 0 ] || log "WARNING: worktree dirty after revert ($dirty files)"
}
build main-$BASE
for p in "$@"; do stem=$(basename $p .patch); build $PREFIX-$stem "$p"; done
# remove only the worktree this script created (path checked literally against its prefix)
case "$WT" in
  /private/tmp/tsc-rs-measure-*) cd $REPO && git worktree remove --force "$WT" && git worktree prune && log "removed $WT" ;;
  *) log "refusing to remove unexpected path $WT" ;;
esac
log "builds done"
