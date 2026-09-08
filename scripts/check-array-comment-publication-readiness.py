#!/usr/bin/env python3
"""Check the one-phase list-comment repair and its independent before evidence."""
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent
read = lambda p: json.loads((ROOT / p).read_text())
digest = lambda b: hashlib.sha256(b).hexdigest()
m = read("ratchets/h2-8a-array-comment-publication-readiness.v1.json")
assert m["version"] == 1 and m["slice"] == "H2.8a-A6-13"
assert m["unresolved"] == m["undispositioned"] == 0
assert m["runtime_paths"] == ["crates/emitter/src/printer.rs"]
assert m["steps"] == ["A6-13-1"]
for row in m["authorities"]:
    assert digest((ROOT / row["path"]).read_bytes()) == row["sha256"], row["path"]
for row in m["baseline_rust"]:
    data = subprocess.check_output(["git", "show", f'{m["base"]}:{row["path"]}'], cwd=ROOT)
    assert digest(data) == row["sha256"], row["path"]
packet = (ROOT / "docs/design/greenfield/slices/h2-8a-array-comment-publication.md").read_text()
assert "A6-13-1" in packet
lines = (ROOT / "vendor/typescript-6.0.3/lib/_tsc.js").read_bytes().splitlines(keepends=True)
assert len(m["owners"]) == 3
for row in m["owners"]:
    assert digest(b"".join(lines[row["start"] - 1:row["end"]])) == row["sha256"]
    assert row["owner"] in packet
architecture = (ROOT / "docs/design/greenfield/emitter-architecture.md").read_text()
assert len(m["architecture"]) == 4
for row in m["architecture"]:
    assert f'| `{row["id"]}` |' in architecture and row["id"] in packet
    assert row["disposition"] in packet
before = read("ratchets/h2-8a-array-comment-publication-before.v1.json")
assert before["base"] == m["base"] and before["native_jobs"] == 2
assert before["status"] == "complete-before-with-two-independent-native-jobs"
assert before["positive_executions_per_case"] == 4
assert before["failed_executions_per_case"] == 2
assert len(before["exact_twice"]) == 38 and len(before["failed_twice"]) == 14
assert len(before["first_failure_comparisons"]) == 14
assert all(r["native_executions"] == 2 and r["boundary"] == "exact source-map result"
           and r["map_fields_except_mappings"] == "exact" for r in before["first_failure_comparisons"])
fixture = read("crates/compiler/tests/fixtures/array-comment-publication.json")
assert fixture["typescript"] == "6.0.3" and fixture["repetitions"] == 2
assert fixture["observer_sha256"] == digest((ROOT / "scripts/observe-array-comment-publication.mjs").read_bytes())
assert fixture["compiler_sha256"] == digest((ROOT / "vendor/typescript-6.0.3/lib/typescript.js").read_bytes())
cases = {r["case_id"]: r for r in fixture["cases"]}
assert len(cases) == len(m["witnesses"]) == 52
assert set(before["exact_twice"]).isdisjoint(before["failed_twice"])
assert set(before["exact_twice"] + before["failed_twice"]) == set(cases)
for row in m["witnesses"]:
    name = row["case_id"]
    assert digest(json.dumps(cases[name], ensure_ascii=False, separators=(",", ":")).encode()) == row["row_sha256"]
    passed = name in before["exact_twice"]
    assert (row["before"] == "exact-twice") == passed
    assert row["disposition"] == ("adjacent-exact" if passed else "owned-duplicate-comment-map")
test = (ROOT / "crates/compiler/tests/integration/h2_8a_array_comment_publication.rs").read_text()
assert "assert_eq!(cases.len(), 52)" in test and "failures.is_empty()" in test
assert "mod h2_8a_array_comment_publication;" in (ROOT / "crates/compiler/tests/contracts.rs").read_text()
helper = (ROOT / "crates/compiler/tests/integration/h2_7c_declaration_blocking.rs").read_text()
assert '"moduleResolution" =>' in helper and "options.module_resolution = Some(value.as_i64().unwrap() as i32)" in helper
printer = m["printer_controls"]
assert len(printer["before_exact_twice"]) == 41 and len(printer["before_failed_twice"]) == 7
assert len(printer["owned_after"]) == 3 and len(printer["retained_after"]) == 4
assert set(printer["owned_after"] + printer["retained_after"]) == set(printer["before_failed_twice"])
assert all("ParentNoNestedComments" in name for name in printer["retained_after"])
assert not subprocess.check_output(["git", "diff", "--name-only", "d0f23fdcaac4fa7e7e93c2c87178d991cf528bb2", m["base"], "--", "crates/emitter"], cwd=ROOT).strip()
print("H2.8a-A6-13 ready:3 owners,1 step,4 architecture rows,52 complete/48 printer witnesses;38 exact/14 map failures before;two independent jobs;unresolved=0,undispositioned=0")
