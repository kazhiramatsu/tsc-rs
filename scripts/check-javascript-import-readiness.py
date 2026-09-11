#!/usr/bin/env python3
"""Validate the bounded JavaScript import retention packet before runtime edits."""
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent
read = lambda name: json.loads((ROOT / name).read_text())
digest = lambda data: hashlib.sha256(data).hexdigest()
row_digest = lambda row: digest(json.dumps(row, ensure_ascii=False, separators=(",", ":")).encode())
m = read("ratchets/h2-8a-javascript-import-readiness.v1.json")
assert m["version"] == 1 and m["slice"] == "H2.8a-A6-4"
assert m["unresolved"] == m["undispositioned"] == 0
for row in m["authorities"]:
    assert digest((ROOT / row["path"]).read_bytes()) == row["sha256"], row["path"]
for row in m["baseline_rust"]:
    original = subprocess.check_output(["git", "show", f'{m["base"]}:{row["path"]}'], cwd=ROOT)
    assert digest(original) == row["sha256"], row["path"]
packet = (ROOT / "docs/design/greenfield/slices/h2-8a-javascript-imports.md").read_text()
lines = (ROOT / "vendor/typescript-6.0.3/lib/_tsc.js").read_bytes().splitlines(keepends=True)
assert len(m["owners"]) == 12
for row in m["owners"]:
    assert digest(b"".join(lines[row["start"] - 1:row["end"]])) == row["sha256"]
    assert row["owner"] in packet
assert m["steps"] == ["A6-4-1", "A6-4-2", "A6-4-3", "A6-4-4"]
assert all(step in packet for step in m["steps"])
architecture = (ROOT / "docs/design/greenfield/emitter-architecture.md").read_text()
assert len(m["architecture"]) == 4
for row in m["architecture"]:
    assert f'| `{row["id"]}` |' in architecture and row["id"] in packet
    assert row["disposition"] in packet
assert "javascript_import_retention_matches_complete_typescript_observations" in (ROOT / m["tests"][0]).read_text()
assert "original_javascript_imports_match_complete_commands" in (ROOT / m["tests"][1]).read_text()
fixture = read("crates/compiler/tests/fixtures/javascript-import-retention.json")
assert fixture["typescript"] == "6.0.3" and fixture["repetitions"] == 2
assert fixture["observer_sha256"] == digest((ROOT / "scripts/observe-javascript-import-retention.mjs").read_bytes())
assert fixture["compiler_sha256"] == digest((ROOT / "vendor/typescript-6.0.3/lib/typescript.js").read_bytes())
cases = {r["case_id"]: r for r in fixture["cases"]}
before = read("ratchets/h2-8a-javascript-import-before.v1.json")
assert before["status"] == "complete-before" and before["eligible"] == 42
assert len(before["exact"]) == 25 and len(before["failed"]) == 17
assert set(before["exact"]).isdisjoint(before["failed"])
assert set(before["exact"]) | set(before["failed"]) == set(cases)
assert len(m["witnesses"]) == len(cases) == 42
for row in m["witnesses"]:
    assert row_digest(cases[row["case_id"]]) == row["row_sha256"]
    assert (row["case_id"] in before["failed"]) == (row["before"] == "failed")
inputs = {r["case_id"]: r for r in read("ratchets/h2-8a-candidate-inputs.v1.json")["cases"]}
observations = {r["case_id"]: r for r in read("ratchets/h2-8a-observations.v1.json")["cases"]}
candidates = {r["case_id"]: r for r in read("ratchets/h2-8a-candidates.v1.json")["cases"]}
global_before = read("ratchets/h2-8a-global-before.v1.json")
assert len(m["originals"]) == 7
for row in m["originals"]:
    name = row["case_id"]
    assert candidates[name]["required_slices"] == ["H2.8a"]
    assert row_digest(inputs[name]) == row["input_sha256"] == candidates[name]["input_sha256"]
    assert row_digest(observations[name]) == row["observation_sha256"]
    assert observations[name]["repetitions"] == 2
    assert {r["repetition"] for r in global_before["failures"] if r["case_id"] == name} == {0, 1}
print("H2.8a-A6-4 ready: 12 owners, 4 steps, 4 architecture rows, 42 fresh witnesses (17 failed / 25 exact before), 7 original witnesses; unresolved=0, undispositioned=0")
