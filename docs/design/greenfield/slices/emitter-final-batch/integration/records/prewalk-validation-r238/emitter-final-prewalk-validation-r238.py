from pathlib import Path
import json,subprocess
r=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');b=r/'docs/design/greenfield/slices/emitter-final-batch/integration';head='34dea8e69de2219e9f1f9b1649c610e0b18938bc';out=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target/emitter-prewalk-r238');out.mkdir(exist_ok=False);report={'head':head,'qualified':False,'steps':[]}
def save():(out/'manifest.json').write_text(json.dumps(report,indent=2)+'\n')
def check():
 assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=r,text=True).strip()==head
 assert not subprocess.check_output(['git','diff','HEAD','--name-only'],cwd=r).strip()
for label,args,count in [
 ('final-format-r238',['cargo','fmt','--all','--','--check'],None),
 ('template-flags-r238',['cargo','test','--offline','-p','tsc-rs-syntax','--test','template_escape_flags','--','--nocapture','--test-threads=1'],5),
 ('host-path-contracts-r238',['cargo','test','--offline','-p','tsc-rs-host','--test','compiler_host_contract','--','--nocapture','--test-threads=1'],14),
 ('project-loader-contracts-r238',['cargo','test','--offline','-p','tsc-rs-harness','--test','contracts','h2_5h_project_emit::','--','--nocapture','--test-threads=1'],4),
 ('final-workspace-clippy-r238',['cargo','clippy','--workspace','--all-targets','--keep-going','--','-D','warnings'],None),
 ('final-static-preflight-r238',['python3','scripts/walk-static-checks.py'],None)
]:
 check();code=subprocess.run(['python3',str(b/'run-local.py'),label,*args],cwd=r).returncode;check();report['steps'].append({'label':label,'exit':code});save()
 if code:raise SystemExit(code)
 if count is not None:assert f'{count} passed; 0 failed; 0 ignored;' in (b/f'records/local/{label}.log').read_text()
report['qualified']=True;save();print(json.dumps(report),flush=True)
