#!/usr/bin/env python3
"""Check the exact A6-38 object-property printer edit and its frozen witnesses."""
from pathlib import Path
import collections
import hashlib
import json
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent

def read(path):
    return json.loads((ROOT / path).read_text())

def digest(data):
    return hashlib.sha256(data).hexdigest()

def sha(path):
    return digest((ROOT / path).read_bytes())

m = read('ratchets/h2-8a-object-property-owners-readiness.v1.json')
assert m['version'] == 1 and m['slice'] == 'H2.8a-A6-38' and m['state'] == 'ready'
assert m['runtime_paths'] == ['crates/emitter/src/printer.rs']
assert m['steps'] == ['A6-38-1', 'A6-38-2']
assert m['unresolved'] == m['undispositioned'] == 0 and m['rust_map_rows'] == 10
for r in m['authorities'] + m['unit_sources']:
    assert sha(r['path']) == r['sha256'], r['path']
base = subprocess.check_output(['git', 'show', m['base'] + ':crates/emitter/src/printer.rs'], cwd=ROOT).decode()
assert digest(base.encode()) == m['baseline_rust'][0]['sha256']
candidate = base
assert [r['kind'] for r in m['source_edits']] == ['PropertyAssignment', 'ShorthandPropertyAssignment']
for edit in m['source_edits']:
    assert candidate.count(edit['before']) == 1
    assert edit['before'].count('self.emit_modifiers(') == 1
    assert 'self.emit_modifiers(' not in edit['after']
    assert edit['before'].startswith(f'            NodeData::{edit["kind"]}(data) => {{\n')
    candidate = candidate.replace(edit['before'], edit['after'])
assert digest(candidate.encode()) == m['planned_printer_sha256']
assert base.count('self.emit_modifiers(') == m['before_modifier_call_count']
assert candidate.count('self.emit_modifiers(') == m['after_modifier_call_count'] == m['before_modifier_call_count'] - 2
actual = (ROOT / 'crates/emitter/src/printer.rs').read_text()
assert actual == base if '--before' in sys.argv else actual in [base, candidate]
packet = (ROOT / 'docs/design/greenfield/slices/h2-8a-object-property-owners.md').read_text()
lines = (ROOT / 'vendor/typescript-6.0.3/lib/_tsc.js').read_bytes().splitlines(keepends=True)
assert len(m['owners']) == len({r['owner'] for r in m['owners']}) == 9
owners = {r['owner']: r for r in m['owners']}
for r in m['owners']:
    assert digest(b''.join(lines[r['start'] - 1:r['end']])) == r['sha256'], r['owner']
    assert lines[r['start'] - 1].decode().strip().startswith('function ' + r['owner'] + '(')
    assert r['owner'] in packet and r['native_owner'] in packet and r['step'] in m['steps']
assert len(m['branches']) == 231 and len(m['calls']) == 216
for r in m['branches']:
    assert digest(r['predicate'].encode()) == r['sha256']
    assert owners[r['owner']]['start'] <= r['line'] <= owners[r['owner']]['end']
    assert r['disposition'] == 'unchanged-prerequisite' and r['reason']
    assert r['step'] in m['steps'] and r['native_owner'] in packet
for r in m['calls']:
    assert r['owner'] in owners and r['disposition'] == 'unchanged-prerequisite'
arch = (ROOT / 'docs/design/greenfield/emitter-architecture.md').read_text()
assert len(m['architecture']) == len({r['id'] for r in m['architecture']}) == 8
for r in m['architecture']:
    assert r['row'] in arch and r['row'].replace('](slices/', '](') in packet
    assert digest(r['row'].encode()) == r['sha256']
assert {r['id'] for r in m['gaps']} == {'EA-GAP-FLAGS', 'EA-GAP-CAPTURE'}
assert all(r['id'] in packet and r['id'] in arch and r['reason'] for r in m['gaps'])
b = read('ratchets/h2-8a-object-property-owners-before.v1.json')
assert b['base'] == m['base'] and b['eligible'] == 48
assert len(b['exact_twice']) == 16 and len(b['failed_twice']) == 32 and not b['typed_boundary_cases']
assert b['primary_attempts'] == b['supplemental_executions'] == 96 and b['repeated_complete_captures_identical']
assert b['actual_exit']['exit_code'] == 101 and b['typescript']['actual_exit']['exit_code'] == 0
assert b['typescript']['cases'] == 48 and b['typescript']['program_executions'] == 96
assert b['original_reuse']['new_executions'] == 0 and b['original_reuse']['reused_primary_attempts'] == 8
assert len(b['original_reuse']['exact_twice']) == 3 and len(b['original_reuse']['failed_twice']) == 1
assert b['original_reuse']['all_other_complete_fields_identical']
f = read('crates/compiler/tests/fixtures/object-property-owners.json')
assert f['observer_sha256'] == sha('scripts/observe-object-property-owners.mjs')
assert f['compiler_sha256'] == sha('vendor/typescript-6.0.3/lib/typescript.js')
assert f['repetitions'] == 2 and len(f['cases']) == 48
assert collections.Counter(d['code'] for r in f['cases'] for d in r['typescript_observation']['reported_diagnostics']) == {1042: 36, 2695: 4}
cases = f['cases'] + read('crates/compiler/tests/fixtures/const-modifier-erasure.json')['cases']
assert len(cases) == len(m['witnesses']) == 128
assert len({r['case_id'] for r in cases}) == 128
for case, r in zip(cases, m['witnesses'], strict=True):
    assert case['case_id'] == r['case_id'] and r['step'] == 'A6-38-2'
    assert digest(json.dumps(case, ensure_ascii=False, separators=(',', ':')).encode()) == r['row_sha256']
    assert r['disposition'] == ('owned' if r['case_id'] in b['failed_twice'] else 'prior-exact')
assert m['after_required'] == {'fresh': 48, 'adjacent': 80, 'original': 4, 'repairs': 32, 'emitter_units': 494, 'emitter_contracts': 451, 'frozen_declaration_reprints': 1350, 'repetitions': 2}
assert 'mod h2_8a_object_property_owners;' in (ROOT / 'crates/compiler/tests/contracts.rs').read_text()
test = (ROOT / 'crates/compiler/tests/integration/h2_8a_object_property_owners.rs').read_text()
assert 'assert_eq!(cases.len(), 128)' in test and 'failures.is_empty()' in test
assert 'supplemental-complete-command' in test
print('H2.8a-A6-38 ready:9 whole owners,231 unchanged predicates,10 Rust rows,128 witnesses;32 repairs targeted;unresolved=0,undispositioned=0')
