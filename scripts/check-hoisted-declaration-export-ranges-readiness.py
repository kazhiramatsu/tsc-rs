#!/usr/bin/env python3
"""Validate A6-19 source pins, immutable before and complete witness ownership."""
from pathlib import Path
import json
import hashlib
import subprocess
import collections

ROOT = Path(__file__).resolve().parent.parent
stem = 'hoisted-declaration-export-ranges'
read = lambda p: json.loads((ROOT / p).read_text())
hash_bytes = lambda b: hashlib.sha256(b).hexdigest()
m = read(f'ratchets/h2-8a-{stem}-readiness.v1.json')
assert m['version'] == 1 and m['slice'] == 'H2.8a-A6-19'
assert m['unresolved'] == m['undispositioned'] == 0
assert m['runtime_paths'] == ['crates/emitter/src/builtins.rs', 'crates/emitter/src/builtins/system.rs']
assert m['steps'] == ['A6-19-1', 'A6-19-2', 'A6-19-3']
for row in m['authorities']:
    assert hash_bytes((ROOT / row['path']).read_bytes()) == row['sha256'], row['path']
for row in m['baseline_rust']:
    assert hash_bytes(subprocess.check_output(['git', 'show', f'{m["base"]}:{row["path"]}'], cwd=ROOT)) == row['sha256'], row['path']
packet = (ROOT / f'docs/design/greenfield/slices/h2-8a-{stem}.md').read_text()
assert all(step in packet for step in m['steps'])
lines = (ROOT / 'vendor/typescript-6.0.3/lib/_tsc.js').read_bytes().splitlines(keepends=True)
assert len(m['owners']) == 25
for row in m['owners']:
    assert hash_bytes(b''.join(lines[row['start'] - 1:row['end']])) == row['sha256'], row['owner']
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
assert len(before['exact_twice']) == 64 and len(before['failed_twice']) == 104
assert len(before['first_failure_comparisons']) == 104
assert all(row['native_executions'] == 2 and row['boundary'] == 'exact source-map result' for row in before['first_failure_comparisons'])
fixture = read(f'crates/compiler/tests/fixtures/{stem}.json')
assert fixture['typescript'] == '6.0.3' and fixture['repetitions'] == 2
assert fixture['observer_sha256'] == hash_bytes((ROOT / f'scripts/observe-{stem}.mjs').read_bytes())
assert fixture['compiler_sha256'] == hash_bytes((ROOT / 'vendor/typescript-6.0.3/lib/typescript.js').read_bytes())
cases = {r['case_id']: r for r in fixture['cases']}
assert len(cases) == len(m['witnesses']) == 168
assert set(before['exact_twice']).isdisjoint(before['failed_twice'])
assert set(before['exact_twice'] + before['failed_twice']) == set(cases)
assert collections.Counter(row['status'] for row in m['witnesses']) == {'adjacent-exact': 64, 'owned': 100, 'outside-es2015-name': 4}
outside = {f'hoisted-export/es5/{module}/class/{shape}' for module in ['commonjs', 'amd'] for shape in ['escaped', 'direct-escaped']}
assert {row['case_id'] for row in m['witnesses'] if row['status'] == 'outside-es2015-name'} == outside
for row in m['witnesses']:
    name = row['case_id']
    assert row['row_sha256'] == hash_bytes(json.dumps(cases[name], ensure_ascii=False, separators=(',', ':')).encode())
    passed = name in before['exact_twice']
    assert (row['before'] == 'exact-twice') == passed
    assert (row['status'] == 'adjacent-exact') == passed
    if not passed:
        assert row['before_map_difference']['all_differences_on_publication_lines'] == (name not in outside)
        assert bool(row['owned_components']) == (name not in outside)
    else:
        assert row['before_map_difference'] is None
    assert all(component in packet for component in row['outside_components'])
diagnostics = [d for case in cases.values() for d in case['typescript_observation']['reported_diagnostics']]
assert collections.Counter(d['code'] for d in diagnostics) == {5107: 140, 2323: 8, 2484: 4, 2300: 8, 2309: 4}
assert sum(any(d['code'] == 5107 for d in case['typescript_observation']['reported_diagnostics']) for case in cases.values()) == 112
test = (ROOT / 'crates/compiler/tests/integration/h2_8a_hoisted_declaration_export_ranges.rs').read_text()
assert 'assert_eq!(cases.len(), 168)' in test and 'failures.is_empty()' in test
assert 'mod h2_8a_hoisted_declaration_export_ranges;' in (ROOT / 'crates/compiler/tests/contracts.rs').read_text()
assert m['after_required_complete_commands'] == 581 and m['after_target_exact_twice'] == 530
consumers = {str(path.relative_to(ROOT)) for path in (ROOT / 'crates/emitter/src').rglob('*.rs')
             if 'hoisted_declaration_exports' in path.read_text() or 'hoisted_function_exports' in path.read_text()}
assert consumers == set(m['runtime_paths']), consumers
print('H2.8a-A6-19 ready:25 whole TS functions,3 steps,6 architecture rows,168 witnesses;64 exact/100 owned/4 outside;two independent before jobs;unresolved=0,undispositioned=0')
