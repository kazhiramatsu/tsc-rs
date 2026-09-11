#!/usr/bin/env python3
"""Validate the H2.8a A4 source, witness and implementation map."""
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent
manifest = json.loads((ROOT / "ratchets/h2-8a-corpus-readiness.v1.json").read_text())
assert manifest["version"] == 1 and manifest["slice"] == "H2.8a-A4"
assert manifest["unresolved"] == manifest["undispositioned"] == 0
digest = lambda data: hashlib.sha256(data).hexdigest()
for item in manifest["authorities"]:
    assert digest((ROOT / item["path"]).read_bytes()) == item["sha256"], item["path"]
for item in manifest["baseline_rust"]:
    data = subprocess.check_output(["git", "show", f'{manifest["base"]}:{item["path"]}'], cwd=ROOT)
    assert digest(data) == item["sha256"], item["path"]
packet = (ROOT / "docs/design/greenfield/slices/h2-8a-corpus.md").read_text()
lines = (ROOT / "vendor/typescript-6.0.3/lib/_tsc.js").read_bytes().splitlines(keepends=True)
assert len(manifest["owners"]) == 8
assert {row["step"] for row in manifest["owners"]} == {"A4-1", "A4-2", "A4-3", "A4-4"}
for row in manifest["owners"]:
    assert digest(b"".join(lines[row["start"] - 1:row["end"]])) == row["sha256"], row["owner"]
    assert row["step"] in packet and (ROOT / row["test"]).is_file()
architecture = (ROOT / "docs/design/greenfield/emitter-architecture.md").read_text()
assert len(manifest["architecture"]) == 4
for row in manifest["architecture"]:
    assert f'| `{row["id"]}` |' in architecture and row["id"] in packet
    assert row["disposition"] in packet
fixture = json.loads((ROOT / "crates/compiler/tests/fixtures/system-dynamic-imports.json").read_text())
assert fixture["repetitions"] == 2 and len(fixture["cases"]) == 24
assert len({row["case_id"] for row in fixture["cases"]}) == 24
assert fixture["observer_sha256"] == digest((ROOT / "scripts/observe-system-dynamic-imports.mjs").read_bytes())
corpus = json.loads((ROOT / "ratchets/h2-8a-output-directory-corpus.v1.json").read_text())
assert corpus["repetitions"] == 2 and len(corpus["cases"]) == 23
for pin in [corpus["generator"], *corpus["inputs"]]:
    assert digest((ROOT / pin["path"]).read_bytes()) == pin["sha256"], pin["path"]
assert len({row["case_id"] for row in corpus["cases"]}) == 23
print("H2.8a-A4 ready: 8 upstream rows, 4 steps, 4 architecture rows, 24 focused + 23 original witnesses; unresolved=0, undispositioned=0")
