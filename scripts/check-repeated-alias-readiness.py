#!/usr/bin/env python3
"""Validate the source-literal resolution fallback repair boundary."""
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent
read = lambda p: json.loads((ROOT / p).read_text())
digest = lambda b: hashlib.sha256(b).hexdigest()
row_digest = lambda r: digest(json.dumps(r, ensure_ascii=False, separators=(",", ":")).encode())
m = read("ratchets/h2-8a-repeated-alias-readiness.v1.json")
assert m["version"] == 1 and m["slice"] == "H2.8a-A6-7"
assert m["unresolved"] == m["undispositioned"] == 0
for row in m["authorities"]:
    assert digest((ROOT / row["path"]).read_bytes()) == row["sha256"], row["path"]
for row in m["baseline_rust"]:
    assert digest(subprocess.check_output(["git", "show", f'{m["base"]}:{row["path"]}'], cwd=ROOT)) == row["sha256"]
packet = (ROOT / "docs/design/greenfield/slices/h2-8a-repeated-target-aliases.md").read_text()
lines = (ROOT / "vendor/typescript-6.0.3/lib/_tsc.js").read_bytes().splitlines(keepends=True)
assert len(m["owners"]) == 8
for row in m["owners"]:
    assert digest(b"".join(lines[row["start"] - 1:row["end"]])) == row["sha256"]
    assert row["owner"] in packet
assert m["steps"] == ["A6-7-1"] and "A6-7-1" in packet
architecture = (ROOT / "docs/design/greenfield/emitter-architecture.md").read_text()
assert len(m["architecture"]) == 3
for row in m["architecture"]:
    assert f'| `{row["id"]}` |' in architecture and row["id"] in packet
    assert row["disposition"] in packet
fixture = read("crates/compiler/tests/fixtures/repeated-target-declaration-aliases.json")
assert fixture["typescript"] == "6.0.3" and fixture["repetitions"] == 2
assert fixture["observer_sha256"] == digest((ROOT / "scripts/observe-repeated-target-declaration-aliases.mjs").read_bytes())
assert fixture["compiler_sha256"] == digest((ROOT / "vendor/typescript-6.0.3/lib/typescript.js").read_bytes())
cases = {r["case_id"]: r for r in fixture["cases"]}
before = read("ratchets/h2-8a-repeated-alias-before.v1.json")
assert before["status"] == "complete-before" and before["checkpoint"] == m["base"]
assert len(cases) == before["eligible"] == len(m["witnesses"]) == 48
assert len(before["failed"]) == 12 and len(before["exact"]) == 36
assert set(before["exact"]).isdisjoint(before["failed"])
assert set(before["exact"]) | set(before["failed"]) == set(cases)
assert {r["case_id"] for r in m["witnesses"]} == set(cases)
for row in m["witnesses"]:
    assert row_digest(cases[row["case_id"]]) == row["row_sha256"]
    assert (row["before"] == "failed") == (row["case_id"] in before["failed"])
inputs = {r["case_id"]: r for r in read("ratchets/h2-8a-candidate-inputs.v1.json")["cases"]}
observations = {r["case_id"]: r for r in read("ratchets/h2-8a-observations.v1.json")["cases"]}
candidates = {r["case_id"]: r for r in read("ratchets/h2-8a-candidates.v1.json")["cases"]}
original_before = read("ratchets/h2-8a-class-original-before.v1.json")
assert len(m["originals"]) == 2
for row in m["originals"]:
    name = row["case_id"]
    assert "jsDeclarationsReexportedCjsAlias.ts#" in name
    assert candidates[name]["required_slices"] == ["H2.8a"]
    assert row_digest(inputs[name]) == row["input_sha256"] == candidates[name]["input_sha256"]
    assert row_digest(observations[name]) == row["observation_sha256"]
    assert (row["before"] == "failed") == (name in original_before["failed"])
assert "repeated_target_declaration_aliases_match_complete_typescript_observations" in (ROOT / m["tests"][0]).read_text()
assert "original_reexported_commonjs_aliases_match_complete_commands" in (ROOT / m["tests"][1]).read_text()
print("H2.8a-A6-7 ready: 8 owners, 1 step, 3 architecture rows, 48 fresh witnesses (12 failed / 36 exact before), 2 originals; unresolved=0, undispositioned=0")
