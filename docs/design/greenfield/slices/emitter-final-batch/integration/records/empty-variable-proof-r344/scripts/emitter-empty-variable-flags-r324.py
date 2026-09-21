from pathlib import Path
import json,os,subprocess,time,gzip,hashlib
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-leading-binding-prototype');T=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target');O=T/'emitter-empty-variable-flags-r324';O.mkdir(exist_ok=False);HEAD='32b6dca3a47b3e61f9d70eb912724eb09fcf1f1f';report={'head':HEAD,'status':'waiting original40 qualification r309','steps':[],'qualified':False};sha=lambda b:hashlib.sha256(b).hexdigest()
def save():(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n')
def clean():
 assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip()==HEAD
 assert not subprocess.check_output(['git','diff','HEAD','--name-only'],cwd=R).strip()
 assert not subprocess.check_output(['git','status','--porcelain','--','crates'],cwd=R).strip()
env=os.environ.copy();env['CARGO_BUILD_JOBS']='2';env['CARGO_TARGET_DIR']=str(T)
def run(label,args):
 start=time.monotonic();log=O/(label+'.log')
 with log.open('xb') as f:code=subprocess.run(args,cwd=R,env=env,stdout=f,stderr=subprocess.STDOUT).returncode
 raw=log.read_bytes();(O/(label+'.log.gz')).write_bytes(gzip.compress(raw,mtime=0));report['steps'].append({'label':label,'argv':args,'exit':code,'seconds':time.monotonic()-start,'log_sha256':sha(raw)});save();print(json.dumps(report['steps'][-1]),flush=True);return code
clean();save()
j=json.loads((T/'emitter-leading-binding-successor-r323/manifest.json').read_bytes());assert j['parser_qualified'] and j['head']==HEAD
clean();report['status']='initialized transform flags probe; temporary test-only append';save()
p=R/'crates/emitter/tests/unit/builtins/tests.rs';before=p.read_bytes();append=Path('/tmp/emitter-empty-variable-flags-r311-test.rs').read_bytes();(O/'flags-test-append.rs').write_bytes(append);report['probe_original_file_sha256']=sha(before);save()
try:
 p.write_bytes(before+append)
 changed=subprocess.check_output(['git','diff','HEAD','--name-only'],cwd=R,text=True).splitlines();assert changed==['crates/emitter/tests/unit/builtins/tests.rs'],changed
 (O/'temporary-flags-test.diff.gz').write_bytes(gzip.compress(subprocess.check_output(['git','diff','HEAD','--',str(p)],cwd=R),mtime=0))
 code=run('initialized-flags',['taskpolicy','-b','nice','-n','15','cargo','test','--offline','-p','tsc-rs-emitter','--lib','empty_variable_list_keeps_recovery_and_has_no_binding_pattern_transform_flags','--','--nocapture','--test-threads=1'])
finally:
 assert p.read_bytes()==before+append,'unexpected concurrent edit to own temporary probe file'
 p.write_bytes(before);clean();report['probe_restored']=True;save()
report['qualified']=code==0;report['status']='parse replay and initialized flags completed; ordinary empty-list commands not yet qualified';save();print(json.dumps(report),flush=True);raise SystemExit(code)
