#!/usr/bin/env python3
"""Check the frozen source-owned ConstKeyword erasure packet."""
from pathlib import Path
import collections
import hashlib
import json
import subprocess

ROOT = Path(__file__).resolve().parent.parent
STEM = "const-modifier-erasure"
read = lambda p: json.loads((ROOT / p).read_text())
digest = lambda b: hashlib.sha256(b).hexdigest()
m = read(f"ratchets/h2-8a-{STEM}-readiness.v1.json")
assert m["version"] == 1 and m["slice"] == "H2.8a-A6-34"
assert m["runtime_paths"] == ["crates/emitter/src/builtins.rs"]
assert m["steps"] == ["A6-34-1", "A6-34-2"]
assert m["unresolved"] == m["undispositioned"] == 0 and m["rust_map_rows"] == 10
for r in m["authorities"] + m["unit_sources"]:
    assert digest((ROOT / r["path"]).read_bytes()) == r["sha256"], r["path"]
for r in m["baseline_rust"]:
    assert digest(subprocess.check_output(["git", "show", f'{m["base"]}:{r["path"]}'], cwd=ROOT)) == r["sha256"]
packet = (ROOT / f"docs/design/greenfield/slices/h2-8a-{STEM}.md").read_text()
lines = (ROOT / "vendor/typescript-6.0.3/lib/_tsc.js").read_bytes().splitlines(keepends=True)
assert len(m["owners"]) == len({r["owner"] for r in m["owners"]}) == 36
for r in m["owners"] + m["properties"]:
    assert digest(b"".join(lines[r["start"] - 1:r["end"]])) == r["sha256"], r["owner"]
    assert r["owner"] in packet and r["step"] in m["steps"] and r["step"] in packet
    assert r["test"] == "const_modifier_erasure_matches_complete_typescript_observations"
for r in m["owners"]:
    assert lines[r["start"] - 1].decode().strip().startswith("function " + r["owner"] + "(")
assert collections.Counter(r["gap"] for r in m["owners"]) == {"partial-or-stale": 2, "shared-prerequisite": 34}
assert len(m["properties"]) == 1 and m["properties"][0]["owner"] == "_computedOptions.preserveConstEnums"
arch = (ROOT / "docs/design/greenfield/emitter-architecture.md").read_text()
assert len(m["architecture"]) == len({r["id"] for r in m["architecture"]}) == 12
for r in m["architecture"]:
    assert r["row"] in arch and r["row"].replace("](slices/", "](") in packet
    assert digest(r["row"].encode()) == r["sha256"] and r["disposition"] == "premise-unchanged"
    assert r["lifecycle_before"] == r["lifecycle_after"] == "active-qualified"
assert {r["id"] for r in m["gaps"]} == {"EA-GAP-FLAGS", "EA-GAP-CAPTURE"}
for r in m["gaps"]:
    assert r["body"] in arch and digest(r["body"].encode()) == r["sha256"] and r["id"] in packet
b = read(f"ratchets/h2-8a-{STEM}-before.v1.json")
assert b["base"] == m["base"] and b["native_jobs"] == 2 and b["first_failure_vectors_identical"]
assert len(b["exact_twice"]) == 52 and len(b["failed_twice"]) == 28 and not b["typed_failures"]
assert b["primary_command_attempts"] == 264 and b["supplemental_executions"] == 132
assert all(r["exit_code"] == 101 for r in b["exits"])
assert b["new_original_before_executions"] == 0 and len(b["prior_adjacent"]["exact_twice"]) == 64
f = read(f"crates/compiler/tests/fixtures/{STEM}.json")
assert f["typescript"] == "6.0.3" and f["repetitions"] == 2
assert f["observer_sha256"] == digest((ROOT / f"scripts/observe-{STEM}.mjs").read_bytes())
assert f["compiler_sha256"] == digest((ROOT / "vendor/typescript-6.0.3/lib/typescript.js").read_bytes())
cases = {c["case_id"]: c for c in f["cases"]}
assert len(cases) == len(m["witnesses"]) == len({r["case_id"] for r in m["witnesses"]}) == 80
assert set(b["exact_twice"]).isdisjoint(b["failed_twice"])
assert set(b["exact_twice"] + b["failed_twice"]) == set(cases)
for r in m["witnesses"]:
    assert r["row_sha256"] == digest(json.dumps(cases[r["case_id"]], ensure_ascii=False, separators=(",", ":")).encode())
    assert (r["disposition"] == "adjacent-exact") == (r["case_id"] in b["exact_twice"])
    assert r["step"] == "A6-34-2"
assert collections.Counter(d["code"] for c in cases.values() for d in c["typescript_observation"]["reported_diagnostics"]) == {8009: 32, 1248: 26, 8006: 4, 8004: 2, 8010: 2, 5107: 2}
assert m["after_required"] == dict(new=80, adjacent=64, original_exact=3, original_complete_unequal=1, emitter_units=494, emitter_contracts=451)
test = (ROOT / "crates/compiler/tests/integration/h2_8a_const_modifier_erasure.rs").read_text()
assert "fn const_modifier_erasure_matches_complete_typescript_observations()" in test
assert "assert_eq!(cases.len(), 80)" in test and "failures.is_empty()" in test
assert "mod h2_8a_const_modifier_erasure;" in (ROOT / "crates/compiler/tests/contracts.rs").read_text()
assert "fn original_class_optional_name_commands()" in (ROOT / "crates/compiler/tests/h2_8a_original_corpus.rs").read_text()
adapter = m["adapter"]["before"]
old = subprocess.check_output(["git", "show", f'{m["base"]}:{adapter["path"]}'], cwd=ROOT)
assert digest(old) == adapter["sha256"]
anchor = b'                    "removeComments" => options.remove_comments = value.as_bool(),\n'
line = b'                    "preserveConstEnums" => options.preserve_const_enums = value.as_bool(),\n'
assert old.count(anchor) == 1
assert (ROOT / adapter["path"]).read_bytes() == old.replace(anchor, anchor + line)
print("H2.8a-A6-34 ready: 36 whole TS owners, 1 property, 12 architecture rows, 2 gaps, 10 Rust rows, 80 witnesses; unresolved=0, undispositioned=0")
