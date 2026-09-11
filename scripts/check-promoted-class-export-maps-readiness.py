#!/usr/bin/env python3
"""Validate the four-producer promoted/export map scope and immutable evidence."""
from pathlib import Path
import collections,hashlib,json
ROOT=Path(__file__).resolve().parent.parent
read=lambda p:json.loads((ROOT/p).read_text());h=lambda b:hashlib.sha256(b).hexdigest()
stem='promoted-class-export-maps';m=read(f'ratchets/h2-8a-{stem}-readiness.v1.json')
assert m['version']==1 and m['slice']=='H2.8a-A6-28-6' and m['state']=='ready' and m['unresolved']==m['undispositioned']==0
assert m['steps']==['A6-28-6'+s for s in 'abcdef']
assert set(m['runtime_paths'])=={'crates/emitter/src/builtins.rs','crates/emitter/src/builtins/es2015.rs','crates/emitter/src/builtins/legacy_decorators.rs','crates/emitter/src/builtins/class_fields/downlevel.rs'}
for r in m['authorities']:assert h((ROOT/r['path']).read_bytes())==r['sha256'],r['path']
b=read(m['baseline_record']);archive=Path(b['archive'])
assert b['eligible']==312 and b['primary_comparison_jobs']==2 and b['exact_twice_per_job']==224 and b['failed_twice']==88
assert b['first_failure_vectors_identical'] and len(b['comparisons'])==88 and not b['typed_failures'] and not b['unrepresented_failed_case_ids']
assert (b['primary_complete_command_executions'],b['supplemental_capture_executions'],b['total_native_command_executions'])==(1072,204,1276)
assert all(r['exit_code']==101 for r in b['exits']) and not b['late_source_archive']['prelaunch_pinned']
for r in b['inputs']:assert h((archive/r['path']).read_bytes())==r['sha256'],r['path']
assert m['baseline_rust']==b['baseline_rust']
for r in m['baseline_rust']:assert h((archive/r['path']).read_bytes())==r['sha256']
assert m['source_dispositions']==b['source_dispositions']=={'adjacent-exact':224,'owned-promoted-class-export-maps':84,'outside-es2015-escaped-declaration-name':4}
assert m['source_cause_memberships']==b['source_cause_memberships']=={'promoted-wrapper-range':48,'typescript-moved-export-name':42,'class-fields-default-export-name':6,'legacy-default-export-name':6,'legacy-named-export-flags':6,'commonjs-export-assignment-metadata':12}
packet=(ROOT/f'docs/design/greenfield/slices/h2-8a-{stem}.md').read_text();assert all(s in packet for s in m['steps'])
lines=(ROOT/'vendor/typescript-6.0.3/lib/_tsc.js').read_bytes().splitlines(keepends=True)
assert len(m['owners'])==len({r['owner'] for r in m['owners']})==98
for r in m['owners']:
 assert h(b''.join(lines[r['start']-1:r['end']]))==r['sha256'],r['owner']
 assert lines[r['start']-1].decode().strip().startswith('function '+r['declaration_name']+'(')
 assert r['step'] in m['steps'] and r['test']
arch=(ROOT/'docs/design/greenfield/emitter-architecture.md').read_text();assert len(m['architecture'])==19
for r in m['architecture']:
 row=next(l for l in arch.splitlines() if l.startswith('| `'+r['id']+'` |'))
 assert h(row.encode())==r['row_sha256'] and r['id'] in packet
 assert r['disposition'] in ['premise-unchanged','modified-requalify']
 assert r['rust_symbol'] and r['visibility'] and r['validation_ref'] and r['evidence'] and r['step'] in m['steps']
 if r['inherited_qualified_premise']:assert r['disposition']=='premise-unchanged' and 'active-qualified' in row
