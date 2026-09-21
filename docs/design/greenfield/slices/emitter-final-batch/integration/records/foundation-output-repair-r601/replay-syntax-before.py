from pathlib import Path
import subprocess,os,importlib.util,json,time,hashlib
r=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');out=Path('/tmp/emitter-final-hosted-failure-r600')
s=importlib.util.spec_from_file_location('foundation',r/'scripts/foundation_witnesses.py');m=importlib.util.module_from_spec(s);s.loader.exec_module(m)
suites=[k for k,v in m.SUITES.items() if v['crate']=='syntax'];args=['taskpolicy','-b','nice','-n','15',*m.command(suites)]
env=dict(os.environ,CARGO_BUILD_JOBS='2',CARGO_TARGET_DIR='/Users/hiramatsu/dev/tsc-rs-emitter-final/target',CARGO_TERM_COLOR='never')
started=time.monotonic();p=subprocess.run(args,cwd=r,env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
(out/'native-syntax-before.log').write_bytes(p.stdout);assert p.returncode==0,p.stdout[-3000:]
try:m.verify_output(suites,p.stdout.decode())
except ValueError as e:error=str(e)
else:raise AssertionError('Expected interleaved-output refusal from the original command')
assert error=='emitter_recovery: missing, ignored, filtered or substituted foundation tests',error
result={'argv':args,'native_exit':p.returncode,'verifier_error':error,'seconds':time.monotonic()-started,'log_sha256':hashlib.sha256(p.stdout).hexdigest(),'scope':'Actual complete syntax batch on unchanged Rust; native succeeded, original output verifier refused'}
(out/'native-syntax-before.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result),flush=True)
