#!/usr/bin/env python3
"""Check the ordinary modifier and spread printer comment-phase prerequisites."""
from pathlib import Path
import collections
import hashlib
import json
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
read = lambda p: json.loads((ROOT / p).read_text())
digest = lambda data: hashlib.sha256(data).hexdigest()
sha = lambda p: digest((ROOT / p).read_bytes())
m = read('ratchets/h2-8a-token-comment-phases-readiness.v1.json')
assert m['version'] == 1 and m['slice'] == 'H2.8a-A6-36' and m['state'] == 'ready'
assert m['runtime_paths'] == ['crates/emitter/src/printer.rs']
assert m['steps'] == [f'A6-36-{i}' for i in range(1, 5)]
assert m['unresolved'] == m['undispositioned'] == 0
for r in m['authorities'] + m['unit_sources']:
    assert sha(r['path']) == r['sha256'], r['path']
for r in m['baseline_rust']:
    old = subprocess.check_output(['git', 'show', f'{m["base"]}:{r["path"]}'], cwd=ROOT)
    assert digest(old) == r['sha256'], r['path']
    if '--before' in sys.argv:
        assert sha(r['path']) == r['sha256'], r['path']
old_printer = subprocess.check_output(['git', 'show', f'{m["base"]}:crates/emitter/src/printer.rs'], cwd=ROOT).decode().splitlines()
calls = [(i + 1, line) for i, line in enumerate(old_printer) if 'self.emit_modifiers(' in line]
assert len(calls) == len(m['native_modifier_calls']) == 28
for (line, text), r in zip(calls, m['native_modifier_calls'], strict=True):
    assert line == r['line'] and text == r['text'] and digest(text.encode()) == r['sha256']
assert sum(r['role'].startswith('extra native phase') for r in m['native_modifier_calls']) == 5
packet = (ROOT / 'docs/design/greenfield/slices/h2-8a-token-comment-phases.md').read_text()
lines = (ROOT / 'vendor/typescript-6.0.3/lib/_tsc.js').read_bytes().splitlines(keepends=True)
assert len(m['owners']) == len({r['owner'] for r in m['owners']}) == 78
assert sum(o['modifier_caller'] for o in m['owners']) == 22
for r in m['owners']:
    assert digest(b''.join(lines[r['start'] - 1:r['end']])) == r['sha256'], r['owner']
    assert lines[r['start'] - 1].decode().strip().startswith('function ' + r['owner'] + '(')
    assert r['owner'] in packet and r['native_owner'] in packet and r['step'] in m['steps']
assert len(m['branches']) == m['branch_count'] == 501
assert len({(r['owner'], r['line'], r['kind'], r['sha256']) for r in m['branches']}) == 501
for r in m['branches']:
    assert digest(r['predicate'].encode()) == r['sha256']
    assert r['disposition'] in ['owned', 'unchanged-prerequisite', 'outside-packet']
    assert r['step'] in m['steps'] and r['native_owner'] in packet and r['reason']
    o = next(o for o in m['owners'] if o['owner'] == r['owner'])
    assert o['start'] <= r['line'] <= o['end']
arch = (ROOT / 'docs/design/greenfield/emitter-architecture.md').read_text()
for r in m['architecture']:
    assert r['row'] in arch and digest(r['row'].encode()) == r['sha256']
    assert r['row'].replace('](slices/', '](') in packet
for r in m['gaps']:
    assert r['body'] in arch and digest(r['body'].encode()) == r['sha256']
    assert r['id'] in packet and r['disposition'] == 'outside-packet'
regression = m['source_file_scope_regression']
assert regression['initial_contracts_job']['exit_code'] == 101
assert len(regression['failed_case_ids']) == len(set(regression['failed_case_ids'])) == 27
assert regression['frozen_reprint_rows'] == 1350
for r in [regression['fixture'], regression['test']]:
    assert sha(r['path']) == r['sha256']
