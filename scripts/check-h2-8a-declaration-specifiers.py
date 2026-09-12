#!/usr/bin/env python3
"""Check declaration-specifier input provenance and the pre-implementation gate."""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
PACKET = ROOT / "ratchets/h2-8a-declaration-specifiers-readiness.v1.json"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def require(value, message):
    if not value:
        raise ValueError(message)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--before-production", action="store_true")
    args = parser.parse_args()
    packet = json.loads(PACKET.read_text())
    require(packet["slice"] == "H2.8a-DECL-SPEC1", "wrong slice")
    require(packet["state"] == "ready-for-implementation", "design is not ready")
    require(packet["unresolved"] == [], "unresolved design rows")
    subprocess.run(["git", "merge-base", "--is-ancestor", packet["base"], "HEAD"],
                   cwd=ROOT, check=True)
    for pin in packet["authority_files"]:
        require(digest((ROOT / pin["path"]).read_bytes()) == pin["sha256"],
                "stale authority: " + pin["path"])
    for anchor in packet["source_anchors"]:
        lines = (ROOT / anchor["path"]).read_bytes().splitlines(keepends=True)
        body = b"".join(lines[anchor["start"] - 1:anchor["end"]])
        require(digest(body) == anchor["sha256"], "stale body: " + anchor["symbol"])
        require(anchor["symbol"].encode() in body, "wrong body: " + anchor["symbol"])
    allowed = set(packet["allowed_production_files"])
    require(len(allowed) == len(packet["allowed_production_files"]), "duplicate edit owner")
    if args.before_production:
        for path, expected in packet["production_before"].items():
            require(digest((ROOT / path).read_bytes()) == expected,
                    "production changed before readiness: " + path)
    for row in packet["semantic_rows"]:
        require(row["classification"] in ["already-exact", "missing", "partial-or-stale"],
                "unclassified semantic row")
        require(row["source_anchor"] in {a["symbol"] for a in packet["source_anchors"]},
                "unknown source anchor")
        require(row["step"] in packet["steps"] and row["witnesses"], "unmapped semantic row")
    require(packet["architecture_rows"] and all(row["symbols"] and row["evidence"]
            for row in packet["architecture_rows"]), "missing architecture mapping")
    baseline = json.loads((ROOT / packet["baseline_receipt"]).read_text())
    require(baseline["actual_exit"] == 101 and baseline["source_unchanged_after_run"],
            "baseline is not a preserved failing command run")
    require(baseline["complete_focused_captures"] == 48, "baseline did not execute both repetitions")
    require(baseline["original_failure_captures"] == 2, "original command repetition missing")
    require(baseline["binary"]["sha256"], "baseline binary identity missing")
    require(len(baseline["case_results"]) == 24, "baseline membership mismatch")
    for case in baseline["case_results"]:
        require(case["repetitions"] == [0, 1], "incomplete native repetition")
    composition = json.loads((ROOT / packet["composition_baseline_receipt"]).read_text())
    require(composition["actual_exit"] == 101 and composition["source_unchanged_after_run"],
            "composition baseline must retain its real failed result")
    require(composition["complete_focused_captures"] == 12
            and len(composition["case_results"]) == 6, "composition baseline incomplete")
    require(composition["binary"]["sha256"], "composition binary identity missing")
    require(all(case["repetitions"] == [0, 1] for case in composition["case_results"]),
            "composition repetition missing")
    artifact_path = ROOT / "crates/compiler/tests/fixtures/h2-8a-declaration-specifiers.json"
    artifact = json.loads(artifact_path.read_text())
    require(artifact["typescript"] == "6.0.3" and artifact["repetitions"] == 2,
            "oracle identity/repetition")
    require(artifact["upstream_failures"] == [] and len(artifact["cases"]) == 24,
            "oracle failed or membership changed")
    require(digest((ROOT / artifact["inputs"]["path"]).read_bytes())
            == artifact["inputs"]["sha256"], "stale input artifact")
    require(digest((ROOT / "scripts/observe-h2-8a-declaration-specifiers.mjs").read_bytes())
            == artifact["observer_sha256"], "stale observer")
    require({row["case_id"] for row in artifact["cases"]}
            == {row["case_id"] for row in baseline["case_results"]}, "native/oracle membership")
    print(json.dumps({"slice": packet["slice"], "gate": "pass", "cases": 30,
                      "semantic_rows": len(packet["semantic_rows"]),
                      "unresolved": 0, "before_production": args.before_production}))


if __name__ == "__main__":
    main()
