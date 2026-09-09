#!/usr/bin/env python3
"""Validate A28 source closure, scope correction and immutable before evidence."""
from pathlib import Path
import collections,hashlib,json,subprocess
ROOT=Path(__file__).resolve().parent.parent
read=lambda p:json.loads((ROOT/p).read_text());h=lambda b:hashlib.sha256(b).hexdigest()
stem='class-field-alias-map-positions';m=read(f'ratchets/h2-8a-{stem}-readiness.v1.json')
assert m['version']==1 and m['slice']=='H2.8a-A6-28' and m['unresolved']==m['undispositioned']==0
assert m['steps']==['A6-28-1','A6-28-2','A6-28-3','A6-28-4']
assert m['runtime_paths']==['crates/emitter/src/builtins/class_fields/downlevel.rs']
for r in m['authorities']:assert h((ROOT/r['path']).read_bytes())==r['sha256'],r['path']
for r in m['baseline_rust']:assert h(subprocess.check_output(['git','show',m['base']+':'+r['path']],cwd=ROOT))==r['sha256'],r['path']
packet=(ROOT/f'docs/design/greenfield/slices/h2-8a-{stem}.md').read_text();assert all(s in packet for s in m['steps'])
lines=(ROOT/'vendor/typescript-6.0.3/lib/_tsc.js').read_bytes().splitlines(keepends=True)
assert len(m['owners'])==len({r['owner'] for r in m['owners']})==70
for r in m['owners']:
 assert h(b''.join(lines[r['start']-1:r['end']]))==r['sha256'],r['owner']
 assert lines[r['start']-1].decode().strip().startswith('function '+r['declaration_name']+'(')
 assert r['step'] in m['steps'] and r['test']
arch=(ROOT/'docs/design/greenfield/emitter-architecture.md').read_text();assert len(m['architecture'])==18
for r in m['architecture']:
 row=next(line for line in arch.splitlines() if line.startswith('| `'+r['id']+'` |'))
 assert h(row.encode())==r['row_sha256'] and r['id'] in packet
 assert r['disposition'] in ['premise-unchanged','modified-requalify']
 if r['inherited_qualified_premise']:assert 'active-qualified' in row and r['disposition']=='premise-unchanged'
 assert r['rust_symbol'] and r['visibility'] and r['validation_ref'] and r['evidence'] and r['step'] in m['steps']
b=read(f'ratchets/h2-8a-{stem}-before.v1.json')
assert b['base']==m['base'] and b['eligible']==492 and b['primary_comparison_jobs']==2
assert b['exact_twice_per_job']==198 and b['failed_twice']==294 and b['first_failure_vectors_identical']
assert len(b['comparisons'])==286 and len(b['typed_failures'])==8 and not b['unrepresented_failed_case_ids']
assert b['primary_complete_command_executions']==1380 and b['supplemental_capture_executions']==690 and b['total_native_command_executions']==2070 and b['all_attempts_including_initial_partial']==3374
assert all(r['exit_code']==101 for r in b['exits'])
for r in b['inputs']:assert h((Path(b['archive'])/r['path']).read_bytes())==r['sha256'],r['path']
cases={}
for band in b['bands']:
 f=read('crates/compiler/tests/fixtures/'+band['fixture']+'.json');assert f['typescript']=='6.0.3' and f['repetitions']==2
 cs={r['case_id']:r for r in f['cases']};assert len(cs)==band['eligible'] and set(band['exact_twice']+band['failed_once'])==set(cs);cases.update(cs)
 if band['fixture']==stem:
  assert len(cs)==384 and f['observer_sha256']==h((ROOT/f'scripts/observe-{stem}.mjs').read_bytes())
  assert f['compiler_sha256']==h((ROOT/'vendor/typescript-6.0.3/lib/typescript.js').read_bytes())
  assert collections.Counter(d['code'] for c in cs.values() for d in c['typescript_observation']['reported_diagnostics'])=={5107:128,4094:12,2695:8,2465:8,2339:8,2816:16}
components={c['case_id']:c for c in b['components']};assert len(cases)==len(components)==len(m['witnesses'])==492
corrected=[]
for w in m['witnesses']:
 c=cases[w['case_id']];assert w['row_sha256']==h(json.dumps(c,ensure_ascii=False,separators=(',',':')).encode())
 assert w['initial_before_status']==components[w['case_id']]['status'] and w['step'] in m['steps']
 if w['status']!=w['initial_before_status']:
  parts=w['case_id'].split('/');assert parts[1]=='es5' and parts[-1] in ['field-arrow','static-block-arrow','concise-arrow','legacy-static-block'];assert w['status']=='outside-es5-static-this-phase';corrected.append(w)
for c in components.values():
 for d in c['differences']:
  if d['kind']=='JavaScriptMap':assert d['differing_map_keys']==['mappings']
assert len(corrected)==16
assert dict(collections.Counter(w['status'] for w in m['witnesses']))==m['source_dispositions']=={'adjacent-exact':198,'owned-static-this-and-expression-positions':192,'outside-retained-es2022':58,'outside-helper-request-order':16,'outside-computed-name-environment':8,'outside-legacy-default-name':4,'outside-es5-static-this-phase':16}
assert m['after_required_complete_commands']==492 and m['after_target_exact_twice']==390
assert m['after_outside_case_ids']==[w['case_id'] for w in m['witnesses'] if w['status'].startswith('outside-')] and len(m['after_outside_case_ids'])==102
assert m['emitter_tests_required']=={'lib':490,'contracts':451}
assert len(m['rust_map'])==8 and all(r['step'] in m['steps'] and r['producer'] and r['consumer'] and r['lifetime'] and r['invalidation'] for r in m['rust_map'])
assert 'mod h2_8a_class_field_alias_map_positions;' in (ROOT/'crates/compiler/tests/contracts.rs').read_text()
print('H2.8a-A6-28 ready:70 whole TS owners,1 production file,4 steps,18 architecture rows;492 commands198 exact/192 owned/102 outside;16-case pre-implementation phase correction;unresolved=0,undispositioned=0')
