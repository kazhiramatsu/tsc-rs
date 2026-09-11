#!/usr/bin/env python3
"""Verify the readonly repair's trial-local diagnostic-cache prerequisite."""
from pathlib import Path
import hashlib
import json
import subprocess

ROOT = Path(__file__).resolve().parent.parent
read = lambda p: json.loads((ROOT / p).read_text())
digest = lambda b: hashlib.sha256(b).hexdigest()
subprocess.run(["python3", "scripts/check-defineproperty-readonly-readiness.py"], cwd=ROOT, check=True)
m = read("ratchets/h2-8a-nonexistent-property-cache-readiness.v1.json")
initial = read("ratchets/h2-8a-defineproperty-readonly-readiness.v1.json")
assert m["version"] == 1 and m["slice"] == "H2.8a-A6-26-shared-prerequisite"
assert m["base"] == initial["base"] and m["unresolved"] == m["undispositioned"] == 0
assert m["runtime_paths"] == sorted(initial["runtime_paths"] + ["crates/checker/src/links.rs"])
assert m["additional_production_path"] == "crates/checker/src/links.rs"
assert m["steps"] == ["A6-26-4", "A6-26-5"]
for r in m["authorities"] + m["initial_runtime_snapshot"]:
    assert digest((ROOT / r["path"]).read_bytes()) == r["sha256"], r["path"]
for r in m["baseline_rust"]:
    assert digest(subprocess.check_output(["git", "show", f'{m["base"]}:{r["path"]}'], cwd=ROOT)) == r["sha256"]
packet = (ROOT / "docs/design/greenfield/slices/h2-8a-nonexistent-property-cache.md").read_text()
assert all(step in packet for step in m["steps"])
assert m["owners"][:24] == initial["owners"] and len(m["owners"]) == 26
lines = (ROOT / "vendor/typescript-6.0.3/lib/_tsc.js").read_bytes().splitlines(keepends=True)
for r in m["owners"]:
    assert digest(b"".join(lines[r["start"] - 1:r["end"]])) == r["sha256"], r["owner"]
    assert lines[r["start"] - 1].decode().strip().startswith("function " + r["owner"] + "(")
assert m["architecture"] == initial["architecture"] and len(m["architecture"]) == 7
for r in m["architecture"]:
    assert r["id"] in packet and r["disposition"] in packet
first = read("ratchets/h2-8a-defineproperty-readonly-first-runtime-after.v1.json")
assert len(first["fresh_exact_twice"]) == 492 and first["completed_fresh_command_executions"] == 984
assert first["original_command_attempts"] == 1 and first["original_completed_commands"] == 0
assert first["original_other_cases_not_run"] == 9 and first["original_full_failure_tuples"] == 0
assert first["failure_boundary"] == "native stack overflow / SIGABRT before complete tuple capture"
units = read("ratchets/h2-8a-defineproperty-readonly-checker-first-after.v1.json")
assert units["passed"] == 1718 and units["failed"] == 0
before = read("ratchets/h2-8a-nonexistent-property-cache-native-before.v1.json")
assert before["base"] == m["base"] and before["tests"]["passed"] == 0 and before["tests"]["failed"] == 2
assert before["tests"]["filtered_out"] == 1718 and before["complete_command_executions"] == 0
assert m["native_before_tests"] == 2 and m["required_full_checker_tests"] == 1720
assert m["required_final_complete_commands"] == 502 and m["required_owned_and_positive_exact_twice"] == 501
assert m["probe_required_before_full_replay"] is True
assert m["optional_unchanged_outside_case"] == initial["after_optional_unchanged_outside_case"]
assert m["journal_restored_on"] == ["Commit", "Rollback", "Reject", "CheckAbort"]
print("H2.8a-A6-26 prerequisite ready: 26 whole TS owners, 11 production paths, 2 steps; first-cycle abort frozen; two native failures; final 1720 units/502 commands; unresolved=0, undispositioned=0")
