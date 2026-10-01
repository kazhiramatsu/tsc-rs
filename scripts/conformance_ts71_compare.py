#!/usr/bin/env python3
"""Compare two TypeScript 7.1 conformance reports configuration by configuration.

usage: conformance_ts71_compare.py <reference report.json> <other report.json>

The reports come from scripts/conformance_ts71.py: the reference is the
one-checker run (target/conformance-ts71/<profile>/report.json) and the other a
sharded control (`--checkers <n>`, target/conformance-ts71/<profile>/checkers-<n>/
report.json) or a run at another source revision. Every lane-A configuration is
compared on its outcome, its tier and the digest of tsc-rs's rendered error
baseline (`rendered_sha256`): two configurations with the same digest produced
the same diagnostics in the same order. The script prints the configurations
that differ, grouped by how, and exits 1 when any differ.
"""

import json
import pathlib
import sys


def rows(path):
    report = json.loads(pathlib.Path(path).read_text())
    out = {}
    for result in report["results"]:
        key = f"{result['suite']}/{result['stem'] or result['case']}"
        out[key] = result
    return report["profile"], out


def describe(result):
    status = result["status"]
    if status == "compared":
        return f"{result['agreement']} ({result['expected']} expected, {result['actual']} actual)"
    if status == "harness-error":
        return f"harness error: {result['reason'].splitlines()[0][:120]}"
    return status


def main(argv):
    if len(argv) != 3:
        sys.exit(__doc__)
    reference_profile, reference = rows(argv[1])
    other_profile, other = rows(argv[2])
    if reference_profile != other_profile:
        sys.exit(f"profiles differ: {reference_profile} vs {other_profile}")
    groups = {"only in one report": [], "outcome": [], "tier": [], "diagnostics": []}
    same = 0
    for key in sorted(set(reference) | set(other)):
        left, right = reference.get(key), other.get(key)
        if left is None or right is None:
            groups["only in one report"].append((key, "reference" if left else "other"))
            continue
        if left["status"] != right["status"]:
            groups["outcome"].append((key, describe(left), describe(right)))
            continue
        if left["status"] != "compared":
            same += 1
            continue
        if left["agreement"] != right["agreement"]:
            groups["tier"].append((key, describe(left), describe(right)))
            continue
        if left.get("rendered_sha256") != right.get("rendered_sha256"):
            groups["diagnostics"].append((key, describe(left), describe(right)))
            continue
        same += 1
    for title, items in groups.items():
        if not items:
            continue
        print(f"{title}: {len(items)}")
        for item in items:
            print("  " + " | ".join(str(part) for part in item))
    differing = sum(len(items) for items in groups.values())
    print(f"{same} configurations identical, {differing} differ ({reference_profile})")
    return 1 if differing else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
