from pathlib import Path
import json,os,subprocess,time,gzip,hashlib
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-leading-binding-prototype')
C=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep')
T=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target')
O=T/'emitter-leading-binding-full-command-r332';O.mkdir(exist_ok=False)
HEAD='4aeb1640156f291f4b5189b8ad9ed370cf17cf9d'
sha=lambda b:hashlib.sha256(b).hexdigest()
def git(*args):return subprocess.check_output(['git',*args],cwd=R)
assert git('rev-parse','HEAD').decode().strip()==HEAD
assert not git('diff','HEAD','--name-only').strip()
proof=json.loads((T/'emitter-combined-recovery-successor-r330/manifest.json').read_bytes())
assert proof['head']==HEAD and proof['parser_qualified']
paths=['crates/compiler/tests/fixtures/export-name-syntax-maps.json','crates/compiler/tests/integration/h2_8a_export_name_syntax_maps.rs']
before={p:(R/p).read_bytes() for p in paths};after={p:(C/p).read_bytes() for p in paths}
assert len(json.loads(after[paths[0]])['cases'])==80
report={'head':HEAD,'syntax_tree':git('rev-parse','HEAD:crates/syntax').decode().strip(),'status':'new80 complete commands twice; test-only fixture copies from official canonical mint','fixture_sources':{p:sha(data) for p,data in after.items()},'qualified':False,'steps':[]}
def save():(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n')
save();env=os.environ.copy();env.update(CARGO_BUILD_JOBS='2',CARGO_TARGET_DIR=str(T))
try:
 for p,data in after.items():(R/p).write_bytes(data)
 assert set(git('diff','HEAD','--name-only').decode().splitlines())==set(paths)
 (O/'test-only.diff.gz').write_bytes(gzip.compress(git('diff','HEAD'),mtime=0))
 args=['taskpolicy','-b','nice','-n','15','cargo','test','--offline','-p','tsc-rs-compiler','--test','contracts','h2_8a_export_name_syntax_maps::export_name_syntax_maps_match_complete_typescript_observations','--','--exact','--nocapture','--test-threads=1']
 start=time.monotonic();log=O/'native80.log'
 with log.open('xb') as f:code=subprocess.run(args,cwd=R,env=env,stdout=f,stderr=subprocess.STDOUT).returncode
 raw=log.read_bytes();(O/'native80.log.gz').write_bytes(gzip.compress(raw,mtime=0));report['steps'].append({'argv':args,'exit':code,'seconds':time.monotonic()-start,'log_sha256':sha(raw)});report['qualified']=code==0;save()
finally:
 for p,data in before.items():
  assert (R/p).read_bytes()==after[p],('concurrent edit',p)
  (R/p).write_bytes(data)
 assert not git('diff','HEAD','--name-only').strip()
 report['test_files_restored']=True;save()
print(json.dumps(report),flush=True);raise SystemExit(code)
