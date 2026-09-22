from pathlib import Path
import json,subprocess
r=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');b=r/'docs/design/greenfield/slices/emitter-final-batch/integration';head='807bd4718c356631c6a175f06b29aa782b2507e1';out=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target/emitter-prewalk-r234');out.mkdir(exist_ok=False);report={'head':head,'qualified':False,'steps':[]}
def save():(out/'manifest.json').write_text(json.dumps(report,indent=2)+'\n')
def check():
 assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=r,text=True).strip()==head
 assert not subprocess.check_output(['git','diff','HEAD','--name-only'],cwd=r).strip()
for label,args in [
 ('final-format-r234',['cargo','fmt','--all','--','--check']),
 ('syntax-emitter-recovery-r234',['env','TSC_RS_RECOVERY_FACTS_OUTPUT='+str(out/'recovery-facts.json'),'cargo','test','--offline','-p','tsc-rs-syntax','--test','emitter_recovery','--','--nocapture','--test-threads=1']),
 ('final-workspace-clippy-r234',['cargo','clippy','--workspace','--all-targets','--keep-going','--','-D','warnings']),
 ('final-static-preflight-r234',['python3','scripts/walk-static-checks.py'])
]:
 check();code=subprocess.run(['python3',str(b/'run-local.py'),label,*args],cwd=r).returncode;check();report['steps'].append({'label':label,'exit':code});save()
 if code:raise SystemExit(code)
 if label=='syntax-emitter-recovery-r234':
  log=(b/f'records/local/{label}.log').read_text()
  for expected in ['2 passed; 0 failed; 0 ignored;','emitter recovery syntax SUMMARY exact=36 failed=0 selected=36','await flag boundary SUMMARY exact=285 failed=0 selected=285','context recovery syntax cases: 432 expected admission=true','context recovery syntax refused_cases: 72 expected admission=false']:assert expected in log,expected
report['qualified']=True;save();print(json.dumps(report),flush=True)
