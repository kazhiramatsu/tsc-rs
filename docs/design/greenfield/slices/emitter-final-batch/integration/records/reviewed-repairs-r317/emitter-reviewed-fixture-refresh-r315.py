from pathlib import Path
import json,gzip,hashlib,subprocess,time
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');B=R/'docs/design/greenfield/slices/emitter-final-batch/integration';T=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target');O=T/'emitter-reviewed-fixture-refresh-r315';O.mkdir(exist_ok=False);p=R/'crates/compiler/tests/fixtures/token-comment-phases.json';before=p.read_bytes();old=json.loads(before);assert len(old['cases'])==129;(O/'token-comment-phases-before.json.gz').write_bytes(gzip.compress(before,mtime=0));report={'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip(),'qualified_native':False,'steps':[]}
def save():(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n')
def run(label,args):
 t=time.monotonic();code=subprocess.run(['python3',str(B/'run-local.py'),label,*args],cwd=R).returncode;report['steps'].append({'label':label,'exit':code,'seconds':time.monotonic()-t});save();assert code==0
save();run('import-comment-controls-oracle-r315',['node','scripts/observe-token-comment-phases.mjs','--write']);new=json.loads(p.read_bytes());assert len(new['cases'])==165;byid={c['case_id']:c for c in new['cases']};assert len(byid)==165
for c in old['cases']:assert byid[c['case_id']]==c,c['case_id']
for key,value in old.items():
 if key not in ['cases','observer_sha256']:assert new[key]==value,key
report['old129_unchanged']=True;report['new36_case_ids']=[c['case_id'] for c in new['cases'] if c['case_id'] not in {x['case_id'] for x in old['cases']}];report['new_fixture_sha256']=hashlib.sha256(p.read_bytes()).hexdigest();save()
run('import-helper-retirement-oracle-r315',['node','scripts/observe-import-helpers.mjs','--check']);report['oracle_refresh_complete']=True;save();print(json.dumps(report),flush=True)
