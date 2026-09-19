from pathlib import Path
import json,os,subprocess,time,gzip,hashlib
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-leading-binding-prototype');T=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target');O=T/'emitter-leading-binding-full-command-r309';O.mkdir(exist_ok=False);HEAD='2df8d1cd0bae5a26efd1d4268bab4cc7cfd9cc12';report={'head':HEAD,'status':'waiting for frozen244 and304 archival','qualified':False,'steps':[]}
def save():(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n')
def check():
 assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip()==HEAD
 assert not subprocess.check_output(['git','diff','HEAD','--name-only'],cwd=R).strip()
 assert not subprocess.check_output(['git','status','--porcelain','--','crates'],cwd=R).strip()
check();save()
while True:
 p=T/'emitter-postprewalk-r304/manifest.json'
 if p.exists() and json.loads(p.read_text())['status'].startswith('completed;'):break
 time.sleep(10)
check();assert (R/'vendor/typescript-6.0.3/lib/lib.es5.d.ts').is_file();env=os.environ.copy();env['CARGO_BUILD_JOBS']='2';env['CARGO_TARGET_DIR']=str(T);report['status']='running normal-profile original40 whole commands x2';save()
args=['taskpolicy','-b','nice','-n','15','cargo','test','--offline','-p','tsc-rs-compiler','--test','contracts','h2_8a_export_name_syntax_maps::export_name_syntax_maps_match_complete_typescript_observations','--','--exact','--nocapture','--test-threads=1'];log=O/'original40.log';start=time.monotonic()
with log.open('xb') as f:code=subprocess.run(args,cwd=R,env=env,stdout=f,stderr=subprocess.STDOUT).returncode
check();raw=log.read_bytes();(O/'original40.log.gz').write_bytes(gzip.compress(raw,mtime=0));report['steps'].append({'label':'original40','argv':args,'exit':code,'seconds':time.monotonic()-start,'log_sha256':hashlib.sha256(raw).hexdigest()});report['qualified']=code==0;report['status']='original40 completed; parse proof and additional controls remain';save();print(json.dumps(report),flush=True);raise SystemExit(code)
