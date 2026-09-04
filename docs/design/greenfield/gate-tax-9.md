# gate-tax 9 — short convergence for small-mistake walk failures

Status: landed with the h2-7b m-2 train (2026-09-04; user directive「その処理を追加してください。ただし次回のローカルCIの直前に実装してください」). Scope: `scripts/chain-walk.sh` + `scripts/walk-static-checks.py`; no generator, artifact, or Rust change.

## The tax measured on 2026-09-04

Seven walk launches converged one train. Four failed for reasons that are pure functions of the tree and were visible in seconds, but surfaced only after the ~15-minute fmt / clippy / PRE_SUITE preflight or 35–40 minutes into the minting rounds:

| attempt | failure | detectable statically | cost |
|---|---|---|---|
| 1 | three stale source pins (hosted policy `rust_source_sha256`; fuzz manifests + their cascade pin) | yes (`walk-preflight.py`, seconds) — but it ran AFTER the preflight | 15 min |
| 5 | `h2-5g-profile` runtime-input closure missing three train files | yes (the generator's own `--check`) | 15 min + 35 min of minting |
| 6 | 117 curated `path:line` anchors moved by an emitter edit (`h2-7a-owner-inventory`) | yes (the generator's `--check`) | 15 min + 38 min of minting |
| 4→7 | every relaunch re-paid fmt / clippy / PRE_SUITE although `crates/**` did not change between 5, 6 and 7 | receipt | 3 × 15 min |

## The three changes

- **9-A static first.** The ORDER coverage, planner coverage, topology audit and `walk-preflight.py` pin-surface check run right after the §3 recovery phase and BEFORE fmt / clippy / PRE_SUITE. A stale pin refuses in seconds.
- **9-B generator preconditions.** `scripts/walk-static-checks.py` asks the generators themselves — `h2-5g-profile.mjs --check`, `h2-7a-owner-inventory.mjs --check`, `h2-7a-close.mjs --check`, each bounded in time — and refuses ONLY on the two static-precondition error families (runtime-input closure / identity; curated anchor headers). A stale artifact, any other exit, or a timeout is the walk's own business and passes. No generator is modified, so the check never changes what the walk mints and never re-stales the ladder.
- **9-C preflight receipt.** When fmt, the layout scan, clippy and PRE_SUITE are green, the driver records a receipt keyed by the Rust tree (every `crates/**/*.rs` and `Cargo.toml`, the root `Cargo.toml` / `Cargo.lock`, `rustc --version`, the PRE_SUITE string). A relaunch with the same key skips them and RECORDS the skip in the run summary (gate-tax 5-E is preserved: the same bytes validated once stay validated; the full gate re-validates everything at the final head). `WALK_PREFLIGHT_RECEIPT=0` forces the full preflight; `WALK_DRY=1` never writes a receipt.

## Not done (deliberately)

- Auto-repairing source pins and anchors in the §3 recovery phase: both repairs are mechanical (a sha refresh; the ±3-line `tsc-port:` window rule), but registering a new runtime input is a runtime-vs-shadow judgment — it stays a static REFUSAL. The two mechanical repairs can join §3 in a later slice once their repin scripts (`scripts/walk-preflight.py` style) exist as pure functions.
- Resuming the ROUND after a mid-mint failure: the planner already reuses fresh outputs; the remaining tax was the preflight, which 9-C removes.
