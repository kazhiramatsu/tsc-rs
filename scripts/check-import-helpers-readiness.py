#!/usr/bin/env python3
"""Verify the source-owned CJS helper import packet and complete before evidence."""
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
m = read('ratchets/h2-8a-import-helpers-readiness.v1.json')
assert m['version'] == 1 and m['slice'] == 'H2.8a-A6-35' and m['state'] == 'ready'
assert m['runtime_paths'] == ['crates/emitter/src/builtins.rs', 'crates/emitter/src/metadata.rs', 'crates/emitter/src/printer.rs']
assert m['steps'] == [f'A6-35-{i}' for i in range(1, 6)]
assert m['unresolved'] == m['undispositioned'] == 0
for r in m['authorities'] + m['unit_sources']:
    assert sha(r['path']) == r['sha256'], r['path']
for r in m['baseline_rust']:
    old = subprocess.check_output(['git', 'show', f'{m["base"]}:{r["path"]}'], cwd=ROOT)
    assert digest(old) == r['sha256'], r['path']
    if '--before' in sys.argv:
        assert sha(r['path']) == r['sha256'], r['path']
packet = (ROOT / 'docs/design/greenfield/slices/h2-8a-import-helpers.md').read_text()
lines = (ROOT / 'vendor/typescript-6.0.3/lib/_tsc.js').read_bytes().splitlines(keepends=True)
assert len(m['owners']) == len({r['owner'] for r in m['owners']}) == 50
for r in m['owners']:
    assert digest(b''.join(lines[r['start'] - 1:r['end']])) == r['sha256'], r['owner']
    assert lines[r['start'] - 1].decode().strip().startswith('function ' + r['owner'] + '(')
    assert r['owner'] in packet and r['step'] in m['steps']
assert len(m['branches']) == m['branch_count']
assert len({(r['owner'], r['line'], r['kind'], r['sha256']) for r in m['branches']}) == len(m['branches'])
for r in m['branches']:
    assert digest(r['predicate'].encode()) == r['sha256']
    assert r['disposition'] in ['owned', 'unchanged-prerequisite', 'outside-packet']
    assert r['step'] in m['steps'] and r['native_owner'] and r['reason']
    owner = next(o for o in m['owners'] if o['owner'] == r['owner'])
    assert owner['start'] <= r['line'] <= owner['end']
    assert r['native_owner'] in packet
arch = (ROOT / 'docs/design/greenfield/emitter-architecture.md').read_text()
for r in m['architecture']:
    assert r['row'] in arch and digest(r['row'].encode()) == r['sha256']
    assert r['row'].replace('](slices/', '](') in packet
for r in m['gaps']:
    assert r['body'] in arch and digest(r['body'].encode()) == r['sha256']
    assert r['id'] in packet and r['disposition'] == 'outside-packet'
b = read('ratchets/h2-8a-import-helpers-before.v1.json')
assert b['base'] == m['base'] and b['eligible'] == 111 and b['native_jobs'] == 2
assert len(b['exact_twice']) == 26 and len(b['failed_twice']) == 85
assert len(b['source_supported_cjs_candidates']) == 80 and len(b['outside_current_cjs_owner']) == 5
assert b['complete_supplemental_tuples_identical']
assert b['primary_command_attempts'] == b['supplemental_executions'] == 274
assert all(j['exit']['exit_code'] == 101 for j in b['jobs'])
assert len(b['original_before']['case_ids']) == 16 and b['original_before']['new_executions'] == 0
assert len(b['original_before']['captures']) == 32
f = read('crates/compiler/tests/fixtures/import-helpers.json')
assert f['typescript'] == '6.0.3' and f['repetitions'] == 2
assert f['observer_sha256'] == sha('scripts/observe-import-helpers.mjs')
assert f['compiler_sha256'] == sha('vendor/typescript-6.0.3/lib/typescript.js')
cases = {c['case_id']: c for c in f['cases']}
assert len(cases) == len(m['witnesses']) == 111
assert set(cases) == set(b['exact_twice'] + b['failed_twice'])
counts = collections.Counter()
for r in m['witnesses']:
    assert r['row_sha256'] == digest(json.dumps(cases[r['case_id']], ensure_ascii=False, separators=(',', ':')).encode())
    expected = 'prior-exact' if r['case_id'] in b['exact_twice'] else 'owned' if r['case_id'] in b['source_supported_cjs_candidates'] else 'outside-packet'
    assert r['disposition'] == expected
    counts[expected] += 1
assert counts == {'prior-exact': 26, 'owned': 80, 'outside-packet': 5}
assert m['after_required'] == {'new_exact': 106, 'new_repairs': 80, 'preserved': 26, 'outside': 5, 'original_exact': 16, 'independent_repetitions': 2, 'emitter_units': True, 'emitter_contracts': 451}
assert 'mod h2_8a_import_helpers;' in (ROOT / 'crates/compiler/tests/contracts.rs').read_text()
test = (ROOT / 'crates/compiler/tests/integration/h2_8a_import_helpers.rs').read_text()
assert 'assert_eq!(cases.len(), 111)' in test and 'failures.is_empty()' in test
assert 'supplemental-complete-command' in test
assert 'fn original_import_helper_collisions_match_complete_commands()' in (ROOT / 'crates/compiler/tests/h2_8a_original_corpus.rs').read_text()
print(f'H2.8a-A6-35 ready:50 whole TS owners,{m["branch_count"]} branch dispositions,111 complete before cases,80 owned/26 prior/5 outside,16 original; unresolved=0,undispositioned=0')
