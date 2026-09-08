#!/usr/bin/env python3
"""Check the class statement layout/name amendment before production edits."""
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent
read = lambda p: json.loads((ROOT / p).read_text())
digest = lambda b: hashlib.sha256(b).hexdigest()
row_digest = lambda r: digest(json.dumps(r, ensure_ascii=False, separators=(",", ":")).encode())
m = read("ratchets/h2-8a-class-layout-readiness.v1.json")
assert m["version"] == 1 and m["slice"] == "H2.8a-A6-8-amendment"
assert m["unresolved"] == m["undispositioned"] == 0
for row in m["authorities"]:
    assert digest((ROOT / row["path"]).read_bytes()) == row["sha256"], row["path"]
for row in m["baseline_rust"]:
    assert digest(subprocess.check_output(["git", "show", f'{m["base"]}:{row["path"]}'], cwd=ROOT)) == row["sha256"]
assert digest((ROOT / "crates/emitter/src/builtins.rs").read_bytes()) == m["prior_class_flags_sha256"]
packet = (ROOT / "docs/design/greenfield/slices/h2-8a-class-statement-layout.md").read_text()
lines = (ROOT / "vendor/typescript-6.0.3/lib/_tsc.js").read_bytes().splitlines(keepends=True)
assert len(m["owners"]) == 14
for row in m["owners"]:
    assert digest(b"".join(lines[row["start"] - 1:row["end"]])) == row["sha256"]
    assert row["owner"] in packet
assert m["steps"] == ["A6-8-4", "A6-8-5", "A6-8-6"]
assert all(step in packet for step in m["steps"])
architecture = (ROOT / "docs/design/greenfield/emitter-architecture.md").read_text()
assert len(m["architecture"]) == 4
for row in m["architecture"]:
    assert f'| `{row["id"]}` |' in architecture and row["id"] in packet
    assert row["disposition"] in packet
f = read("crates/compiler/tests/fixtures/class-statement-layout.json")
assert f["typescript"] == "6.0.3" and f["repetitions"] == 2
assert f["observer_sha256"] == digest((ROOT / "scripts/observe-class-statement-layout.mjs").read_bytes())
assert f["compiler_sha256"] == digest((ROOT / "vendor/typescript-6.0.3/lib/typescript.js").read_bytes())
b = read("ratchets/h2-8a-class-layout-before.v1.json")
assert b["checkpoint"] == m["base"] and b["status"] == "complete-before-amendment"
assert len(f["cases"]) == len(m["witnesses"]) == b["eligible"] == 64
assert len(b["exact"]) == len(b["failed"]) == 32
cases = {r["case_id"]: r for r in f["cases"]}
assert set(b["exact"]).isdisjoint(b["failed"])
assert set(b["exact"] + b["failed"]) == set(cases) == {r["case_id"] for r in m["witnesses"]}
for row in m["witnesses"]:
    assert row_digest(cases[row["case_id"]]) == row["row_sha256"]
    assert (row["before"] == "failed") == (row["case_id"] in b["failed"])
before_files = {r["path"]: r["sha256"] for r in b["working_source_files"]}
for row in m["baseline_rust"]:
    assert before_files[row["path"]] == row["sha256"]
assert before_files["crates/emitter/src/builtins.rs"] == m["prior_class_flags_sha256"]
first = read("ratchets/h2-8a-class-transform-first-after.v1.json")
assert first["fresh_exact"] == 46 and first["unit_tests_passed"] == 98
assert first["original_exact"] == 5 and len(first["original_failed"]) == 5
assert first["runtime_sha256"] == m["prior_class_flags_sha256"]
test = (ROOT / "crates/compiler/tests/integration/h2_8a_class_statement_layout.rs").read_text()
assert "class_statement_layout_match_complete_typescript_observations" in test
print("H2.8a-A6-8 amendment ready: 14 owners, 3 steps, 4 architecture rows, 64 complete commands (32 exact / 32 failed before); unresolved=0, undispositioned=0")
