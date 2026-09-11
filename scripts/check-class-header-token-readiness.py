#!/usr/bin/env python3
"""Check the bounded class keyword/comment owner before implementation."""
from pathlib import Path
import collections
import hashlib
import json
import subprocess

ROOT = Path(__file__).resolve().parent.parent
STEM = "class-header-token"
read = lambda p: json.loads((ROOT / p).read_text())
digest = lambda b: hashlib.sha256(b).hexdigest()
m = read(f"ratchets/h2-8a-{STEM}-readiness.v1.json")
assert m["version"] == 1 and m["slice"] == "H2.8a-A6-33"
assert m["unresolved"] == m["undispositioned"] == 0
assert m["runtime_paths"] == ["crates/emitter/src/printer.rs"]
assert m["steps"] == ["A6-33-1", "A6-33-2"]
for row in m["authorities"] + m["unit_sources"]:
    assert digest((ROOT / row["path"]).read_bytes()) == row["sha256"], row["path"]
for row in m["baseline_rust"]:
    assert digest(subprocess.check_output(["git", "show", f'{m["base"]}:{row["path"]}'], cwd=ROOT)) == row["sha256"]
packet = (ROOT / f"docs/design/greenfield/slices/h2-8a-{STEM}.md").read_text()
assert all(step in packet for step in m["steps"])
test_sources = "\n".join((ROOT / p).read_text() for p in [
    "crates/compiler/tests/integration/h2_8a_class_header_token.rs",
    "crates/emitter/tests/class_header_token_metadata_contract.rs",
])
lines = (ROOT / "vendor/typescript-6.0.3/lib/_tsc.js").read_bytes().splitlines(keepends=True)
assert len(m["owners"]) == len({r["owner"] for r in m["owners"]}) == 30
for row in m["owners"]:
    assert digest(b"".join(lines[row["start"] - 1:row["end"]])) == row["sha256"], row["owner"]
    assert lines[row["start"] - 1].decode().strip().startswith("function " + row["owner"] + "(")
    assert row["owner"] in packet and row["step"] in m["steps"] and row["test"]
    assert f'fn {row["test"]}(' in test_sources, row["test"]
assert collections.Counter(r["gap"] for r in m["owners"]) == {"missing": 2, "partial-or-stale": 1, "shared-prerequisite": 27}
assert {row["step"] for row in m["owners"]} == set(m["steps"])
architecture = (ROOT / "docs/design/greenfield/emitter-architecture.md").read_text()
assert len(m["architecture"]) == len({r["id"] for r in m["architecture"]}) == 8
for row in m["architecture"]:
    assert row["row"] in architecture and row["row"].replace("](slices/", "](") in packet
    assert digest(row["row"].encode()) == row["row_sha256"]
    assert row["disposition"] == "premise-unchanged"
    assert row["lifecycle_before"] == row["lifecycle_after"] == "active-qualified"
b = read(f"ratchets/h2-8a-{STEM}-before.v1.json")
assert b["base"] == m["base"] and b["native_jobs"] == 2 and b["first_failure_vectors_identical"]
assert len(b["exact_twice"]) == 22 and len(b["failed_twice"]) == len(b["comparisons"]) == 66
assert not b["typed_failures"] and {r["boundary"] for r in b["comparisons"]} == {"exact source-map result"}
assert (b["primary_command_attempts"], b["supplemental_executions"]) == (220, 110)
assert all(r["exit_code"] == 101 for r in b["exits"])
for route, fixture, count in [("ordinary", f"crates/compiler/tests/fixtures/{STEM}.json", 88), ("metadata", "crates/emitter/tests/fixtures/class-header-token-printer-metadata.json", 32)]:
    f = read(fixture)
    assert f["typescript"] == "6.0.3" and f["repetitions"] == 2
    observer = f"scripts/observe-{STEM}" + ("-printer-metadata" if route == "metadata" else "") + ".mjs"
    assert f["observer_sha256"] == digest((ROOT / observer).read_bytes())
    assert f["compiler_sha256"] == digest((ROOT / "vendor/typescript-6.0.3/lib/typescript.js").read_bytes())
    cases = {c["case_id"]: c for c in f["cases"]}
    witnesses = m["witnesses"][route]
    assert len(cases) == len(witnesses) == count
    assert {r["case_id"] for r in witnesses} == set(cases)
    for row in witnesses:
        assert row["row_sha256"] == digest(json.dumps(cases[row["case_id"]], ensure_ascii=False, separators=(",", ":")).encode())
assert collections.Counter(r["disposition"] for r in m["witnesses"]["ordinary"]) == {"adjacent-exact": 22, "owned-class-keyword-comments": 58, "mixed-keyword-and-outside-modifier-comments": 8}
assert collections.Counter(r["disposition"] for r in m["witnesses"]["metadata"]) == {"adjacent-exact": 20, "owned-class-keyword-comments": 8, "owned-name-end-suppression": 4}
assert set(b["exact_twice"]).isdisjoint(b["failed_twice"])
assert set(b["exact_twice"] + b["failed_twice"]) == {r["case_id"] for r in m["witnesses"]["ordinary"]}
mb = read(f"ratchets/h2-8a-{STEM}-metadata-before.v1.json")
assert mb["base"] == m["base"] and mb["first_failure_vectors_identical"]
assert len(mb["exact_twice"]) == 20 and len(mb["failed_twice"]) == 12 and len(mb["captures"]) == 24
assert mb["primary_print_attempts"] == mb["complete_actual_prints"] == 64 and mb["program_command_attempts"] == 0
assert mb["exit"]["exit_code"] == 101
assert m["after_minimum_exact_twice"] == {"new_ordinary": 80, "prior_ordinary": 64, "original": 3, "metadata": 32}
assert m["rust_map_rows"] == 9
assert "mod h2_8a_class_header_token;" in (ROOT / "crates/compiler/tests/contracts.rs").read_text()
assert "fn original_class_optional_name_commands()" in (ROOT / "crates/compiler/tests/h2_8a_original_corpus.rs").read_text()
print("H2.8a-A6-33 ready:30 whole TS functions,8 architecture rows,2 steps;88 new +64 prior +4 original commands;32 direct prints;58 owned22 prior8 mixed; unresolved=0,undispositioned=0")
