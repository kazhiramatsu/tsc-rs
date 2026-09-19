"""Full original EF7 comparisons at frozen r120 source; keep stale-KNOWN failures visible."""
from pathlib import Path
import hashlib,json,os,re,subprocess
root=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-recovery-next')
base=root/'docs/design/greenfield/slices/emitter-final-batch/integration'
target=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target')
source_head='b451489e4a18abbff42651d8eb814537f4c5f800'
known=json.loads((base/'records/local/census-known-build-pause-r122.json').read_text())
binary=Path(known['binary']['immutable']);digest=known['binary']['sha256']
assert known['head']==source_head
record=base/'records/local/universe-full-driver-r124.json';assert not record.exists()
state={'head':source_head,'binary':known['binary'],'performance_qualification':False,'stale_known_retirement':'pending corpus proof','steps':[]}
def check():
 assert subprocess.check_output(['git','-C',str(root),'rev-parse','HEAD'],text=True).strip()==source_head
 assert not subprocess.check_output(['git','-C',str(root),'diff','HEAD','--name-only']).strip()
 assert hashlib.sha256(binary.read_bytes()).hexdigest()==digest
for label,test,count,stale_count in [
 ('universe-217-r124','universe_rows_match_complete_production_commands',217,1),
 ('plan-base-1798-r124','plan_base_rows_match_complete_production_commands',1798,35),
]:
 check()
 env=os.environ.copy()
 for key in ['TSC_RS_EMITTER_FINAL_SHARD','TSC_RS_EMITTER_FINAL_CASE_FILTER','TSC_RS_EMITTER_FINAL_CASE_SET']:
  env.pop(key,None)
 directory=target/(label+'-differences');assert not directory.exists()
 env['TSC_RS_EMITTER_FINAL_FAILURE_DIR']=str(directory)
 code=subprocess.run(['python3',str(base/'run-local.py'),label,str(binary),'--exact',test,'--nocapture','--test-threads=1'],cwd=root,env=env).returncode
 check()
 log=(base/f'records/local/{label}.log').read_text()
 ids=re.findall(r'emitter-final universe EXACT x2 (\S+)',log)
 stales=re.findall(r'KNOWN rows replay exact now; retire them: (\[.*\])',log)
 step={'label':label,'raw_test_exit':code,'exact_x2':len(ids),'expected_cases':count,'new_divergences':log.count('DIVERGING'),'stale_guard_rows':len(json.loads(stales[0])) if len(stales)==1 else None}
 state['steps'].append(step);record.write_text(json.dumps(state,indent=2)+'\n')
 assert code==101 and len(ids)==len(set(ids))==count and step['new_divergences']==0,step
 assert log.count('panicked at')==1 and len(stales)==1 and len(json.loads(stales[0]))==stale_count,step
 assert not directory.exists(), 'unexpected difference captures'
 print(json.dumps(step),flush=True)
state['finished']='Both full comparison sets exact twice; Rust tests remain red only for stale KNOWN. Corpus proof and retirement still required.'
record.write_text(json.dumps(state,indent=2)+'\n')
print(json.dumps(state),flush=True)
raise SystemExit(101)
