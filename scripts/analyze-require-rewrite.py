#!/usr/bin/env python3
"""Validate final complete captures and write a new immutable receipt index."""
import hashlib
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parent.parent
run = ROOT / "target/dec-next-runs" / sys.argv[1]
destination = ROOT / sys.argv[2]
assert not destination.exists(), "retain earlier evidence"
sha = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
prelaunch = json.loads((run / "prelaunch.json").read_text())
execution = json.loads((run / "execution.json").read_text())
assert execution["actual_exit"] == 0
assert execution["inputs_changed_during_run"] == []
assert execution["prelaunch_sha256"] == sha(run / "prelaunch.json")
assert execution["log_sha256"] == sha(run / "run.log")
assert prelaunch["archive"]["sha256"] == sha(Path(prelaunch["archive"]["path"]))
for path, record in execution["archived_binaries"].items():
    assert sha(ROOT / path) == record["sha256"]
expected_ids = set()
for fixture in ["h2-8a-require-rewrite.json", "h2-8a-require-rewrite-composition.json", "h2-8a-require-rewrite-substitution.json"]:
    artifact = json.loads((ROOT / "crates/compiler/tests/fixtures" / fixture).read_text())
    expected_ids.update(case["case_id"] for case in artifact["cases"])
assert len(expected_ids) == 68
captures = {}
by_id = {}
for path in sorted((run / "captures").glob("*.json")):
    capture = json.loads(path.read_text())
    case_id = capture["case_id"]
    assert case_id in expected_ids
    assert capture["error"] is None and capture["partial_writes"] is None
    assert capture["actual"] == capture["expected"], case_id
    by_id.setdefault(case_id, []).append(capture["actual"])
    captures[path.name] = sha(path)
assert set(by_id) == expected_ids
assert all(len(rows) == 2 and rows[0] == rows[1] for rows in by_id.values())
log = (run / "run.log").read_text()
assert "test result: ok. 4 passed; 0 failed" in log
assert log.count("H2.8a output matrix EXACT x2") == 2
before = json.loads((ROOT / "ratchets/h2-8a-require-rewrite-before.v1.json").read_text())
prior = set(before["focused_exact_twice"])
assert len(prior) == 15 and prior <= expected_ids
baseline_captures = Path(before["run_directory"]) / "captures"
for path in baseline_captures.glob("*.json"):
    capture = json.loads(path.read_text())
    if capture["case_id"] in prior:
        assert capture["actual"] == by_id[capture["case_id"]][0], capture["case_id"]
production = {}
for path in ["crates/emitter/src/builtins.rs", "crates/emitter/src/builtins/relative_imports.rs", "crates/emitter/src/printer.rs"]:
    assert sha(ROOT / path) == prelaunch["inputs"][path], path
    production[path] = sha(ROOT / path)
receipt = {
    "version": 1,
    "status": "focused-and-original-complete-commands-exact-twice",
    "run_directory": str(run),
    "measured_head": prelaunch["head"],
    "execution": execution,
    "production": production,
    "focused_count": 60,
    "focused_exact_twice": 60,
    "focused_repairs": 45,
    "prior_positive_payloads_unchanged": 15,
    "composition_exact_twice": 4,
    "substitution_exact_twice": 4,
    "original_exact_twice": 2,
    "supplemental_command_executions": 136,
    "captures": captures,
    "exact_ids": sorted(expected_ids),
    "analyzer_sha256": sha(Path(__file__).resolve()),
}
with destination.open("x") as stream:
    stream.write(json.dumps(receipt, indent=2) + "\n")
print("68 focused/composition + 2 original commands exact twice; 45 repairs, 15 preserved positives; 136 supplemental captures exact")
