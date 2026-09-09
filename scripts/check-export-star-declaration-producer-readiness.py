#!/usr/bin/env python3
"""Check the source-owned A6-31 export-star producer and complete before evidence."""
from pathlib import Path
import collections
import hashlib
import json
import subprocess

ROOT = Path(__file__).resolve().parent.parent
STEM = "export-star-declaration-producer"
read = lambda p: json.loads((ROOT / p).read_text())
digest = lambda b: hashlib.sha256(b).hexdigest()
m = read(f"ratchets/h2-8a-{STEM}-readiness.v1.json")
assert m["version"] == 1 and m["slice"] == "H2.8a-A6-31"
assert m["unresolved"] == m["undispositioned"] == 0
assert m["runtime_paths"] == ["crates/checker/src/node_builder/statements.rs"]
assert m["steps"] == ["A6-31-1", "A6-31-2", "A6-31-3"]
for row in m["authorities"] + m["unit_sources"]:
    assert digest((ROOT / row["path"]).read_bytes()) == row["sha256"], row["path"]
for row in m["baseline_rust"]:
    assert digest(subprocess.check_output(["git", "show", f'{m["base"]}:{row["path"]}'], cwd=ROOT)) == row["sha256"]
packet = (ROOT / f"docs/design/greenfield/slices/h2-8a-{STEM}.md").read_text()
assert all(step in packet for step in m["steps"])
lines = (ROOT / "vendor/typescript-6.0.3/lib/_tsc.js").read_bytes().splitlines(keepends=True)
assert len(m["owners"]) == len({r["owner"] for r in m["owners"]}) == 11
for row in m["owners"]:
    assert digest(b"".join(lines[row["start"] - 1:row["end"]])) == row["sha256"], row["owner"]
    assert lines[row["start"] - 1].decode().strip().startswith("function " + row["owner"] + "(")
    assert row["owner"] in packet and row["step"] in m["steps"] and row["test"]
assert collections.Counter(r["gap"] for r in m["owners"]) == {"partial-or-stale": 1, "shared-prerequisite": 10}
assert len(m["declarations"]) == 1
for row in m["declarations"]:
    assert row["declaration_kind"] == "property-assignment-arrow"
    assert digest(b"".join(lines[row["start"] - 1:row["end"]])) == row["sha256"]
    assert lines[row["start"] - 1].decode().strip().startswith(row["owner"] + ":")
    assert row["owner"] in packet
architecture = (ROOT / "docs/design/greenfield/emitter-architecture.md").read_text()
assert len(m["architecture"]) == len({r["id"] for r in m["architecture"]}) == 8
for row in m["architecture"]:
    assert row["row"] in architecture and row["row"].replace("](slices/", "](") in packet
    assert digest(row["row"].encode()) == row["row_sha256"]
    assert row["disposition"] == "premise-unchanged"
    assert row["lifecycle_before"] == row["lifecycle_after"] == "active-qualified"
b = read(f"ratchets/h2-8a-{STEM}-before.v1.json")
assert b["base"] == m["base"] and b["native_jobs"] == 2 and b["first_failure_vectors_identical"]
assert (b["positive_executions_per_case"], b["failed_executions_per_case"]) == (4, 2)
assert len(b["exact_twice"]) == 32 and len(b["failed_twice"]) == 24 and not b["typed_failures"]
assert (b["primary_complete_command_executions"], b["supplemental_executions"], b["original_primary_executions"], b["total_native_executions_including_originals"]) == (176, 88, 8, 272)
assert all(r["exit_code"] == 101 for r in b["exits"])
assert collections.Counter(r["boundary"] for r in b["first_failure_comparisons"]) == {"exact source-map result": 24}
f = read(f"crates/compiler/tests/fixtures/{STEM}.json")
assert f["typescript"] == "6.0.3" and f["repetitions"] == 2
assert f["observer_sha256"] == digest((ROOT / f"scripts/observe-{STEM}.mjs").read_bytes())
assert f["compiler_sha256"] == digest((ROOT / "vendor/typescript-6.0.3/lib/typescript.js").read_bytes())
cases = {c["case_id"]: c for c in f["cases"]}
assert len(cases) == len(m["witnesses"]) == 56
assert set(b["exact_twice"]).isdisjoint(b["failed_twice"])
assert set(b["exact_twice"] + b["failed_twice"]) == set(cases)
assert collections.Counter(r["disposition"] for r in m["witnesses"]) == {"adjacent-exact": 32, "owned-export-star-declaration-producer": 24}
for row in m["witnesses"]:
    assert row["row_sha256"] == digest(json.dumps(cases[row["case_id"]], ensure_ascii=False, separators=(",", ":")).encode())
    assert (row["disposition"] == "adjacent-exact") == (row["case_id"] in b["exact_twice"])
    if row["disposition"] != "adjacent-exact":
        assert "/js/" in row["case_id"] and row["first_boundary"] == "exact source-map result"
assert collections.Counter(d["code"] for c in cases.values() for d in c["typescript_observation"]["reported_diagnostics"]) == {8006: 5, 2307: 8, 2823: 2, 5101: 3, 5107: 3}
assert m["original"] == b["original"] and m["original"]["eligible"] == 4
assert len(m["original"]["exact_twice"]) == 3 and len(m["original"]["failed_twice"]) == 1 and len(m["original"]["captures"]) == 2
assert m["after_required_exact_twice"] == 60 and m["checker_units_required"] == 28
test = (ROOT / "crates/compiler/tests/integration/h2_8a_export_star_declaration_producer.rs").read_text()
assert "assert_eq!(cases.len(), 56)" in test and "failures.is_empty()" in test
assert "mod h2_8a_export_star_declaration_producer;" in (ROOT / "crates/compiler/tests/contracts.rs").read_text()
assert "fn original_export_star_declaration_producer_commands()" in (ROOT / "crates/compiler/tests/h2_8a_original_corpus.rs").read_text()
print("H2.8a-A6-31 ready: 11 whole TS functions + resolver property, 8 architecture rows, 3 steps; 56 fresh + 4 original commands; unresolved=0, undispositioned=0")