cases={}
for band in b['bands']:
 f=read('crates/compiler/tests/fixtures/'+band['fixture']+'.json');assert f['typescript']=='6.0.3' and f['repetitions']==2
 cs={c['case_id']:c for c in f['cases']};assert len(cs)==band['eligible'] and set(band['exact_twice']+band['failed_once'])==set(cs);cases.update(cs)
 if band['fixture']==stem:
  assert len(cs)==144 and f['observer_sha256']==h((ROOT/f'scripts/observe-{stem}.mjs').read_bytes())
  assert f['compiler_sha256']==h((ROOT/'vendor/typescript-6.0.3/lib/typescript.js').read_bytes())
  assert collections.Counter(d['code'] for c in cs.values() for d in c['typescript_observation']['reported_diagnostics'])=={5107:120}
components={c['case_id']:c for c in b['components']};assert len(cases)==len(m['witnesses'])==312
for w in m['witnesses']:
 assert w['row_sha256']==h(json.dumps(cases[w['case_id']],ensure_ascii=False,separators=(',',':')).encode())
 assert w['status']==components[w['case_id']]['status'] and w['causes']==components[w['case_id']]['causes'] and w['step'] in m['steps']
for c in b['components']:
 for d in c['differences']:assert d['kind']=='JavaScriptMap' and d['differing_map_keys']==['mappings']
p=read('ratchets/h2-8a-class-field-initializer-comments-shared-after.v1.json')
assert m['prior_exact_case_ids']==[n for band in p['bands']+b['bands'] for n in band['exact_twice']] and len(set(m['prior_exact_case_ids']))==658
assert m['prior_repair_case_ids']==[n for n in p['original_A28_owned_failures'] if not n.endswith('/wrapper-comments')] and len(m['prior_repair_case_ids'])==16
assert m['new_owned_case_ids']==[c['case_id'] for c in b['components'] if c['status'].startswith('owned-')] and len(m['new_owned_case_ids'])==84
assert m['original_A28_other_owned_cases']==[n for n in p['original_A28_owned_failures'] if n.endswith('/wrapper-comments')] and len(m['original_A28_other_owned_cases'])==4
assert (m['after_required_complete_commands'],m['after_target_exact_twice'],m['original_A28_target_exact_twice'])==(868,758,390)
assert m['emitter_tests_required']=={'lib':490,'contracts':451} and not m['unit_test_id_replacements']
assert 'mod h2_8a_promoted_class_export_maps;' in (ROOT/'crates/compiler/tests/contracts.rs').read_text()
for change in m['authority_refresh']:
 for r in change['events']:
  assert h(Path(r['archived_path']).read_bytes())==r['old_sha256'] and r['reason']
for r in {r['path']:r for change in m['authority_refresh'] for r in change['events']}.values():
 assert h((ROOT/r['path']).read_bytes())==r['sha256']
for amendment in m.get('source_amendments',[]):
 assert amendment['step']=='A6-28-6c' and amendment['runtime_path']=='crates/emitter/src/builtins/legacy_decorators.rs'
 assert amendment['after_target_exact_twice']==758 and amendment['after_required_complete_commands']==868
 assert amendment['amended_minimum_exact_twice']>=758
 first=read(amendment['first_after_record']);assert h((ROOT/amendment['first_after_record']).read_bytes())==amendment['first_after_sha256']
 assert first['shared_required_failures']==amendment['remaining_owned_cases'] and not first['adjacent_regressions'] and not first['unrepresented_failed_case_ids']
 assert amendment['first_candidate_exact_case_ids']==[n for band in first['bands'] for n in band['exact_twice']]
 assert amendment['amended_minimum_exact_twice']==first['exact_twice']+len(first['shared_required_failures'])
 for e in first['inputs']:assert h((Path(first['archive'])/e['path']).read_bytes())==e['sha256']
 assert amendment['owner']==next(o for o in m['owners'] if o['owner']=='generateConstructorDecorationExpression')
 assert amendment['baseline_rust'] and all(h((Path(first['archive'])/e['path']).read_bytes())==e['sha256'] for e in amendment['baseline_rust'])
print('H2.8a-A6-28-6 ready:98 whole TS owners,4 producer files,6 steps,19 architecture rows;312 before224exact/84owned/4outside twice;868 after/min758;original A28 retains4 owned comments;unresolved=0,undispositioned=0')
