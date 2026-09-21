from pathlib import Path
import gzip,hashlib,json,subprocess,time
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');B=R/'docs/design/greenfield/slices/emitter-final-batch/integration';O=Path('/tmp/emitter-utf16-retirement-resume-r430');O.mkdir(exist_ok=False);h=lambda b:hashlib.sha256(b).hexdigest()
old=gzip.decompress(Path('/tmp/emitter-utf16-retirement-apply-r425/before.json.gz').read_bytes());f=R/'crates/compiler/tests/fixtures/utf16-identity-recovery-controls.json';assert f.read_bytes()==old;before=json.loads(old)
report={'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip(),'prior_failed_mint':'/tmp/emitter-utf16-retirement-apply-r425/manifest.json','reason':'Observer intentionally uses exclusive wx creation. Preserve existing bytes outside canonical output before re-observing; no observer semantics changed.','qualified_final':False};assert report['head']=='8859338f1c761d867cbc7f5ef58987dc3777f944'
f.rename(O/'before.json');label='utf16-retirement-oracle-r430';t=time.monotonic()
try:
 code=subprocess.run(['python3',str(B/'run-local.py'),label,'node','scripts/observe-utf16-identity-recovery-controls.mjs','--write'],cwd=R).returncode
 report.update(label=label,exit=code,seconds=time.monotonic()-t);assert code==0
 new=json.loads(f.read_bytes());assert len(new['cases'])==len(before['cases'])==65;changed=[]
 for a,b in zip(before['cases'],new['cases']):
  assert {k:v for k,v in a.items() if k!='native'}=={k:v for k,v in b.items() if k!='native'}
  if a['native']!=b['native']:changed.append((a['id'],a['native'],b['native']))
 assert changed==[('b3-b4-rejected/keyword-escape','typed-recovery-refusal','exact')]
 assert {k:v for k,v in before.items() if k not in ['cases','observer_sha256']}=={k:v for k,v in new.items() if k not in ['cases','observer_sha256']}
 assert sum(c['native']=='typed-recovery-refusal' for c in new['cases'])==8
 p=R/'crates/compiler/tests/h2_8a_utf16_identity_recovery_controls.rs';s=p.read_text();assert s.count(h(old))==1;s=s.replace(h(old),h(f.read_bytes()));p.write_text(s)
 report.update(type_script_observations_unchanged=True,retirement=changed,fixture_sha256=h(f.read_bytes()),test_sha256=h(p.read_bytes()),observer_sha256=new['observer_sha256'])
finally:
 (O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n')
 if not f.exists():f.write_bytes(old)
print(json.dumps(report))
