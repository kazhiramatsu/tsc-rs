from pathlib import Path
import gzip,hashlib,json,subprocess,time
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');B=R/'docs/design/greenfield/slices/emitter-final-batch/integration';P=Path('/tmp/emitter-utf16-retirement-r424');O=Path('/tmp/emitter-utf16-retirement-apply-r425');m=json.loads((P/'manifest.json').read_bytes());proof=json.loads(Path(m['proof']).read_bytes());assert proof['keyword_exact_each'] and proof['keyword_repeats_equal'];O.mkdir(exist_ok=False);h=lambda b:hashlib.sha256(b).hexdigest()
f=R/'crates/compiler/tests/fixtures/utf16-identity-recovery-controls.json';old=f.read_bytes();assert h(old)==m['fixture_before_sha256'];before=json.loads(old);(O/'before.json.gz').write_bytes(gzip.compress(old,mtime=0));head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip();assert head==proof['head']
for row in m['files']:
 p=R/row['path'];assert h(p.read_bytes())==row['before'];data=(P/row['path']).read_bytes();assert h(data)==row['after'];p.write_bytes(data)
label='utf16-retirement-oracle-r425';t=time.monotonic();code=subprocess.run(['python3',str(B/'run-local.py'),label,'node','scripts/observe-utf16-identity-recovery-controls.mjs','--write'],cwd=R).returncode;report={'head':head,'label':label,'exit':code,'seconds':time.monotonic()-t,'native_retirement_scope':'One keyword-escape exact x2 on base885; all TypeScript tuples must remain unchanged and8otherrefusalsretained. BothRusttests requirefreshvalidation.','qualified_final':False};(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n');assert code==0
new=json.loads(f.read_bytes());assert len(new['cases'])==len(before['cases'])==65;changed=[]
for a,b in zip(before['cases'],new['cases']):
 assert {k:v for k,v in a.items() if k!='native'}=={k:v for k,v in b.items() if k!='native'}
 if a['native']!=b['native']:changed.append((a['id'],a['native'],b['native']))
assert changed==[('b3-b4-rejected/keyword-escape','typed-recovery-refusal','exact')]
assert {k:v for k,v in before.items() if k not in ['cases','observer_sha256']}=={k:v for k,v in new.items() if k not in ['cases','observer_sha256']}
assert sum(c['native']=='typed-recovery-refusal' for c in new['cases'])==8
p=R/'crates/compiler/tests/h2_8a_utf16_identity_recovery_controls.rs';s=p.read_text();assert s.count(h(old))==1;s=s.replace(h(old),h(f.read_bytes()));p.write_text(s);report.update(type_script_observations_unchanged=True,retirement=changed,fixture_sha256=h(f.read_bytes()),test_sha256=h(p.read_bytes()),observer_sha256=new['observer_sha256']);(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report))
