#!/usr/bin/env python3
"""Verify setter name/type ownership, invariant witnesses and frozen before input."""
from pathlib import Path
import collections,hashlib,json,subprocess
ROOT=Path(__file__).resolve().parent.parent
stem='defineproperty-setter-names'
read=lambda p:json.loads((ROOT/p).read_text())
h=lambda b:hashlib.sha256(b).hexdigest()
m=read(f'ratchets/h2-8a-{stem}-readiness.v1.json')
assert m['version']==1 and m['slice']=='H2.8a-A6-25'
assert m['unresolved']==m['undispositioned']==0
assert m['runtime_paths']==['crates/checker/src/node_builder/statements.rs']
assert m['steps']==['A6-25-1','A6-25-2','A6-25-3']
for r in m['authorities']:assert h((ROOT/r['path']).read_bytes())==r['sha256'],r['path']
for r in m['baseline_rust']:assert h(subprocess.check_output(['git','show',f'{m["base"]}:{r["path"]}'],cwd=ROOT))==r['sha256'],r['path']
packet=(ROOT/f'docs/design/greenfield/slices/h2-8a-{stem}.md').read_text()
assert all(s in packet for s in m['steps'])
lines=(ROOT/'vendor/typescript-6.0.3/lib/_tsc.js').read_bytes().splitlines(keepends=True)
assert len(m['owners'])==20
for r in m['owners']:
 assert h(b''.join(lines[r['start']-1:r['end']]))==r['sha256'],r['owner']
 assert lines[r['start']-1].decode().strip().startswith('function '+r['declaration_name']+'(')
 assert r['owner'] in packet
architecture=(ROOT/'docs/design/greenfield/emitter-architecture.md').read_text()
assert len(m['architecture'])==6
for r in m['architecture']:
 assert f'| `{r["id"]}` |' in architecture and r['id'] in packet and r['disposition'] in packet
all_cases={};components={};outside=[]
for family,eligible,exact,owned,remaining,supplement in [(stem,88,56,24,8,144),('defineproperty-setter-annotations',20,16,4,0,36)]:
 b=read(f'ratchets/h2-8a-{family}-before.v1.json')
 assert b['base']==m['base'] and b['eligible']==eligible
 assert b['primary_comparison_jobs']==2 and b['positive_primary_executions_per_case']==4 and b['failed_primary_executions_per_case']==2
 assert b['capture_is_full_actual_tuple'] is False and b['supplemental_executions']==supplement
 assert b['fresh_complete_comparison_and_supplemental_executions']==supplement*3
 assert len(b['exact_twice'])==exact and len(b['failed_twice'])==owned+remaining
 assert set(b['exact_twice']).isdisjoint(b['failed_twice'])
 assert collections.Counter(r['boundary'] for r in b['first_failure_comparisons'])=={'callback bytes for /project/out/main.d.ts':owned+remaining}
 f=read(f'crates/compiler/tests/fixtures/{family}.json')
 assert f['typescript']=='6.0.3' and f['repetitions']==2
 assert f['observer_sha256']==h((ROOT/f'scripts/observe-{family}.mjs').read_bytes())
 assert f['compiler_sha256']==h((ROOT/'vendor/typescript-6.0.3/lib/typescript.js').read_bytes())
 cases={r['case_id']:r for r in f['cases']};assert len(cases)==eligible
 assert set(b['exact_twice']+b['failed_twice'])==set(cases)
 current={r['case_id']:r for r in b['components']};assert set(current)==set(cases)
 assert sum(r['status']=='owned-setter-parameter-name' for r in current.values())==owned
 for r in current.values():
  assert r['supplemental_error'] is None
  assert (r['status']=='adjacent-exact')==(r['case_id'] in b['exact_twice'])
  if r['status']=='owned-setter-parameter-name':assert all(d['parameter_name_only'] and d['kind']=='Declaration' for d in r['differences'])
  elif r['status']=='outside-descriptor-readonly':
   assert all(d['readonly_only'] and d['kind']=='Declaration' for d in r['differences'])
   outside.append(r['case_id'])
  else:assert r['status']=='adjacent-exact'
 all_cases.update(cases);components.update(current)
 assert 'mod h2_8a_'+family.replace('-','_')+';' in (ROOT/'crates/compiler/tests/contracts.rs').read_text()
main=read(f'ratchets/h2-8a-{stem}-before.v1.json')
assert main['original']['eligible']==1 and main['original']['native_executions']==main['original']['full_tuples']==2
assert main['total_complete_comparison_and_supplemental_executions']==434
assert main['adjacent_declaration_blocking']=={'cases':22,'native_emit_executions':44,'command_reporting':False,'passed':True}
assert len(all_cases)==len(m['witnesses'])==108
for r in m['witnesses']:
 assert r['row_sha256']==h(json.dumps(all_cases[r['case_id']],ensure_ascii=False,separators=(',',':')).encode())
 assert r['status']==components[r['case_id']]['status']
assert sorted(outside)==sorted(m['after_outside_failure_case_ids']) and len(outside)==8
fault=read('crates/compiler/tests/fixtures/setter-symbol-invariant.json')
assert fault['typescript']=='6.0.3' and fault['repetitions']==2
assert fault['observer_sha256']==h((ROOT/'scripts/observe-setter-symbol-invariant.mjs').read_bytes())
assert fault['compiler_sha256']==h((ROOT/'vendor/typescript-6.0.3/lib/typescript.js').read_bytes())
assert len(fault['rows'])==2 and fault['rows'][0]['observation']['error'] is None
assert fault['rows'][1]['observation']=={'output':None,'error':'Error: Debug Failure. False expression.'}
native=read(f'ratchets/h2-8a-{stem}-native-before.v1.json')
assert native['base']==m['base'] and native['exit_code']==101
assert native['test_results']=={'passed':0,'failed':2}
assert m['after_required_complete_commands']==213 and m['after_target_exact_twice']==205
assert m['checker_node_builder_units_required']==62
assert m['before_complete_comparison_and_supplemental_executions']==542
print('H2.8a-A6-25 ready:20 whole TS functions,3 steps,6 architecture rows;108 fresh 72 exact/28 owned/8 readonly outside;original1 failed twice;two native controls failed;unresolved=0,undispositioned=0')
