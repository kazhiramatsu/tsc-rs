#!/usr/bin/env python3
"""Verify the parsed/updated class-flag repair and immutable observations."""
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent
read = lambda p: json.loads((ROOT / p).read_text())
digest = lambda b: hashlib.sha256(b).hexdigest()
row_digest = lambda r: digest(json.dumps(r, ensure_ascii=False, separators=(",", ":")).encode())
m = read("ratchets/h2-8a-class-transform-readiness.v1.json")
assert m["version"] == 1 and m["slice"] == "H2.8a-A6-8"
assert m["unresolved"] == m["undispositioned"] == 0
for row in m["authorities"]:
    assert digest((ROOT / row["path"]).read_bytes()) == row["sha256"], row["path"]
for row in m["baseline_rust"]:
    assert digest(subprocess.check_output(["git", "show", f'{m["base"]}:{row["path"]}'], cwd=ROOT)) == row["sha256"]
packet = (ROOT / "docs/design/greenfield/slices/h2-8a-class-transform-flags.md").read_text()
lines = (ROOT / "vendor/typescript-6.0.3/lib/_tsc.js").read_bytes().splitlines(keepends=True)
assert len(m["owners"]) == 14
for row in m["owners"]:
    assert digest(b"".join(lines[row["start"] - 1:row["end"]])) == row["sha256"]
    assert row["owner"] in packet
assert m["steps"] == ["A6-8-1", "A6-8-2", "A6-8-3"]
assert all(step in packet for step in m["steps"])
architecture = (ROOT / "docs/design/greenfield/emitter-architecture.md").read_text()
assert len(m["architecture"]) == 3
for row in m["architecture"]:
    assert f'| `{row["id"]}` |' in architecture and row["id"] in packet
    assert row["disposition"] in packet
fixtures = [
    ("crates/compiler/tests/fixtures/class-transform-flags.json", "scripts/observe-class-transform-flags.mjs"),
    ("crates/emitter/tests/fixtures/class-expression-updates.json", "scripts/observe-class-expression-updates.mjs"),
]
f, g = [read(path) for path, _ in fixtures]
for fixture, (_, observer) in zip((f, g), fixtures):
    assert fixture["typescript"] == "6.0.3" and fixture["repetitions"] == 2
    assert fixture["observer_sha256"] == digest((ROOT / observer).read_bytes())
    assert fixture["compiler_sha256"] == digest((ROOT / "vendor/typescript-6.0.3/lib/typescript.js").read_bytes())
before = read("ratchets/h2-8a-class-transform-before.v1.json")
unit = read("ratchets/h2-8a-class-flags-unit-before.v1.json")
expression = read("ratchets/h2-8a-class-expression-unit-before.v1.json")
original = read("ratchets/h2-8a-class-transform-original-before.v1.json")
for record in (before, unit, expression, original):
    assert record["status"] == "complete-before" and record["checkpoint"] == m["base"]
cases = {r["case_id"]: r for r in f["cases"]}
assert len(cases) == before["eligible"] == len(m["witnesses"]) == 50
assert len(before["exact"]) == 36 and len(before["failed"]) == 14
assert set(before["exact"]).isdisjoint(before["failed"])
assert set(before["exact"]) | set(before["failed"]) == set(cases)
assert {r["case_id"] for r in m["witnesses"]} == set(cases)
for row in m["witnesses"]:
    assert row_digest(cases[row["case_id"]]) == row["row_sha256"]
    assert (row["before"] == "failed") == (row["case_id"] in before["failed"])
assert sum(len(r["typescript_parse_flags"]) for r in f["cases"]) == m["parse_rows"] == 96
assert unit["parse_inputs"] == 50 and unit["update_inputs"] == len(f["update_controls"]["updates"]) == 9
assert unit["failed_rows"] == {"parsed class flag": 74, "updated class flag": 6}
assert row_digest(f["update_controls"]) == m["update_controls_sha256"]
assert expression["update_inputs"] == len(g["cases"]) == 6
assert len(expression["exact"]) == 2 and len(expression["failed"]) == 4
assert set(expression["exact"]).isdisjoint(expression["failed"])
assert set(expression["exact"] + expression["failed"]) == {r["case_id"] for r in g["cases"]}
assert row_digest(g["cases"]) == m["expression_updates_sha256"]
inputs = {r["case_id"]: r for r in read("ratchets/h2-8a-candidate-inputs.v1.json")["cases"]}
observations = {r["case_id"]: r for r in read("ratchets/h2-8a-observations.v1.json")["cases"]}
candidates = {r["case_id"]: r for r in read("ratchets/h2-8a-candidates.v1.json")["cases"]}
assert len(m["originals"]) == original["eligible"] == 10
assert len(original["exact"]) == 3 and len(original["failed"]) == 7
assert {r["case_id"] for r in m["originals"]} == set(original["exact"] + original["failed"])
for row in m["originals"]:
    name = row["case_id"]
    assert candidates[name]["required_slices"] == ["H2.8a"]
    assert row_digest(inputs[name]) == row["input_sha256"] == candidates[name]["input_sha256"]
    assert row_digest(observations[name]) == row["observation_sha256"]
    assert (row["before"] == "failed") == (name in original["failed"])
    residual = any(part in name for part in ("DefaultsErr", "ClassInstance2"))
    assert row["after_requirement"] == ("independent-declaration-residue" if residual else "exact")
for path, names in zip(m["tests"], (
    ["class_transform_flags_match_complete_typescript_observations"],
    ["original_static_class_transforms_match_complete_commands"],
    ["parsed_class_transform_flags_match_typescript_owned_bits", "updated_class_transform_flags_match_typescript_owned_bits", "updated_class_expression_transform_flags_match_typescript_owned_bits"],
)):
    assert all(name in (ROOT / path).read_text() for name in names)
print("H2.8a-A6-8 ready: 14 owners, 3 steps, 3 architecture rows, 50 complete witnesses, 96 parse rows, 15 updates, 10 originals; unresolved=0, undispositioned=0")
