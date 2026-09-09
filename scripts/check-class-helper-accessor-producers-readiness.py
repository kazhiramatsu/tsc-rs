#!/usr/bin/env python3
"""Check class helper/accessor readiness against immutable complete before evidence."""
from pathlib import Path
import collections,hashlib,json
ROOT=Path(__file__).resolve().parent.parent
read=lambda p:json.loads((ROOT/p).read_text());h=lambda b:hashlib.sha256(b).hexdigest()
stem='class-helper-accessor-producers';m=read(f'ratchets/h2-8a-{stem}-readiness.v1.json')
assert m['version']==1 and m['slice']=='H2.8a-A6-29' and m['state']=='ready' and m['unresolved']==m['undispositioned']==0
assert m['steps']==['A6-29'+s for s in 'abcd'] and not m['test_paths']
assert set(m['runtime_paths'])=={'crates/emitter/src/builtins/class_fields/downlevel.rs','crates/emitter/src/factory.rs','crates/emitter/src/builtins/es2015.rs'}
for e in m['authorities']:assert h((ROOT/e['path']).read_bytes())==e['sha256'],e['path']
b=read(m['baseline_record']);archive=Path(b['archive'])
assert (b['eligible'],b['primary_comparison_jobs'],b['exact_twice_per_job'],b['failed_twice'])==(144,2,32,112)
assert b['base']==m['base'] and b['first_failure_vectors_identical'] and len(b['comparisons'])==112
assert not b['typed_failures'] and not b['unrepresented_failed_case_ids']
assert (b['primary_complete_command_executions'],b['supplemental_capture_executions'],b['total_native_command_executions'])==(352,176,528)
assert all(e['exit_code']==101 for e in b['exits'])
for e in b['inputs']:assert h((archive/e['path']).read_bytes())==e['sha256'],e['path']
assert m['baseline_rust']==b['baseline_rust'] and {e['path'] for e in b['baseline_rust']}==set(m['runtime_paths'])
for e in b['baseline_rust']:assert h((archive/e['path']).read_bytes())==e['sha256']
assert m['prospective_scope_amendment']==b['prospective_scope_amendment']
amend=read(m['prospective_scope_amendment']['path'])
assert h((ROOT/m['prospective_scope_amendment']['path']).read_bytes())==m['prospective_scope_amendment']['sha256']
assert amend['prelaunch_pinned'] and not amend['production_started'] and amend['added_path']=='crates/emitter/src/builtins/es2015.rs'
assert len(amend['original_prospective_production_paths'])==2 and len(amend['proof'])==3
assert h(Path(amend['first_archive_path']).read_bytes())==amend['added_path_sha256']
assert m['source_dispositions']==b['source_dispositions']=={'owned-class-helper-accessor-producer':76,'adjacent-exact':32,'outside-retained-getter-modifier-comment':4,'outside-retained-es2022':32}
assert m['source_cause_memberships']==b['source_cause_memberships']=={'class-name-helper-request-order':48,'es5-accessor-receiver-flag-replacement':24,'accessor-setter-modifier-source-range':24,'retained-getter-modifier-comment':8,'retained-anonymous-accessor-receiver':12,'retained-static-field-map-ranges':4,'retained-accessor-map-ranges':16}
packet=(ROOT/f'docs/design/greenfield/slices/h2-8a-{stem}.md').read_text();assert all(s in packet for s in m['steps'])
lines=(ROOT/'vendor/typescript-6.0.3/lib/_tsc.js').read_bytes().splitlines(keepends=True)
assert len(m['owners'])==len({o['owner'] for o in m['owners']})==69
for o in m['owners']:
 assert h(b''.join(lines[o['start']-1:o['end']]))==o['sha256'],o['owner']
 assert lines[o['start']-1].decode().strip().startswith('function '+o['declaration_name']+'(')
 assert o['step'] in m['steps'] and o['test']
assert len(m['declarations'])==3
for d in m['declarations']:
 assert h(b''.join(lines[d['start']-1:d['end']]))==d['sha256'],d['owner']
 assert not d['priority_present'] and not d['dependencies_present'] and d['step']=='A6-29a'
