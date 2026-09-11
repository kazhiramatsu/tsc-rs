#!/usr/bin/env python3
"""Check the bounded H2.8a root diagnostic streams dependency packet."""
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent
read = lambda name: json.loads((ROOT / name).read_text())
digest = lambda data: hashlib.sha256(data).hexdigest()
row_digest = lambda row: digest(json.dumps(row, ensure_ascii=False, separators=(",", ":")).encode())
manifest = read("ratchets/h2-8a-root-diagnostic-readiness.v1.json")
assert manifest["version"] == 1 and manifest["slice"] == "H2.8a-A6-3"
assert manifest["unresolved"] == manifest["undispositioned"] == 0
for row in manifest["authorities"]:
    assert digest((ROOT / row["path"]).read_bytes()) == row["sha256"], row["path"]
for row in manifest["baseline_rust"]:
    original = subprocess.check_output(["git", "show", f'{manifest["base"]}:{row["path"]}'], cwd=ROOT)
    assert digest(original) == row["sha256"], row["path"]
packet = (ROOT / "docs/design/greenfield/slices/h2-8a-root-diagnostic-streams.md").read_text()
lines = (ROOT / "vendor/typescript-6.0.3/lib/_tsc.js").read_bytes().splitlines(keepends=True)
assert len(manifest["owners"]) == 6
for row in manifest["owners"]:
    assert digest(b"".join(lines[row["start"] - 1:row["end"]])) == row["sha256"], row["owner"]
    assert row["owner"] in packet
assert manifest["steps"] == ["A6-3-1", "A6-3-2"]
assert all(step in packet for step in manifest["steps"])
architecture = (ROOT / "docs/design/greenfield/emitter-architecture.md").read_text()
assert len(manifest["architecture"]) == 2
for row in manifest["architecture"]:
    assert f'| `{row["id"]}` |' in architecture and row["id"] in packet
    assert row["disposition"] in packet
assert "original_root_diagnostic_streams_match_complete_commands" in (ROOT / manifest["test"]).read_text()
inputs = {r["case_id"]: r for r in read("ratchets/h2-8a-candidate-inputs.v1.json")["cases"]}
observations = {r["case_id"]: r for r in read("ratchets/h2-8a-observations.v1.json")["cases"]}
candidates = {r["case_id"]: r for r in read("ratchets/h2-8a-candidates.v1.json")["cases"]}
before = read("ratchets/h2-8a-global-before.v1.json")
assert before["status"] == "complete-before" and before["revision"] == manifest["base"]
assert before["exact"] == 666 and before["failed"] == 103 and before["eligible"] == 769
failed = {r["case_id"] for r in before["failures"]}
assert len(failed) == 103 and len(before["failures"]) == 206
assert len(manifest["witnesses"]) == len({r["case_id"] for r in manifest["witnesses"]}) == 6
for row in manifest["witnesses"]:
    name = row["case_id"]
    assert candidates[name]["required_slices"] == ["H2.8a"]
    assert row_digest(inputs[name]) == row["input_sha256"] == candidates[name]["input_sha256"]
    assert row_digest(observations[name]) == row["observation_sha256"]
    assert observations[name]["repetitions"] == 2
    assert (name in failed) == (row["before"] == "failed")
    if name in failed:
        assert {r["repetition"] for r in before["failures"] if r["case_id"] == name} == {0, 1}
assert sum(r["before"] == "failed" for r in manifest["witnesses"]) == 2
print("H2.8a-A6-3 ready: 6 owners, 2 steps, 2 architecture rows, 6 complete original witnesses (2 failed / 4 exact before); unresolved=0, undispositioned=0")
