#!/usr/bin/env python3
"""Validate A20 owner pins, complete before witnesses and typed context scope."""
from pathlib import Path
import json
import hashlib
import subprocess
import collections

ROOT = Path(__file__).resolve().parent.parent
stem = 'synthetic-namespace-export-modifiers'
read = lambda p: json.loads((ROOT / p).read_text())
h = lambda b: hashlib.sha256(b).hexdigest()
m = read(f'ratchets/h2-8a-{stem}-readiness.v1.json')
assert m['version'] == 1 and m['slice'] == 'H2.8a-A6-20'
assert m['unresolved'] == m['undispositioned'] == 0
assert m['runtime_paths'] == ['crates/checker/src/node_builder/statements.rs']
assert m['steps'] == ['A6-20-1']
for row in m['authorities']:
    assert h((ROOT / row['path']).read_bytes()) == row['sha256'], row['path']
for row in m['baseline_rust']:
    assert h(subprocess.check_output(['git', 'show', f'{m["base"]}:{row["path"]}'], cwd=ROOT)) == row['sha256'], row['path']
packet = (ROOT / f'docs/design/greenfield/slices/h2-8a-{stem}.md').read_text()
assert all(step in packet for step in m['steps'])
lines = (ROOT / 'vendor/typescript-6.0.3/lib/_tsc.js').read_bytes().splitlines(keepends=True)
assert len(m['owners']) == 21
for row in m['owners']:
    assert h(b''.join(lines[row['start'] - 1:row['end']])) == row['sha256'], row['owner']
    assert lines[row['start'] - 1].decode().strip().startswith('function ' + row['owner'] + '(')
    assert row['owner'] in packet
architecture = (ROOT / 'docs/design/greenfield/emitter-architecture.md').read_text()
assert len(m['architecture']) == 6
for row in m['architecture']:
    assert f'| `{row["id"]}` |' in architecture and row['id'] in packet
    assert row['disposition'] in packet
before = read(f'ratchets/h2-8a-{stem}-before.v1.json')
assert before['base'] == m['base'] and before['native_jobs'] == 2
assert before['positive_executions_per_case'] == 4 and before['failed_executions_per_case'] == 2
assert len(before['exact_twice']) == 72 and len(before['failed_twice']) == 12
assert len(before['first_failure_comparisons']) == 12
assert all(row['native_executions'] == 2 and row['boundary'] == 'callback bytes for /project/out/main.d.ts' for row in before['first_failure_comparisons'])
fixture = read(f'crates/compiler/tests/fixtures/{stem}.json')
assert fixture['typescript'] == '6.0.3' and fixture['repetitions'] == 2
assert fixture['observer_sha256'] == h((ROOT / f'scripts/observe-{stem}.mjs').read_bytes())
assert fixture['compiler_sha256'] == h((ROOT / 'vendor/typescript-6.0.3/lib/typescript.js').read_bytes())
cases = {r['case_id']: r for r in fixture['cases']}
assert len(cases) == len(m['witnesses']) == 84
assert set(before['exact_twice']).isdisjoint(before['failed_twice'])
assert set(before['exact_twice'] + before['failed_twice']) == set(cases)
assert collections.Counter(row['status'] for row in m['witnesses']) == {'adjacent-exact': 72, 'owned-export-modifier': 12}
for row in m['witnesses']:
    name = row['case_id']
    assert row['row_sha256'] == h(json.dumps(cases[name], ensure_ascii=False, separators=(',', ':')).encode())
    passed = name in before['exact_twice']
    assert (row['before'] == 'exact-twice') == passed
    assert (row['status'] == 'adjacent-exact') == passed
    assert bool(row['component_differences']) != passed
    if not passed:
        assert '/script/' in name and row['later_complete_command_fields'] == 'not-qualified'
        assert all(part['only_adds_export_modifier'] for part in row['component_differences'])
diagnostics = [d for case in cases.values() for d in case['typescript_observation']['reported_diagnostics']]
assert collections.Counter(d['code'] for d in diagnostics) == {5107: 42, 7012: 3, 7005: 3, 7006: 6}
assert sum(any(d['code'] == 5107 for d in case['typescript_observation']['reported_diagnostics']) for case in cases.values()) == 42
original = read(f'ratchets/h2-8a-{stem}-original-before.v1.json')
assert original['base'] == m['base'] and original['eligible'] == 4 and original['exact'] == 0
assert original['native_jobs'] == 2 and original['native_executions_per_case'] == 4
assert original['full_tuple_capture_job'] == 2 and original['full_tuples'] == 8
assert original['same_first_boundaries_across_jobs'] and not original['first_job_full_tuple_capture']
test = (ROOT / 'crates/compiler/tests/integration/h2_8a_synthetic_namespace_export_modifiers.rs').read_text()
assert 'assert_eq!(cases.len(), 84)' in test and 'failures.is_empty()' in test
assert 'mod h2_8a_synthetic_namespace_export_modifiers;' in (ROOT / 'crates/compiler/tests/contracts.rs').read_text()
assert 'fn original_synthetic_namespace_exports_match_complete_commands()' in (ROOT / 'crates/compiler/tests/h2_8a_original_corpus.rs').read_text()
assert m['after_required_complete_commands'] == 244 and m['after_target_exact_twice'] == 243
assert m['checker_unit_tests_required'] == 30
print('H2.8a-A6-20 ready:21 whole TS functions,1 step,6 architecture rows,84 witnesses;72 exact/12 owned;four original controls frozen;unresolved=0,undispositioned=0')
