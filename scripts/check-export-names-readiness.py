#!/usr/bin/env python3
"""Validate the bounded declaration export-name repair and complete before set."""
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent
read = lambda p: json.loads((ROOT / p).read_text())
digest = lambda b: hashlib.sha256(b).hexdigest()
row_digest = lambda r: digest(json.dumps(r, ensure_ascii=False, separators=(",", ":")).encode())
m = read("ratchets/h2-8a-export-names-readiness.v1.json")
assert m["version"] == 1 and m["slice"] == "H2.8a-A6-9"
assert m["unresolved"] == m["undispositioned"] == 0
for row in m["authorities"]:
    assert digest((ROOT / row["path"]).read_bytes()) == row["sha256"], row["path"]
for row in m["baseline_rust"]:
    assert digest(subprocess.check_output(["git", "show", f'{m["base"]}:{row["path"]}'], cwd=ROOT)) == row["sha256"]
packet = (ROOT / "docs/design/greenfield/slices/h2-8a-export-specifier-names.md").read_text()
lines = (ROOT / "vendor/typescript-6.0.3/lib/_tsc.js").read_bytes().splitlines(keepends=True)
assert len(m["owners"]) == 12
for row in m["owners"]:
    assert digest(b"".join(lines[row["start"] - 1:row["end"]])) == row["sha256"]
    assert row["owner"] in packet
assert m["steps"] == ["A6-9-1"] and "A6-9-1" in packet
architecture = (ROOT / "docs/design/greenfield/emitter-architecture.md").read_text()
assert len(m["architecture"]) == 3
for row in m["architecture"]:
    assert f'| `{row["id"]}` |' in architecture and row["id"] in packet
    assert row["disposition"] in packet
assert len(m["witnesses"]) == 56
for fixture, record, count, failed_count in [
    ("export-specifier-names", "export-names", 48, 22),
    ("cjs-default-reexport-names", "cjs-default-reexport", 8, 0),
]:
    f = read(f"crates/compiler/tests/fixtures/{fixture}.json")
    assert f["typescript"] == "6.0.3" and f["repetitions"] == 2
    assert f["observer_sha256"] == digest((ROOT / f"scripts/observe-{fixture}.mjs").read_bytes())
    assert f["compiler_sha256"] == digest((ROOT / "vendor/typescript-6.0.3/lib/typescript.js").read_bytes())
    cases = {r["case_id"]: r for r in f["cases"]}
    before = read(f"ratchets/h2-8a-{record}-before.v1.json")
    assert before["status"] == "complete-before" and before["checkpoint"] == m["base"]
    witnesses = [r for r in m["witnesses"] if r["fixture"] == fixture]
    assert len(cases) == before["eligible"] == len(witnesses) == count
    assert len(before["failed"]) == failed_count
    assert set(before["exact"]).isdisjoint(before["failed"])
    assert set(before["exact"]) | set(before["failed"]) == set(cases)
    assert {r["case_id"] for r in witnesses} == set(cases)
    for row in witnesses:
        assert row_digest(cases[row["case_id"]]) == row["row_sha256"]
        failed = row["case_id"] in before["failed"]
        assert (row["before"] == "failed") == failed
        disposition = "owned-declaration-name" if failed else "adjacent-exact"
        if "string-default" in row["case_id"]:
            disposition = "independent-javascript-name-syntax"
        assert row["disposition"] == disposition
inputs = {r["case_id"]: r for r in read("ratchets/h2-8a-candidate-inputs.v1.json")["cases"]}
observations = {r["case_id"]: r for r in read("ratchets/h2-8a-observations.v1.json")["cases"]}
candidates = {r["case_id"]: r for r in read("ratchets/h2-8a-candidates.v1.json")["cases"]}
original_before = read("ratchets/h2-8a-export-names-original-before.v1.json")
assert original_before["status"] == "complete-before" and original_before["checkpoint"] == m["base"]
assert len(m["originals"]) == original_before["eligible"] == 12
assert {r["case_id"] for r in m["originals"]} == set(original_before["failed"])
for row in m["originals"]:
    name = row["case_id"]
    assert candidates[name]["required_slices"] == ["H2.8a"]
    assert row_digest(inputs[name]) == row["input_sha256"] == candidates[name]["input_sha256"]
    assert row_digest(observations[name]) == row["observation_sha256"]
    assert row["before"] == "failed"
    assert row["disposition"] == ("owned-declaration-name-and-independent-tslib" if "ImportHelpers" in name else "owned-declaration-name")
for path, name in zip(m["tests"], [
    "export_specifier_names_match_complete_typescript_observations",
    "cjs_default_reexport_names_match_complete_typescript_observations",
    "original_export_specifier_names_match_complete_commands",
]):
    assert name in (ROOT / path).read_text()
print("H2.8a-A6-9 ready: 12 owners, 1 step, 3 architecture rows, 56 fresh witnesses (22 failed / 34 exact before), 12 originals; unresolved=0, undispositioned=0")
