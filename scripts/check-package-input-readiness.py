#!/usr/bin/env python3
"""Validate H2.8a A5's package input-resolution owner and witness closure."""
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent
manifest = json.loads((ROOT / "ratchets/h2-8a-package-input-readiness.v1.json").read_text())
digest = lambda data: hashlib.sha256(data).hexdigest()
assert manifest["version"] == 1 and manifest["slice"] == "H2.8a-A5"
assert manifest["unresolved"] == manifest["undispositioned"] == 0
for row in manifest["authorities"]:
    assert digest((ROOT / row["path"]).read_bytes()) == row["sha256"], row["path"]
for row in manifest["baseline_rust"]:
    original = subprocess.check_output(["git", "show", f'{manifest["base"]}:{row["path"]}'], cwd=ROOT)
    assert digest(original) == row["sha256"], row["path"]
packet = (ROOT / "docs/design/greenfield/slices/h2-8a-package-inputs.md").read_text()
lines = (ROOT / "vendor/typescript-6.0.3/lib/_tsc.js").read_bytes().splitlines(keepends=True)
assert len(manifest["owners"]) == 13
assert {row["id"] for row in manifest["steps"]} == {f"A5-{i}" for i in range(1, 7)}
for row in manifest["owners"]:
    assert digest(b"".join(lines[row["start"] - 1:row["end"]])) == row["sha256"], row["owner"]
    assert row["owner"] in packet and row["step"] in packet
    assert (ROOT / row["test"]).is_file()
for row in manifest["steps"]:
    assert row["id"] in packet and (ROOT / row["test"]).is_file()
architecture = (ROOT / "docs/design/greenfield/emitter-architecture.md").read_text()
assert len(manifest["architecture"]) == 5
for row in manifest["architecture"]:
    assert f'| `{row["id"]}` |' in architecture and row["id"] in packet
    assert row["disposition"] in packet
fixture = json.loads((ROOT / "crates/compiler/tests/fixtures/package-output-inputs.json").read_text())
assert fixture["repetitions"] == 2 and len(fixture["cases"]) == 46
assert len({row["case_id"] for row in fixture["cases"]}) == 46
assert fixture["observer_sha256"] == digest((ROOT / "scripts/observe-package-output-inputs.mjs").read_bytes())
assert fixture["compiler_sha256"] == digest((ROOT / "vendor/typescript-6.0.3/lib/typescript.js").read_bytes())
for row in fixture["cases"]:
    observed = row["typescript_observation"]
    assert {"loaded_files", "resolutions", "type_resolutions", "writes", "reported_diagnostics", "emit_result", "exit_code"} <= observed.keys()
format_fixture = json.loads((ROOT / "crates/compiler/tests/fixtures/output-root-format.json").read_text())
assert format_fixture["repetitions"] == 2 and len(format_fixture["cases"]) == 50
assert len({row["case_id"] for row in format_fixture["cases"]}) == 50
assert format_fixture["observer_sha256"] == digest((ROOT / "scripts/observe-output-root-format.mjs").read_bytes())
print("H2.8a-A5 ready: 13 upstream rows, 6 steps, 5 architecture rows, 46 + 50 complete witnesses; unresolved=0, undispositioned=0")
