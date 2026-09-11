#!/usr/bin/env python3
"""Check the bounded initializer-comment retirement and its frozen source evidence."""
from pathlib import Path
import collections,hashlib,json,subprocess
ROOT=Path(__file__).resolve().parent.parent
read=lambda p:json.loads((ROOT/p).read_text());h=lambda b:hashlib.sha256(b).hexdigest()
stem='class-field-initializer-comments';m=read(f'ratchets/h2-8a-{stem}-readiness.v1.json')
assert m['version']==1 and m['slice']=='H2.8a-A6-28-5' and m['unresolved']==m['undispositioned']==0
assert m['steps']==['A6-28-5a','A6-28-5b','A6-28-5c']
assert set(m['runtime_paths'])=={'crates/emitter/src/builtins/class_fields/downlevel.rs','crates/emitter/src/metadata.rs','crates/emitter/src/printer.rs','crates/emitter/src/factory/parsed_metadata.rs'}
for r in m['authorities']:assert h((ROOT/r['path']).read_bytes())==r['sha256'],r['path']
b=read(m['baseline_record']);archive=Path(b['archive'])
assert b['eligible']==64 and b['primary_comparison_jobs']==2 and b['exact_twice_per_job']==b['failed_twice']==32
assert b['first_failure_vectors_identical'] and len(b['comparisons'])==32 and not b['typed_failures'] and not b['unrepresented_failed_case_ids']
assert (b['primary_complete_command_executions'],b['supplemental_capture_executions'],b['total_native_command_executions'])==(192,96,288)
assert all(r['exit_code']==101 for r in b['exits'])
for r in b['inputs']:assert h((archive/r['path']).read_bytes())==r['sha256'],r['path']
assert m['baseline_rust']==b['baseline_rust']
for r in m['baseline_rust']:assert h((archive/r['path']).read_bytes())==r['sha256']
assert b['source_causal_js_diff']=={'operation':'delete','expected_interval':' /* initializer end */','actual_interval':'','cases':32,'comparison_unchanged':True}
packet=(ROOT/f'docs/design/greenfield/slices/h2-8a-{stem}.md').read_text();assert all(s in packet for s in m['steps'])
lines=(ROOT/'vendor/typescript-6.0.3/lib/_tsc.js').read_bytes().splitlines(keepends=True)
assert len(m['owners'])==len({r['owner'] for r in m['owners']})==76
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
f=read(f'crates/compiler/tests/fixtures/{stem}.json');cases={c['case_id']:c for c in f['cases']}
assert f['typescript']=='6.0.3' and f['repetitions']==2 and len(cases)==len(m['witnesses'])==64
assert f['observer_sha256']==h((ROOT/f'scripts/observe-{stem}.mjs').read_bytes())
assert f['compiler_sha256']==h((ROOT/'vendor/typescript-6.0.3/lib/typescript.js').read_bytes())
assert collections.Counter(d['code'] for c in cases.values() for d in c['typescript_observation']['reported_diagnostics'])=={5107:32}
components={c['case_id']:c for c in b['components']}
for w in m['witnesses']:
 assert w['row_sha256']==h(json.dumps(cases[w['case_id']],ensure_ascii=False,separators=(',',':')).encode())
 assert w['status']==components[w['case_id']]['status'] and w['step'] in m['steps']
for c in b['components']:
 for d in c['differences']:
  if d['kind']=='JavaScriptMap':assert d['differing_map_keys']==['mappings']
a=read('ratchets/h2-8a-class-field-alias-map-positions-first-after.v1.json')
assert m['prior_exact_case_ids']==[n for band in a['bands'] for n in band['exact_twice']] and len(m['prior_exact_case_ids'])==362
assert m['prior_repair_case_ids']==[n for n in a['owned_failures'] if n.endswith('/field-comments')] and len(m['prior_repair_case_ids'])==8
assert m['original_A28_other_owned_cases']==[n for n in a['owned_failures'] if n not in m['prior_repair_case_ids']] and len(m['original_A28_other_owned_cases'])==20
assert (m['after_required_complete_commands'],m['after_target_exact_twice'],m['original_A28_target_exact_twice'])==(556,434,390)
assert m['emitter_tests_required']=={'lib':490,'contracts':451}
assert m['unit_test_id_replacement']=={'old':'factory::parsed_metadata::tests::parsed_constants_preserve_bits_and_code_units_without_javascript_comment_ownership','new':'factory::parsed_metadata::tests::parsed_constants_preserve_number_bits_and_string_code_units'}
assert len(m['rust_map'])==5 and all(r['producer'] and r['consumer'] and r['lifetime'] and r['step'] in m['steps'] for r in m['rust_map'])
assert 'mod h2_8a_class_field_initializer_comments;' in (ROOT/'crates/compiler/tests/contracts.rs').read_text()
for r in m['authority_refresh']:
 assert h(Path(r['archived_path']).read_bytes())==r['old_sha256'] and r['reason']
for r in {r['path']:r for r in m['authority_refresh']}.values():
 assert h((ROOT/r['path']).read_bytes())==r['sha256']
print('H2.8a-A6-28-5 ready:76 whole TS owners,4 production files,3 steps,19 architecture rows;556 commands/minimum434 exact;original A28 still open20 other owned;unresolved=0,undispositioned=0')
