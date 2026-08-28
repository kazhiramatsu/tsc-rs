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
# gate-tax 5 (docs/design/greenfield/gate-tax-5.md): one walk per converge,
# inside a locked transaction — PRE_SUITE red-suite hook, ALL pin surfaces
# preflighted at once (walk-preflight.py), mechanical ORDER-topology audit,
# prospective stale-cone plan (new-ci, report-only), repin BEFORE the write
# attempt, per-round 5g receipt-outcome enforcement, run-ID'd log
# directories, and a per-workspace lock.
#
# Usage: bash scripts/chain-walk.sh [readiness-slice-id]
#   [readiness-slice-id]  optional .github/ci/slice-readiness.mjs --check arg
#   SKIP_PREFLIGHT=1      escape hatch for the fmt/clippy gate (dangerous;
#                         only when the Rust tree is provably already final)
#   WALK_DRY=1            stop after the preflight + ORDER coverage checks
#                         (use to verify an ORDER edit without walking)
#   PRE_SUITE="<cmd>"     red-suite-first: run this suite before any re-mint;
#                         nonzero exit refuses the walk (gate-tax 5-E)
#   WALK_PLAN=0           skip the prospective-plan report (default: run it)
#   WALK_EXPECT_OBS=0|1   5g enforcement override: 0 = strict (any
#                         observation red), 1 = disabled (deliberate
#                         re-anchor; RECORDED in the run summary)
#   TSRS_H2_5G_FRESH=1    recorded fresh escape (exempt from enforcement)
# Runs demoted (taskpolicy -b nice -n 15) per the standing background-priority
# directive. Logs land in target/chain-walk/runs/<run-id>/ (symlink:
# target/chain-walk/runs/latest).
set -u
cd "$(dirname "$0")/.."
REPIN="scripts/chain-walk-repin.py"

RUN_ID="$(date +%Y%m%d-%H%M%S)-$$"
RUN_DIR="target/chain-walk/runs/$RUN_ID"
LOCK_DIR="target/chain-walk/lock"

summary() {
  echo "$*"
  [ -d "$RUN_DIR" ] && echo "$*" >> "$RUN_DIR/summary.log"
}

release_lock() {
  rm -f "$LOCK_DIR/owner"
  rmdir "$LOCK_DIR" 2>/dev/null || true
}

acquire_lock() {
  mkdir -p target/chain-walk
  if ! mkdir "$LOCK_DIR" 2>/dev/null; then
    local owner_pid
    owner_pid=$(cut -d' ' -f1 "$LOCK_DIR/owner" 2>/dev/null || echo "")
    if [ -n "$owner_pid" ] && kill -0 "$owner_pid" 2>/dev/null; then
      echo "REFUSING TO WALK: another walk (pid $owner_pid) holds $LOCK_DIR"
      echo "(one walk per converge — gate-tax 5-F; wait for it or kill it first)"
      exit 2
    fi
    echo "stale walk lock (pid ${owner_pid:-unknown} not running) — breaking it"
    release_lock
    if ! mkdir "$LOCK_DIR" 2>/dev/null; then
      echo "REFUSING TO WALK: lost the lock race on $LOCK_DIR"; exit 2
    fi
  fi
  echo "$$ $RUN_ID" > "$LOCK_DIR/owner"
  trap release_lock EXIT
}

if [ "${WALK_DRY:-0}" != "1" ]; then
  acquire_lock
  mkdir -p "$RUN_DIR"
  ln -sfn "$RUN_ID" target/chain-walk/runs/latest
  summary "chain walk run $RUN_ID (logs: $RUN_DIR)"
fi

