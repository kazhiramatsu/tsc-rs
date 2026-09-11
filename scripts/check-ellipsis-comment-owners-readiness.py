#!/usr/bin/env python3
"""Validate the A6-37 ordinary ellipsis comment and admission prerequisites."""
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


manifest = read('ratchets/h2-8a-ellipsis-comment-owners-readiness.v1.json')
assert manifest['version'] == 1 and manifest['slice'] == 'H2.8a-A6-37'
assert manifest['state'] == 'ready'
assert manifest['runtime_paths'] == ['crates/emitter/src/printer.rs', 'crates/emitter/src/builtins.rs']
assert manifest['steps'] == [f'A6-37-{i}' for i in range(1, 6)]
assert manifest['unresolved'] == manifest['undispositioned'] == 0
for entry in manifest['authorities'] + manifest['unit_sources']:
    assert sha(entry['path']) == entry['sha256'], entry['path']
for entry in manifest['baseline_rust']:
    data = subprocess.check_output(['git', 'show', manifest['base'] + ':' + entry['path']], cwd=ROOT)
    assert digest(data) == entry['sha256'], entry['path']
    if '--before' in sys.argv:
        assert sha(entry['path']) == entry['sha256'], entry['path']
base_builtins = subprocess.check_output(
    ['git', 'show', manifest['base'] + ':crates/emitter/src/builtins.rs'], cwd=ROOT
).decode().splitlines()
admission = [(index + 1, line) for index, line in enumerate(base_builtins) if 'downlevels_es2018' in line]
assert len(admission) == len(manifest['native_admission_references']) == 7
for (line, text), entry in zip(admission, manifest['native_admission_references'], strict=True):
    assert line == entry['line'] and text == entry['text']
    assert digest(text.encode()) == entry['sha256']
packet = (ROOT / 'docs/design/greenfield/slices/h2-8a-ellipsis-comment-owners.md').read_text()
source_lines = (ROOT / 'vendor/typescript-6.0.3/lib/_tsc.js').read_bytes().splitlines(keepends=True)
assert len(manifest['owners']) == len({entry['owner'] for entry in manifest['owners']}) == 124
owners = {entry['owner']: entry for entry in manifest['owners']}
for entry in owners.values():
    assert digest(b''.join(source_lines[entry['start'] - 1:entry['end']])) == entry['sha256'], entry['owner']
    assert source_lines[entry['start'] - 1].decode().strip().startswith('function ' + entry['owner'] + '(')
    assert entry['owner'] in packet and entry['native_owner'] in packet
    assert entry['step'] in manifest['steps']
assert len(manifest['branches']) == manifest['branch_count'] == 552
assert len({(entry['owner'], entry['line'], entry['kind'], entry['sha256']) for entry in manifest['branches']}) == 552
for entry in manifest['branches']:
    assert digest(entry['predicate'].encode()) == entry['sha256']
    assert entry['disposition'] in ['owned', 'unchanged-prerequisite', 'outside-packet']
    assert entry['step'] in manifest['steps'] and entry['native_owner'] in packet and entry['reason']
    owner = owners[entry['owner']]
    assert owner['start'] <= entry['line'] <= owner['end']
architecture = (ROOT / 'docs/design/greenfield/emitter-architecture.md').read_text()
for entry in manifest['architecture']:
    assert entry['row'] in architecture and digest(entry['row'].encode()) == entry['sha256']
    assert entry['row'].replace('](slices/', '](') in packet
for entry in manifest['gaps']:
    assert entry['body'] in architecture and digest(entry['body'].encode()) == entry['sha256']
    assert entry['id'] in packet and entry['disposition'] == 'outside-packet'
