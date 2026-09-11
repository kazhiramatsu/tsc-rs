#!/usr/bin/env python3
"""Validate the bounded System publication repair and its frozen before."""
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent
read = lambda p: json.loads((ROOT / p).read_text())
digest = lambda b: hashlib.sha256(b).hexdigest()
row_digest = lambda r: digest(json.dumps(r, ensure_ascii=False, separators=(",", ":")).encode())
m = read("ratchets/h2-8a-system-publication-readiness.v1.json")
assert m["version"] == 1 and m["slice"] == "H2.8a-A6-11"
assert m["unresolved"] == m["undispositioned"] == 0
assert m["runtime_paths"] == ["crates/emitter/src/builtins/system.rs", "crates/emitter/src/printer.rs"]
for row in m["authorities"]:
    assert digest((ROOT / row["path"]).read_bytes()) == row["sha256"], row["path"]
for row in m["baseline_rust"]:
    assert digest(subprocess.check_output(["git", "show", f'{m["base"]}:{row["path"]}'], cwd=ROOT)) == row["sha256"]
packet = (ROOT / "docs/design/greenfield/slices/h2-8a-system-variable-publication.md").read_text()
lines = (ROOT / "vendor/typescript-6.0.3/lib/_tsc.js").read_bytes().splitlines(keepends=True)
assert len(m["owners"]) == 33
for row in m["owners"]:
    assert digest(b"".join(lines[row["start"] - 1:row["end"]])) == row["sha256"]
    assert row["owner"] in packet
assert m["steps"] == [f"A6-11-{i}" for i in range(1, 7)]
assert all(step in packet for step in m["steps"])
architecture = (ROOT / "docs/design/greenfield/emitter-architecture.md").read_text()
assert len(m["architecture"]) == 10
for row in m["architecture"]:
    assert f'| `{row["id"]}` |' in architecture and row["id"] in packet
    assert row["disposition"] in packet
before = read("ratchets/h2-8a-system-variable-publication-before.v1.json")
assert before["status"] == "complete-before" and before["checkpoint"] == m["base"]
assert before["eligible"] == 72 and len(before["exact"]) == 28 and len(before["failed"]) == 44
assert before["source_map_result_failures"] == 20 and before["javascript_byte_failures"] == 24
assert before["preliminary_run_qualified"] is False
assert len(before["first_failure_comparisons"]) == 44
# Historical repetitions=2 here counts duplicate panic renderings; the correction
# authority below records one actual failed execution. Frozen bytes are retained.
assert all(row["repetitions"] == 2 for row in before["first_failure_comparisons"])
for row in m["baseline_rust"]:
    saved = next(r for r in before["working_source_files"] if r["path"] == row["path"])
    assert saved["sha256"] == row["sha256"]
after_path = "ratchets/h2-8a-export-name-syntax-after.v1.json"
assert subprocess.check_output(["git", "show", f'{m["base"]}:{after_path}'], cwd=ROOT) == (ROOT / after_path).read_bytes()
previous = read(after_path)
assert len(m["witnesses"]) == 272
for stem, count, exact in [
    ("system-variable-publication", 72, before["exact"]),
    ("export-name-syntax", 160, previous["exact_twice"]["Export name syntax"]),
    ("export-name-syntax-maps", 40, previous["exact_twice"]["Export name syntax maps"]),
]:
    fixture = read(f"crates/compiler/tests/fixtures/{stem}.json")
    assert fixture["typescript"] == "6.0.3" and fixture["repetitions"] == 2
    assert fixture["observer_sha256"] == digest((ROOT / f"scripts/observe-{stem}.mjs").read_bytes())
    assert fixture["compiler_sha256"] == digest((ROOT / "vendor/typescript-6.0.3/lib/typescript.js").read_bytes())
    cases = {r["case_id"]: r for r in fixture["cases"]}
    witnesses = [r for r in m["witnesses"] if r["fixture"] == stem]
    assert len(cases) == len(witnesses) == count
    assert set(cases) == {r["case_id"] for r in witnesses}
    if stem == "system-variable-publication":
        assert set(exact).isdisjoint(before["failed"])
        assert set(exact) | set(before["failed"]) == set(cases)
    for row in witnesses:
        name = row["case_id"]
        assert row_digest(cases[name]) == row["row_sha256"]
        passed = name in exact
        assert (row["before"] == "exact-twice") == passed
        disposition = "adjacent-exact" if passed else "owned-system-publication"
        if not passed and stem != "system-variable-publication":
            if not (name.startswith("system/") and "direct-extended-unicode" not in name):
                disposition = "retained-TS2484-or-H2.9"
        assert row["disposition"] == disposition
