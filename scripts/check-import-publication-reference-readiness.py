#!/usr/bin/env python3
"""Verify declaration-reference publication ownership and complete before runs."""
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent
read = lambda p: json.loads((ROOT / p).read_text())
digest = lambda b: hashlib.sha256(b).hexdigest()
m = read("ratchets/h2-8a-import-publication-reference-readiness.v1.json")
assert m["version"] == 1 and m["slice"] == "H2.8a-A6-14"
assert m["unresolved"] == m["undispositioned"] == 0
assert m["runtime_paths"] == ["crates/emitter/src/builtins.rs", "crates/emitter/src/printer.rs"]
assert m["steps"] == ["A6-14-1", "A6-14-2", "A6-14-3"]
for row in m["authorities"]:
    assert digest((ROOT / row["path"]).read_bytes()) == row["sha256"], row["path"]
for row in m["baseline_rust"]:
    assert digest(subprocess.check_output(["git", "show", f'{m["base"]}:{row["path"]}'], cwd=ROOT)) == row["sha256"]
packet = (ROOT / "docs/design/greenfield/slices/h2-8a-import-publication-reference.md").read_text()
assert all(step in packet for step in m["steps"])
lines = (ROOT / "vendor/typescript-6.0.3/lib/_tsc.js").read_bytes().splitlines(keepends=True)
assert len(m["owners"]) == 13
for row in m["owners"]:
    assert digest(b"".join(lines[row["start"] - 1:row["end"]])) == row["sha256"]
    assert row["owner"] in packet
architecture = (ROOT / "docs/design/greenfield/emitter-architecture.md").read_text()
assert len(m["architecture"]) == 6
for row in m["architecture"]:
    assert f'| `{row["id"]}` |' in architecture and row["id"] in packet
    assert row["disposition"] in packet
before = read("ratchets/h2-8a-import-publication-reference-before.v1.json")
assert before["base"] == m["base"] and before["native_jobs"] == 2
assert before["status"] == "complete-before-with-two-independent-native-jobs"
assert before["positive_executions_per_case"] == 4
assert before["failed_executions_per_case"] == 2
assert len(before["exact_twice"]) == 33 and len(before["failed_twice"]) == 54
assert len(before["first_failure_comparisons"]) == 54
assert all(r["native_executions"] == 2 and r["changed_map"] == "main.js"
           and r["other_map_fields_and_lib_map"] == "exact" for r in before["first_failure_comparisons"])
fixture = read("crates/compiler/tests/fixtures/import-publication-reference.json")
queries = read("crates/checker/tests/fixtures/import-publication-resolver.json")
for artifact, stem in [(fixture, "import-publication-reference"), (queries, "import-publication-resolver")]:
    assert artifact["typescript"] == "6.0.3" and artifact["repetitions"] == 2
    assert artifact["observer_sha256"] == digest((ROOT / f"scripts/observe-{stem}.mjs").read_bytes())
    assert artifact["compiler_sha256"] == digest((ROOT / "vendor/typescript-6.0.3/lib/typescript.js").read_bytes())
cases = {r["case_id"]: r for r in fixture["cases"]}
assert len(cases) == len(m["witnesses"]) == 87
assert set(before["exact_twice"]).isdisjoint(before["failed_twice"])
assert set(before["exact_twice"] + before["failed_twice"]) == set(cases)
for row in m["witnesses"]:
    name = row["case_id"]
    assert digest(json.dumps(cases[name], ensure_ascii=False, separators=(",", ":")).encode()) == row["row_sha256"]
    passed = name in before["exact_twice"]
    assert (row["before"] == "exact-twice") == passed
    assert row["disposition"] == ("adjacent-exact" if passed else "owned-declaration-reference-map")
assert len(queries["cases"]) == 18
assert before["resolver_queries"] == m["resolver_queries"]
assert set(m["resolver_queries"]["exact_twice"]) == {r["case_id"] for r in queries["cases"]}
assert m["resolver_queries"]["exit_code"] == 0
assert m["resolver_queries"]["projections_compared"] == ["getReferencedExportContainer:Reference", "getReferencedImportDeclaration"]
test = (ROOT / "crates/compiler/tests/integration/h2_8a_import_publication_reference.rs").read_text()
assert "assert_eq!(cases.len(), 87)" in test and "failures.is_empty()" in test
assert "mod h2_8a_import_publication_reference;" in (ROOT / "crates/compiler/tests/contracts.rs").read_text()
query_test = (ROOT / "crates/checker/tests/unit/emit/tests.rs").read_text()
assert "fn import_publication_declaration_references_match_typescript_resolver()" in query_test
printer = read("crates/emitter/tests/fixtures/string-literal-identifier-source.json")
printer_before = read(m["printer_dependency"]["before_record"])
assert printer["typescript"] == "6.0.3" and printer["repetitions"] == 2
assert printer["observer_sha256"] == digest((ROOT / "scripts/observe-string-literal-identifier-source.mjs").read_bytes())
assert printer["compiler_sha256"] == digest((ROOT / "vendor/typescript-6.0.3/lib/typescript.js").read_bytes())
assert len(printer["cases"]) == 72
assert len(printer_before["exact_twice"]) == 64 and len(printer_before["failed_twice"]) == 8
assert set(printer_before["exact_twice"] + printer_before["failed_twice"]) == {r["case_id"] for r in printer["cases"]}
assert printer_before["failed_executions_per_case"] == 2
print("H2.8a-A6-14 ready:13 owners,3 steps,6 architecture rows,87 complete/18 resolver/72 printer witnesses;33 exact/54 repeated map failures before;unresolved=0,undispositioned=0")
