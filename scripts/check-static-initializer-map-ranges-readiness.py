#!/usr/bin/env python3
"""Check the frozen static-property range owner and all complete witnesses."""
import collections
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent
read = lambda p: json.loads((ROOT / p).read_text())
digest = lambda b: hashlib.sha256(b).hexdigest()
stem = 'static-initializer-map-ranges'
m = read(f'ratchets/h2-8a-{stem}-readiness.v1.json')
assert m['version'] == 1 and m['slice'] == 'H2.8a-A6-18'
assert m['unresolved'] == m['undispositioned'] == 0
assert m['runtime_paths'] == ['crates/emitter/src/builtins/class_fields/downlevel.rs']
assert m['steps'] == ['A6-18-1', 'A6-18-2', 'A6-18-3', 'A6-18-4', 'A6-18-5', 'A6-18-6']
for row in m['authorities']:
    assert digest((ROOT / row['path']).read_bytes()) == row['sha256'], row['path']
for row in m['baseline_rust']:
    assert digest(subprocess.check_output(['git', 'show', f'{m["base"]}:{row["path"]}'], cwd=ROOT)) == row['sha256'], row['path']
packet = (ROOT / f'docs/design/greenfield/slices/h2-8a-{stem}.md').read_text()
assert all(step in packet for step in m['steps'])
lines = (ROOT / 'vendor/typescript-6.0.3/lib/_tsc.js').read_bytes().splitlines(keepends=True)
assert len(m['owners']) == 14
for row in m['owners']:
    assert digest(b''.join(lines[row['start'] - 1:row['end']])) == row['sha256'], row['owner']
    assert row['owner'] in packet
assert [r['owner'] for r in m['owners'] if r.get('kind') == 'binding-fragment'] == ['transformClassFields option bindings']
architecture = (ROOT / 'docs/design/greenfield/emitter-architecture.md').read_text()
assert len(m['architecture']) == 8
for row in m['architecture']:
    assert f'| `{row["id"]}` |' in architecture and row['id'] in packet
    assert row['disposition'] in packet
before = read(f'ratchets/h2-8a-{stem}-before.v1.json')
assert before['base'] == m['base'] and before['native_jobs'] == 2
assert before['status'] == 'complete-before-with-two-independent-native-jobs'
assert before['positive_executions_per_case'] == 4 and before['failed_executions_per_case'] == 2
assert len(before['exact_twice']) == 9 and len(before['failed_twice']) == 95
comparisons = {r['case_id']: r for r in before['first_failure_comparisons']}
assert len(comparisons) == 95 and all(r['native_executions'] == 2 and r['boundary'] == 'exact source-map result' for r in comparisons.values())
fixture = read(f'crates/compiler/tests/fixtures/{stem}.json')
assert fixture['typescript'] == '6.0.3' and fixture['repetitions'] == 2
assert fixture['observer_sha256'] == digest((ROOT / f'scripts/observe-{stem}.mjs').read_bytes())
assert fixture['compiler_sha256'] == digest((ROOT / 'vendor/typescript-6.0.3/lib/typescript.js').read_bytes())
cases = {r['case_id']: r for r in fixture['cases']}
assert len(cases) == len(m['witnesses']) == 104
assert set(before['exact_twice']).isdisjoint(before['failed_twice'])
assert set(before['exact_twice'] + before['failed_twice']) == set(cases)
assert sum(any(d['code'] == 5107 for d in r['typescript_observation']['reported_diagnostics']) for r in cases.values()) == 44
assert collections.Counter(r['status'] for r in m['witnesses']) == {'adjacent-exact': 9, 'owned-only': 21, 'owned-and-outside': 43, 'outside-only': 31}
for row in m['witnesses']:
    name = row['case_id']
    assert digest(json.dumps(cases[name], ensure_ascii=False, separators=(',', ':')).encode()) == row['row_sha256']
    passed = name in before['exact_twice']
    assert (row['before'] == 'exact-twice') == passed
    assert (row['status'] == 'adjacent-exact') == passed
    assert bool(row['before_map_diffs']) == (not passed)
    assert row['diagnostic_mode'] == ('deprecation-output-control' if any(d['code'] == 5107 for d in cases[name]['typescript_observation']['reported_diagnostics']) else 'active-semantic-control')
    if not passed:
        assert row['owned_components'] or row['outside_components']
        for component in row['outside_components']:
            assert component in packet
