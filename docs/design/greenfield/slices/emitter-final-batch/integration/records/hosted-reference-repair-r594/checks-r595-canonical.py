from pathlib import Path
import datetime,gzip,hashlib,json,os,subprocess,time
r=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');out=Path(__file__).parent/'checks-r595-canonical';out.mkdir(exist_ok=False)
checks=[('helper-controls',['node','--test','scripts/command-fixture-parents.test.mjs']),('driver-syntax',['bash','-n','scripts/chain-walk.sh']),('helper-metadata',['node','scripts/command-fixture-parents.mjs','--check']),('workspace-audit',['cargo','xtask','workspace-audit']),('planner',['python3','.github/ci/test_replay.py']),('pin-preflight',['python3','scripts/walk-preflight.py']),('qualification-tests',['node','--test','.github/ci/qualification.test.mjs']),('qualification-cli',['node','.github/ci/qualification.mjs','check'])]
env=os.environ.copy();env['CARGO_BUILD_JOBS']='2';env['CARGO_TARGET_DIR']='/Users/hiramatsu/dev/tsc-rs-emitter-final/target'
for name,cmd in checks:
 start=datetime.datetime.now(datetime.timezone.utc).isoformat();t=time.monotonic();log=out/(name+'.log');argv=['taskpolicy','-b','nice','-n','15',*cmd]
 print('START',name,flush=True)
 with log.open('xb') as f:p=subprocess.run(argv,cwd=r,env=env,stdout=f,stderr=subprocess.STDOUT)
 raw=log.read_bytes();(out/(name+'.log.gz')).write_bytes(gzip.compress(raw,mtime=0));receipt={'argv':argv,'cwd':str(r),'started_at':start,'seconds':round(time.monotonic()-t,3),'exit':p.returncode,'log_sha256':hashlib.sha256(raw).hexdigest(),'log_bytes':len(raw)}
 (out/(name+'.json')).write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps(receipt),flush=True);print(raw.decode(errors='replace')[-1700:],flush=True)
 if p.returncode:raise SystemExit(p.returncode)
