#!/usr/bin/env python3
"""Validate the source-name provenance repair and its immutable complete before."""
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent
read = lambda p: json.loads((ROOT / p).read_text())
digest = lambda b: hashlib.sha256(b).hexdigest()
row_digest = lambda r: digest(json.dumps(r, ensure_ascii=False, separators=(",", ":")).encode())
m = read("ratchets/h2-8a-export-name-syntax-readiness.v1.json")
assert m["version"] == 1 and m["slice"] == "H2.8a-A6-10"
assert m["unresolved"] == m["undispositioned"] == 0
assert m["runtime_paths"] == ["crates/emitter/src/builtins.rs", "crates/emitter/src/builtins/system.rs"]
for row in m["authorities"]:
    assert digest((ROOT / row["path"]).read_bytes()) == row["sha256"], row["path"]
for row in m["baseline_rust"]:
    assert digest(subprocess.check_output(["git", "show", f'{m["base"]}:{row["path"]}'], cwd=ROOT)) == row["sha256"]
packet = (ROOT / "docs/design/greenfield/slices/h2-8a-export-name-syntax.md").read_text()
lines = (ROOT / "vendor/typescript-6.0.3/lib/_tsc.js").read_bytes().splitlines(keepends=True)
assert len(m["owners"]) == 24
for row in m["owners"]:
    assert digest(b"".join(lines[row["start"] - 1:row["end"]])) == row["sha256"]
    assert row["owner"] in packet
assert m["steps"] == [f"A6-10-{i}" for i in range(1, 5)]
assert all(step in packet for step in m["steps"])
architecture = (ROOT / "docs/design/greenfield/emitter-architecture.md").read_text()
assert len(m["architecture"]) == 7
for row in m["architecture"]:
    assert f'| `{row["id"]}` |' in architecture and row["id"] in packet
    assert row["disposition"] in packet
assert len(m["witnesses"]) == 200
for stem, count, failed_count in [("export-name-syntax", 160, 110), ("export-name-syntax-maps", 40, 38)]:
    f = read(f"crates/compiler/tests/fixtures/{stem}.json")
    assert f["typescript"] == "6.0.3" and f["repetitions"] == 2
    assert f["observer_sha256"] == digest((ROOT / f"scripts/observe-{stem}.mjs").read_bytes())
    assert f["compiler_sha256"] == digest((ROOT / "vendor/typescript-6.0.3/lib/typescript.js").read_bytes())
    cases = {r["case_id"]: r for r in f["cases"]}
    b = read(f"ratchets/h2-8a-{stem}-before.v1.json")
    assert b["status"] == "complete-before" and b["checkpoint"] == m["base"]
    witnesses = [r for r in m["witnesses"] if r["fixture"] == stem]
    assert len(cases) == b["eligible"] == len(witnesses) == count
    assert len(b["failed"]) == failed_count
    assert set(b["exact"]).isdisjoint(b["failed"])
    assert set(b["exact"]) | set(b["failed"]) == set(cases)
    assert {r["case_id"] for r in witnesses} == set(cases)
    for row in witnesses:
        name = row["case_id"]
        assert row_digest(cases[name]) == row["row_sha256"]
        failed = name in b["failed"]
        assert (row["before"] == "failed") == failed
        disposition = "owned-name-provenance" if failed else "adjacent-exact"
        if failed and stem.endswith("-maps"):
            if "direct-extended-unicode" in name:
                disposition = "later-H2.9-syntax-recovery"
                assert [d["code"] for d in cases[name]["typescript_observation"]["reported_diagnostics"]] == [1127, 1005, 1005]
            elif name.startswith("system/") and name.endswith("/external-single-quote"):
                disposition = "independent-system-publication-order"
        elif failed:
            if name.startswith("system/") and "/local-unicode" not in name:
                disposition = "independent-system-publication-order"
            elif name.endswith("/quoted-then-direct"):
                disposition = "independent-TS2484-diagnostic-name"
        assert row["disposition"] == disposition
for stem, count in [("export-specifier-names", 48), ("cjs-default-reexport-names", 8)]:
    f = read(f"crates/compiler/tests/fixtures/{stem}.json")
    assert len(f["cases"]) == count and f["repetitions"] == 2
    assert f["observer_sha256"] == digest((ROOT / f"scripts/observe-{stem}.mjs").read_bytes())
for path, name in zip(m["tests"], ["export_name_syntax_matches_complete_typescript_observations", "export_name_syntax_maps_match_complete_typescript_observations"]):
    assert name in (ROOT / path).read_text()
print("H2.8a-A6-10 ready: 24 owners,4 steps,7 architecture rows,200 witnesses (52 exact /148 failed before); independent/later cases explicit, unresolved=0, undispositioned=0")
