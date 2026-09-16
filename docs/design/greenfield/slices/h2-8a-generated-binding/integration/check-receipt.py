#!/usr/bin/env python3
"""Validate this received snapshot and review evidence, without admitting runtime changes."""
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
receipt = json.loads((HERE / "received.v1.json").read_text())
assert receipt["status"] == "changes-required"
assert not receipt["production_applied"] and not receipt["hosted_registration_applied"]
for row in receipt["submission_files"]:
    path = HERE.parent / row["path"]
    content = path.read_bytes()
    assert len(content) == row["bytes"], path
    assert hashlib.sha256(content).hexdigest() == row["sha256"], path
for row in receipt["review_files"]:
    assert hashlib.sha256((HERE / row["path"]).read_bytes()).hexdigest() == row["sha256"], row
comparisons = {}
for label, expected_exact in (("baseline", 6), ("candidate", 1)):
    probe = json.loads((HERE / f"{label}-probe.v1.json").read_text())
    assert (probe["rows"], probe["repetitions"], probe["exact"], probe["mismatches"]) == (
        7, 2, expected_exact, 7 - expected_exact)
    assert len(probe["commands"]) == 4 and all(command["exit"] == 0 for command in probe["commands"])
    comparisons[label] = {row["key"]: row["exact"] for row in probe["comparison"]}
    for owner in ("native", "typescript"):
        first = (HERE / f"{label}-{owner}-1.jsonl").read_bytes()
        second = (HERE / f"{label}-{owner}-2.jsonl").read_bytes()
        assert first == second, (label, owner)
        assert len(first.splitlines()) == 7
assert comparisons["baseline"].keys() == comparisons["candidate"].keys()
regressions = [key for key in comparisons["baseline"]
               if comparisons["baseline"][key] and not comparisons["candidate"][key]]
assert len(regressions) == receipt["controls"]["new_regressions"] == 5
print(json.dumps({"receipt": "valid", "production_admission": "blocked",
                  "submission_files": len(receipt["submission_files"]),
                  "new_regressions": len(regressions)}))
