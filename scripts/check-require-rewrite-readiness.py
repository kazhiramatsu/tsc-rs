#!/usr/bin/env python3
"""Check the source-owned require rewrite packet before runtime edits."""
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent
manifest = json.loads((ROOT / "ratchets/h2-8a-require-rewrite-readiness.v1.json").read_text())
# The schema deliberately uses only this small, checked vocabulary; source
# identities, array members and cross-reference invariants are checked below.
schema = json.loads((ROOT / "docs/design/greenfield/slices/h2-8a-require-rewrite-readiness.schema.json").read_text())
assert schema["type"] == "object" and schema["additionalProperties"] is False
assert set(manifest) == set(schema["required"]) == set(schema["properties"])
kinds = {"integer": int, "string": str, "array": list, "object": dict}
for key, rule in schema["properties"].items():
    assert set(rule) <= {"type", "const", "items"}, key
    assert type(manifest[key]) is kinds[rule["type"]], key
    if "const" in rule:
        assert manifest[key] == rule["const"], key
    if "items" in rule:
        assert set(rule["items"]) == {"type"}, key
        assert all(type(item) is kinds[rule["items"]["type"]] for item in manifest[key]), key
digest = lambda data: hashlib.sha256(data).hexdigest()
assert manifest["version"] == 1 and manifest["slice"] == "H2.8a-require-rewrite"
assert manifest["unresolved"] == manifest["undispositioned"] == 0
for pin in manifest["authorities"]:
    assert digest((ROOT / pin["path"]).read_bytes()) == pin["sha256"], pin["path"]
for pin in manifest["baseline_rust"]:
    original = subprocess.check_output(["git", "show", f'{manifest["base"]}:{pin["path"]}'], cwd=ROOT)
    assert digest(original) == pin["sha256"], pin["path"]
packet = (ROOT / "docs/design/greenfield/slices/h2-8a-require-rewrite.md").read_text()
source = (ROOT / "vendor/typescript-6.0.3/lib/_tsc.js").read_bytes().splitlines(keepends=True)
assert len(manifest["owners"]) == 19
for row in manifest["owners"]:
    assert digest(b"".join(source[row["start"]-1:row["end"]])) == row["sha256"], row["owner"]
    assert row["step"] in packet and row["owner"] in packet
architecture = (ROOT / "docs/design/greenfield/emitter-architecture.md").read_text()
assert len(manifest["architecture"]) == 7
for row in manifest["architecture"]:
    assert row["row"] in architecture and row["id"] in packet
fixture = json.loads((ROOT / "crates/compiler/tests/fixtures/h2-8a-require-rewrite.json").read_text())
assert fixture["repetitions"] == 2 and fixture["upstream_failures"] == []
assert len(fixture["cases"]) == len({case["case_id"] for case in fixture["cases"]}) == manifest["focused_count"] == 60
assert fixture["observer_sha256"] == digest((ROOT / "scripts/observe-require-rewrite.mjs").read_bytes())
composition = json.loads((ROOT / "crates/compiler/tests/fixtures/h2-8a-require-rewrite-composition.json").read_text())
assert composition["repetitions"] == 2 and composition["upstream_failures"] == []
assert len(composition["cases"]) == manifest["composition_count"] == 4
assert composition["observer_sha256"] == digest((ROOT / "scripts/observe-require-rewrite-composition.mjs").read_bytes())
substitution = json.loads((ROOT / "crates/compiler/tests/fixtures/h2-8a-require-rewrite-substitution.json").read_text())
assert substitution["repetitions"] == 2 and substitution["upstream_failures"] == []
assert len(substitution["cases"]) == manifest["substitution_count"] == 4
assert substitution["observer_sha256"] == digest((ROOT / "scripts/observe-require-rewrite-substitution.mjs").read_bytes())
assert (ROOT / "crates/compiler/tests/h2_8a_require_rewrite.rs").is_file()
print("require rewrite ready: 19 owners, 7 architecture rows, 60 focused + 8 composition + 2 original commands; unresolved=0, undispositioned=0")
