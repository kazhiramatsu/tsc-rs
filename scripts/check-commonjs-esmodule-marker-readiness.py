#!/usr/bin/env python3
"""Validate the A23 binder-fact seam and complete marker witnesses."""
from pathlib import Path
import collections,hashlib,json,subprocess
ROOT=Path(__file__).resolve().parent.parent
stem='commonjs-esmodule-marker';read=lambda p:json.loads((ROOT/p).read_text());h=lambda b:hashlib.sha256(b).hexdigest()
m=read(f'ratchets/h2-8a-{stem}-readiness.v1.json')
assert m['version']==1 and m['slice']=='H2.8a-A6-23' and m['unresolved']==m['undispositioned']==0
assert m['runtime_paths']==['crates/emitter/src/builtins.rs','crates/emitter/src/resolver.rs','crates/checker/src/program.rs','crates/checker/src/emit.rs']
assert m['steps']==['A6-23-1','A6-23-2','A6-23-3']
for r in m['authorities']:assert h((ROOT/r['path']).read_bytes())==r['sha256'],r['path']
for r in m['baseline_rust']:assert h(subprocess.check_output(['git','show',f'{m["base"]}:{r["path"]}'],cwd=ROOT))==r['sha256'],r['path']
packet=(ROOT/f'docs/design/greenfield/slices/h2-8a-{stem}.md').read_text();assert all(s in packet for s in m['steps'])
lines=(ROOT/'vendor/typescript-6.0.3/lib/_tsc.js').read_bytes().splitlines(keepends=True);assert len(m['owners'])==15
for r in m['owners']:
 assert h(b''.join(lines[r['start']-1:r['end']]))==r['sha256'],r['owner']
 assert lines[r['start']-1].decode().strip().startswith('function '+r['declaration_name']+'(') and r['owner'] in packet
architecture=(ROOT/'docs/design/greenfield/emitter-architecture.md').read_text();assert len(m['architecture'])==6
for r in m['architecture']:assert f'| `{r["id"]}` |' in architecture and r['id'] in packet and r['disposition'] in packet
before=read(f'ratchets/h2-8a-{stem}-before.v1.json');assert before['base']==m['base']
assert before['primary_comparison_jobs']==2 and before['positive_primary_executions_per_case']==4 and before['failed_primary_executions_per_case']==2
assert before['supplemental_capture_executions']==152 and before['fresh_total_native_command_executions']==456 and before['total_native_command_executions']==470
assert before['capture_is_full_actual_tuple'] is False
assert len(before['exact_twice'])==61 and len(before['failed_twice'])==30
assert collections.Counter(r['boundary'] for r in before['first_failure_comparisons'])=={'exact source-map result':30}
assert before['original']['eligible']==before['original']['failed']==7 and before['original']['full_tuples']==14
assert before['original']['native_executions_per_case']==2
f=read(f'crates/compiler/tests/fixtures/{stem}.json');assert f['typescript']=='6.0.3' and f['repetitions']==2
assert f['observer_sha256']==h((ROOT/f'scripts/observe-{stem}.mjs').read_bytes()) and f['compiler_sha256']==h((ROOT/'vendor/typescript-6.0.3/lib/typescript.js').read_bytes())
cs={r['case_id']:r for r in f['cases']};assert len(cs)==len(m['witnesses'])==91
assert set(before['exact_twice']).isdisjoint(before['failed_twice']) and set(before['exact_twice']+before['failed_twice'])==set(cs)
assert collections.Counter(r['status'] for r in m['witnesses'])=={'adjacent-exact':61,'owned-extra-esmodule-marker':28,'outside-other-output':2}
components={r['case_id']:r for r in before['components']};assert set(components)==set(cs)
for r in m['witnesses']:
 assert r['row_sha256']==h(json.dumps(cs[r['case_id']],ensure_ascii=False,separators=(',',':')).encode())
 assert (r['status']=='adjacent-exact')==(r['case_id'] in before['exact_twice'])
 c=components[r['case_id']];assert c['status']==r['status']
 assert c['supplemental_error'] is None and c['supplemental_exit_code']==c['expected_exit_code']
 if r['status']=='owned-extra-esmodule-marker':
  assert any(d['kind']=='JavaScript' for d in c['differences'])
  assert all(d['extra_marker_only'] if d['kind']=='JavaScript' else d['differing_map_keys']==['mappings'] for d in c['differences'])
assert collections.Counter(d['code'] for c in cs.values() for d in c['typescript_observation']['reported_diagnostics'])=={5107:26,2304:5}
test=(ROOT/'crates/compiler/tests/integration/h2_8a_commonjs_esmodule_marker.rs').read_text()
assert 'assert_eq!(cases.len(), 91)' in test and 'failures.is_empty()' in test and 'assert_cases_with_inspection' in test
assert 'mod h2_8a_commonjs_esmodule_marker;' in (ROOT/'crates/compiler/tests/contracts.rs').read_text()
assert 'fn original_commonjs_esmodule_markers_match_complete_commands()' in (ROOT/'crates/compiler/tests/h2_8a_original_corpus.rs').read_text()
assert m['after_required_complete_commands']==397 and m['after_target_exact_twice']==395
assert m['after_outside_failure_case_ids']==['commonjs-marker/amd/mjs-exports-auto','commonjs-marker/umd/mjs-exports-auto']
assert m['emitter_unit_tests_required']=={'lib':485,'contracts':451} and m['checker_emit_unit_tests_required']==17
print('H2.8a-A6-23 ready:15 whole TS functions,3 steps,6 architecture rows;91 fresh61 exact/28 owned/2 outside;7 original marker failures twice;unresolved=0,undispositioned=0')
