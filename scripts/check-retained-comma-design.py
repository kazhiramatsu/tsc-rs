#!/usr/bin/env python3
"""Verify the A40 comma design amendment; no production readiness claim."""
from pathlib import Path
import hashlib
import json
import subprocess

ROOT = Path(__file__).resolve().parent.parent
read = lambda p: json.loads((ROOT / p).read_text())
sha = lambda p: hashlib.sha256((ROOT / p).read_bytes()).hexdigest()
m = read('ratchets/h2-8a-retained-comma-design.v1.json')
assert m['state'] == 'comma-factory-design-resolved-typechecked; whole A40 readiness remains open'
for p in m['authorities']:
    assert sha(p['path']) == p['sha256'], p['path']
assert sha(m['closes_finding']['path']) == m['closes_finding']['sha256']
prior_review = read(m['closes_finding']['path'])
assert m['closes_finding']['id'] in {r['id'] for r in prior_review['open_findings']}
subprocess.run(['node', 'scripts/observe-retained-comma-sources.mjs', '--check'], cwd=ROOT, check=True)
source = read('ratchets/h2-8a-retained-comma-sources.v1.json')
assert source['counts'] == {'owners': 15, 'direct_id_calls': 32, 'pre_retained_identity_requests': 40}
old = (ROOT / m['predecessor_candidate']['path']).read_text()
for edit in m['source_edits']:
    assert old.count(edit['before']) == 1
    old = old.replace(edit['before'], edit['after'])
assert old == (ROOT / m['candidate']['path']).read_text()
assert sha(m['candidate']['path']) == m['candidate']['sha256']
assert len(m['predicate_map']) == 7
assert all(r['ts'] and r['rust'] and r['reason'] for r in m['predicate_map'])
assert m['implementation']['new_shared_api'] is False
assert m['implementation']['new_state_representation'] is False
assert m['implementation']['allowed_production_files'] == ['crates/emitter/src/builtins/class_fields.rs']
requests = {r['start_utf16']: r for r in source['pre_retained_identity_requests']}
assert len(requests) == len(m['identity_proof']['pre_retained_requests']) == 40
for row in m['identity_proof']['pre_retained_requests']:
    request = requests[row['start_utf16']]
    assert request['sha256'] == row['sha256'] and request['owner']['name'] == row['owner']
    assert row['domain'] and row['excludes_unanchored_comma_operand']
assert sum(m['identity_proof']['direct_call_counts'].values()) == 32
assert m['identity_proof']['resolver_boundary'] and m['identity_proof']['custom_transform_guard']
check = read('ratchets/h2-8a-retained-lexical-design-check.v2.json')
assert check['candidate']['sha256'] == m['candidate']['sha256']
assert check['candidate']['lines'] == 3904 and check['validation']['actual_exit'] == 0
assert check['validation']['attempt'] == 4 and check['validation']['runtime_tests_executed'] == 0
assert check['attempts'][-1]['terminal']['production_unchanged']
assert len(check['source_pins']) == 678
fixture = read('crates/compiler/tests/fixtures/retained-comma-factory.json')
assert fixture['repetitions'] == 2 and fixture['program_executions'] == 16
assert not fixture['upstream_failures']
assert m['witness_ids'] == [c['case_id'] for c in fixture['cases']]
before = read('ratchets/h2-8a-retained-lexical-owners-before.v3.json')
assert before['eligible'] == 530 and len(before['exact_twice']) == 393 and len(before['failed_twice']) == 137
assert len(before['required_repair_ids']) == 80 and len(before['successor_controls']) == 57
assert before['minimum_exact_after'] == 473
assert before['new_native_cases'] == 8 and before['reused_cases'] == 522
assert before['primary_attempts'] == before['supplemental_executions'] == 1060
assert m['acceptance'] == {'eligible': 530, 'prior_exact': 393, 'required_repairs': 80,
    'successor_controls': 57, 'minimum_exact_after': 473, 'primary_attempts': 1060, 'supplemental_executions': 1060}
assert set(m['witness_ids']) <= set(before['exact_twice'] + before['failed_twice'])
print('A40 comma design verified:7 predicates/32 ID sites/40 request domains;candidate3904 lines typechecked;before530=393 exact/137 failed;whole A40 readiness remains open')
