#!/usr/bin/env python3
"""Validate independent comment endpoints against immutable before evidence."""
from pathlib import Path
import collections,hashlib,json
ROOT=Path(__file__).resolve().parent.parent
read=lambda p:json.loads((ROOT/p).read_text());h=lambda b:hashlib.sha256(b).hexdigest()
stem='one-sided-class-comments';m=read(f'ratchets/h2-8a-{stem}-readiness.v1.json')
assert m['version']==1 and m['slice']=='H2.8a-A6-28-7' and m['state']=='ready' and m['unresolved']==m['undispositioned']==0
assert m['steps']==['A6-28-7'+s for s in 'abcdef']
assert set(m['runtime_paths'])=={'crates/emitter/src/metadata.rs','crates/emitter/src/lib.rs','crates/emitter/src/comment_cursor.rs','crates/emitter/src/printer.rs','crates/emitter/src/builtins/es2015.rs','crates/emitter/src/builtins.rs'}
assert set(m['test_paths'])=={'crates/emitter/tests/unit/lib/tests.rs','crates/emitter/tests/unit/comment_scope_predicate/tests.rs','crates/emitter/tests/integration/active_transform_contract.rs'}
for e in m['authorities']:assert h((ROOT/e['path']).read_bytes())==e['sha256'],e['path']
b=read(m['baseline_record']);archive=Path(b['archive'])
assert (b['eligible'],b['primary_comparison_jobs'],b['exact_twice_per_job'],b['failed_twice'])==(128,2,116,12)
assert b['first_failure_vectors_identical'] and len(b['comparisons'])==12 and not b['typed_failures'] and not b['unrepresented_failed_case_ids']
assert (b['primary_complete_command_executions'],b['supplemental_capture_executions'],b['total_native_command_executions'])==(488,244,732)
assert all(e['exit_code']==101 for e in b['exits'])
for e in b['inputs']:assert h((archive/e['path']).read_bytes())==e['sha256'],e['path']
assert m['baseline_rust']==b['baseline_rust'] and {e['path'] for e in b['baseline_rust']}==set(m['runtime_paths'])
for e in b['baseline_rust']:assert h((archive/e['path']).read_bytes())==e['sha256']
assert m['source_dispositions']==b['source_dispositions']=={'adjacent-exact':116,'owned-class-partial-comment-end':12}
assert m['source_cause_memberships']==b['source_cause_memberships']=={'class-inner-partial-comment-end':12}
packet=(ROOT/f'docs/design/greenfield/slices/h2-8a-{stem}.md').read_text();assert all(s in packet for s in m['steps'])
lines=(ROOT/'vendor/typescript-6.0.3/lib/_tsc.js').read_bytes().splitlines(keepends=True)
assert len(m['owners'])==len({o['owner'] for o in m['owners']})==117
for o in m['owners']:
 assert h(b''.join(lines[o['start']-1:o['end']]))==o['sha256'],o['owner']
 assert lines[o['start']-1].decode().strip().startswith('function '+o['declaration_name']+'(')
 assert o['step'] in m['steps'] and o['test']
arch=(ROOT/'docs/design/greenfield/emitter-architecture.md').read_text();assert len(m['architecture'])==19
for row in m['architecture']:
 line=next(l for l in arch.splitlines() if l.startswith('| `'+row['id']+'` |'))
 assert h(line.encode())==row['row_sha256'] and row['id'] in packet
 assert row['disposition'] in ['premise-unchanged','modified-requalify'] and row['step'] in m['steps']
 assert row['rust_symbol'] and row['visibility'] and row['validation_ref'] and row['evidence']
 if row['inherited_qualified_premise']:assert row['disposition']=='premise-unchanged' and 'active-qualified' in line
f=read(f'crates/compiler/tests/fixtures/{stem}.json');assert f['typescript']=='6.0.3' and f['repetitions']==2
assert f['observer_sha256']==h((ROOT/f'scripts/observe-{stem}.mjs').read_bytes()) and f['compiler_sha256']==h((ROOT/'vendor/typescript-6.0.3/lib/typescript.js').read_bytes())
cases={c['case_id']:c for c in f['cases']};assert len(cases)==len(m['witnesses'])==128
assert collections.Counter(d['code'] for c in cases.values() for d in c['typescript_observation']['reported_diagnostics'])=={5107:64}
assert collections.Counter(c['typescript_observation']['exit_code'] for c in cases.values())=={0:64,2:64}
assert all(len(c['typescript_observation']['writes'])==4 for c in cases.values())
components={c['case_id']:c for c in b['components']}
for w in m['witnesses']:
 assert w['row_sha256']==h(json.dumps(cases[w['case_id']],ensure_ascii=False,separators=(',',':')).encode())
 assert w['status']==components[w['case_id']]['status'] and w['causes']==components[w['case_id']]['causes'] and w['step'] in m['steps']
for c in b['components']:
 if c['status']=='adjacent-exact':assert not c['differences'];continue
 assert collections.Counter(d['kind'] for d in c['differences'])=={'JavaScript':1,'JavaScriptMap':1}
 for d in c['differences']:
  if d['kind']=='JavaScriptMap':assert d['differing_map_keys']==['mappings']
prior=read('ratchets/h2-8a-promoted-class-export-maps-amended-after.v1.json');reg=read(m['qualified_prerequisite'])
assert prior['exact_twice']==reg['exact_twice']==764 and not prior['adjacent_regressions'] and not prior['shared_required_failures']
assert all(s['exit']['exit_code']==0 for s in reg['emitter_suites'].values())
assert m['prior_exact_case_ids']==[n for band in prior['bands']+b['bands'] for n in band['exact_twice']] and len(set(m['prior_exact_case_ids']))==880
assert m['prior_repair_case_ids']==prior['original_A28_owned_failures'] and len(m['prior_repair_case_ids'])==4 and all(n.endswith('/wrapper-comments') for n in m['prior_repair_case_ids'])
assert m['new_owned_case_ids']==[c['case_id'] for c in b['components'] if c['status'].startswith('owned-')] and len(m['new_owned_case_ids'])==12
assert (m['after_required_complete_commands'],m['after_target_exact_twice'],m['original_A28_target_exact_twice'])==(996,896,390)
assert m['emitter_tests_required']=={'lib':494,'contracts':451} and not m['unit_test_id_replacements']
assert m['unit_test_id_additions']==['tests::comment_ranges_validate_independent_endpoints_and_preserve_source_identity']+['printer::comment_scope_predicate_tests::'+n for n in ['one_sided_comment_ranges_claim_present_endpoint_and_inherit_other','one_sided_zero_position_has_no_comment_extent','jsx_one_sided_ranges_require_present_suppressed_side']]
assert not set(m['unit_test_id_additions']) & set(reg['emitter_suites']['library']['test_ids'])
assert len(m['rust_map'])==9 and all(row['step'] in m['steps'] and row['producer'] and row['consumer'] and row['lifetime'] and row['invalidation'] for row in m['rust_map'])
assert 'mod h2_8a_one_sided_class_comments;' in (ROOT/'crates/compiler/tests/contracts.rs').read_text()
for change in m['authority_refresh']:
 for e in change['events']:assert h(Path(e['archived_path']).read_bytes())==e['old_sha256'] and e['reason']
for e in {e['path']:e for change in m['authority_refresh'] for e in change['events']}.values():assert h((ROOT/e['path']).read_bytes())==e['sha256']
print('H2.8a-A6-28-7 ready:117 whole TS owners,6 production files,19 architecture rows;128 before116exact/12owned twice;996 after/min896 and494/451 emitter tests;unresolved=0,undispositioned=0')
