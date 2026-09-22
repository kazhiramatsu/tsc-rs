from pathlib import Path
import subprocess,os,importlib.util,json,time,hashlib,sys
r=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');out=Path('/tmp/emitter-final-hosted-failure-r600')
s=importlib.util.spec_from_file_location('foundation',r/'scripts/foundation_witnesses.py');m=importlib.util.module_from_spec(s);s.loader.exec_module(m)
suites=[k for k,v in m.SUITES.items() if v['crate']=='syntax']
env=dict(os.environ,CARGO_BUILD_JOBS='2',CARGO_TARGET_DIR='/Users/hiramatsu/dev/tsc-rs-emitter-final/target',CARGO_TERM_COLOR='never')
for name in ['foundations-normalized.log','native-syntax-before.log']:
 try:m.verify_output(suites,(out/name).read_text())
 except ValueError as e:assert str(e)=='emitter_recovery: missing, ignored, filtered or substituted foundation tests',str(e)
 else:raise AssertionError('Old interleaved output unexpectedly passed')
args=m.command(suites);started=time.monotonic();p=subprocess.run(args,cwd=r,env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
(out/'native-syntax-after.log').write_bytes(p.stdout);assert p.returncode==0,p.stdout[-3000:]
text=p.stdout.decode();passed=m.verify_output(suites,text)
assert 'successes:' in text and 'await flag boundary SUMMARY exact=285 failed=0 selected=285' in text and 'context recovery syntax cases: 788 expected admission=true' in text
try:m.verify_output(suites,text.replace('successes:\n','successes:\ntest fake ... ok\n',1))
except ValueError:pass
else:raise AssertionError('Injected fake status unexpectedly passed')
result={'argv':args,'native_exit':p.returncode,'verified_tests':passed,'seconds':time.monotonic()-started,'log_sha256':hashlib.sha256(p.stdout).hexdigest(),'old_raw_hosted_and_native_refused':True,'injected_status_refused':True}
(out/'native-syntax-after.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result),flush=True)
# Original observers and original runner for all seventeen targets.
started=time.monotonic();args=[sys.executable,'-c',"import sys,os;sys.path.insert(0,'scripts');import foundation_witnesses as m;m.run(list(m.SUITES),os.environ)"]
with (out/'native-foundations-all-after.log').open('wb') as f:p=subprocess.run(args,cwd=r,env=env,stdout=f,stderr=subprocess.STDOUT)
raw=(out/'native-foundations-all-after.log').read_bytes();result={'argv':args,'exit':p.returncode,'seconds':time.monotonic()-started,'log_sha256':hashlib.sha256(raw).hexdigest(),'targets':list(m.SUITES)}
(out/'native-foundations-all-after.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result),flush=True)
if p.returncode:print(raw[-5000:].decode(errors='replace'));sys.exit(p.returncode)
print(raw[-3500:].decode(errors='replace'),flush=True)
