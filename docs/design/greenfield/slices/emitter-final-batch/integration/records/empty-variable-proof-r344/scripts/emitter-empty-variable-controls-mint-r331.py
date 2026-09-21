from pathlib import Path
import json,subprocess,gzip,hashlib,time
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep')
T=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target')
B=R/'docs/design/greenfield/slices/emitter-final-batch/integration'
O=T/'emitter-empty-variable-controls-mint-r331';O.mkdir(exist_ok=False)
while not (T/'emitter-empty-variable-controls-mint-r328/manifest.json').exists():time.sleep(10)
assert json.loads((T/'emitter-empty-variable-controls-mint-r328/manifest.json').read_bytes())['oracle_exit']==0
sha=lambda b:hashlib.sha256(b).hexdigest()
probe=json.loads((T/'emitter-empty-variable-flags-r324/manifest.json').read_bytes())
assert probe['qualified'] and probe['probe_restored']
assert json.loads(Path('/tmp/emitter-empty-variable-prototype-r325/manifest.json').read_bytes())['syntax_test_status']=='216 PASS'
p=R/'crates/compiler/tests/fixtures/emitter-context-recovery.json'
script=R/'scripts/observe-emitter-context-recovery.mjs'
raw=p.read_bytes();old=json.loads(raw);oldscript=script.read_bytes()
assert len(old['cases'])==528 and len(old['refused_cases'])==72
assert old['observer_sha256']==sha(oldscript)
(O/'previous600.json.gz').write_bytes(gzip.compress(raw,mtime=0))
(O/'original-observer.mjs').write_bytes(oldscript)
draft=Path('/tmp/emitter-empty-variable-controls-r331/observe-emitter-context-recovery.mjs').read_bytes()
assert sha(draft)==json.loads(Path('/tmp/emitter-empty-variable-controls-r331/proposal.json').read_bytes())['observer_sha256']
script.write_bytes(draft);p.unlink()
try:
 code=subprocess.run(['python3',str(B/'run-local.py'),'empty-variable-controls-oracle-r331','node','scripts/observe-emitter-context-recovery.mjs','--write'],cwd=R).returncode
 assert code==0
 new=json.loads(p.read_bytes());byid={c['case_id']:c for c in new['cases']}
 assert len(byid)==len(new['cases'])==588
 for c in old['cases']:assert byid[c['case_id']]==c,c['case_id']
 assert new['refused_cases']==old['refused_cases']
 for key,value in old.items():
  if key not in ['cases','observer_sha256']:assert new[key]==value,key
 fresh=[c for c in new['cases'] if c['case_id'] not in {x['case_id'] for x in old['cases']}]
 assert len(fresh)==60
 for c in fresh:
  opts=json.loads(c['config'])['compilerOptions']
  assert all(opts[k] is True for k in ['sourceMap','declaration','declarationMap'])
except BaseException:
 p.write_bytes(raw);script.write_bytes(oldscript);raise
roster=R/'crates/compiler/tests/fixtures/emitter-r77-regressions.json'
before=roster.read_bytes();j=json.loads(before)
assert len(j['cases'])==70 and j['fixtures']['emitter-context-recovery.json']==sha(raw)
(O/'original-r77-roster.json').write_bytes(before)
oldhash=j['fixtures']['emitter-context-recovery.json'];newhash=sha(p.read_bytes())
after=before.replace(oldhash.encode(),newhash.encode());assert after!=before
changed=json.loads(after);assert changed['cases']==j['cases'];changed['fixtures']['emitter-context-recovery.json']=oldhash;assert changed==j
roster.write_bytes(after)
consumer=R/'crates/compiler/tests/integration/h2_8a_import_helpers.rs';s=consumer.read_text()
assert s.count('assert_eq!(cases.len(), 528);')==1
consumer.write_text(s.replace('assert_eq!(cases.len(), 528);','assert_eq!(cases.len(), 588);'))
report={'oracle_exit':0,'old528_and72_unchanged':True,'new60_case_ids':[c['case_id'] for c in fresh],'fixture_sha256':newhash,'observer_sha256':sha(draft),'r77_case_ids_unchanged':True,'qualified_native':False,'scope':'Official canonical TypeScript observations twice; native outputs and combined original16994 proof pending.'}
(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report),flush=True)
