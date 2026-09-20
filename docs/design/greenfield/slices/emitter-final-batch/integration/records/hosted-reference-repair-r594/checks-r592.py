from pathlib import Path
import datetime,gzip,hashlib,json,os,subprocess,time
r=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');out=Path(__file__).parent/'checks-r592';out.mkdir(exist_ok=False)
checks=[('controls-full', ['python3', '.github/ci/replay.py', 'witnesses']), ('system-full', ['python3', 'scripts/witness.py', 'emitter-system-controls', '--all'])]
env=os.environ.copy();env['CARGO_BUILD_JOBS']='2';env['CARGO_TARGET_DIR']='/Users/hiramatsu/dev/tsc-rs-emitter-final/target'
env['WITNESS_SUITES']='["extra", "followup", "followup2", "followup3", "direct", "bundle-sinks", "declaration-map-cli", "module-identities", "bundle-original-javascript", "bundle-program", "bundle-declarations", "utf16-recovery-corpus", "map-option-projection", "config-library", "prologue-comments", "literal-update-pipeline", "declaration-comments", "jsdoc-return", "bundle-metadata-t1", "parameter-temporaries", "transpile-routes", "utf16-identity-recovery", "utf16-review-fix", "utf16-tagged-template", "utf16-literal-witnesses", "utf16-original-commands", "resolution-cache"]'
failed=[]
for name,cmd in checks:
 start=datetime.datetime.now(datetime.timezone.utc).isoformat();t=time.monotonic();log=out/(name+'.log');argv=['taskpolicy','-b','nice','-n','15',*cmd]
 print('START',name,flush=True)
 with log.open('xb') as f:p=subprocess.run(argv,cwd=r,env=env,stdout=f,stderr=subprocess.STDOUT)
 raw=log.read_bytes();(out/(name+'.log.gz')).write_bytes(gzip.compress(raw,mtime=0));receipt={'argv':argv,'cwd':str(r),'started_at':start,'seconds':round(time.monotonic()-t,3),'exit':p.returncode,'log_sha256':hashlib.sha256(raw).hexdigest(),'log_bytes':len(raw)}
 (out/(name+'.json')).write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps(receipt),flush=True);print('\n'.join(x[:750] for x in raw.decode(errors='replace').splitlines()[-10:]),flush=True)
 if p.returncode:failed.append((name,p.returncode))

print(json.dumps({"failed":failed}),flush=True)
raise SystemExit(1 if failed else 0)
