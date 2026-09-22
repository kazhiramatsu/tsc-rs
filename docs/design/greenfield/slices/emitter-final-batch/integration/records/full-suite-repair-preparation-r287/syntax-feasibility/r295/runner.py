from pathlib import Path
import datetime,gzip,hashlib,json,os,subprocess,time
ROOT=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-leading-binding-prototype')
OUT=Path('/tmp/emitter-leading-binding-runtime-r295');OUT.mkdir(exist_ok=False)
HEAD='628562c31a5102348a7e8769f3a2aa88deef70f1'
def git(*args):return subprocess.check_output(['git',*args],cwd=ROOT)
def sha(data):return hashlib.sha256(data).hexdigest()
assert git('rev-parse','HEAD').decode().strip()==HEAD
changes=['crates/syntax/src/recovery/context.rs','crates/syntax/src/recovery.rs','crates/syntax/tests/unit/parser/recovery.rs']
assert set(git('diff','HEAD','--name-only').decode().splitlines())==set(changes)
manifest={'base_head':HEAD,'scope':'isolated small syntax feasibility tests only; canonical628 and its r244 battery untouched; no full command or corpus qualification','changes':changes,'steps':[],'previous_failed_preparation':'r293 wrong edition; r294 unit tests called private parser function with borrowed strings; fixed test inputs only, production283 unchanged'}
def save():(OUT/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
def run(label,args,env=None):
 started=time.monotonic(); rawpath=OUT/(label+'.log')
 with rawpath.open('xb') as f:code=subprocess.run(args,cwd=ROOT,env=env,stdout=f,stderr=subprocess.STDOUT).returncode
 raw=rawpath.read_bytes();(OUT/(label+'.log.gz')).write_bytes(gzip.compress(raw,mtime=0))
 row={'label':label,'argv':args,'exit':code,'seconds':round(time.monotonic()-started,3),'log_sha256':sha(raw)}
 manifest['steps'].append(row);save();print(json.dumps(row),flush=True)
 print(raw.decode(errors='replace')[-6000:],flush=True)
 return code
save()
code=run('rustfmt',['rustfmt','--edition','2021','--config','skip_children=true',*changes]);assert code==0
changed=set(git('diff','HEAD','--name-only').decode().splitlines());assert changed==set(changes),changed
manifest['source_sha256']={p:sha((ROOT/p).read_bytes()) for p in changes};(OUT/'proposal.patch.gz').write_bytes(gzip.compress(git('diff','HEAD','--',*changes),mtime=0));save()
env=os.environ.copy();env['CARGO_BUILD_JOBS']='1';env['CARGO_TARGET_DIR']='/tmp/emitter-leading-binding-r293-target';env['CARGO_NET_OFFLINE']='true'
manifest['test_env']={k:env[k] for k in ['CARGO_BUILD_JOBS','CARGO_TARGET_DIR','CARGO_NET_OFFLINE']};save()
args=['taskpolicy','-b','nice','-n','15','cargo','test','--offline','-p','tsc-rs-syntax','--lib','--config','profile.dev.package.tsc-rs-syntax.opt-level=0','--config','profile.dev.package.tsc-rs-types.opt-level=0','--config','profile.dev.package.tsc-rs-diagnostics.opt-level=0']
code=run('new-composite-unit-tests',args+['leading_invalid_binding_recovery','--','--nocapture','--test-threads=1'],env)
if code:raise SystemExit(code)
code=run('all-recovery-unit-tests',args+['recovery_tests','--','--nocapture','--test-threads=1'],env)
manifest['syntax_tests_passed']=code==0;manifest['full_command_qualified']=False;save();raise SystemExit(code)
