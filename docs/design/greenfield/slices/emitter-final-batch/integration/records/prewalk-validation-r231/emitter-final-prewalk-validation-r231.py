from pathlib import Path
import json,subprocess
r=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');b=r/'docs/design/greenfield/slices/emitter-final-batch/integration';head='5bc9fc0fd60710667efee8d4abeccccae3faa5bd';out=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target/emitter-prewalk-r231');out.mkdir(exist_ok=False);report={'head':head,'qualified':False,'steps':[]}
def save():(out/'manifest.json').write_text(json.dumps(report,indent=2)+'\n')
def check():
 assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=r,text=True).strip()==head
 assert not subprocess.check_output(['git','diff','HEAD','--name-only'],cwd=r).strip()
for label,args in [
 ('final-format-r231',['cargo','fmt','--all','--','--check']),
 ('class-arrow-recovery-test-r231',['cargo','test','--offline','-p','tsc-rs-syntax','--lib','class_member_','--','--nocapture','--test-threads=1']),
 ('final-workspace-clippy-r231',['cargo','clippy','--workspace','--all-targets','--','-D','warnings']),
 ('final-static-preflight-r231',['python3','scripts/walk-static-checks.py'])
]:
 check();code=subprocess.run(['python3',str(b/'run-local.py'),label,*args],cwd=r).returncode;check();report['steps'].append({'label':label,'exit':code});save()
 if code:raise SystemExit(code)
 if label=='class-arrow-recovery-test-r231':assert '2 passed; 0 failed; 0 ignored;' in (b/f'records/local/{label}.log').read_text()
report['qualified']=True;save();print(json.dumps(report),flush=True)
