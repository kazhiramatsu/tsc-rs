#!/usr/bin/env python3
"""Static generator preconditions for the oracle chain walk (gate-tax 9-A).

Two failure classes cost 35-40 minutes of minting each on 2026-09-04 before the
walk reached the failing rung, although both are pure functions of the tree:

  1. the H2.5g profile's runtime input closure (a new crates/** file not listed
     in NEW_RUNTIME_INPUTS / NON_RUNTIME_SHADOW_INPUTS in
     crates/oracle/h2-5g-profile.mjs), and
  2. the h2-7a owner-inventory / close curated `path:line` anchors whose
     ±3-line `tsc-port:` window moved after an emitter edit.

This preflight asks the GENERATORS themselves (their `--check` mode, bounded in
time) and classifies the stderr: only the two static-precondition error
families refuse; a stale artifact, any other exit, or a timeout is the walk's
own business and passes here.  No generator is modified (a generator edit
would re-stale the ladder), so the check never changes what the walk mints.

Exit 0 = preconditions hold; exit 2 = refuse (the offending lines are printed).
"""
from __future__ import annotations

import subprocess
import sys

CHECKS = (
    # (label, command, timeout seconds, refusing stderr fragments)
    (
        "h2-5g-profile runtime input closure",
        ["node", "crates/oracle/h2-5g-profile.mjs", "--check"],
        180,
        ("runtime input closure is missing", "runtime input identity changed"),
    ),
    (
        "h2-7a-owner-inventory curated anchors",
        ["node", "crates/oracle/h2-7a-owner-inventory.mjs", "--check"],
        300,
        ("curated Rust anchor", "is not at a tsc-port header", "anchor header"),
    ),
    (
        "h2-7a-close retained arms / anchors",
        ["node", "crates/oracle/h2-7a-close.mjs", "--check"],
        300,
        ("curated Rust anchor", "is not at a tsc-port header", "retained_arms anchor"),
    ),
)


def run(label: str, command: list[str], timeout: int, fragments: tuple[str, ...]) -> int:
    try:
        completed = subprocess.run(
            command, capture_output=True, text=True, timeout=timeout, check=False
        )
    except subprocess.TimeoutExpired:
        print(f"walk-static-checks: {label}: no precondition error within {timeout}s (pass; the walk mints it)")
        return 0
    except FileNotFoundError as error:
        print(f"walk-static-checks: {label}: cannot run {command[0]}: {error}")
        return 2
    text = completed.stdout + completed.stderr
    hits = [line for line in text.splitlines() if any(fragment in line for fragment in fragments)]
    if hits:
        print(f"walk-static-checks: {label}: PRECONDITION FAILED")
        for line in hits[:6]:
            print(f"  {line.strip()[:220]}")
        return 2
    verdict = "clean" if completed.returncode == 0 else f"exit {completed.returncode} (stale or other: the walk's job)"
    print(f"walk-static-checks: {label}: {verdict}")
    return 0


def main() -> int:
    worst = 0
    for label, command, timeout, fragments in CHECKS:
        worst = max(worst, run(label, command, timeout, fragments))
    if worst:
        print("walk-static-checks: fix the preconditions above (register the new runtime inputs;")
        print("  re-point the curated anchors with the ±3-line tsc-port window rule), then walk once.")
    return worst


if __name__ == "__main__":
    sys.exit(main())
