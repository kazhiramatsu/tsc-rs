#!/usr/bin/env python3
"""Check A27's MetaProperty source closure and immutable before witnesses."""
from pathlib import Path
import collections,hashlib,json,subprocess
ROOT=Path(__file__).resolve().parent.parent
read=lambda p:json.loads((ROOT/p).read_text());h=lambda b:hashlib.sha256(b).hexdigest()
stem='meta-property-token-maps';m=read(f'ratchets/h2-8a-{stem}-readiness.v1.json')
assert m['version']==1 and m['slice']=='H2.8a-A6-27' and m['unresolved']==m['undispositioned']==0
assert m['steps']==['A6-27-1','A6-27-2','A6-27-3','A6-27-4']
assert m['runtime_paths']==['crates/emitter/src/builtins.rs','crates/emitter/src/builtins/system.rs','crates/emitter/src/printer.rs']
for r in m['authorities']:assert h((ROOT/r['path']).read_bytes())==r['sha256'],r['path']
for r in m['baseline_rust']:assert h(subprocess.check_output(['git','show',m['base']+':'+r['path']],cwd=ROOT))==r['sha256'],r['path']
packet=(ROOT/f'docs/design/greenfield/slices/h2-8a-{stem}.md').read_text();assert all(s in packet for s in m['steps'])
lines=(ROOT/'vendor/typescript-6.0.3/lib/_tsc.js').read_bytes().splitlines(keepends=True)
assert len(m['owners'])==len({r['owner'] for r in m['owners']})==39
for r in m['owners']:
 assert h(b''.join(lines[r['start']-1:r['end']]))==r['sha256'],r['owner']
 assert lines[r['start']-1].decode().strip().startswith('function '+r['declaration_name']+'(')
 assert r['step'] in m['steps'] and r['test']
arch=(ROOT/'docs/design/greenfield/emitter-architecture.md').read_text();assert len(m['architecture'])==13
for r in m['architecture']:
 row=next(line for line in arch.splitlines() if line.startswith('| `'+r['id']+'` |'))
 assert h(row.encode())==r['row_sha256'] and r['id'] in packet and r['disposition'] in ['premise-unchanged','modified-requalify']
 if r['inherited_qualified_premise']:assert 'active-qualified' in row and r['disposition']=='premise-unchanged'
 assert r['rust_symbol'] and r['visibility'] and r['validation_ref'] and r['evidence'] and r['step'] in m['steps']
b=read(f'ratchets/h2-8a-{stem}-before.v1.json');assert b['base']==m['base'] and b['eligible']==228
assert b['primary_comparison_jobs']==2 and b['exact_twice_per_job']==142 and b['failed_twice']==86
assert b['primary_complete_command_executions']==740 and b['supplemental_capture_executions']==370 and b['total_native_command_executions']==1110
assert b['emitter_library']['passed']==488 and b['emitter_library']['failed']==2 and len(b['emitter_library']['test_ids'])==490
assert b['native_invariants']['map_differences_per_job']==12 and b['native_invariants']['absent_name_typed_refusals_per_job']==4
assert len(b['native_name_context'])==6 and sum(r['name_not_queried'] for r in b['native_name_context'])==3 and all(r['value_query_matches_phase'] for r in b['native_name_context'])
assert len(b['first_failure_comparisons'])==79 and len(b['typed_failures'])==7
cases={}
for band in b['bands']:
 f=read('crates/compiler/tests/fixtures/'+band['fixture']+'.json');assert f['typescript']=='6.0.3' and f['repetitions']==2
 cs={r['case_id']:r for r in f['cases']};assert set(band['exact_twice']+band['failed_once'])==set(cs) and len(cs)==band['eligible'];cases.update(cs)
 if band['fixture']==stem:
  assert f['observer_sha256']==h((ROOT/f'scripts/observe-{stem}.mjs').read_bytes()) and f['compiler_sha256']==h((ROOT/'vendor/typescript-6.0.3/lib/typescript.js').read_bytes())
  assert collections.Counter(d['code'] for c in cs.values() for d in c['typescript_observation']['reported_diagnostics'])=={5107:100,1343:9}
components={c['case_id']:c for c in b['components']};assert len(cases)==len(components)==len(m['witnesses'])==228
for w in m['witnesses']:
 c=cases[w['case_id']];assert w['row_sha256']==h(json.dumps(c,ensure_ascii=False,separators=(',',':')).encode())
 assert w['status']==components[w['case_id']]['status'] and w['step'] in m['steps']
for c in components.values():
 for d in c['differences']:
  if d['kind']=='JavaScriptMap':assert d['differing_map_keys']==['mappings']
assert collections.Counter(w['status'] for w in m['witnesses'])=={'adjacent-exact':142,'owned-printer-meta-property':75,'module-name-context-and-printer-required':7,'outside-class-field-alias-maps':4}
assert m['after_required_complete_commands']==228 and m['after_target_exact_twice']==224 and m['after_outside_case_ids']==b['outside_class_alias_maps']
for name in m['after_outside_case_ids']:
 c=cases[name];assert name.endswith('/class-fields-alias') and all('new.target' not in f['text'] and 'import.meta' not in f['text'] for f in c['files'])
f=read('crates/emitter/tests/fixtures/meta-property-token-map-invariants.json');assert len(f['rows'])==8 and f['repetitions']==2
assert f['observer_sha256']==h((ROOT/'scripts/observe-meta-property-token-map-invariants.mjs').read_bytes())
assert m['emitter_tests_required']=={'lib':490,'contracts':451}
assert len(m['rust_map'])==9 and all(r['step'] in m['steps'] and r['producer'] and r['consumer'] and r['lifetime'] and r['invalidation'] for r in m['rust_map'])
t=(ROOT/'crates/emitter/tests/unit/builtins/tests.rs').read_text();assert all('fn '+name.split('::')[-1]+'()' in t for name in b['emitter_library']['failed_test_ids'])
assert 'mod h2_8a_meta_property_token_maps;' in (ROOT/'crates/compiler/tests/contracts.rs').read_text()
print('H2.8a-A6-27 ready:39 whole TS owners,3 production files,4 steps,13 architecture rows;228 commands142 exact/82 owned/4 outside;8 internal rows,6 query rows,490 current units;unresolved=0,undispositioned=0')
