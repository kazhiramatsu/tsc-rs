#!/usr/bin/env python3
"""Reobserve audit controls twice and verify every existing observation survives."""
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[6]
records = []
for script, name, old_count, new_count in (
    ("scripts/observe-import-helpers.mjs", "crates/compiler/tests/fixtures/import-helpers.json", 111, 480),
    ("scripts/observe-output-directory-corpus.mjs", "ratchets/h2-8a-output-directory-corpus.v1.json", 23, 23),
):
    path = ROOT / name
    old_bytes = subprocess.check_output(["git", "show", "ed71c45343b2063414d5563c9b5455631d7c5c68:" + name], cwd=ROOT)
    before = json.loads(old_bytes)
    assert len(before["cases"]) == old_count
    subprocess.run(["node", script, "--write"], cwd=ROOT, check=True)
    new_bytes = path.read_bytes()
    after = json.loads(new_bytes)
    assert len(after["cases"]) == new_count
    assert before["cases"] == after["cases"][:old_count], name
    records.append({"path": name, "before_sha256": hashlib.sha256(old_bytes).hexdigest(),
                    "after_sha256": hashlib.sha256(new_bytes).hexdigest(),
                    "unchanged_cases": old_count, "added_cases": new_count - old_count,
                    "repetitions": after["repetitions"]})
(Path(__file__).resolve().parent / "records/audit-oracles.v1.json").write_text(
    json.dumps({"schema": 1, "artifacts": records}, indent=2) + "\n")
