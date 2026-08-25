#!/bin/bash
# Oracle chain-walk driver. THE only sanctioned way to converge the pinned
# oracle ladder after crate/artifact changes — never hand-author a walk loop
# in a session scratchpad.
#
# HARD PREFLIGHT (do not remove): the Rust tree must be at FINAL bytes before
# any re-mint. A post-walk `cargo fmt`/clippy fix re-stales the profile ladder
# (h2_2c_acceptance.rs bytes are pinned by 15 profile ratchets + the hosted
# qualification policy) and costs a full re-converge. Paid twice: slice A
# (57-min re-observation) and the H2.6a ca-2 train (2026-08-25, 3-line fmt
# reflow -> full profile-ladder re-cascade).
#
# Usage: bash scripts/chain-walk.sh [readiness-slice-id]
#   [readiness-slice-id]  optional .github/ci/slice-readiness.mjs --check arg
#   SKIP_PREFLIGHT=1      escape hatch for the fmt/clippy gate (dangerous;
#                         only when the Rust tree is provably already final)
#   WALK_DRY=1            stop after the preflight + ORDER coverage checks
#                         (use to verify an ORDER edit without walking)
# Runs demoted (taskpolicy -b nice -n 15) per the standing background-priority
# directive. Logs land in /tmp/chain-walk-<name>.log.
set -u
cd "$(dirname "$0")/.."
REPIN="scripts/chain-walk-repin.py"

if [ "${SKIP_PREFLIGHT:-0}" != "1" ]; then
  echo "preflight: cargo fmt --all -- --check"
  if ! taskpolicy -b nice -n 15 cargo fmt --all -- --check >/tmp/chain-walk-fmt.log 2>&1; then
    echo "REFUSING TO WALK: rustfmt is red (see /tmp/chain-walk-fmt.log)."
    echo "Run 'cargo fmt --all', land the bytes, THEN walk. A post-walk fmt"
    echo "change re-stales the profile ladder and repeats the whole converge."
    exit 2
  fi
  echo "preflight: cargo clippy --workspace --all-targets -- -D warnings"
  if ! taskpolicy -b nice -n 15 cargo clippy --workspace --all-targets -- -D warnings >/tmp/chain-walk-clippy.log 2>&1; then
    echo "REFUSING TO WALK: clippy is red (see /tmp/chain-walk-clippy.log)."
    echo "Fix clippy to final bytes first; clippy-driven edits after the walk"
    echo "re-stale the ladder exactly like fmt."
    exit 2
  fi
  echo "preflight: clean"
fi

# Lineage order (qualification BEFORE profile; h2-5h-a witnesses last).
# Extend this list in the slice that adds a new oracle script.
ORDER=(
  l0-option-inventory
  h1-owner-inventory
  h1-rust-omission-inventory
  h1-printer-foundation
  h1-active-transform
  h1-emit-oracle
  h1-emit-qualification
  h2-transition
  h2-1a-qualification h2-1a-profile
  h2-1b-qualification h2-1b-profile
  h2-1c-qualification h2-1c-profile
  h2-1d-qualification h2-1d-profile
  h2-1e-qualification h2-1e-profile
  h2-2a-qualification h2-2a-profile
  h2-2b-qualification h2-2b-profile
  h2-2c-qualification h2-2c-profile
  h2-2d-qualification h2-2d-profile
  h2-3a-qualification h2-3a-profile
  h2-3b-qualification h2-3b-profile
  h2-3c-qualification h2-3c-profile
  h2-3d-qualification h2-3d-profile
  h2-4a-qualification h2-4a-profile
  h2-4b-qualification h2-4b-profile
  h2-5a-qualification h2-5a-profile
  h2-5b-qualification h2-5b-profile
  h2-5c-qualification h2-5c-profile
  h2-5d-qualification h2-5d-profile
  h2-5e-qualification h2-5e-profile
  h2-5f-qualification h2-5f-profile
  h2-5g-qualification h2-5g-profile
  h2-5h-qualification
  h2-5h-a-foundation
  h2-5h-a-owner-graph
  h2-5h-a-gap-matrix
  h2-5h-a-dispositions
  h2-5h-a-comment-scope-witnesses
  h2-5h-a-es2015-generators-witnesses
  h2-6a-witnesses
  h2-6a-qualification
)
# Coverage self-check: ORDER must stay in sync with the chain scripts on
# disk, so the slice that adds or retires an oracle script CANNOT forget to
# update this driver — the walk refuses to start on drift. Chain scripts are
# crates/oracle/h2-[0-9]*.mjs minus *-owner-controls (verified read-only in
# the tail below; no ORDER entry needed). h2-baseline.mjs does not match the
# glob and stays out by design (approved-runner mint only). The closed
# l0-*/h1-* families are pinned by the explicit ORDER entries themselves.
drift=0
for f in crates/oracle/h2-[0-9]*.mjs; do
  base=$(basename "$f" .mjs)
  case "$base" in *-owner-controls) continue;; esac
  if ! printf '%s\n' "${ORDER[@]}" | grep -qx "$base"; then
    echo "ORDER DRIFT: $f is not in scripts/chain-walk.sh ORDER — extend ORDER in this slice"
    drift=1
  fi
