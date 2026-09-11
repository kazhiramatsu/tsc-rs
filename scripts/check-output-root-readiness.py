#!/usr/bin/env python3
"""Validate the H2.8a A2 source, witness and implementation map."""
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent
manifest = json.loads((ROOT / "ratchets/h2-8a-root-readiness.v1.json").read_text())
assert manifest["version"] == 1 and manifest["slice"] == "H2.8a-A2"
assert manifest["unresolved"] == manifest["undispositioned"] == 0
digest = lambda data: hashlib.sha256(data).hexdigest()
for item in manifest["authorities"]:
    assert digest((ROOT / item["path"]).read_bytes()) == item["sha256"], item["path"]
for item in manifest.get("amended_dependencies", []):
    original = subprocess.check_output(["git", "show", f'{item["baseline"]}:{item["path"]}'], cwd=ROOT)
    current = (ROOT / item["path"]).read_bytes()
    marker = item["marker"].encode()
    assert digest(original) == item["original_file_sha256"]
    assert original.count(marker) == current.count(marker) == 1
    assert digest(original[original.index(marker):]) == item["unchanged_tail_sha256"]
    assert current[current.index(marker):] == original[original.index(marker):]
    assert (ROOT / item["amendment"]).is_file()
for item in manifest.get("amended_metadata_dependencies", []):
    original = subprocess.check_output(["git", "show", f'{item["baseline"]}:{item["path"]}'], cwd=ROOT)
    current = (ROOT / item["path"]).read_bytes()
    start, end = item["start_marker"].encode(), item["end_marker"].encode()
    assert digest(original) == item["original_file_sha256"]
    assert original.count(start) == current.count(start) == 1
    assert original.count(end) == current.count(end) == 1
    assert original[:original.index(start)] == current[:current.index(start)]
    assert original[original.index(end):] == current[current.index(end):]
    assert digest(current[current.index(start):current.index(end)]) == item["current_region_sha256"]
    assert (ROOT / item["amendment"]).is_file()
for item in manifest["baseline_rust"]:
    data = subprocess.check_output(["git", "show", f'{manifest["base"]}:{item["path"]}'], cwd=ROOT)
    assert digest(data) == item["sha256"], item["path"]
packet = (ROOT / "docs/design/greenfield/slices/h2-8a-roots.md").read_text()
lines = (ROOT / "vendor/typescript-6.0.3/lib/_tsc.js").read_bytes().splitlines(keepends=True)
assert {row["step"] for row in manifest["owners"]} == {f"A2-{i}" for i in range(1, 7)}
assert len(manifest["owners"]) == 15
for row in manifest["owners"]:
    assert digest(b"".join(lines[row["start"] - 1:row["end"]])) == row["sha256"], row["owner"]
    assert row["step"] in packet
    assert (ROOT / row["test"]).is_file()
architecture = (ROOT / "docs/design/greenfield/emitter-architecture.md").read_text()
assert len(manifest["architecture"]) == 5
for row in manifest["architecture"]:
    assert f'| `{row["id"]}` |' in architecture and row["id"] in packet
    assert row["disposition"] in packet
fixture = json.loads((ROOT / "crates/compiler/tests/fixtures/output-roots.json").read_text())
assert fixture["repetitions"] == 2
assert len(fixture["cases"]) == 27 and len(fixture["supplemental_cases"]) == 13
assert len(fixture["edge_cases"]) == 10
cases = fixture["cases"] + fixture["supplemental_cases"] + fixture["edge_cases"]
assert len({row["case_id"] for row in cases}) == 50
assert fixture["observer_sha256"] == digest((ROOT / "scripts/observe-output-roots.mjs").read_bytes())
print("H2.8a-A2 ready: 15 upstream rows, 6 steps, 5 architecture rows, 50 witnesses; unresolved=0, undispositioned=0")