if [ "${SKIP_PREFLIGHT:-0}" != "1" ]; then
  echo "preflight: cargo fmt --all -- --check"
  if ! taskpolicy -b nice -n 15 cargo fmt --all -- --check >/tmp/chain-walk-fmt.log 2>&1; then
    echo "REFUSING TO WALK: rustfmt is red (see /tmp/chain-walk-fmt.log)."
    echo "Run 'cargo fmt --all', land the bytes, THEN walk. A post-walk fmt"
    echo "change re-stales the profile ladder and repeats the whole converge."
    exit 2
  fi
  # Inline-tests layout (gt6 lesson, 2026-08-28): the workspace-audit that
  # rejects inline `mod tests {` bodies in src runs only in the FULL GATE —
  # i.e. after a walk — so a violating file converges a walk and then the
  # layout fix re-stales the whole ladder. Catch the one rule here, pre-walk.
  echo "preflight: inline mod tests scan (crates/*/src)"
  if grep -rn --include='*.rs' -E '^[[:space:]]*mod tests \{' crates/*/src >/tmp/chain-walk-inline-tests.log 2>&1; then
    echo "REFUSING TO WALK: inline 'mod tests {' body in src (see /tmp/chain-walk-inline-tests.log)."
    echo "Move the body to crates/<crate>/tests/unit/<module>/tests.rs and keep"
    echo "only '#[cfg(test)] #[path = ...] mod tests;' in src (workspace-audit rule)."
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

# Red-suite-first (gate-tax 5-E): never converge unvalidated bytes. When a
# Rust fix answers a red suite, PRE_SUITE runs that suite on the fixed
# binary before any re-mint.
if [ "${WALK_DRY:-0}" != "1" ] && [ -n "${PRE_SUITE:-}" ]; then
  summary "pre-suite (gate-tax 5-E): $PRE_SUITE"
  if ! taskpolicy -b nice -n 15 bash -c "$PRE_SUITE" >"$RUN_DIR/pre-suite.log" 2>&1; then
    echo "REFUSING TO WALK: PRE_SUITE failed (see $RUN_DIR/pre-suite.log)."
    echo "Fix the suite red first — converging unvalidated bytes repeats the"
    echo "whole converge when the fix changes crates/*.rs."
    exit 2
  fi
  summary "pre-suite: green"
fi

# Lineage order (qualification BEFORE profile; h2-5h-a witnesses BEFORE the
# owner graph that pins them — gate-tax 5-D, audited mechanically below).
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
  h2-5h-a-comment-scope-witnesses
  h2-5h-a-owner-graph
  h2-5h-a-gap-matrix
  h2-5h-a-dispositions
  h2-5h-a-es2015-generators-witnesses
  h2-6a-witnesses
  h2-6a-qualification
  h2-6b-witnesses
  h2-6b-qualification
)
# Coverage self-check: ORDER must stay in sync with the chain scripts on
# disk, so the slice that adds or retires an oracle script CANNOT forget to
# update this driver — the walk refuses to start on drift. Chain scripts are
# crates/oracle/h2-[0-9]*.mjs minus *-owner-controls (verified read-only in
# the tail below; no ORDER entry needed) and minus *-check-resume (a
# side-effect-free library imported by its qualification script and
# exercised by gate-tax-5.test.mjs; it mints nothing). h2-baseline.mjs does
# not match the glob and stays out by design (approved-runner mint only).
# The closed l0-*/h1-* families are pinned by the explicit ORDER entries
# themselves.
drift=0
for f in crates/oracle/h2-[0-9]*.mjs; do
  base=$(basename "$f" .mjs)
  case "$base" in *-owner-controls|*-check-resume) continue;; esac
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

# ORDER-topology audit (gate-tax 5-D): a producer appearing after its
# consumer costs a third full round every converge; refuse like drift.
python3 scripts/walk-topology-audit.py "${ORDER[@]}" || exit 2

# All-surfaces pin preflight (gate-tax 5-F): report EVERY stale pin surface
# at once — harness pins, pin-index, policy source pins, schema consts,
# fuzz manifests — so the operator fixes everything in one pass and the
# walk runs ONCE (measured: 3 walks / ~65 min avoidable on the gt4 landing).
python3 scripts/walk-preflight.py || {
  echo "REFUSING TO WALK: stale pin surfaces above — fix ALL, then walk once."
  exit 2
}