done
for name in "${ORDER[@]}"; do
  if [ ! -f "crates/oracle/${name}.mjs" ]; then
    echo "ORDER DRIFT: crates/oracle/${name}.mjs no longer exists — retire its ORDER entry in this slice"
    drift=1
  fi
done
[ $drift -eq 0 ] || exit 2
echo "coverage: ORDER in sync (${#ORDER[@]} chain scripts)"
if [ "${WALK_DRY:-0}" = "1" ]; then
  # Probe mode (also the gate's structural-preflight hook): the harness pin
  # audit runs here so a stale test-const fails the gate in seconds, not at
  # the workspace-tests phase 40 minutes in.
  python3 scripts/pin-audit.py || exit 1
  echo "WALK_DRY=1: stopping after preflight + coverage + pin-audit checks"
  exit 0
fi

round=1
while true; do
  stale=()
  for name in "${ORDER[@]}"; do
    script="crates/oracle/${name}.mjs"
    [ -f "$script" ] || continue
    if ! taskpolicy -b nice -n 15 node "$script" --check >/tmp/chain-walk-${name}.log 2>&1; then
      stale+=("$name")
      echo "round $round STALE: $name"
      if ! taskpolicy -b nice -n 15 node "$script" --write >>/tmp/chain-walk-${name}.log 2>&1; then
        echo "  write failed; refreshing stale pins in $script"
        python3 "$REPIN" "$script" | tee -a /tmp/chain-walk-${name}.log
        taskpolicy -b nice -n 15 node "$script" --write >>/tmp/chain-walk-${name}.log 2>&1 \
          || { echo "WRITE FAILED after repin: $name (see /tmp/chain-walk-${name}.log)"; exit 1; }
      fi
    fi
  done
  if [ ${#stale[@]} -eq 0 ]; then
    echo "walk round $round: CLEAN"
    break
  fi
  echo "walk round $round re-minted: ${stale[*]}"
  round=$((round+1))
  [ $round -gt 6 ] && { echo "walk did not converge in 6 rounds"; exit 1; }
done

# Owner-control artifacts should be crate-byte-insensitive; verify, never
# auto-write (a stale one needs explicit review).
for oc in crates/oracle/h2-*-owner-controls.mjs; do
  if ! taskpolicy -b nice -n 15 node "$oc" --check >/tmp/chain-walk-oc.log 2>&1; then
    echo "OWNER-CONTROL STALE (review!): $oc"; exit 1
  fi
done
echo "owner-control checks: clean"

# Post-convergence: harness test consts must match the just-converged
# artifacts. Report-only — a stale pin can be a legitimate re-mint needing
# `scripts/pin-audit.py --fix` OR a real regression the harness tests would
# catch; never auto-fix inside the walk.
python3 scripts/pin-audit.py || { echo "PIN AUDIT STALE (fix + targeted harness tests before the gate)"; exit 1; }

rc=0
taskpolicy -b nice -n 15 node --test .github/ci/qualification.test.mjs >/tmp/chain-walk-qual-test.log 2>&1 || rc=1
echo "qual test exit: $rc"
qc=0
taskpolicy -b nice -n 15 node .github/ci/qualification.mjs check >/tmp/chain-walk-qual-check.log 2>&1 || qc=1
echo "qual check exit: $qc"; tail -1 /tmp/chain-walk-qual-check.log
if [ $# -ge 1 ]; then
  node .github/ci/slice-readiness.mjs --check "$1" || { echo "readiness FAILED: $1"; exit 1; }
fi
[ $rc -eq 0 ] && [ $qc -eq 0 ] || exit 1
echo "chain walk: converged and green"
