#!/usr/bin/env python3
"""Validate A21 source closure, complete witnesses and typed scope restoration plan."""
from pathlib import Path
import json,hashlib,subprocess,collections
ROOT=Path(__file__).resolve().parent.parent
stem='jsdoc-implements-serialization';read=lambda p:json.loads((ROOT/p).read_text());h=lambda b:hashlib.sha256(b).hexdigest()
m=read(f'ratchets/h2-8a-{stem}-readiness.v1.json')
assert m['version']==1 and m['slice']=='H2.8a-A6-21'
assert m['unresolved']==m['undispositioned']==0
assert m['runtime_paths']==['crates/checker/src/class.rs','crates/checker/src/node_builder/statements.rs','crates/checker/src/node_builder/serialize.rs','crates/checker/src/node_builder/mod.rs']
assert m['unit_path']=='crates/checker/tests/unit/node_builder_statements/tests.rs'
assert m['steps']==['A6-21-1','A6-21-2','A6-21-3']
for r in m['authorities']:assert h((ROOT/r['path']).read_bytes())==r['sha256'],r['path']
for r in m['baseline_rust']:assert h(subprocess.check_output(['git','show',f'{m["base"]}:{r["path"]}'],cwd=ROOT))==r['sha256'],r['path']
packet=(ROOT/f'docs/design/greenfield/slices/h2-8a-{stem}.md').read_text();assert all(s in packet for s in m['steps'])
lines=(ROOT/'vendor/typescript-6.0.3/lib/_tsc.js').read_bytes().splitlines(keepends=True);assert len(m['owners'])==19
for r in m['owners']:
 assert h(b''.join(lines[r['start']-1:r['end']]))==r['sha256'],r['owner']
 assert lines[r['start']-1].decode().strip().startswith('function '+r['owner']+'(')
 assert r['owner'] in packet
architecture=(ROOT/'docs/design/greenfield/emitter-architecture.md').read_text();assert len(m['architecture'])==6
for r in m['architecture']:assert f'| `{r["id"]}` |' in architecture and r['id'] in packet and r['disposition'] in packet
before=read(f'ratchets/h2-8a-{stem}-before.v1.json')
assert before['base']==m['base'] and before['native_jobs']==2
assert before['positive_executions_per_case']==4 and before['failed_executions_per_case']==2
assert len(before['exact_twice'])==36 and len(before['failed_twice'])==60
assert len(before['first_failure_comparisons'])==60
assert collections.Counter(r['boundary'] for r in before['first_failure_comparisons'])=={'callback bytes for /project/out/main.d.ts':58,'exact source-map result':2}
f=read(f'crates/compiler/tests/fixtures/{stem}.json');assert f['typescript']=='6.0.3' and f['repetitions']==2
assert f['observer_sha256']==h((ROOT/f'scripts/observe-{stem}.mjs').read_bytes())
assert f['compiler_sha256']==h((ROOT/'vendor/typescript-6.0.3/lib/typescript.js').read_bytes())
cs={r['case_id']:r for r in f['cases']};assert len(cs)==len(m['witnesses'])==96
assert set(before['exact_twice']).isdisjoint(before['failed_twice'])
assert set(before['exact_twice']+before['failed_twice'])==set(cs)
assert collections.Counter(r['status'] for r in m['witnesses'])=={'adjacent-exact':36,'owned-implements-and-tracking':58,'outside-source-map-result':2}
for r in m['witnesses']:
 assert r['row_sha256']==h(json.dumps(cs[r['case_id']],ensure_ascii=False,separators=(',',':')).encode())
 assert (r['status']=='adjacent-exact')==(r['case_id'] in before['exact_twice'])
 if r['status']=='owned-implements-and-tracking':assert r['component']['missing_implements'] and not r['component']['other_removed_lines']
 if r['status']=='outside-source-map-result':assert '/es5/' in r['case_id'] and r['case_id'].endswith('/typed-class-expression')
assert collections.Counter(d['code'] for c in cs.values() for d in c['typescript_observation']['reported_diagnostics'])=={5107:48,7006:16,2304:7,2694:3,1003:3,2503:1}
assert before['original']['eligible']==8 and before['original']['failed']==8 and before['original']['full_tuples']==16
assert before['original']['native_executions_per_case']==2 and before['original']['native_jobs']==1
assert before['invalid_package_launch']['native_executions']==0
test=(ROOT/'crates/compiler/tests/integration/h2_8a_jsdoc_implements_serialization.rs').read_text()
assert 'assert_eq!(cases.len(), 96)' in test and 'failures.is_empty()' in test
assert 'mod h2_8a_jsdoc_implements_serialization;' in (ROOT/'crates/compiler/tests/contracts.rs').read_text()
assert 'fn original_jsdoc_implements_match_complete_commands()' in (ROOT/'crates/compiler/tests/h2_8a_original_corpus.rs').read_text()
assert m['after_required_complete_commands']==344 and m['after_target_exact_twice']==342 and m['checker_unit_tests_required']==31
print('H2.8a-A6-21 ready:19 whole TS functions,3 steps,6 architecture rows,96 witnesses;36 exact/58 owned/2 outside;8 original failures twice;unresolved=0,undispositioned=0')