scout = m['scout_disposition']
assert scout['complete_native_commands_executed'] == 0 and scout['scout_exit_code'] == 101
assert scout['status'] == 'first-native-job-is-scout-only'
helper = 'crates/compiler/tests/integration/h2_7c_declaration_blocking.rs'
base_helper = subprocess.check_output(['git', 'show', f'{m["base"]}:{helper}'], cwd=ROOT, text=True)
needle = '                    "esModuleInterop" => options.es_module_interop = value.as_bool(),\n'
replacement = needle + '                    "useDefineForClassFields" => {\n                        options.use_define_for_class_fields = value.as_bool()\n                    }\n'
assert base_helper.count(needle) == 1
assert (ROOT / helper).read_text() == base_helper.replace(needle, replacement)
test = (ROOT / 'crates/compiler/tests/integration/h2_8a_static_initializer_map_ranges.rs').read_text()
assert 'assert_eq!(cases.len(), 104)' in test and 'failures.is_empty()' in test
assert 'mod h2_8a_static_initializer_map_ranges;' in (ROOT / 'crates/compiler/tests/contracts.rs').read_text()
amendment = m['literal_key_amendment']
extra_before = read(amendment['before'])
assert extra_before['status'] == 'complete-before-two-independent-jobs-on-first-a6-18-after'
assert extra_before['native_jobs'] == 2 and extra_before['failed_executions_per_case'] == 2
assert extra_before['positive_executions_per_case'] == 4
assert len(extra_before['exact_twice']) == 14 and len(extra_before['failed_twice']) == 6
assert len(extra_before['first_failure_comparisons']) == 6
assert all(r['boundary'] == 'exact source-map result' and r['native_executions'] == 2 for r in extra_before['first_failure_comparisons'])
first_after = read(amendment['runtime_reference'])
assert first_after['exact_twice'] == 272 and first_after['eligible'] == 347
assert first_after['exit_code'] == 101 and first_after['fresh_failed_once'] == 75
assert extra_before['runtime_reference_sha256'] == digest((ROOT / amendment['runtime_reference']).read_bytes())
assert extra_before['runtime_production_sha256'] == amendment['runtime_production_sha256']
assert amendment['runtime_production_sha256'] == next(r['sha256'] for r in first_after['working_files'] if r['path'] == m['runtime_paths'][0])
extra_fixture = read('crates/compiler/tests/fixtures/property-literal-key-ranges.json')
assert extra_fixture['typescript'] == '6.0.3' and extra_fixture['repetitions'] == 2
assert extra_fixture['observer_sha256'] == digest((ROOT / 'scripts/observe-property-literal-key-ranges.mjs').read_bytes())
assert extra_fixture['compiler_sha256'] == fixture['compiler_sha256']
extra_cases = {r['case_id']: r for r in extra_fixture['cases']}
assert len(extra_cases) == len(amendment['witnesses']) == 20
assert set(extra_before['exact_twice']).isdisjoint(extra_before['failed_twice'])
assert set(extra_before['exact_twice'] + extra_before['failed_twice']) == set(extra_cases)
assert sum(any(d['code'] == 5107 for d in r['typescript_observation']['reported_diagnostics']) for r in extra_cases.values()) == 10
for row in amendment['witnesses']:
    name = row['case_id']
    assert digest(json.dumps(extra_cases[name], ensure_ascii=False, separators=(',', ':')).encode()) == row['row_sha256']
    passed = name in extra_before['exact_twice']
    assert (row['before'] == 'exact-twice') == passed
    assert row['disposition'] == ('adjacent-exact' if passed else 'owned-literal-key-reuse')
assert 'assert_eq!(cases.len(), 20)' in test
assert m['after_required_complete_commands'] == 367 and m['after_target_exact_twice'] == 293
second = m['expression_range_amendment']
assert digest((ROOT / second['checkpoint']).read_bytes()) == second['checkpoint_sha256']
second_record = read(second['checkpoint'])
assert second_record['exact_twice'] == 293 and second_record['eligible'] == 367
assert len(second_record['new_map_components']) == 12
print('H2.8a-A6-18 amended ready:14 source spans,6 steps,8 architecture rows,104+20 witnesses;main9/21/43/31, literal14 exact/6 owned;two independent jobs at each distinct before;unresolved=0,undispositioned=0')
