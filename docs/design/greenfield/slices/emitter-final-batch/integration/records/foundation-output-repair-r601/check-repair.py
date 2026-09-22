from pathlib import Path
import subprocess,json,time,hashlib,sys
r=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');out=Path('/tmp/emitter-final-hosted-failure-r600')
commands=[('planner',[sys.executable,'.github/ci/test_replay.py','-v']),('all-pins',[sys.executable,'scripts/walk-preflight.py']),('qualification-tests',['node','--test','.github/ci/qualification.test.mjs','.github/ci/slice-readiness.test.mjs','.github/ci/bounded-artifact-schemas.test.mjs','.github/ci/h2-8a-artifact-schemas.test.mjs','.github/ci/fci-h2-5g-membership.test.mjs','.github/ci/gate-tax-5.test.mjs']),('qualification-contracts',['node','.github/ci/qualification.mjs','check'])]
for label,args in commands:
 started=time.monotonic()
 with (out/(label+'.log')).open('wb') as f:p=subprocess.run(args,cwd=r,stdout=f,stderr=subprocess.STDOUT)
 raw=(out/(label+'.log')).read_bytes();record={'argv':args,'exit':p.returncode,'seconds':time.monotonic()-started,'log_sha256':hashlib.sha256(raw).hexdigest()}
 (out/(label+'.json')).write_text(json.dumps(record,indent=2)+'\n');print(json.dumps({'label':label,**record}),flush=True);print(raw[-1500:].decode(errors='replace'),flush=True)
 if p.returncode:sys.exit(p.returncode)
