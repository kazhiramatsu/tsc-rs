#!/usr/bin/env python3
"""Check the computed synthetic-default alias consumer and complete before."""
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent
read = lambda p: json.loads((ROOT / p).read_text())
digest = lambda b: hashlib.sha256(b).hexdigest()
m = read('ratchets/h2-8a-synthetic-default-alias-readiness.v1.json')
assert m['version'] == 1 and m['slice'] == 'H2.8a-A6-15'
assert m['unresolved'] == m['undispositioned'] == 0
assert m['runtime_paths'] == ['crates/checker/src/node_builder/statements.rs']
assert m['steps'] == ['A6-15-1']
for row in m['authorities']:
    assert digest((ROOT / row['path']).read_bytes()) == row['sha256'], row['path']
for row in m['baseline_rust']:
    assert digest(subprocess.check_output(['git', 'show', f'{m["base"]}:{row["path"]}'], cwd=ROOT)) == row['sha256']
packet = (ROOT / 'docs/design/greenfield/slices/h2-8a-synthetic-default-alias.md').read_text()
assert 'A6-15-1' in packet
lines = (ROOT / 'vendor/typescript-6.0.3/lib/_tsc.js').read_bytes().splitlines(keepends=True)
assert len(m['owners']) == 4
for row in m['owners']:
    assert digest(b''.join(lines[row['start'] - 1:row['end']])) == row['sha256']
    assert row['owner'] in packet
architecture = (ROOT / 'docs/design/greenfield/emitter-architecture.md').read_text()
assert len(m['architecture']) == 5
for row in m['architecture']:
    assert f'| `{row["id"]}` |' in architecture and row['id'] in packet
    assert row['disposition'] in packet
before = read('ratchets/h2-8a-synthetic-default-alias-before.v1.json')
assert before['base'] == m['base'] and before['native_jobs'] == 2
assert before['status'] == 'complete-before-with-two-independent-native-jobs'
assert before['positive_executions_per_case'] == 4
assert before['failed_executions_per_case'] == 2
assert len(before['exact_twice']) == 78 and len(before['failed_twice']) == 6
assert len(before['first_failure_comparisons']) == 6
assert all(r['native_executions'] == 2 and r['boundary'] == 'callback bytes for /project/out/main.d.ts' for r in before['first_failure_comparisons'])
assert all('/js/cjs-named-default/synthetic-unset/' in name for name in before['failed_twice'])
fixture = read('crates/compiler/tests/fixtures/synthetic-default-alias.json')
assert fixture['typescript'] == '6.0.3' and fixture['repetitions'] == 2
assert fixture['observer_sha256'] == digest((ROOT / 'scripts/observe-synthetic-default-alias.mjs').read_bytes())
assert fixture['compiler_sha256'] == digest((ROOT / 'vendor/typescript-6.0.3/lib/typescript.js').read_bytes())
cases = {r['case_id']: r for r in fixture['cases']}
assert len(cases) == len(m['witnesses']) == 84
assert set(before['exact_twice']).isdisjoint(before['failed_twice'])
assert set(before['exact_twice'] + before['failed_twice']) == set(cases)
assert sum(bool(r['typescript_observation']['reported_diagnostics']) for r in cases.values()) == 64
for row in m['witnesses']:
    name = row['case_id']
    assert digest(json.dumps(cases[name], ensure_ascii=False, separators=(',', ':')).encode()) == row['row_sha256']
    passed = name in before['exact_twice']
    assert (row['before'] == 'exact-twice') == passed
    assert row['disposition'] == ('adjacent-exact' if passed else 'owned-computed-option-name')
    assert row['diagnostic_mode'] == ('deprecation-output-control' if cases[name]['typescript_observation']['reported_diagnostics'] else 'active-semantic-control')
test = (ROOT / 'crates/compiler/tests/integration/h2_8a_synthetic_default_alias.rs').read_text()
assert 'assert_eq!(cases.len(), 84)' in test and 'failures.is_empty()' in test
assert 'mod h2_8a_synthetic_default_alias;' in (ROOT / 'crates/compiler/tests/contracts.rs').read_text()
original = read('ratchets/h2-8a-global-after-a6-14.v1.json')
assert original['revision'] == m['base']
assert original['exact'] == 706 and original['failed'] == 63
assert len(original['regressions']) == 2
assert all('jsDeclarationsReexportAliasesEsModuleInterop.ts#' in name for name in original['regressions'])
print('H2.8a-A6-15 ready:4 owners,1 step,5 architecture rows,84 witnesses;78 exact/6 owned before;two independent native jobs;unresolved=0,undispositioned=0')