source=read(m['source_audit'])
assert [{k:v for k,v in o.items() if k not in ['step','test']} for o in m['owners']]==source['owners']
assert [{k:v for k,v in o.items() if k not in ['step','test']} for o in m['declarations']]==source['declarations']
arch=(ROOT/'docs/design/greenfield/emitter-architecture.md').read_text();assert len(m['architecture'])==19
for row in m['architecture']:
 line=next(l for l in arch.splitlines() if l.startswith('| `'+row['id']+'` |'))
 assert h(line.encode())==row['row_sha256'] and row['id'] in packet
 assert row['disposition'] in ['premise-unchanged','modified-requalify'] and row['step'] in m['steps']
 assert row['rust_symbol'] and row['visibility'] and row['validation_ref'] and row['evidence']
 if row['inherited_qualified_premise']:assert row['disposition']=='premise-unchanged' and 'active-qualified' in line
f=read(f'crates/compiler/tests/fixtures/{stem}.json');assert f['typescript']=='6.0.3' and f['repetitions']==2
assert f['observer_sha256']==h((ROOT/f'scripts/observe-{stem}.mjs').read_bytes()) and f['compiler_sha256']==h((ROOT/'vendor/typescript-6.0.3/lib/typescript.js').read_bytes())
cases={c['case_id']:c for c in f['cases']};assert len(cases)==len(m['witnesses'])==144
assert collections.Counter(d['code'] for c in cases.values() for d in c['typescript_observation']['reported_diagnostics'])=={5107:48,4094:48}
assert collections.Counter(c['typescript_observation']['exit_code'] for c in cases.values())=={0:64,1:48,2:32}
assert collections.Counter(len(c['typescript_observation']['writes']) for c in cases.values())=={2:48,4:96}
components={c['case_id']:c for c in b['components']}
for w in m['witnesses']:
 assert w['row_sha256']==h(json.dumps(cases[w['case_id']],ensure_ascii=False,separators=(',',':')).encode())
 assert w['status']==components[w['case_id']]['status'] and w['causes']==components[w['case_id']]['causes'] and w['step'] in m['steps']
for c in b['components']:
 if c['status']=='adjacent-exact':assert not c['differences'];continue
 assert c['differences'] and all(d['kind'] in ['JavaScript','JavaScriptMap'] for d in c['differences'])
 for d in c['differences']:
  if d['kind']=='JavaScriptMap':assert d['differing_map_keys']==['mappings']
prior=read(m['prior_after']);reg=read(m['qualified_prerequisite'])
assert prior['exact_twice']==reg['exact_twice']==896 and prior['eligible']==reg['eligible']==996
assert not prior['adjacent_regressions'] and not prior['shared_required_failures']
assert all(s['exit']['exit_code']==0 for s in reg['emitter_suites'].values())
assert m['prior_exact_case_ids']==[n for band in prior['bands']+b['bands'] for n in band['exact_twice']] and len(set(m['prior_exact_case_ids']))==928
refinement=read(m['prior_repair_refinement']);assert m['prior_repair_case_ids']==[c['case_id'] for c in refinement['cases']]
assert len(m['prior_repair_case_ids'])==16 and set(m['prior_repair_case_ids'])<=set(prior['failed_once'])
vectors={c['case_id']:c for c in prior['comparisons']}
for c in refinement['cases']:
 v=vectors[c['case_id']];assert v['boundary']==c['boundary']
 assert h(v['actual_debug'].encode())==c['actual_sha256'] and h(v['expected_debug'].encode())==c['expected_sha256']
assert m['new_owned_case_ids']==[c['case_id'] for c in b['components'] if c['status'].startswith('owned-')] and len(m['new_owned_case_ids'])==76
assert len(set(m['prior_exact_case_ids']+m['prior_repair_case_ids']+m['new_owned_case_ids']))==1020
assert (m['after_required_complete_commands'],m['after_target_exact_twice'],m['original_A28_target_exact_twice'])==(1140,1020,412)
assert m['emitter_tests_required']=={'lib':494,'contracts':451} and not m['unit_test_id_replacements'] and not m['unit_test_id_additions']
assert len(m['rust_map'])==8 and all(row['step'] in m['steps'] and row['producer'] and row['consumer'] and row['lifetime'] and row['invalidation'] for row in m['rust_map'])
assert 'mod h2_8a_class_helper_accessor_producers;' in (ROOT/'crates/compiler/tests/contracts.rs').read_text()
for change in m['authority_refresh']:
 for e in change['events']:assert h(Path(e['archived_path']).read_bytes())==e['old_sha256'] and e['reason']
for e in {e['path']:e for change in m['authority_refresh'] for e in change['events']}.values():assert h((ROOT/e['path']).read_bytes())==e['sha256']
print('H2.8a-A6-29 ready:69 whole TS functions,3 helpers,3 production paths,19 architecture rows;144 repeated before32exact/76owned/36outside;1140 after/min1020 and494/451 emitter tests;unresolved=0,undispositioned=0')