before = read('ratchets/h2-8a-ellipsis-comment-owners-before.v1.json')
assert before['base'] == manifest['base'] and before['eligible'] == 398 and before['native_jobs'] == 2
assert len(before['exact_twice']) == 293 and len(before['failed_twice']) == 105
assert len(before['source_supported_candidates']) == 97 and len(before['outside_current_comment_owner']) == 8
assert len(before['guard_candidates']) == 80 and len(before['ordinary_dot_or_name_phase_candidates']) == 17
assert before['complete_supplemental_tuples_identical'] and before['all374_before1_emitted_tuples_identical']
assert before['primary_command_attempts'] == before['supplemental_executions'] == 1382
assert before['preserved_a36_positives'] == 270 and not before['prior_positive_regressions']
assert len(before['jobs']) == 2 and all(job['exit']['exit_code'] == 101 for job in before['jobs'])
assert len(before['object_rest_map_only']) == 3
assert all(entry['all_other_complete_fields_identical'] for entry in before['object_rest_map_only'])
direct = before['direct_before']
assert direct['eligible'] == 144 and len(direct['exact_twice']) == 105 and len(direct['failed_twice']) == 39
assert direct['prints'] == 288 and len(direct['captures']) == 78
assert direct['repeated_complete_failures_identical'] and direct['job']['exit_code'] == 101
assert before['new_typescript_programs'] == 210 and before['new_typescript_direct_prints'] == 288
assert len(before['typescript_jobs']) == 2 and all(job['exit']['exit_code'] == 0 for job in before['typescript_jobs'])

cases = []
for name, count in [('ellipsis-comment-owners', 105), ('token-comment-phases', 129),
                    ('class-header-token', 88), ('class-optional-name', 64),
                    ('class-helper-accessor-producers', 12)]:
    fixture = read(f'crates/compiler/tests/fixtures/{name}.json')
    rows = fixture['cases']
    if name == 'class-helper-accessor-producers':
        rows = [case for case in rows if case['case_id'].endswith('/static-modifier-comments')]
    assert len(rows) == count
    cases.extend(rows)
    if name == 'ellipsis-comment-owners':
        assert fixture['typescript'] == '6.0.3' and fixture['repetitions'] == 2
        assert fixture['observer_sha256'] == sha('scripts/observe-ellipsis-comment-owners.mjs')
        assert fixture['compiler_sha256'] == sha('vendor/typescript-6.0.3/lib/typescript.js')
cases = {case['case_id']: case for case in cases}
assert len(cases) == len(manifest['witnesses']) == 398
assert set(cases) == set(before['exact_twice'] + before['failed_twice'])
counts = collections.Counter()
for entry in manifest['witnesses']:
    case = cases[entry['case_id']]
    assert entry['row_sha256'] == digest(json.dumps(case, ensure_ascii=False, separators=(',', ':')).encode())
    expected = 'prior-exact' if entry['case_id'] in before['exact_twice'] else (
        'owned' if entry['case_id'] in before['source_supported_candidates'] else 'outside-packet')
    assert entry['disposition'] == expected
    counts[expected] += 1
assert counts == {'prior-exact': 293, 'owned': 97, 'outside-packet': 8}
fixture = read('crates/emitter/tests/fixtures/ellipsis-comment-printer-metadata.json')
assert fixture['observer_sha256'] == sha('scripts/observe-ellipsis-comment-printer-metadata.mjs')
assert fixture['compiler_sha256'] == sha('vendor/typescript-6.0.3/lib/typescript.js')
assert fixture['route'] == 'direct-printer-metadata' and fixture['repetitions'] == 2
assert len(fixture['cases']) == len(manifest['direct_witnesses']) == 144
for case, entry in zip(fixture['cases'], manifest['direct_witnesses'], strict=True):
    assert case['case_id'] == entry['case_id']
    assert entry['row_sha256'] == digest(json.dumps(case, ensure_ascii=False, separators=(',', ':')).encode())
assert manifest['after_required'] == {
    'new_exact': 390, 'new_repairs': 97, 'preserved': 293, 'outside': 8,
    'direct_exact': 144, 'prior_token_direct_exact': 96, 'class_header_direct_exact': 32,
    'independent_repetitions': 2, 'emitter_units': 494, 'emitter_contracts': 451,
    'frozen_declaration_reprints': 1350,
}
assert 'mod h2_8a_ellipsis_comment_owners;' in (ROOT / 'crates/compiler/tests/contracts.rs').read_text()
test = (ROOT / 'crates/compiler/tests/integration/h2_8a_ellipsis_comment_owners.rs').read_text()
assert 'assert_eq!(cases.len(), 398)' in test and 'failures.is_empty()' in test
assert 'supplemental-complete-command' in test
print('H2.8a-A6-37 ready:124 whole TS owners,552 dispositions,398 complete before cases;97 owned/293 prior/8 outside;144 direct controls;unresolved=0,undispositioned=0')
