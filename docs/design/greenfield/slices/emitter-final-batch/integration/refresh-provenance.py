#!/usr/bin/env python3
"""Regenerate current dependent artifacts; reject any case/input/observation change."""
import hashlib
import json
from pathlib import Path
import subprocess
ROOT = Path(__file__).resolve().parents[6]
paths = [f"ratchets/h2-{phase}-{kind}.v1.json" for phase in ("7de", "8a")
         for kind in ("candidates", "candidate-inputs", "observations")]
before = {name: (ROOT / name).read_bytes() for name in paths}
for phase in ("7de", "8a"):
    for kind in ("candidates", "observations"):
        subprocess.run(["node", f"crates/oracle/h2-{phase}-{kind}.mjs", "--write"], cwd=ROOT, check=True)
rows = []
for name, old in before.items():
    new = (ROOT / name).read_bytes()
    a, b = json.loads(old), json.loads(new)
    changed = [key for key in a if a[key] != b[key]]
    assert set(a) == set(b) and set(changed) <= {"inputs"}, (name, changed)
    rows.append({"path": name, "changed_fields": changed, "cases_unchanged": len(a["cases"]),
                 "before_sha256": hashlib.sha256(old).hexdigest(), "after_sha256": hashlib.sha256(new).hexdigest()})
output = Path(__file__).resolve().parent / "records/current-provenance-refresh.v1.json"
output.write_text(json.dumps({"schema": 1, "artifacts": rows}, indent=2) + "\n")
