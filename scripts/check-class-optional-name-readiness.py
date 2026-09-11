#!/usr/bin/env python3
"""Check the source-owned optional class name branch before production edits."""
from pathlib import Path
import collections
import hashlib
import json
import subprocess

ROOT = Path(__file__).resolve().parent.parent
STEM = "class-optional-name"
read = lambda p: json.loads((ROOT / p).read_text())
digest = lambda b: hashlib.sha256(b).hexdigest()
m = read(f"ratchets/h2-8a-{STEM}-readiness.v1.json")
assert m["version"] == 1 and m["slice"] == "H2.8a-A6-32"
assert m["unresolved"] == m["undispositioned"] == 0
assert m["runtime_paths"] == ["crates/emitter/src/printer.rs"]
assert m["steps"] == ["A6-32-1", "A6-32-2"]
for row in m["authorities"] + m["unit_sources"]:
    assert digest((ROOT / row["path"]).read_bytes()) == row["sha256"], row["path"]
for row in m["baseline_rust"]:
    assert digest(subprocess.check_output(["git", "show", f'{m["base"]}:{row["path"]}'], cwd=ROOT)) == row["sha256"]
packet = (ROOT / f"docs/design/greenfield/slices/h2-8a-{STEM}.md").read_text()
assert all(step in packet for step in m["steps"])
lines = (ROOT / "vendor/typescript-6.0.3/lib/_tsc.js").read_bytes().splitlines(keepends=True)
assert len(m["owners"]) == len({r["owner"] for r in m["owners"]}) == 13
for row in m["owners"]:
    assert digest(b"".join(lines[row["start"] - 1:row["end"]])) == row["sha256"], row["owner"]
    assert lines[row["start"] - 1].decode().strip().startswith("function " + row["owner"] + "(")
    assert row["owner"] in packet and row["step"] in m["steps"] and row["test"]
assert collections.Counter(r["gap"] for r in m["owners"]) == {"partial-or-stale": 1, "shared-prerequisite": 12}
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
assert len(b["exact_twice"]) == 40 and len(b["failed_twice"]) == 24 and len(b["typed_failures"]) == 24
assert not b["comparisons"] and all('transformed ClassDeclaration is missing child name' in r["boundary"] for r in b["typed_failures"])
assert (b["primary_command_attempts"], b["supplemental_executions"]) == (208, 104)
assert all(r["exit_code"] == 101 for r in b["exits"])
f = read(f"crates/compiler/tests/fixtures/{STEM}.json")
assert f["typescript"] == "6.0.3" and f["repetitions"] == 2
assert f["observer_sha256"] == digest((ROOT / f"scripts/observe-{STEM}.mjs").read_bytes())
assert f["compiler_sha256"] == digest((ROOT / "vendor/typescript-6.0.3/lib/typescript.js").read_bytes())
cases = {c["case_id"]: c for c in f["cases"]}
assert len(cases) == len(m["witnesses"]) == 64
assert set(b["exact_twice"]).isdisjoint(b["failed_twice"])
assert set(b["exact_twice"] + b["failed_twice"]) == set(cases)
assert collections.Counter(r["disposition"] for r in m["witnesses"]) == {"adjacent-exact": 40, "owned-optional-class-name": 18, "outside-class-keyword-comments": 6}
for row in m["witnesses"]:
    assert row["row_sha256"] == digest(json.dumps(cases[row["case_id"]], ensure_ascii=False, separators=(",", ":")).encode())
    assert (row["disposition"] == "adjacent-exact") == (row["case_id"] in b["exact_twice"])
assert collections.Counter(d["code"] for c in cases.values() for d in c["typescript_observation"]["reported_diagnostics"]) == {1211: 26, 5107: 8}
assert b["original"]["eligible"] == 4 and len(b["original"]["exact_twice"]) == 3
assert len(b["original"]["failed_twice"]) == 1
assert m["after_minimum_exact_twice"] == 58 and m["rust_map_rows"] == 8
assert "mod h2_8a_class_optional_name;" in (ROOT / "crates/compiler/tests/contracts.rs").read_text()
assert "fn original_class_optional_name_commands()" in (ROOT / "crates/compiler/tests/h2_8a_original_corpus.rs").read_text()
print("H2.8a-A6-32 ready:13 whole TS functions,8 architecture rows,2 steps;64 fresh +4 original commands;18 owned,40 adjacent,6 comment controls; unresolved=0,undispositioned=0")
