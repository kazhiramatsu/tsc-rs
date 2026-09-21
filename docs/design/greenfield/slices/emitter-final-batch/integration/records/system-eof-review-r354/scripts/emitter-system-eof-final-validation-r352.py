from pathlib import Path
import json,subprocess,os,hashlib,gzip,time
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-leading-binding-prototype');C=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');T=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target');O=T/'emitter-system-eof-final-validation-r352'
prior=json.loads((T/'emitter-system-eof-full-command-r342/manifest.json').read_bytes());assert prior['test_files_restored'] and prior['qualified'], 'finish342 and resolve its failures before altering frozen bytes'
O.mkdir(exist_ok=False);draft=Path('/tmp/emitter-system-eof-final-r351');proposal=json.loads((draft/'manifest.json').read_bytes());sha=lambda b:hashlib.sha256(b).hexdigest()
for row in proposal['paths']:
 p=row['path'];assert sha((R/p).read_bytes())==row['original_sha256'];assert sha((draft/p).read_bytes())==row['draft_sha256']
for row in proposal['paths']:(R/row['path']).write_bytes((draft/row['path']).read_bytes())
for p in ['crates/emitter/tests/fixtures/meta-property-token-map-invariants.json']:(R/p).write_bytes((C/p).read_bytes())
report={'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip(),'scope':'Final private EOF/System patch plus actual182 pre-trivia typed anchor guard, behavior-neutral actual spellings, generated-map geometry assertions and12official MetaProperty invariants (original8+4synthetic) with recording on/off. Full342 output controls retained for valid-anchor behavior; final canonical fullCI remains required.','qualified':False,'steps':[]}
def save():(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n')
env=os.environ.copy();env.update(CARGO_BUILD_JOBS='2',CARGO_TARGET_DIR=str(T))
def run(label,args,cwd=R):
 log=O/(label+'.log');start=time.monotonic()
 with log.open('xb') as f:code=subprocess.run(args,cwd=cwd,env=env,stdout=f,stderr=subprocess.STDOUT).returncode
 raw=log.read_bytes();(O/(label+'.log.gz')).write_bytes(gzip.compress(raw,mtime=0));row={'label':label,'argv':args,'exit':code,'seconds':time.monotonic()-start,'log_sha256':sha(raw)};report['steps'].append(row);save();print(json.dumps(row),flush=True);return code
save();codes=[]
codes.append(run('fmt',['cargo','fmt','--all']))
assert codes[-1]==0
codes.append(run('emitter-lib',['taskpolicy','-b','nice','-n','15','cargo','test','--offline','-p','tsc-rs-emitter','--lib']))
codes.append(run('meta-readiness',['python3','scripts/check-meta-property-token-maps-readiness.py'],C))
report['qualified']=not any(codes)
diff=subprocess.check_output(['git','diff','HEAD'],cwd=R);(O/'final-private.diff.gz').write_bytes(gzip.compress(diff,mtime=0));report['final_diff_sha256']=sha(diff);report['source_files']={p:sha((R/p).read_bytes()) for p in subprocess.check_output(['git','diff','HEAD','--name-only'],cwd=R,text=True).splitlines()};save();raise SystemExit(max(codes))
