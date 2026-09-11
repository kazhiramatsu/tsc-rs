#!/usr/bin/env python3
"""Verify the frozen binding-name prerequisite and its bounded runtime amendment."""
from pathlib import Path
import collections, hashlib, json, subprocess

ROOT = Path(__file__).resolve().parent.parent
read = lambda p: json.loads((ROOT / p).read_text())
digest = lambda b: hashlib.sha256(b).hexdigest()
subprocess.run(["python3", "scripts/check-defineproperty-setter-names-readiness.py"], cwd=ROOT, check=True)
m = read("ratchets/h2-8a-binding-name-map-ranges-readiness.v1.json")
assert m["version"] == 1 and m["slice"] == "H2.8a-A6-25-shared-prerequisite"
assert m["unresolved"] == m["undispositioned"] == 0
assert m["runtime_paths"] == ["crates/checker/src/node_builder/statements.rs", "crates/checker/src/node_builder/signatures.rs"]
assert m["steps"] == [f"A6-25-{n}" for n in range(4, 8)]
for r in m["authorities"]:
    assert digest((ROOT / r["path"]).read_bytes()) == r["sha256"], r["path"]
for r in m["baseline_rust"]:
    assert digest(subprocess.check_output(["git", "show", f'{m["base"]}:{r["path"]}'], cwd=ROOT)) == r["sha256"], r["path"]
packet = (ROOT / "docs/design/greenfield/slices/h2-8a-binding-name-map-ranges.md").read_text()
assert all(step in packet for step in m["steps"])
lines = (ROOT / "vendor/typescript-6.0.3/lib/_tsc.js").read_bytes().splitlines(keepends=True)
assert len(m["owners"]) == 24
for r in m["owners"]:
    assert digest(b"".join(lines[r["start"] - 1:r["end"]])) == r["sha256"], r["owner"]
    assert ("function " + r["owner"] + "(").encode() in lines[r["start"] - 1]
    assert r["owner"] in packet
architecture = (ROOT / "docs/design/greenfield/emitter-architecture.md").read_text()
assert len(m["architecture"]) == 6
for r in m["architecture"]:
    assert f'| `{r["id"]}` |' in architecture and r["id"] in packet and r["disposition"] in packet
b = read("ratchets/h2-8a-binding-name-map-ranges-before.v1.json")
assert b["base"] == m["base"] and b["eligible"] == 48
assert b["primary_comparison_jobs"] == 2 and b["positive_primary_executions_per_case"] == 4 and b["failed_primary_executions_per_case"] == 2
assert len(b["exact_twice"]) == 12 and len(b["failed_twice"]) == 36
assert b["capture_is_full_actual_tuple"] is False and b["supplemental_executions"] == 60
assert b["fresh_complete_comparison_and_supplemental_executions"] == m["shared_before_complete_comparison_and_supplemental_executions"] == 180
assert collections.Counter(r["boundary"] for r in b["first_failure_comparisons"]) == {"exact source-map result": 36}
assert collections.Counter(len(r["differences"]) for r in b["components"]) == {0: 12, 1: 28, 2: 8}
assert all(r["supplemental_error"] is None for r in b["components"])
f = read("crates/compiler/tests/fixtures/binding-name-map-ranges.json")
assert f["typescript"] == "6.0.3" and f["repetitions"] == 2
assert f["observer_sha256"] == digest((ROOT / "scripts/observe-binding-name-map-ranges.mjs").read_bytes())
assert f["compiler_sha256"] == digest((ROOT / "vendor/typescript-6.0.3/lib/typescript.js").read_bytes())
assert set(b["exact_twice"]).isdisjoint(b["failed_twice"])
assert {r["case_id"] for r in f["cases"]} == set(b["exact_twice"] + b["failed_twice"])
assert "mod h2_8a_binding_name_map_ranges;" in (ROOT / "crates/compiler/tests/contracts.rs").read_text()
native = read("ratchets/h2-8a-binding-name-map-ranges-source-before.v1.json")
assert native["base"] == m["base"] and native["exit"]["exit_code"] == 101
assert native["test_results"] == {"passed": 1, "failed": 2}
assert m["after_required_complete_commands"] == 261 and m["after_target_exact_twice"] == 253
assert m["checker_node_builder_units_required"] == 65 and m["first_after_outside_readonly_failures"] == 8
print("H2.8a-A6-25 prerequisite ready: 24 whole TS owners, 4 steps, 6 architecture rows; 48 fresh 12 exact/36 owned; native 1 pass/2 fail; unresolved=0, undispositioned=0")
