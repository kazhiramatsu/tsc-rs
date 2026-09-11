#!/usr/bin/env python3
"""Validate A24's module factory, host boundaries and full-command witnesses."""
from pathlib import Path
import collections,hashlib,json,subprocess
ROOT=Path(__file__).resolve().parent.parent
stem='module-transformer-selection';read=lambda p:json.loads((ROOT/p).read_text());h=lambda b:hashlib.sha256(b).hexdigest()
m=read(f'ratchets/h2-8a-{stem}-readiness.v1.json')
assert m['version']==1 and m['slice']=='H2.8a-A6-24' and m['unresolved']==m['undispositioned']==0
assert m['runtime_paths']==['crates/emitter/src/builtins.rs']
assert m['steps']==['A6-24-1','A6-24-2','A6-24-3']
for r in m['authorities']:assert h((ROOT/r['path']).read_bytes())==r['sha256'],r['path']
for r in m['baseline_rust']:assert h(subprocess.check_output(['git','show',f'{m["base"]}:{r["path"]}'],cwd=ROOT))==r['sha256'],r['path']
packet=(ROOT/f'docs/design/greenfield/slices/h2-8a-{stem}.md').read_text();assert all(s in packet for s in m['steps'])
lines=(ROOT/'vendor/typescript-6.0.3/lib/_tsc.js').read_bytes().splitlines(keepends=True);assert len(m['owners'])==10
for r in m['owners']:
 assert h(b''.join(lines[r['start']-1:r['end']]))==r['sha256'],r['owner']
 assert lines[r['start']-1].decode().strip().startswith('function '+r['declaration_name']+'(') and r['owner'] in packet
architecture=(ROOT/'docs/design/greenfield/emitter-architecture.md').read_text();assert len(m['architecture'])==6
for r in m['architecture']:assert f'| `{r["id"]}` |' in architecture and r['id'] in packet and r['disposition'] in packet
before=read(f'ratchets/h2-8a-{stem}-before.v1.json');assert before['base']==m['base'] and before['eligible']==184
assert before['primary_comparison_jobs']==2 and before['positive_primary_executions_per_case']==4 and before['failed_primary_executions_per_case']==2
assert before['capture_is_full_actual_tuple'] is False
assert set(before['exact_twice']).isdisjoint(before['failed_twice'])
assert before['supplemental_capture_executions']==2*len(before['exact_twice'])+len(before['failed_twice'])
assert before['fresh_total_native_command_executions']==before['supplemental_capture_executions']*3
assert before['complete_comparison_and_supplemental_native_commands']==before['fresh_total_native_command_executions']+12
assert collections.Counter(r['boundary'] for r in before['first_failure_comparisons'])=={'exact source-map result':len(before['failed_twice'])}
assert before['original']['eligible']==6 and before['original']['exact']==4 and before['original']['failed']==2 and before['original']['full_failure_tuples']==4
assert before['original']['native_executions_per_case']==2
assert len(before['existing_activity_controls_passed'])==8 and len(before['new_native_controls_failed'])==3
assert before['factory_list_comparisons']==368 and before['factory_list_exact']==280 and len(before['factory_list_differences'])==88
f=read(f'crates/compiler/tests/fixtures/{stem}.json');assert f['typescript']=='6.0.3' and f['repetitions']==2
assert f['observer_sha256']==h((ROOT/f'scripts/observe-{stem}.mjs').read_bytes()) and f['compiler_sha256']==h((ROOT/'vendor/typescript-6.0.3/lib/typescript.js').read_bytes())
cs={r['case_id']:r for r in f['cases']};assert len(cs)==len(m['witnesses'])==184
assert set(before['exact_twice']+before['failed_twice'])==set(cs)
components={r['case_id']:r for r in before['components']};assert set(components)==set(cs)
for r in m['witnesses']:
 assert r['row_sha256']==h(json.dumps(cs[r['case_id']],ensure_ascii=False,separators=(',',':')).encode())
 assert (r['status']=='adjacent-exact')==(r['case_id'] in before['exact_twice'])
 assert r['status'] in ['adjacent-exact','owned-module-factory']
 c=components[r['case_id']];assert c['status']==r['status']
 assert c['supplemental_error'] is None and c['supplemental_exit_code']==c['expected_exit_code']
 if r['status']=='owned-module-factory':
  assert r['case_id'].split('/')[1] in ['none','amd','umd']
  assert any(d['kind']=='JavaScript' for d in c['differences'])
  assert all(d['kind']=='JavaScript' or d['kind']=='JavaScriptMap' and d['differing_map_keys']==['mappings'] for d in c['differences'])
assert collections.Counter(d['code'] for c in cs.values() for d in c['typescript_observation']['reported_diagnostics'])=={5107:140,5095:8,5071:4}
test=(ROOT/'crates/compiler/tests/integration/h2_8a_module_transformer_selection.rs').read_text()
assert 'assert_eq!(cases.len(), 184)' in test and 'failures.is_empty()' in test and 'assert_cases_with_inspection' in test
assert 'mod h2_8a_module_transformer_selection;' in (ROOT/'crates/compiler/tests/contracts.rs').read_text()
assert 'fn original_module_transformer_selection_matches_complete_commands()' in (ROOT/'crates/compiler/tests/h2_8a_original_corpus.rs').read_text()
assert m['after_required_complete_commands']==m['after_target_exact_twice']==368 and m['after_outside_failure_case_ids']==[]
assert m['emitter_unit_tests_required']=={'lib':488,'contracts':451}
assert m['compiler_activity_controls_required']==before['existing_activity_controls_passed'] and m['new_native_controls_required']==before['new_native_controls_failed']
print(f"H2.8a-A6-24 ready:10 whole TS functions,3 steps,6 architecture rows;184 fresh {len(before['exact_twice'])} exact/{len(before['failed_twice'])} owned;6 originals 4 exact/2 failed twice;8 current controls passed;unresolved=0,undispositioned=0")
