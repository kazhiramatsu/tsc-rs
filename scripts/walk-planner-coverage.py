#!/usr/bin/env python3
"""Planner coverage self-check (gate-tax 8, S4).

The prospective planner's LADDER_ORDER must equal the driver's ORDER
exactly (same rungs, same sequence). A planner that lags ORDER reports
an incomplete stale cone: the 2026-08-30 W4 runs planned 63/65 rungs,
silently missing the h2-6c rows. The slice that extends ORDER must
extend new-ci/src/bin/plan.rs in the same commit (WALK_DRY verifies).

Usage: walk-planner-coverage.py <rung>... (the ORDER list, in order)
Exit 0 = in sync; exit 1 = drift (details on stdout).
"""
import re
import sys

PLAN_RS = "new-ci/src/bin/plan.rs"

order = sys.argv[1:]
if not order:
    print("PLANNER DRIFT: no ORDER rungs passed to walk-planner-coverage.py")
    sys.exit(1)

try:
    src = open(PLAN_RS, encoding="utf-8").read()
except OSError as err:
    print(f"PLANNER DRIFT: cannot read {PLAN_RS}: {err}")
    sys.exit(1)

match = re.search(r"const LADDER_ORDER: &\[&str\] = &\[(.*?)\];", src, re.S)
if not match:
    print(f"PLANNER DRIFT: LADDER_ORDER const not found in {PLAN_RS}")
    sys.exit(1)

ladder = re.findall(r'"([a-z0-9-]+)"', match.group(1))
if ladder != order:
    missing = [name for name in order if name not in ladder]
    extra = [name for name in ladder if name not in order]
    detail = []
    if missing:
        detail.append(f"missing={missing}")
    if extra:
        detail.append(f"extra={extra}")
    if not detail:
        detail.append("same rungs, different sequence")
    print(
        "PLANNER DRIFT: LADDER_ORDER != ORDER ("
        + ", ".join(detail)
        + f") — extend {PLAN_RS} in this slice"
    )
    sys.exit(1)

print(f"coverage: planner LADDER_ORDER in sync ({len(ladder)} rungs)")
