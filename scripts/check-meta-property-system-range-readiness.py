#!/usr/bin/env python3
"""Check the A27 System import.meta range dependency without rewriting initial evidence."""
from pathlib import Path
import hashlib,json,subprocess
ROOT=Path(__file__).resolve().parent.parent
h=lambda b:hashlib.sha256(b).hexdigest();read=lambda p:json.loads((ROOT/p).read_text())
subprocess.run(['python3',str(ROOT/'scripts/check-meta-property-token-maps-readiness.py')],check=True,cwd=ROOT)
m=read('ratchets/h2-8a-meta-property-system-range-readiness.v1.json');assert m['version']==1 and m['slice']=='H2.8a-A6-27-system-range' and m['unresolved']==m['undispositioned']==0
assert m['steps']==['A6-27-5'] and m['runtime_paths']==['crates/emitter/src/builtins.rs','crates/emitter/src/builtins/system.rs','crates/emitter/src/printer.rs']
for r in m['authorities']:assert h((ROOT/r['path']).read_bytes())==r['sha256'],r['path']
first=read('ratchets/h2-8a-meta-property-token-maps-first-after.v1.json');assert first['exact_twice']==208 and len(first['failed_once'])==20 and first['native_complete_command_executions']==436
assert first['selected_native_tests_passed']==2 and first['native_invariant_exact_repetitions']==16 and first['native_query_cases_passed']==6
assert first['all_remaining_first_vectors_identical_to_before'] and first['full_emitter_suites_run'] is False
assert first['classification_correction']['corrected_owner_counts']=={'owned-printer-meta-property':59,'module-name-context-and-printer-required':7,'owned-system-import-meta-synthetic-range':16,'outside-class-field-alias-maps':4,'adjacent-exact':142}
assert m['system_range_case_ids']==first['classification_correction']['case_ids'] and len(m['system_range_case_ids'])==16
for r in first['initial_runtime_snapshot']:
 assert h((Path(first['archive'])/r['path']).read_bytes())==r['sha256'],r['path']
 if r['path']!='crates/emitter/src/builtins/system.rs':assert h((ROOT/r['path']).read_bytes())==r['sha256'],r['path']
lines=(ROOT/'vendor/typescript-6.0.3/lib/_tsc.js').read_bytes().splitlines(keepends=True);assert len(m['owners'])==len({r['owner'] for r in m['owners']})==44
for r in m['owners']:assert h(b''.join(lines[r['start']-1:r['end']]))==r['sha256'] and lines[r['start']-1].decode().strip().startswith('function '+r['declaration_name']+'('),r['owner']
fixture=read('crates/compiler/tests/fixtures/meta-property-token-maps.json');cases={r['case_id']:r for r in fixture['cases']}
assert all('/system/import-meta-' in n and not n.endswith('no-map') for n in m['system_range_case_ids'])
assert len(m['no_map_adjacent_case_ids'])==2 and all(cases[n]['options']['sourceMap'] is False and '/system/import-meta-no-map' in n for n in m['no_map_adjacent_case_ids'])
assert m['after_required_complete_commands']==228 and m['after_target_exact_twice']==224 and m['emitter_tests_required']=={'lib':490,'contracts':451}
arch=(ROOT/'docs/design/greenfield/emitter-architecture.md').read_text();assert len(m['architecture'])==13
for r in m['architecture']:
 line=next(line for line in arch.splitlines() if line.startswith('| `'+r['id']+'` |'));assert h(line.encode())==r['row_sha256']
assert next(r for r in m['architecture'] if r['id']=='E-METADATA-BASE')['disposition']=='modified-requalify'
packet=(ROOT/'docs/design/greenfield/slices/h2-8a-meta-property-system-range.md').read_text();assert 'A6-27-5' in packet and 'set_original_and_range' in packet and 'misclassified' in packet
print('H2.8a-A6-27 shared ready:44 whole TS owners,3 production paths,one added step;16 System-range cases corrected,2 no-map adjacent;first208/228 and2native pass frozen;final228/490/451 required;unresolved=0,undispositioned=0')