assert len(read(regression['fixture']['path'])['rows']) == 1350
b = read('ratchets/h2-8a-token-comment-phases-before.v1.json')
assert b['base'] == m['base'] and b['eligible'] == 293 and b['native_jobs'] == 2
assert len(b['exact_twice']) == 215 and len(b['failed_twice']) == 78
assert len(b['source_supported_candidates']) == 55 and len(b['outside_current_printer_owner']) == 23
assert b['complete_supplemental_tuples_identical']
assert b['primary_command_attempts'] == b['supplemental_executions'] == 1016
assert all(j['exit']['exit_code'] == 101 for j in b['jobs'])
assert len(b['original_before']['case_ids']) == 3
assert b['original_before']['new_executions'] == 6
assert len(b['original_before']['captures']) == 6
direct = b['direct_before']
assert direct['eligible'] == 96 and len(direct['exact_twice']) == 64 and len(direct['failed_twice']) == 32
assert direct['repeated_complete_failures_identical']
assert direct['job']['exit']['exit_code'] == 101
assert len(direct['captures']) == 64
f = read('crates/compiler/tests/fixtures/token-comment-phases.json')
assert f['typescript'] == '6.0.3' and f['repetitions'] == 2
assert f['observer_sha256'] == sha('scripts/observe-token-comment-phases.mjs')
assert f['compiler_sha256'] == sha('vendor/typescript-6.0.3/lib/typescript.js')
cases = {c['case_id']: c for c in f['cases']}
assert len(cases) == 129
for name, count in [('class-header-token', 88), ('class-optional-name', 64)]:
    old = read(f'crates/compiler/tests/fixtures/{name}.json')['cases']
    assert len(old) == count
    cases.update({c['case_id']: c for c in old})
old = [c for c in read('crates/compiler/tests/fixtures/class-helper-accessor-producers.json')['cases'] if c['case_id'].endswith('/static-modifier-comments')]
assert len(old) == 12
cases.update({c['case_id']: c for c in old})
assert len(cases) == len(m['witnesses']) == 293
assert set(cases) == set(b['exact_twice'] + b['failed_twice'])
counts = collections.Counter()
for r in m['witnesses']:
    assert r['row_sha256'] == digest(json.dumps(cases[r['case_id']], ensure_ascii=False, separators=(',', ':')).encode())
    expected = 'prior-exact' if r['case_id'] in b['exact_twice'] else 'owned' if r['case_id'] in b['source_supported_candidates'] else 'outside-packet'
    assert r['disposition'] == expected
    counts[expected] += 1
assert counts == {'prior-exact': 215, 'owned': 55, 'outside-packet': 23}
f = read('crates/emitter/tests/fixtures/token-comment-phase-printer-metadata.json')
assert f['observer_sha256'] == sha('scripts/observe-token-comment-phase-printer-metadata.mjs')
assert f['compiler_sha256'] == sha('vendor/typescript-6.0.3/lib/typescript.js')
assert f['route'] == 'direct-printer-metadata' and f['repetitions'] == 2
assert len(f['cases']) == len(m['direct_witnesses']) == 96
for c, r in zip(f['cases'], m['direct_witnesses'], strict=True):
    assert c['case_id'] == r['case_id']
    assert r['row_sha256'] == digest(json.dumps(c, ensure_ascii=False, separators=(',', ':')).encode())
assert m['after_required'] == {'new_exact': 270, 'new_repairs': 55, 'preserved': 215, 'outside': 23, 'original_exact': 3, 'direct_exact': 96, 'independent_repetitions': 2, 'emitter_units': 494, 'emitter_contracts': 451, 'class_header_direct_controls': 32}
assert 'mod h2_8a_token_comment_phases;' in (ROOT / 'crates/compiler/tests/contracts.rs').read_text()
test = (ROOT / 'crates/compiler/tests/integration/h2_8a_token_comment_phases.rs').read_text()
assert 'assert_eq!(cases.len(), 293)' in test and 'failures.is_empty()' in test
assert 'supplemental-complete-command' in test
assert 'fn original_spread_token_comments_match_complete_commands()' in (ROOT / 'crates/compiler/tests/h2_8a_original_corpus.rs').read_text()
print('H2.8a-A6-36 ready:78 whole TS owners,501 branch dispositions,293 complete before cases,55 owned/215 prior/23 outside;96 direct controls,3 original; unresolved=0,undispositioned=0')
