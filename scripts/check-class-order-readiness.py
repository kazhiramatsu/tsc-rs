#!/usr/bin/env python3
"""Validate the bounded class declaration evaluation/completion packet."""
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent
read = lambda p: json.loads((ROOT / p).read_text())
digest = lambda b: hashlib.sha256(b).hexdigest()
row_digest = lambda r: digest(json.dumps(r, ensure_ascii=False, separators=(",", ":")).encode())
m = read("ratchets/h2-8a-class-order-readiness.v1.json")
assert m["version"] == 1 and m["slice"] == "H2.8a-A6-6"
assert m["unresolved"] == m["undispositioned"] == 0
for row in m["authorities"]:
    assert digest((ROOT / row["path"]).read_bytes()) == row["sha256"], row["path"]
for row in m["baseline_rust"]:
    assert digest(subprocess.check_output(["git", "show", f'{m["base"]}:{row["path"]}'], cwd=ROOT)) == row["sha256"]
packet = (ROOT / "docs/design/greenfield/slices/h2-8a-class-dependency-order.md").read_text()
lines = (ROOT / "vendor/typescript-6.0.3/lib/_tsc.js").read_bytes().splitlines(keepends=True)
assert len(m["owners"]) == 6
for row in m["owners"]:
    assert digest(b"".join(lines[row["start"] - 1:row["end"]])) == row["sha256"]
    assert row["owner"] in packet
assert m["steps"] == ["A6-6-1", "A6-6-2"] and all(step in packet for step in m["steps"])
architecture = (ROOT / "docs/design/greenfield/emitter-architecture.md").read_text()
assert len(m["architecture"]) == 3
for row in m["architecture"]:
    assert f'| `{row["id"]}` |' in architecture and row["id"] in packet
    assert row["disposition"] in packet
all_cases = {}
failed = set()
for fixture_name, before_name, total, fail_count in [
    ("class-declaration-dependency-order", "class-order", 24, 8),
    ("class-declaration-member-order", "class-member", 4, 4),
]:
    fixture = read(f"crates/compiler/tests/fixtures/{fixture_name}.json")
    assert fixture["typescript"] == "6.0.3" and fixture["repetitions"] == 2
    assert fixture["observer_sha256"] == digest((ROOT / f"scripts/observe-{fixture_name}.mjs").read_bytes())
    assert fixture["compiler_sha256"] == digest((ROOT / "vendor/typescript-6.0.3/lib/typescript.js").read_bytes())
    cases = {r["case_id"]: r for r in fixture["cases"]}
    before = read(f"ratchets/h2-8a-{before_name}-before.v1.json")
    assert before["status"] == "complete-before" and before["checkpoint"] == m["base"]
    assert len(cases) == before["eligible"] == total
    assert len(before["failed"]) == fail_count and len(before["exact"]) == total - fail_count
    assert set(before["exact"]).isdisjoint(before["failed"])
    assert set(before["exact"]) | set(before["failed"]) == set(cases)
    assert set(all_cases).isdisjoint(cases)
    all_cases.update(cases)
    failed.update(before["failed"])
assert len(m["witnesses"]) == len(all_cases) == 28
for row in m["witnesses"]:
    assert row_digest(all_cases[row["case_id"]]) == row["row_sha256"]
    assert (row["before"] == "failed") == (row["case_id"] in failed)
inputs = {r["case_id"]: r for r in read("ratchets/h2-8a-candidate-inputs.v1.json")["cases"]}
observations = {r["case_id"]: r for r in read("ratchets/h2-8a-observations.v1.json")["cases"]}
candidates = {r["case_id"]: r for r in read("ratchets/h2-8a-candidates.v1.json")["cases"]}
before = read("ratchets/h2-8a-class-original-before.v1.json")
assert before["checkpoint"] == m["base"] and before["status"] == "complete-before"
assert len(before["exact"]) == 19 and len(before["failed"]) == 3 and len(before["failures"]) == 6
assert len(m["originals"]) == before["eligible"] == 22
assert {r["case_id"] for r in m["originals"]} == set(before["exact"]) | set(before["failed"])
for row in m["originals"]:
    name = row["case_id"]
    assert candidates[name]["required_slices"] == ["H2.8a"]
    assert row_digest(inputs[name]) == row["input_sha256"] == candidates[name]["input_sha256"]
    assert row_digest(observations[name]) == row["observation_sha256"]
    assert (row["before"] == "failed") == (name in before["failed"])
    if row["before"] == "failed":
        assert {r["repetition"] for r in before["failures"] if r["case_id"] == name} == {0, 1}
assert sum(r["disposition"] == "class-owner" for r in m["originals"]) == 2
assert sum(r["disposition"] == "adjacent-alias-regression" for r in m["originals"]) == 1
assert sum(r["disposition"] == "adjacent-exact" for r in m["originals"]) == 19
for name in ["class_declaration_dependency_order_matches_complete_typescript_observations", "class_declaration_member_order_matches_complete_typescript_observations"]:
    assert name in (ROOT / m["tests"][0]).read_text()
assert "original_require_alias_declarations_match_complete_commands" in (ROOT / m["tests"][1]).read_text()
print("H2.8a-A6-6 ready: 6 owners, 2 steps, 3 architecture rows, 28 fresh witnesses (12 failed / 16 exact before), 2 original class failures and 20 adjacent originals (one alias regression retained); unresolved=0, undispositioned=0")
