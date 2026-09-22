from pathlib import Path
import json,subprocess,shutil,os,hashlib
root=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-recovery-next');base=root/'docs/design/greenfield/slices/emitter-final-batch/integration'
out=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target/emitter-noemit-r153');out.mkdir(exist_ok=False)
head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip();assert head.startswith('42f9ccefc')
def run(label,args,env=None):
 assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()==head
 assert not subprocess.check_output(['git','diff','HEAD','--name-only'],cwd=root).strip()
 return subprocess.run(['python3',str(base/'run-local.py'),label,*map(str,args)],cwd=root,env=env).returncode
label='noemit-build-r153'
code=run(label,['cargo','test','--offline','-p','tsc-rs-compiler','--test','emitter_final_batch','--no-run','--message-format=json'])
if code:raise SystemExit(code)
artifacts=[]
for line in (base/f'records/local/{label}.log').read_text().splitlines():
 try:event=json.loads(line)
 except json.JSONDecodeError:continue
 if event.get('reason')=='compiler-artifact' and event.get('executable') and event.get('profile',{}).get('test'):
  binary=out/'emitter-final-batch';shutil.copy2(event['executable'],binary);os.chmod(binary,0o555);artifacts.append({'path':str(binary),'sha256':hashlib.sha256(binary.read_bytes()).hexdigest()})
assert len(artifacts)==1
(out/'manifest.json').write_text(json.dumps({'head':head,'artifacts':artifacts},indent=2)+'\n')
env=os.environ.copy();env['TSC_RS_EMITTER_FINAL_CAPTURE_DIR']=str(out/'captures')
code=run('noemit-native-r153',[binary,'--exact','no_emit_census_rows_match_complete_production_commands','--nocapture','--test-threads=1'],env)
raise SystemExit(code)
