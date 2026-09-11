#!/usr/bin/env python3
"""Verify A6-39 source-derived edits and the frozen 454-case qualification scope."""
from pathlib import Path
import collections
import hashlib
import json
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
read = lambda p: json.loads((ROOT / p).read_text())
h = lambda b: hashlib.sha256(b).hexdigest()
sha = lambda p: h((ROOT / p).read_bytes())
m = read('ratchets/h2-8a-retained-field-producers-readiness.v1.json')
assert (m['version'], m['slice'], m['state']) == (1, 'H2.8a-A6-39', 'ready')
assert m['unresolved'] == m['undispositioned'] == 0
assert m['steps'] == ['A6-39-1', 'A6-39-2', 'A6-39-3']
assert m['runtime_paths'] == ['crates/emitter/src/builtins/class_fields.rs']
for row in m['authorities']:
    assert sha(row['path']) == row['sha256'], row['path']
path = m['runtime_paths'][0]
base = subprocess.check_output(['git', 'show', m['base'] + ':' + path], cwd=ROOT)
assert h(base) == m['baseline_rust_sha256']
candidate = base.decode()
for edit in m['source_edits']:
    assert candidate.count(edit['before']) == 1
    candidate = candidate.replace(edit['before'], edit['after'])
assert h(candidate.encode()) == m['candidate_rust_sha256']
actual = (ROOT / path).read_bytes()
assert h(actual) == m['production_preimage_sha256'] if '--before' in sys.argv else h(actual) in [m['baseline_rust_sha256'], m['production_preimage_sha256'], m['candidate_rust_sha256']]
# Routing, allocator and initializer-member-access owners are unchanged byte for byte.
for name, next_name in [('allocate_temp_name', 'prepend_hoisted_declarations'),
                        ('auto_accessor_names', 'transform_members'),
                        ('create_receiver_access', 'inject_initializers_into_constructor')]:
    marker = '    fn ' + name + '('; end = '    fn ' + next_name + '('
    assert base.decode().split(marker)[1].split(end)[0] == candidate.split(marker)[1].split(end)[0], name
assert candidate.count('initialize_transform_flags(') == base.decode().count('initialize_transform_flags(') == 1
assert 'receiver: Option<&str>' not in candidate
assert 'class_receiver: Option<&str>' not in candidate
assert 'create_accessor_storage_access' in candidate and 'complete_created_node_flags' in candidate
source = (ROOT / 'vendor/typescript-6.0.3/lib/_tsc.js').read_bytes().splitlines(keepends=True)
assert len(m['owners']) == len({r['owner'] for r in m['owners']}) == 47
owners = {r['owner']: r for r in m['owners']}
for row in m['owners']:
    assert h(b''.join(source[row['start'] - 1:row['end']])) == row['sha256'], row['owner']
    assert source[row['start'] - 1].decode().strip().startswith('function ' + row['owner'] + '(')
    assert row['native_owner'] and row['reason'] and row['step'] in m['steps'] and row['test']
assert len(m['branches']) == 230 and len(m['calls']) == 285
for row in m['branches'] + m['calls']:
    owner = owners[row['owner']]
    assert owner['start'] <= row['line'] <= owner['end']
    assert row['native_owner'] and row['step'] in m['steps']
    assert row['disposition'] in ['producer-owned', 'unchanged-factory-seam', 'future-owned-fail-closed']
    if row['disposition'] == 'future-owned-fail-closed':
        assert row['next_owner'] == 'A6-40' and row['guard'] and row['control']
    if 'predicate' in row:
        assert h(row['predicate'].encode()) == row['sha256']
arch = (ROOT / 'docs/design/greenfield/emitter-architecture.md').read_text()
packet = (ROOT / 'docs/design/greenfield/slices/h2-8a-retained-field-producers.md').read_text()
assert len(m['architecture']) == len({r['id'] for r in m['architecture']}) == 11
for row in m['architecture']:
    assert row['row'] in arch and row['id'] in packet
    assert h(row['row'].encode()) == row['sha256']
    assert row['validation_ref'] and row['step'] in m['steps']
    if row['inherited_qualified_premise']:
        assert row['disposition'] == 'premise-unchanged' and '`active-qualified`' in row['row']
assert len(m['rust_map']) == 13
assert all(r['producer_owner_consumer'] and r['gap_action_evidence'] and r['lifetime'] and r['invalidation'] for r in m['rust_map'])
assert {r['id'] for r in m['gaps']} == {'EA-GAP-FLAGS', 'EA-GAP-CAPTURE'}
b = read('ratchets/h2-8a-retained-accessor-owners-before.v1.json')
assert b['base'] == m['base'] and b['eligible'] == 454
assert len(b['exact_twice']) == 228 and len(b['failed_twice']) == 226
assert b['primary_attempts'] == b['supplemental_executions'] == 908
assert not b['typed_boundary_cases'] and b['repetition_requirement_satisfied']
assert b['exit']['exit_code'] == 101
assert m['prior_exact_ids'] == sorted(b['exact_twice'])
cases = (read('crates/compiler/tests/fixtures/retained-accessor-owners.json')['cases']
         + read('crates/compiler/tests/fixtures/class-helper-accessor-producers.json')['cases']
         + [r for r in read('crates/compiler/tests/fixtures/class-field-alias-map-positions.json')['cases'] if r['options']['target'] == 9]
         + read('crates/compiler/tests/fixtures/decorator-receiver-context.json')['cases'])
assert len(cases) == len(m['witnesses']) == 454
for case, row in zip(cases, m['witnesses'], strict=True):
    assert case['case_id'] == row['case_id']
    assert h(json.dumps(case, ensure_ascii=False, separators=(',', ':')).encode()) == row['row_sha256']
    if row['status'] == 'successor-negative':
        assert row['causes'] and all(c.startswith(('A6-40:', 'A6-41:')) for c in row['causes'])
    if row['status'] == 'required-repair':
        assert not row['causes'] and row['case_id'] in b['failed_twice']
assert collections.Counter(r['status'] for r in m['witnesses']) == {'prior-exact': 228, 'required-repair': 127, 'successor-negative': 99}
assert set(m['required_repair_ids']) | set(m['successor_negative_ids']) == set(b['failed_twice'])
assert not set(m['required_repair_ids']) & set(m['successor_negative_ids'])
assert m['after_required'] == {'complete_cases': 454, 'minimum_exact_twice': 355, 'required_repairs': 127, 'prior_exact': 228, 'primary_attempts': 908, 'supplemental_executions': 908, 'emitter_units': 494, 'emitter_contracts': 451, 'frozen_declaration_reprints': 1350}
assert sha(m['architecture_preimage']['path']) == m['architecture_preimage']['sha256']
assert m['resource_policy'] == {'heavy_jobs': 1, 'cargo_build_jobs': 2, 'nice': 15, 'taskpolicy': 'background'}
subprocess.run(['node', 'scripts/observe-retained-field-producer-sources.mjs', '--check'], cwd=ROOT, check=True)
subprocess.run(['node', 'scripts/classify-retained-field-producers.mjs', '--check'], cwd=ROOT, check=True)
assert 'fn identifier_text(' not in candidate
assert 'move_instance_initializers |=' not in candidate
print('H2.8a-A6-39 ready:47 whole owners/230 predicates/285 calls;13 Rust rows;454 witnesses;127 required repairs/228 preserved/99 successor negatives;unresolved=0,undispositioned=0')
