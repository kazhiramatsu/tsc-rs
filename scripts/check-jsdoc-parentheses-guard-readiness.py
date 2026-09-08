#!/usr/bin/env python3
"""Check the direct JSDoc parenthesis guard and complete before."""
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent
read = lambda p: json.loads((ROOT / p).read_text())
digest = lambda b: hashlib.sha256(b).hexdigest()
m = read('ratchets/h2-8a-jsdoc-parentheses-guard-readiness.v1.json')
assert m['version'] == 1 and m['slice'] == 'H2.8a-A6-16'
assert m['unresolved'] == m['undispositioned'] == 0
assert m['runtime_paths'] == ['crates/checker/src/expr.rs']
assert m['steps'] == ['A6-16-1']
for row in m['authorities']:
    assert digest((ROOT / row['path']).read_bytes()) == row['sha256'], row['path']
for row in m['baseline_rust']:
    assert digest(subprocess.check_output(['git', 'show', f'{m["base"]}:{row["path"]}'], cwd=ROOT)) == row['sha256']
packet = (ROOT / 'docs/design/greenfield/slices/h2-8a-jsdoc-parentheses-guard.md').read_text()
assert 'A6-16-1' in packet
lines = (ROOT / 'vendor/typescript-6.0.3/lib/_tsc.js').read_bytes().splitlines(keepends=True)
assert len(m['owners']) == 7
for row in m['owners']:
    assert digest(b''.join(lines[row['start'] - 1:row['end']])) == row['sha256']
    assert row['owner'] in packet
architecture = (ROOT / 'docs/design/greenfield/emitter-architecture.md').read_text()
assert len(m['architecture']) == 5
for row in m['architecture']:
    assert f'| `{row["id"]}` |' in architecture and row['id'] in packet
    assert row['disposition'] in packet
before = read('ratchets/h2-8a-jsdoc-parentheses-guard-before.v1.json')
assert before['base'] == m['base'] and before['native_jobs'] == 2
assert before['status'] == 'complete-before-with-two-independent-native-jobs'
assert before['positive_executions_per_case'] == 4
assert before['failed_executions_per_case'] == 2
assert len(before['exact_twice']) == 36 and len(before['failed_twice']) == 4
assert len(before['first_failure_comparisons']) == 4
assert all(r['native_executions'] == 2 for r in before['first_failure_comparisons'])
assert sorted(r['boundary'] for r in before['first_failure_comparisons']) == ['callback bytes for /project/out/main.d.ts']*2 + ['exact ordered reported diagnostics']*2
assert before['failed_twice'] == ['es5/js/declaration-literal', 'es2015/js/declaration-one', 'es2015/js/declaration-three', 'es2015/js/declaration-literal']
fixture = read('crates/compiler/tests/fixtures/jsdoc-parentheses-guard.json')
assert fixture['typescript'] == '6.0.3' and fixture['repetitions'] == 2
assert fixture['observer_sha256'] == digest((ROOT / 'scripts/observe-jsdoc-parentheses-guard.mjs').read_bytes())
assert fixture['compiler_sha256'] == digest((ROOT / 'vendor/typescript-6.0.3/lib/typescript.js').read_bytes())
cases = {r['case_id']: r for r in fixture['cases']}
assert len(cases) == len(m['witnesses']) == 40
assert set(before['exact_twice']).isdisjoint(before['failed_twice'])
assert set(before['exact_twice'] + before['failed_twice']) == set(cases)
assert sum(any(d['code'] == 5107 for d in r['typescript_observation']['reported_diagnostics']) for r in cases.values()) == 20
for row in m['witnesses']:
    name = row['case_id']
    assert digest(json.dumps(cases[name], ensure_ascii=False, separators=(',', ':')).encode()) == row['row_sha256']
    passed = name in before['exact_twice']
    assert (row['before'] == 'exact-twice') == passed
    assert row['disposition'] == ('adjacent-exact' if passed else 'owned-direct-jsdoc-guard')
    assert row['diagnostic_mode'] == ('deprecation-output-control' if any(d['code'] == 5107 for d in cases[name]['typescript_observation']['reported_diagnostics']) else 'active-semantic-control')
test = (ROOT / 'crates/compiler/tests/integration/h2_8a_jsdoc_parentheses_guard.rs').read_text()
assert 'assert_eq!(cases.len(), 40)' in test and 'failures.is_empty()' in test
assert 'mod h2_8a_jsdoc_parentheses_guard;' in (ROOT / 'crates/compiler/tests/contracts.rs').read_text()
original = read('ratchets/h2-8a-global-after-a6-14.v1.json')
assert original['exact'] == 706 and original['failed'] == 63
assert any(r['case_id'] == 'typescript-6.0.3/compiler/jsdocTypeCast.ts#default' for r in original['failures'])
for name in ['typescript-6.0.3/compiler/jsdocTypecastNoTypeNoCrash.ts#default', 'typescript-6.0.3/conformance/jsdoc/checkJsdocSatisfiesTag15.ts#default']:
    assert name in original['exact_twice']
print('H2.8a-A6-16 ready:7 owners,1 step,5 architecture rows,40 witnesses;36 exact/4 owned before;two independent native jobs;unresolved=0,undispositioned=0')
