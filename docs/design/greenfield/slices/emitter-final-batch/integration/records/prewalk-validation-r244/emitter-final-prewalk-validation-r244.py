from pathlib import Path
import json,subprocess,sys
r=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');b=r/'docs/design/greenfield/slices/emitter-final-batch/integration';head='628562c31a5102348a7e8769f3a2aa88deef70f1';out=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target/emitter-prewalk-r244');out.mkdir(exist_ok=False);report={'head':head,'qualified':False,'steps':[]}
def save():(out/'manifest.json').write_text(json.dumps(report,indent=2)+'\n')
def check():
 assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=r,text=True).strip()==head
 assert not subprocess.check_output(['git','diff','HEAD','--name-only'],cwd=r).strip()
 assert not subprocess.check_output(['git','status','--porcelain','--','crates'],cwd=r).strip()
compiler=['cargo','test','--offline','-p','tsc-rs-compiler','--no-fail-fast']
for target in ['contracts','h2_6a_map_option_projection','emitter_final_rows','h2_5h_utf16_literal_rows','h2_7e_declaration_maps','h2_7e_declaration_map_apis']:compiler+=['--test',target]
compiler+=['--','--nocapture','--test-threads=1']
steps=[
 ('final-format-r244',['cargo','fmt','--all','--','--check'],True),
 ('final-workspace-clippy-r244',['cargo','clippy','--workspace','--all-targets','--keep-going','--','-D','warnings'],True),
 ('compiler-affected-targets-r244',compiler,False),
 ('harness-all-contracts-r244',['cargo','test','--offline','-p','tsc-rs-harness','--test','contracts','--','--nocapture','--test-threads=1'],False),
 ('xtask-tests-r244',['cargo','test','--offline','-p','tsc-rs-xtask','--bin','xtask','--','--nocapture','--test-threads=1'],False),
 ('baseline-example-build-r244',['cargo','build','--offline','-p','tsc-rs-compiler','--example','h2_baseline_qualification'],False),
 ('final-static-preflight-r244',['python3','scripts/walk-static-checks.py'],False)
]
for label,args,stop in steps:
 check();code=subprocess.run(['python3',str(b/'run-local.py'),label,*args],cwd=r).returncode;check();report['steps'].append({'label':label,'exit':code});save()
 if code and stop:raise SystemExit(code)
report['qualified']=all(x['exit']==0 for x in report['steps']);save();print(json.dumps(report),flush=True)
raise SystemExit(0 if report['qualified'] else 1)