# The ladder is only proven for the crate bytes it was converged at. The
# green tail below records that tree hash; here (and in the gate's
# structural-preflight via WALK_DRY) a drifted tree refuses in seconds
# instead of failing the gate's oracle phase minutes in. Paid 2026-08-26:
# a 5-line post-walk xtask fix red-ended the gate on stale h1-rust-omissions.
crate_tree_sha() {
  find crates -name '*.rs' -type f -print0 | sort -z | xargs -0 shasum -a 256 | shasum -a 256 | cut -d' ' -f1
}
CONVERGED_RECORD="target/chain-walk/converged-crates.sha256"
if [ "${WALK_DRY:-0}" = "1" ]; then
  if [ -f "$CONVERGED_RECORD" ]; then
    if [ "$(crate_tree_sha)" != "$(cat "$CONVERGED_RECORD")" ]; then
      echo "LADDER NOT CONVERGED FOR THESE CRATE BYTES: crates/*.rs changed"
      echo "since the last green chain walk — run scripts/chain-walk.sh first."
      exit 1
    fi
    echo "converged-walk record: crate tree matches"
  else
    echo "converged-walk record: absent (a green walk will mint it)"
  fi
  echo "WALK_DRY=1: stopping after preflight + coverage + topology + pin-surface checks"
  exit 0
fi

# Prospective stale-cone plan (gate-tax 5-F, report-only): predict the
# post-walk re-mint cone and every pin surface that will go stale AFTER
# the re-mint (e.g. schema consts pinning a re-minted artifact), so the
# operator expects the post-walk repairs instead of discovering them at
# the gate. Best-effort: new-ci is a zero-dependency out-of-workspace
# crate; if it cannot build or the tree state defeats it, say so and walk.
if [ "${WALK_PLAN:-1}" = "1" ]; then
  if taskpolicy -b nice -n 15 cargo build --manifest-path new-ci/Cargo.toml --release --bin plan >"$RUN_DIR/plan-build.log" 2>&1; then
    plan_base=$(git merge-base HEAD origin/main 2>/dev/null || git rev-parse HEAD)
    plan_head=$(git stash create 2>/dev/null || true)
    if [ -z "$plan_head" ]; then plan_head=HEAD; else
      summary "plan: dirty tree snapshot via git stash create (untracked files are invisible to the plan)"
    fi
    if taskpolicy -b nice -n 15 new-ci/target/release/plan "$plan_base" "$plan_head" >"$RUN_DIR/plan.log" 2>&1; then
      summary "prospective plan: $(tail -1 "$RUN_DIR/plan.log") (report: new-ci/plan-report.md)"
    else
      summary "prospective plan: unavailable (report-only; see $RUN_DIR/plan.log)"
    fi
  else
    summary "prospective plan: new-ci build failed (report-only; see $RUN_DIR/plan-build.log)"
  fi
fi

if [ -n "${WALK_EXPECT_OBS:-}" ]; then
  summary "RECORDED OVERRIDE: WALK_EXPECT_OBS=${WALK_EXPECT_OBS} (5g enforcement $( [ "$WALK_EXPECT_OBS" = "1" ] && echo disabled — deliberate re-anchor || echo strict ))"
fi
if [ "${TSRS_H2_5G_FRESH:-0}" = "1" ]; then
  summary "RECORDED OVERRIDE: TSRS_H2_5G_FRESH=1 (fresh full observation approved)"
fi

