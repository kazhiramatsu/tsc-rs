#!/usr/bin/env python3
"""Check the frozen H2.8a A1 design inputs before production edits."""
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent
manifest = json.loads((ROOT / "ratchets/h2-8a-directory-readiness.v1.json").read_text())
assert manifest["version"] == 1
assert manifest["slice"] == "H2.8a-A1"
assert manifest["unresolved"] == manifest["undispositioned"] == 0
digest = lambda data: hashlib.sha256(data).hexdigest()
for item in manifest["authorities"]:
    assert digest((ROOT / item["path"]).read_bytes()) == item["sha256"], item["path"]
for item in manifest["baseline_rust"]:
    data = subprocess.check_output(["git", "show", f'{manifest["base"]}:{item["path"]}'], cwd=ROOT)
    assert digest(data) == item["sha256"], item["path"]
lines = (ROOT / "vendor/typescript-6.0.3/lib/_tsc.js").read_bytes().splitlines(keepends=True)
assert len(manifest["owners"]) == 7
assert {item["step"] for item in manifest["owners"]} == {f"A1-{i}" for i in range(1, 7)}
packet = (ROOT / "docs/design/greenfield/slices/h2-8.md").read_text()
for item in manifest["owners"]:
    assert digest(b"".join(lines[item["start"] - 1:item["end"]])) == item["sha256"], item["owner"]
    assert item["step"] in packet and item["owner"] in packet
    assert (ROOT / item["test"]).is_file(), item["test"]
architecture = (ROOT / "docs/design/greenfield/emitter-architecture.md").read_text()
assert len(manifest["architecture"]) == 6
for item in manifest["architecture"]:
    assert f'| `{item["id"]}` |' in architecture
    assert item["id"] in packet and item["disposition"] in packet
fixture = json.loads((ROOT / "crates/compiler/tests/fixtures/output-directories.json").read_text())
assert fixture["repetitions"] == 2
assert len(fixture["cases"]) == len({case["case_id"] for case in fixture["cases"]}) == 34
assert fixture["observer_sha256"] == digest((ROOT / "scripts/observe-output-directories.mjs").read_bytes())
extra = json.loads((ROOT / "crates/compiler/tests/fixtures/module-export-identifiers.json").read_text())
assert extra["repetitions"] == 2
assert len(extra["cases"]) == len({case["case_id"] for case in extra["cases"]}) == 8
assert extra["observer_sha256"] == digest((ROOT / "scripts/observe-module-export-identifiers.mjs").read_bytes())
print("H2.8a-A1 amended ready: 7 owners, 6 steps, 6 architecture rows, 42 witnesses; unresolved=0, undispositioned=0")
