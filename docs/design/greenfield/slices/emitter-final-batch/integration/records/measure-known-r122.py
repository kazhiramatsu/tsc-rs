"""Measure the 36 existing KNOWN rows at immutable r120 source, without retiring them."""
from pathlib import Path
import datetime, hashlib, json, os, re, shutil, signal, subprocess

root=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-recovery-next')
base=root/'docs/design/greenfield/slices/emitter-final-batch/integration'
target=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target')
expected_head='b451489e4a18abbff42651d8eb814537f4c5f800'
head=lambda:subprocess.check_output(['git','-C',str(root),'rev-parse','HEAD'],text=True).strip()
assert head()==expected_head
assert not subprocess.check_output(['git','-C',str(root),'diff','HEAD','--name-only']).strip()
manifest=json.loads((target/'emitter-census-r78-immutable/manifest.json').read_text())
native=json.loads((target/'emitter-r120-immutable/manifest.json').read_text())
record=base/'records/local/census-known-build-pause-r122.json'
assert not record.exists()
state={'head':expected_head,'protocol':'Only census pauses during serial Cargo build. Other immutable native commands are functional comparisons, not performance qualification. No source/HEAD changes or xtask build. KNOWN retirement remains pending full corpus proof.'}
now=lambda:datetime.datetime.now(datetime.timezone.utc).isoformat()
save=lambda:record.write_text(json.dumps(state,indent=2)+'\n')
def verify():
 assert hashlib.sha256(Path(manifest['original_executable']).read_bytes()).hexdigest()==manifest['sha256']
 mapped=subprocess.check_output(['lsof','-a','-p','56011','-d','txt','-Fni'],text=True)
 assert f"i{manifest['original_inode']}\nn{manifest['original_executable']}\n" in mapped
 assert hashlib.sha256(Path(native['immutable_executable']).read_bytes()).hexdigest()==native['sha256']
def run(label,args,env=None):
 assert head()==expected_head
 assert not subprocess.check_output(['git','-C',str(root),'diff','HEAD','--name-only']).strip()
 return subprocess.run(['python3',str(base/'run-local.py'),label,*map(str,args)],cwd=root,env=env).returncode
verify();os.kill(56011,signal.SIGSTOP);state['paused_at']=now();save()
try:
 code=run('compiler-universe-build-r122',['cargo','test','--offline','-p','tsc-rs-compiler','--test','emitter_final_universe','--no-run'])
finally:
 verify();os.kill(56011,signal.SIGCONT);state['resumed_at']=now();save()
if code:raise SystemExit(code)
log=(base/'records/local/compiler-universe-build-r122.log').read_text()
matches=re.findall(r'Executable tests/emitter_final_universe.rs \(([^)]+)\)',log)
assert len(matches)==1,matches
source=Path(matches[0]);out=target/'emitter-r122-known-immutable';out.mkdir(exist_ok=False)
binary=out/'emitter_final_universe';shutil.copy2(source,binary);os.chmod(binary,0o555)
digest=hashlib.sha256(binary.read_bytes()).hexdigest()
state['binary']={'original':str(source),'immutable':str(binary),'sha256':digest};save()
env=os.environ.copy()
for key in ['TSC_RS_EMITTER_FINAL_SHARD','TSC_RS_EMITTER_FINAL_CASE_FILTER']:
 env.pop(key,None)
env['TSC_RS_EMITTER_FINAL_CASE_SET']='known'
failure=target/'emitter-known-r122-differences';assert not failure.exists()
env['TSC_RS_EMITTER_FINAL_FAILURE_DIR']=str(failure)
code=run('emitter-known-36-r122',[binary,'--exact','universe_rows_match_complete_production_commands','plan_base_rows_match_complete_production_commands','--nocapture','--test-threads=1'],env)
assert hashlib.sha256(binary.read_bytes()).hexdigest()==digest
state['test_exit']=code;state['finished_at']=now();save()
print(json.dumps(state),flush=True)
raise SystemExit(code)
