#!/usr/bin/env python3
"""Mint added target controls and verify every prior input/observation survives."""
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[6]
BASE = "ed71c45343b2063414d5563c9b5455631d7c5c68"
FIXTURES = "crates/compiler/tests/fixtures/h2_8c_transpile/"
SCRIPT = "scripts/observe-transpile-routes.mjs"

def run(*args):
    subprocess.run(["node", SCRIPT, *args], cwd=ROOT, check=True)

old = {name: subprocess.check_output(["git", "show", BASE + ":" + FIXTURES + name], cwd=ROOT)
       for name in ("inputs.v1.json", "expected.v1.json")}
run("--write-inputs", FIXTURES + "inputs.v1.json")
run("--inputs", FIXTURES + "inputs.v1.json", "--out", FIXTURES + "expected.v1.json")
records = []
for name, before in old.items():
    after = (ROOT / FIXTURES / name).read_bytes()
    previous, current = json.loads(before), json.loads(after)
    assert current["case_count"] == 291
    assert previous["cases"] == current["cases"][:287], name
    records.append({"path": FIXTURES + name, "unchanged_cases": 287, "added_cases": 4,
                    "before_sha256": hashlib.sha256(before).hexdigest(),
                    "after_sha256": hashlib.sha256(after).hexdigest()})
with tempfile.TemporaryDirectory() as temporary:
    second = str(Path(temporary) / "expected.json")
    run("--inputs", FIXTURES + "inputs.v1.json", "--out", second)
    assert Path(second).read_bytes() == (ROOT / FIXTURES / "expected.v1.json").read_bytes()
(Path(__file__).resolve().parent / "records/audit-transpile-oracle.v1.json").write_text(
    json.dumps({"schema": 1, "repetitions": 2, "artifacts": records}, indent=2) + "\n")
