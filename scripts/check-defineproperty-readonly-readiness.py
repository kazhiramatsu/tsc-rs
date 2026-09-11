#!/usr/bin/env python3
"""Check semantic readonly ownership, immutable witnesses and caller closure."""
from pathlib import Path
import collections
import hashlib
import json
import re
import subprocess

ROOT = Path(__file__).resolve().parent.parent
read = lambda p: json.loads((ROOT / p).read_text())
digest = lambda b: hashlib.sha256(b).hexdigest()
m = read("ratchets/h2-8a-defineproperty-readonly-readiness.v1.json")
assert m["version"] == 1 and m["slice"] == "H2.8a-A6-26"
assert m["unresolved"] == m["undispositioned"] == 0
assert m["steps"] == ["A6-26-1", "A6-26-2", "A6-26-3"]
assert len(m["runtime_paths"]) == 10 and len(set(m["runtime_paths"])) == 10
for r in m["authorities"]:
    assert digest((ROOT / r["path"]).read_bytes()) == r["sha256"], r["path"]
baseline = {}
for r in m["baseline_rust"]:
    source = subprocess.check_output(["git", "show", f'{m["base"]}:{r["path"]}'], cwd=ROOT)
    assert digest(source) == r["sha256"], r["path"]
    baseline[r["path"]] = source.decode()
packet = (ROOT / "docs/design/greenfield/slices/h2-8a-defineproperty-readonly.md").read_text()
assert all(step in packet for step in m["steps"])
lines = (ROOT / "vendor/typescript-6.0.3/lib/_tsc.js").read_bytes().splitlines(keepends=True)
assert len(m["owners"]) == len({r["owner"] for r in m["owners"]}) == 24
for r in m["owners"]:
    assert digest(b"".join(lines[r["start"] - 1:r["end"]])) == r["sha256"], r["owner"]
    assert lines[r["start"] - 1].decode().strip().startswith("function " + r["owner"] + "(")
    assert r["owner"] in packet
calls = []
for p in m["runtime_paths"]:
    source = baseline[p]
    for match in re.finditer(r"\b(?:self\.checker|checker|self\.st|self|state)\s*\.is_readonly_symbol\([^()]+\)", source):
        calls.append({"path": p, "line": source[:match.start()].count("\n") + 1, "expression": match[0]})
assert calls == m["call_sites"] and len(calls) == 25
build = read("ratchets/h2-8a-defineproperty-readonly-first-after-build.v1.json")
assert build["executed_tests"] == build["complete_command_executions"] == 0
assert build["exit"]["exit_code"] == 101
assert m["additional_existing_test_callers"] == build["additional_test_callers"]
assert len(m["additional_existing_test_callers"]) == 3 and m["native_test_api_callers"] == 5
for r in m["additional_existing_test_callers"]:
    assert r["expression"] in baseline[r["path"]].splitlines()[r["line"] - 1]
architecture = (ROOT / "docs/design/greenfield/emitter-architecture.md").read_text()
assert len(m["architecture"]) == 7
for r in m["architecture"]:
    assert f'| `{r["id"]}` |' in architecture and r["id"] in packet and r["disposition"] in packet
cases = {}
for family, eligible, exact, failed, captures in [
    ("defineproperty-readonly", 224, 164, 60, 388),
    ("defineproperty-readonly-exports", 16, 8, 8, 24),
]:
    b = read(f"ratchets/h2-8a-{family}-before.v1.json")
    assert b["base"] == m["base"] and b["eligible"] == eligible
    assert b["primary_comparison_jobs"] == 2
    assert b["positive_primary_executions_per_case"] == 4 and b["failed_primary_executions_per_case"] == 2
    assert b["supplemental_executions"] == captures and b["capture_is_full_actual_tuple"] is False
    assert b["fresh_complete_comparison_and_supplemental_executions"] == captures * 3
    assert len(b["exact_twice"]) == exact and len(b["failed_twice"]) == failed
    assert set(b["exact_twice"]).isdisjoint(b["failed_twice"])
    f = read(f"crates/compiler/tests/fixtures/{family}.json")
    assert f["typescript"] == "6.0.3" and f["repetitions"] == 2
    assert f["observer_sha256"] == digest((ROOT / f"scripts/observe-{family}.mjs").read_bytes())
    assert f["compiler_sha256"] == digest((ROOT / "vendor/typescript-6.0.3/lib/typescript.js").read_bytes())
    assert {r["case_id"] for r in f["cases"]} == set(b["exact_twice"] + b["failed_twice"])
    assert all(r["supplemental_error"] is None for r in b["components"])
    if eligible == 224:
        assert collections.Counter(r["boundary"] for r in b["first_failure_comparisons"]) == {
            "callback bytes for /project/out/main.d.ts": 54, "exact ordered reported diagnostics": 6,
        }
        for r in b["components"]:
            assert len(r["differences"]) == (0 if r["case_id"] in b["exact_twice"] else 1)
            assert all(d["kind"] == "Declaration" and d["readonly_modifier_only"] for d in r["differences"])
    else:
        assert all(not r["differences"] for r in b["components"])
        assert collections.Counter(r["boundary"] for r in b["first_failure_comparisons"]) == {"exact ordered reported diagnostics": 8}
    for row in f["cases"]:
        cases[row["case_id"]] = {"case_id": row["case_id"], "family": family,
            "status": "adjacent-exact" if row["case_id"] in b["exact_twice"] else "owned-descriptor-readonly",
            "row_sha256": digest(json.dumps(row, ensure_ascii=False, separators=(",", ":")).encode())}
    assert "mod h2_8a_" + family.replace("-", "_") + ";" in (ROOT / "crates/compiler/tests/contracts.rs").read_text()
assert len(cases) == len(m["witnesses"]) == 240
assert all(row == cases[row["case_id"]] for row in m["witnesses"])
original = read("ratchets/h2-8a-defineproperty-readonly-before.v1.json")["original_adjacent"]
assert original["eligible"] == 1 and original["native_complete_command_executions"] == len(original["full_tuples"]) == 2
assert all(r["differences"] == ["$.reported_diagnostics[0].message"] for r in original["full_tuples"])
assert m["after_optional_unchanged_outside_case"] == original["full_tuples"][0]["case_id"]
native = read("ratchets/h2-8a-defineproperty-readonly-checker-before.v1.json")
assert native["base"] == m["base"] and native["passed"] == 1717 and native["failed"] == 1
assert native["failed_test"] == "structural::tests::defineproperty_readonly_queries_the_descriptor_type"
assert len(native["test_ids"]) == m["checker_units_required"] == 1718
assert m["before_complete_comparison_and_supplemental_executions"] == 1238
assert m["after_required_complete_commands"] == 502 and m["after_required_owned_and_positive_exact_twice"] == 501
print("H2.8a-A6-26 ready: 24 whole TS owners, 25 calls/10 files, 3 steps, 7 architecture rows; 240 fresh 172 exact/68 owned; checker 1717 pass/1 owned fail; unresolved=0, undispositioned=0")
