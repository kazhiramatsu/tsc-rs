#!/usr/bin/env python3
"""Check the bounded EOF-comment repair before/after the production edit."""
import gzip
import hashlib
import json
from pathlib import Path
import subprocess
import sys

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[5]
sha = lambda data: hashlib.sha256(data).hexdigest()
manifest = json.loads((HERE / 'readiness.v1.json').read_text())
schema = json.loads((HERE / 'readiness.schema.json').read_text())
assert schema['type'] == 'object' and schema['additionalProperties'] is False
assert set(manifest) == set(schema['required']) == set(schema['properties'])
kinds = {'integer': int, 'string': str, 'array': list, 'object': dict}
for key, rule in schema['properties'].items():
    assert set(rule) <= {'type', 'const'}, key
    assert type(manifest[key]) is kinds[rule['type']], key
    if 'const' in rule:
        assert manifest[key] == rule['const'], key
assert manifest['production_paths'] == ['crates/emitter/src/printer.rs']
assert len(manifest['baseline_rust']) == 1
for pin in manifest['baseline_rust']:
    original = subprocess.check_output(['git', 'show', f"{manifest['base']}:{pin['path']}"], cwd=ROOT)
    assert sha(original) == pin['sha256'], pin['path']
    if '--before' in sys.argv:
        assert sha((ROOT / pin['path']).read_bytes()) == pin['sha256'], pin['path']
    assert b'fn write_transformed_source_file(' in (ROOT / pin['path']).read_bytes()
assert len(manifest['authorities']) == 8
for pin in manifest['authorities']:
    assert sha((ROOT / pin['path']).read_bytes()) == pin['sha256'], pin['path']
packet = (HERE / 'empty-source.md').read_text()
assert len(manifest['owners']) == 4
for owner in manifest['owners']:
    lines = (ROOT / owner['path']).read_bytes().splitlines(keepends=True)
    assert sha(b''.join(lines[owner['start'] - 1:owner['end']])) == owner['sha256']
    assert lines[owner['start'] - 1].decode().startswith('  function ' + owner['owner'] + '(')
    assert owner['owner'] in packet
architecture = (ROOT / 'docs/design/greenfield/emitter-architecture.md').read_text()
assert len(manifest['architecture']) == 2
for row in manifest['architecture']:
    assert row['row'] in architecture and row['id'] in packet
fixture = json.loads((ROOT / 'crates/compiler/tests/fixtures/h2-8b-config-source-span-commands.json').read_text())
assert fixture['repetitions'] == 2 and fixture['upstream_failures'] == []
assert len(fixture['cases']) == manifest['witness']['fixture_cases'] == 6
assert sum(case['case_id'] == manifest['witness']['case_id'] for case in fixture['cases']) == 1
log = gzip.decompress((HERE / 'baseline-config-library.log.gz').read_bytes())
assert sha(log) == manifest['witness']['log_sha256']
assert b'test result: FAILED. 22 passed; 2 failed;' in log
assert b'forced-empty: exact source-map result' in log
assert manifest['after_required'] == dict(config_library_tests=24, config_library_inputs=96,
                                          typed_refusals=2, prologue_tests=1, prologue_inputs=8)
print('OPS-DEBT-EMPTY-SOURCE ready: 4 source owners, 2 architecture rows, 1 reproduced defect; no profile change')
