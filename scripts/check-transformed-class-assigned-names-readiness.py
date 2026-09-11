#!/usr/bin/env python3
"""Validate A22 current-parent name selection and immutable complete witnesses."""
from pathlib import Path
import collections, hashlib, json, subprocess
ROOT=Path(__file__).resolve().parent.parent
stem='transformed-class-assigned-names';read=lambda p:json.loads((ROOT/p).read_text());h=lambda b:hashlib.sha256(b).hexdigest()
m=read(f'ratchets/h2-8a-{stem}-readiness.v1.json')
assert m['version']==1 and m['slice']=='H2.8a-A6-22'
assert m['unresolved']==m['undispositioned']==0
assert m['runtime_paths']==['crates/emitter/src/builtins/es2015.rs','crates/emitter/src/builtins/class_fields/downlevel.rs'] and m['steps']==['A6-22-1','A6-22-2','A6-22-3']
for r in m['authorities']:assert h((ROOT/r['path']).read_bytes())==r['sha256'],r['path']
for r in m['baseline_rust']:assert h(subprocess.check_output(['git','show',f'{m["base"]}:{r["path"]}'],cwd=ROOT))==r['sha256'],r['path']
packet=(ROOT/f'docs/design/greenfield/slices/h2-8a-{stem}.md').read_text();assert all(s in packet for s in m['steps'])
lines=(ROOT/'vendor/typescript-6.0.3/lib/_tsc.js').read_bytes().splitlines(keepends=True);assert len(m['owners'])==32
for r in m['owners']:
 assert h(b''.join(lines[r['start']-1:r['end']]))==r['sha256'],r['owner']
 assert lines[r['start']-1].decode().strip().startswith('function '+r['declaration_name']+'(')
 assert r['owner'] in packet
architecture=(ROOT/'docs/design/greenfield/emitter-architecture.md').read_text();assert len(m['architecture'])==7
for r in m['architecture']:assert f'| `{r["id"]}` |' in architecture and r['id'] in packet and r['disposition'] in packet
before=read(f'ratchets/h2-8a-{stem}-before.v1.json')
assert before['base']==m['base'] and before['primary_comparison_jobs']==2
assert before['positive_primary_executions_per_case']==4 and before['failed_primary_executions_per_case']==2
assert before['positive_total_native_executions_per_case']==6 and before['failed_total_native_executions_per_case']==3
assert before['supplemental_capture_executions']==182 and before['total_native_command_executions']==546
assert before['capture_is_full_actual_tuple'] is False
assert len(before['exact_twice'])==74 and len(before['failed_twice'])==34
assert collections.Counter(r['boundary'] for r in before['first_failure_comparisons'])=={'exact source-map result':34}
f=read(f'crates/compiler/tests/fixtures/{stem}.json');assert f['typescript']=='6.0.3' and f['repetitions']==2
assert f['observer_sha256']==h((ROOT/f'scripts/observe-{stem}.mjs').read_bytes())
assert f['compiler_sha256']==h((ROOT/'vendor/typescript-6.0.3/lib/typescript.js').read_bytes())
cs={r['case_id']:r for r in f['cases']};assert len(cs)==len(m['witnesses'])==108
assert set(before['exact_twice']).isdisjoint(before['failed_twice'])
assert set(before['exact_twice']+before['failed_twice'])==set(cs)
assert collections.Counter(r['status'] for r in m['witnesses'])=={'adjacent-exact':74,'owned-current-parent-name':26,'outside-map-only':8}
components={r['case_id']:r for r in before['components']};assert set(components)==set(cs)
for r in m['witnesses']:
 assert r['row_sha256']==h(json.dumps(cs[r['case_id']],ensure_ascii=False,separators=(',',':')).encode())
 assert (r['status']=='adjacent-exact')==(r['case_id'] in before['exact_twice'])
 component=components[r['case_id']];assert component['status']==r['status']
 if r['status']=='owned-current-parent-name':assert component['js_differences'] and all(d['rename_only'] for d in component['js_differences'])
 if r['status']=='outside-map-only':assert not component['js_differences'] and component['map_differences']
assert collections.Counter(d['code'] for c in cs.values() for d in c['typescript_observation']['reported_diagnostics'])=={5107:54}
test=(ROOT/'crates/compiler/tests/integration/h2_8a_transformed_class_assigned_names.rs').read_text()
assert 'assert_eq!(cases.len(), 108)' in test and 'failures.is_empty()' in test
assert 'assert_cases_with_inspection' in test and 'capture_artifact_bytes' in test
assert 'mod h2_8a_transformed_class_assigned_names;' in (ROOT/'crates/compiler/tests/contracts.rs').read_text()
assert m['after_required_complete_commands']==434 and m['after_target_exact_twice']==426
assert m['emitter_unit_tests_required']=={'lib':483,'contracts':451}
first=read(f'ratchets/h2-8a-{stem}-first-after.v1.json')
assert first['eligible']==434 and first['exact_twice']==415 and first['failed_once']==19
assert len(first['unexpected_case_ids'])==11 and len(first['retained_case_ids'])==8
assert len(first['first_failure_comparisons'])==19 and first['exit_code']==101
assert (ROOT/'crates/emitter/src/execute.rs').read_text().count('.with_source_file_text_mode(SourceFileTextMode::Canonical)')==2
unit=read(f'ratchets/h2-8a-{stem}-emitter-first-after.v1.json')
assert unit['library_tests_passed']==483 and unit['contracts_passed']==450 and unit['contracts_failed']==1 and unit['exit_code']==101
assert m['unit_path']=='crates/emitter/tests/integration/active_transform_contract.rs'
unit_source=(ROOT/m['unit_path']).read_text()
assert 'fn transform_and_print_canonical_at_target(' in unit_source and 'fn empty_function_body_does_not_reown_the_open_brace_trailing_comment()' in unit_source
print('H2.8a-A6-22 ready:32 whole TS functions,3 steps,7 architecture rows,108 witnesses;74 exact/26 owned/8 outside;11 first-after regressions and1 unit-mode failure frozen;unresolved=0,undispositioned=0')
