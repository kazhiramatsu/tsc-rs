#!/usr/bin/env python3
"""Validate the class-instance clone owner and frozen complete-command before."""
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent
read = lambda p: json.loads((ROOT / p).read_text())
digest = lambda b: hashlib.sha256(b).hexdigest()
m = read('ratchets/h2-8a-commonjs-class-instance-readiness.v1.json')
assert m['version'] == 1 and m['slice'] == 'H2.8a-A6-17'
assert m['unresolved'] == m['undispositioned'] == 0
assert m['runtime_paths'] == ['crates/checker/src/annotate.rs', 'crates/checker/src/node_builder/statements.rs', 'crates/checker/src/node_builder/type_nodes.rs']
assert m['steps'] == ['A6-17-1', 'A6-17-2', 'A6-17-3']
for row in m['authorities']:
    assert digest((ROOT / row['path']).read_bytes()) == row['sha256'], row['path']
for row in m['baseline_rust']:
    assert digest(subprocess.check_output(['git', 'show', f'{m["base"]}:{row["path"]}'], cwd=ROOT)) == row['sha256']
packet = (ROOT / 'docs/design/greenfield/slices/h2-8a-commonjs-class-instance.md').read_text()
assert all(step in packet for step in m['steps'])
lines = (ROOT / 'vendor/typescript-6.0.3/lib/_tsc.js').read_bytes().splitlines(keepends=True)
assert len(m['owners']) == 5
for row in m['owners']:
    assert digest(b''.join(lines[row['start'] - 1:row['end']])) == row['sha256']
    assert row['owner'] in packet
architecture = (ROOT / 'docs/design/greenfield/emitter-architecture.md').read_text()
assert len(m['architecture']) == 5
for row in m['architecture']:
    assert f'| `{row["id"]}` |' in architecture and row['id'] in packet
    assert row['disposition'] in packet
before = read('ratchets/h2-8a-commonjs-class-instance-before.v1.json')
assert before['base'] == m['base'] and before['native_jobs'] == 2
assert before['status'] == 'complete-before-with-two-independent-native-jobs'
assert before['positive_executions_per_case'] == 4 and before['failed_executions_per_case'] == 2
assert len(before['exact_twice']) == 12 and len(before['failed_twice']) == 20
comparisons = {r['case_id']: r for r in before['first_failure_comparisons']}
assert len(comparisons) == 20 and all(r['native_executions'] == 2 for r in comparisons.values())
assert sorted(r['boundary'] for r in comparisons.values()) == ['callback bytes for /project/out/main.d.ts']*10 + ['exact source-map result']*10
fixture = read('crates/compiler/tests/fixtures/commonjs-class-instance.json')
assert fixture['typescript'] == '6.0.3' and fixture['repetitions'] == 2
assert fixture['observer_sha256'] == digest((ROOT / 'scripts/observe-commonjs-class-instance.mjs').read_bytes())
assert fixture['compiler_sha256'] == digest((ROOT / 'vendor/typescript-6.0.3/lib/typescript.js').read_bytes())
cases = {r['case_id']: r for r in fixture['cases']}
assert len(cases) == len(m['witnesses']) == 32
assert set(before['exact_twice']).isdisjoint(before['failed_twice'])
assert set(before['exact_twice'] + before['failed_twice']) == set(cases)
assert sum(any(d['code'] == 5107 for d in r['typescript_observation']['reported_diagnostics']) for r in cases.values()) == 16
for row in m['witnesses']:
    name = row['case_id']
    assert digest(json.dumps(cases[name], ensure_ascii=False, separators=(',', ':')).encode()) == row['row_sha256']
    passed = name in before['exact_twice']
    assert (row['before'] == 'exact-twice') == passed
    disposition = 'adjacent-exact' if passed else 'owned-class-instance-declaration' if comparisons[name]['boundary'] == 'callback bytes for /project/out/main.d.ts' else 'outside-static-initializer-mapping'
    assert row['disposition'] == disposition
    assert row['diagnostic_mode'] == ('deprecation-output-control' if any(d['code'] == 5107 for d in cases[name]['typescript_observation']['reported_diagnostics']) else 'active-semantic-control')
query = read('ratchets/h2-8a-commonjs-class-instance-native-query.v1.json')
assert query['base'] == m['base'] and query['exit_code'] == 0
assert query['status'] == 'completed-native-query-research-not-complete-command-qualification'
assert query['preparation'] == 'check_source_file_then_raw_export_symbol_without_declaration_entry_preparation'
assert query['extra_member_rows'] == 'early-query-only-not-final-production-type'
assert len(query['rows']) == 4
ordinary = [r for r in query['rows'] if not r['extra']]
assert [r['repetition'] for r in ordinary] == [0, 1]
assert all(r['object_flags'] == 16 and r['symbol_is_class'] and not r['same_as_declared_type'] for r in ordinary)
test = (ROOT / 'crates/compiler/tests/integration/h2_8a_commonjs_class_instance.rs').read_text()
assert 'assert_eq!(cases.len(), 32)' in test and 'failures.is_empty()' in test
assert 'mod h2_8a_commonjs_class_instance;' in (ROOT / 'crates/compiler/tests/contracts.rs').read_text()
print('H2.8a-A6-17 ready:5 owners,3 steps,5 architecture rows,32 witnesses;12 exact/10 owned/10 outside before;two independent jobs;unresolved=0,undispositioned=0')
