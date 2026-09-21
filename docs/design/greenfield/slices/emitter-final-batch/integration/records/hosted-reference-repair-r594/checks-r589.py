from pathlib import Path
import datetime,gzip,hashlib,json,os,subprocess,time
r=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');out=Path(__file__).parent/'checks-r589';out.mkdir(exist_ok=False)
checks=[('factory-boundaries', ['cargo', 'test', '--offline', '-p', 'tsc-rs-emitter', '--lib', 'factory::original_provenance_tests::access_parenthesization_matches_typescript_kind_and_range_boundaries', '--', '--exact', '--nocapture', '--test-threads=1']), ('import-original', ['cargo', 'xtask', 'h2-5g-probe', '--indices', '8368']), ('heritage-complete', ['cargo', 'test', '--offline', '-p', 'tsc-rs-compiler', '--test', 'contracts', 'emitter_residual_audit::heritage_factory_boundaries_match_complete_typescript_commands', '--', '--exact', '--nocapture', '--test-threads=1']), ('foundation-bundle', ['python3', 'scripts/witness.py', 'program-bundle-facts', '--all']), ('resolution-cache', ['python3', 'scripts/witness.py', 'resolution-cache', '--all']), ('parameter-temporaries', ['python3', 'scripts/witness.py', 'parameter-temporaries', '--all']), ('utf16-original', ['python3', 'scripts/witness.py', 'utf16-original-commands', '--all']), ('jsdoc-original', ['cargo', 'test', '--offline', '-p', 'tsc-rs-compiler', '--test', 'contracts', 'emitter_residual_audit::jsdoc_original_command_matches_complete_typescript_observations', '--', '--exact', '--nocapture', '--test-threads=1'])]
env=os.environ.copy();env['CARGO_BUILD_JOBS']='2';env['CARGO_TARGET_DIR']='/Users/hiramatsu/dev/tsc-rs-emitter-final/target'
for name,cmd in checks:
 start=datetime.datetime.now(datetime.timezone.utc).isoformat();t=time.monotonic();log=out/(name+'.log');argv=['taskpolicy','-b','nice','-n','15',*cmd]
 print('START',name,flush=True)
 with log.open('xb') as f:p=subprocess.run(argv,cwd=r,env=env,stdout=f,stderr=subprocess.STDOUT)
 raw=log.read_bytes();(out/(name+'.log.gz')).write_bytes(gzip.compress(raw,mtime=0));receipt={'argv':argv,'cwd':str(r),'started_at':start,'seconds':round(time.monotonic()-t,3),'exit':p.returncode,'log_sha256':hashlib.sha256(raw).hexdigest(),'log_bytes':len(raw)}
 (out/(name+'.json')).write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps(receipt),flush=True);print('\n'.join(x[:750] for x in raw.decode(errors='replace').splitlines()[-10:]),flush=True)
 if p.returncode:raise SystemExit(p.returncode)