minted_5g=0
obs_count=0
round=1
while true; do
  stale=()
  for name in "${ORDER[@]}"; do
    script="crates/oracle/${name}.mjs"
    [ -f "$script" ] || continue
    rung_log="$RUN_DIR/${name}.log"
    check_rc=0
    # a stale outcome record from a prior run must never feed enforcement:
    # absent = "no outcome" notice, never a false verdict
    [ "$name" = "h2-5g-qualification" ] && rm -f target/h2-5g/check-outcome.v1.json
    taskpolicy -b nice -n 15 node "$script" --check >"$rung_log" 2>&1 || check_rc=1
    if [ "$name" = "h2-5g-qualification" ]; then
      # gate-tax 5-C: read the machine outcome record, never prose.
      enforce_line=$(python3 scripts/walk-5g-enforce.py \
        --outcome target/h2-5g/check-outcome.v1.json \
        --minted-this-walk "$minted_5g" \
        --observations-so-far "$obs_count") || {
          summary "round $round 5g enforcement: $enforce_line"
          echo "5G ENFORCEMENT RED — see gate-tax-5.md C (walk refuses to converge)"
          exit 1
        }
      summary "round $round 5g: $enforce_line"
      case "$enforce_line" in *"observed=1"*) obs_count=$((obs_count+1));; esac
    fi
    if [ $check_rc -ne 0 ]; then
      stale+=("$name")
      echo "round $round STALE: $name"
      [ "$name" = "h2-5g-qualification" ] && minted_5g=1
      # gate-tax 5-D repin-early: refresh stale pins BEFORE the single
      # write attempt (repin is idempotent and only rewrites stale pin
      # values), instead of check->write(fail)->repin->write.
      python3 "$REPIN" "$script" | tee -a "$rung_log"
      taskpolicy -b nice -n 15 node "$script" --write >>"$rung_log" 2>&1 \
        || { echo "WRITE FAILED after repin: $name (see $rung_log)"; exit 1; }
    fi
  done
  if [ ${#stale[@]} -eq 0 ]; then
    summary "walk round $round: CLEAN"
    break
  fi
  summary "walk round $round re-minted: ${stale[*]}"
  round=$((round+1))
  [ $round -gt 6 ] && { echo "walk did not converge in 6 rounds"; exit 1; }
done

# Owner-control artifacts should be crate-byte-insensitive; verify, never
# auto-write (a stale one needs explicit review).
for oc in crates/oracle/h2-*-owner-controls.mjs; do
  if ! taskpolicy -b nice -n 15 node "$oc" --check >"$RUN_DIR/owner-controls.log" 2>&1; then
    echo "OWNER-CONTROL STALE (review!): $oc"; exit 1
  fi
done
echo "owner-control checks: clean"

# Post-convergence: every pin surface must match the just-minted artifacts
# (the same all-surfaces pass as the preflight — a schema const pinning a
# re-minted artifact is caught HERE in seconds, not 40 minutes into the
# gate). Report-only — a stale surface can be a legitimate re-mint needing
# its recorded repair (pin-audit.py --fix, schema-const patch) OR a real
# regression; never auto-fix inside the walk.
python3 scripts/walk-preflight.py || {
  echo "PIN SURFACES STALE AFTER THE WALK (fix with the recorded repairs +"
  echo "targeted tests, then re-run the walk tail — see gate-tax-5.md F)"
  exit 1
}

rc=0
taskpolicy -b nice -n 15 node --test .github/ci/qualification.test.mjs >"$RUN_DIR/qual-test.log" 2>&1 || rc=1
echo "qual test exit: $rc"
qc=0
taskpolicy -b nice -n 15 node .github/ci/qualification.mjs check >"$RUN_DIR/qual-check.log" 2>&1 || qc=1
echo "qual check exit: $qc"; tail -1 "$RUN_DIR/qual-check.log"
if [ $# -ge 1 ]; then
  node .github/ci/slice-readiness.mjs --check "$1" || { echo "readiness FAILED: $1"; exit 1; }
fi
[ $rc -eq 0 ] && [ $qc -eq 0 ] || exit 1
mkdir -p "$(dirname "$CONVERGED_RECORD")"
crate_tree_sha > "$CONVERGED_RECORD"
echo "$RUN_ID" > target/chain-walk/converged-run-id
summary "chain walk: converged and green (crate-tree record minted; certificate run $RUN_ID)"
