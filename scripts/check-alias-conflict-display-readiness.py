#!/usr/bin/env python3
"""Check the alias-conflict display owner and independent before runs."""
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent
read = lambda p: json.loads((ROOT / p).read_text())
digest = lambda b: hashlib.sha256(b).hexdigest()
m = read("ratchets/h2-8a-alias-conflict-display-readiness.v1.json")
assert m["version"] == 1 and m["slice"] == "H2.8a-A6-12"
assert m["unresolved"] == m["undispositioned"] == 0
assert m["runtime_paths"] == ["crates/checker/src/modules.rs"]
assert m["steps"] == ["A6-12-1"]
for row in m["authorities"]:
    assert digest((ROOT / row["path"]).read_bytes()) == row["sha256"], row["path"]
for row in m["baseline_rust"]:
    assert digest(subprocess.check_output(["git", "show", f'{m["base"]}:{row["path"]}'], cwd=ROOT)) == row["sha256"]
packet = (ROOT / "docs/design/greenfield/slices/h2-8a-alias-conflict-display.md").read_text()
assert "A6-12-1" in packet
lines = (ROOT / "vendor/typescript-6.0.3/lib/_tsc.js").read_bytes().splitlines(keepends=True)
assert len(m["owners"]) == 5
for row in m["owners"]:
    assert digest(b"".join(lines[row["start"] - 1:row["end"]])) == row["sha256"]
    assert row["owner"] in packet
architecture = (ROOT / "docs/design/greenfield/emitter-architecture.md").read_text()
assert len(m["architecture"]) == 5
for row in m["architecture"]:
    assert f'| `{row["id"]}` |' in architecture and row["id"] in packet
    assert row["disposition"] in packet
before = read("ratchets/h2-8a-alias-conflict-display-before.v1.json")
assert before["base"] == m["base"] and before["native_jobs"] == 2
assert before["status"] == "complete-before-with-two-independent-native-jobs"
assert before["positive_executions_per_case"] == 4
assert before["failed_executions_per_case"] == 2
assert len(before["exact_twice"]) == len(before["failed_twice"]) == 12
assert len(before["owned_diagnostic_failures"]) == 10
assert len(before["outside_javascript_getter_failures"]) == 2
assert len(before["first_failure_comparisons"]) == 12
assert all(r["native_executions"] == 2 for r in before["first_failure_comparisons"])
fixture = read("crates/compiler/tests/fixtures/alias-conflict-display.json")
assert fixture["typescript"] == "6.0.3" and fixture["repetitions"] == 2
assert fixture["observer_sha256"] == digest((ROOT / "scripts/observe-alias-conflict-display.mjs").read_bytes())
assert fixture["compiler_sha256"] == digest((ROOT / "vendor/typescript-6.0.3/lib/typescript.js").read_bytes())
cases = {r["case_id"]: r for r in fixture["cases"]}
assert len(cases) == len(m["witnesses"]) == 24
assert set(before["exact_twice"]).isdisjoint(before["failed_twice"])
assert set(before["exact_twice"] + before["failed_twice"]) == set(cases)
for row in m["witnesses"]:
    name = row["case_id"]
    assert digest(json.dumps(cases[name], ensure_ascii=False, separators=(",", ":")).encode()) == row["row_sha256"]
    passed = name in before["exact_twice"]
    assert (row["before"] == "exact-twice") == passed
    expected = "adjacent-exact" if passed else "retained-JS-import-getter" if name in before["outside_javascript_getter_failures"] else "owned-diagnostic-display"
    assert row["disposition"] == expected
test = (ROOT / "crates/compiler/tests/integration/h2_8a_alias_conflict_display.rs").read_text()
assert "assert_eq!(cases.len(), 24)" in test and "failures.is_empty()" in test
assert "mod h2_8a_alias_conflict_display;" in (ROOT / "crates/compiler/tests/contracts.rs").read_text()
print("H2.8a-A6-12 ready:5 owners,1 step,5 architecture rows,24 witnesses;12 exact/10 owned/2 outside before;two independent native jobs;unresolved=0,undispositioned=0")
