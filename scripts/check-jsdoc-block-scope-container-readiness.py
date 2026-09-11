#!/usr/bin/env python3
"""Validate the source-owned A6-30 binder predicate packet and frozen witnesses."""
from pathlib import Path
import collections
import hashlib
import json
import subprocess

ROOT = Path(__file__).resolve().parent.parent
STEM = "jsdoc-block-scope-container"


def read(path):
    return json.loads((ROOT / path).read_text())


def digest(data):
    return hashlib.sha256(data).hexdigest()


m = read(f"ratchets/h2-8a-{STEM}-readiness.v1.json")
assert m["version"] == 1 and m["slice"] == "H2.8a-A6-30"
assert m["unresolved"] == m["undispositioned"] == 0
assert m["runtime_paths"] == ["crates/binder/src/bind.rs"]
assert m["steps"] == ["A6-30-1", "A6-30-2", "A6-30-3", "A6-30-4"]
for row in m["authorities"] + m["unit_sources"]:
    assert digest((ROOT / row["path"]).read_bytes()) == row["sha256"], row["path"]
for row in m["baseline_rust"]:
    baseline = subprocess.check_output(["git", "show", f'{m["base"]}:{row["path"]}'], cwd=ROOT)
    assert digest(baseline) == row["sha256"], row["path"]
packet = (ROOT / f"docs/design/greenfield/slices/h2-8a-{STEM}.md").read_text()
assert all(step in packet for step in m["steps"])
lines = (ROOT / "vendor/typescript-6.0.3/lib/_tsc.js").read_bytes().splitlines(keepends=True)
assert len(m["owners"]) == len({r["owner"] for r in m["owners"]}) == 21
for row in m["owners"]:
    assert digest(b"".join(lines[row["start"] - 1:row["end"]])) == row["sha256"], row["owner"]
    assert lines[row["start"] - 1].decode().strip().startswith("function " + row["owner"] + "(")
    assert row["owner"] in packet and row["step"] in m["steps"]
assert collections.Counter(r["gap"] for r in m["owners"]) == {"partial-or-stale": 2, "shared-prerequisite": 19}
architecture = (ROOT / "docs/design/greenfield/emitter-architecture.md").read_text()
assert len(m["architecture"]) == len({r["id"] for r in m["architecture"]}) == 8
for row in m["architecture"]:
    assert row["row"] in architecture and row["row"].replace("](slices/", "](") in packet
    assert digest(row["row"].encode()) == row["sha256"]
    assert row["disposition"] == "premise-unchanged"
    assert row["lifecycle_before"] == row["lifecycle_after"] == "active-qualified"
before = read(f"ratchets/h2-8a-{STEM}-before.v1.json")
assert before["base"] == m["base"] and before["native_jobs"] == 2
assert before["positive_executions_per_case"] == 4 and before["failed_executions_per_case"] == 2
assert before["first_failure_vectors_identical"] and not before["typed_failures"]
assert len(before["exact_twice"]) == 32 and len(before["failed_twice"]) == 28
assert before["primary_complete_command_executions"] == 184 and before["supplemental_executions"] == 92
assert all(r["exit_code"] == 101 for r in before["exits"])
fixture = read(f"crates/compiler/tests/fixtures/{STEM}.json")
assert fixture["typescript"] == "6.0.3" and fixture["repetitions"] == 2
assert fixture["observer_sha256"] == digest((ROOT / f"scripts/observe-{STEM}.mjs").read_bytes())
assert fixture["compiler_sha256"] == digest((ROOT / "vendor/typescript-6.0.3/lib/typescript.js").read_bytes())
cases = {c["case_id"]: c for c in fixture["cases"]}
assert len(cases) == len(m["witnesses"]) == 60
assert set(before["exact_twice"]).isdisjoint(before["failed_twice"])
assert set(before["exact_twice"] + before["failed_twice"]) == set(cases)
assert collections.Counter(r["disposition"] for r in m["witnesses"]) == {"adjacent-exact": 32, "owned-current-container-module-test": 28}
for row in m["witnesses"]:
    assert row["row_sha256"] == digest(json.dumps(cases[row["case_id"]], ensure_ascii=False, separators=(",", ":")).encode())
    assert (row["disposition"] == "adjacent-exact") == (row["case_id"] in before["exact_twice"])
    if row["disposition"] != "adjacent-exact":
        assert row["first_boundary"] == "exact ordered reported diagnostics"
        assert "/script/" not in row["case_id"]
assert collections.Counter(d["code"] for c in cases.values() for d in c["typescript_observation"]["reported_diagnostics"]) == {2300: 24}
assert m["original"] == before["source_dispositions"]["original_reference"]
assert len(m["original"]["owned"]) == len(m["original"]["adjacent_exact"]) == 2
assert all(len(r["tuples"]) == 2 for r in m["original"]["owned"])
assert m["after_required_exact_twice"] == 64
test = (ROOT / "crates/compiler/tests/integration/h2_8a_jsdoc_block_scope_container.rs").read_text()
assert "assert_eq!(cases.len(), 60)" in test and "failures.is_empty()" in test
assert "mod h2_8a_jsdoc_block_scope_container;" in (ROOT / "crates/compiler/tests/contracts.rs").read_text()
assert "fn original_jsdoc_block_scope_container_commands()" in (ROOT / "crates/compiler/tests/h2_8a_original_corpus.rs").read_text()
print("H2.8a-A6-30 ready: 21 whole TS owners, 8 architecture rows, 4 steps; 60 fresh + 4 original commands; unresolved=0, undispositioned=0")