assert sum(r["before"] == "exact-twice" for r in m["witnesses"]) == 206
assert sum(r["disposition"] == "owned-system-publication" for r in m["witnesses"]) == 56
assert sum(r["disposition"] == "retained-TS2484-or-H2.9" for r in m["witnesses"]) == 10
fixture = read("crates/compiler/tests/fixtures/system-dynamic-imports.json")
assert len(fixture["cases"]) == 24 and fixture["repetitions"] == 2
assert fixture["observer_sha256"] == digest((ROOT / "scripts/observe-system-dynamic-imports.mjs").read_bytes())
test = (ROOT / "crates/compiler/tests/integration/h2_8a_system_variable_publication.rs").read_text()
assert "system_variable_publication_matches_complete_typescript_observations" in test
assert "assert_eq!(cases.len(), 72)" in test and "failures.is_empty()" in test
assert "mod h2_8a_system_variable_publication;" in (ROOT / "crates/compiler/tests/contracts.rs").read_text()
printer = read(m["printer_controls"]["fixture"])
printer_before = read(m["printer_controls"]["before"])
assert len(printer["cases"]) == m["printer_controls"]["count"] == 48
assert printer["repetitions"] == 2
assert printer["observer_sha256"] == digest((ROOT / "scripts/observe-list-comment-flags.mjs").read_bytes())
assert printer["compiler_sha256"] == digest((ROOT / "vendor/typescript-6.0.3/lib/typescript.js").read_bytes())
assert printer_before["status"] == "complete-printer-before"
assert set(printer_before["exact_twice"]).isdisjoint(printer_before["failed_twice"])
assert set(printer_before["exact_twice"] + printer_before["failed_twice"]) == {r["case_id"] for r in printer["cases"]}
assert len(read("ratchets/h2-8a-system-variable-publication-first-after.v1.json")["exact"]) == 64
correction = read("ratchets/h2-8a-system-publication-execution-correction.v1.json")
assert correction["status"] == "execution-count-correction"
assert all(r["native_executions_per_failed_case"] == 1 for r in correction["active_system_records"])
assert [(r["exact_cases"], r["failed_cases"]) for r in correction["active_system_records"]] == [(28, 44), (64, 8), (68, 4)]
token = read(m["token_comment_controls"]["fixture"])
token_before = read(m["token_comment_controls"]["before"])
assert len(token["cases"]) == m["token_comment_controls"]["count"] == 54
assert token["repetitions"] == 2
assert token["observer_sha256"] == digest((ROOT / "scripts/observe-declaration-token-comments.mjs").read_bytes())
assert token["compiler_sha256"] == digest((ROOT / "vendor/typescript-6.0.3/lib/typescript.js").read_bytes())
assert token_before["status"] == "complete-token-comment-before"
assert len(token_before["exact_twice"]) == 42 and len(token_before["failed_twice"]) == 12
assert set(token_before["exact_twice"]).isdisjoint(token_before["failed_twice"])
assert set(token_before["exact_twice"] + token_before["failed_twice"]) == {r["case_id"] for r in token["cases"]}
assert len(token_before["first_failure_comparisons"]) == 12
assert all(r["repetitions"] == 2 for r in token_before["first_failure_comparisons"])
second = read("ratchets/h2-8a-system-variable-publication-second-after.v1.json")
assert len(second["exact_twice"]["System variable publication"]) == 68
print("Additional controls:54 complete token commands (42 exact/12 failed before),48 printer observations (38 exact/10 failed before)")
print("H2.8a-A6-11 ready: 33 owners,6 steps,10 architecture rows,272 witnesses (206 exact /66 failed before);56 owned failures,10 retained outside; unresolved=0,undispositioned=0")
