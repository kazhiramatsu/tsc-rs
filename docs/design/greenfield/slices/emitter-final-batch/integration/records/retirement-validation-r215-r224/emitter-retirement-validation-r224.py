"""Replay the formerly red 36 rows and strict guards after retirement, then lint."""
from pathlib import Path
import argparse,hashlib,json,os,re,shutil,subprocess
p=argparse.ArgumentParser();p.add_argument('--tree',type=Path,required=True);p.add_argument('--head',required=True);a=p.parse_args();r=a.tree.resolve();b=r/'docs/design/greenfield/slices/emitter-final-batch/integration';t=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target');out=t/'emitter-retirement-r224';out.mkdir(exist_ok=False);manifest={'head':a.head,'qualified':False,'steps':[],'artifacts':[]};sha=lambda data:hashlib.sha256(data).hexdigest()
def save():(out/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
def check():
 assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=r,text=True).strip()==a.head
 assert not subprocess.check_output(['git','diff','HEAD','--name-only'],cwd=r).strip()
def run(label,args,env=None):
 check();code=subprocess.run(['python3',str(b/'run-local.py'),label,*map(str,args)],cwd=r,env=env).returncode;check();manifest['steps'].append({'label':label,'exit':code});save()
 if code:raise SystemExit(code)
run('retirement-compiler-build-r224',['cargo','test','--offline','-p','tsc-rs-compiler','--test','emitter_final_universe','--test','transpile_routes_contract','--no-run','--message-format=json'])
artifacts={}
for line in (b/'records/local/retirement-compiler-build-r224.log').read_text().splitlines():
 try:e=json.loads(line)
 except ValueError:continue
 if e.get('reason')!='compiler-artifact' or not e.get('executable') or not e['profile']['test']:continue
 assert e['profile']['test'] and 'tsc-rs-compiler' in e['package_id'];name=e['target']['name'];p=out/name;shutil.copy2(e['executable'],p);os.chmod(p,0o555);item={'target':name,'binary':str(p),'sha256':sha(p.read_bytes())};artifacts[name]=p;manifest['artifacts'].append(item)
assert set(artifacts)=={'emitter_final_universe','transpile_routes_contract'};save()
run('retirement-comparison-guards-r224',[artifacts['emitter_final_universe'],'--exact','known_checker_divergence_rejects_changed_output_and_diagnostics','known_refusal_rejects_changed_error_or_partial_writes','universe_shards_are_disjoint_and_complete','--nocapture','--test-threads=1'])
old=json.loads((b/'records/parse-known-before-retirement-r122.json').read_text())['cases'];assert len(old)==36;seen=set()
for index,row in enumerate(old):
 case=row['case_id'];assert case not in seen;seen.add(case);env=os.environ.copy()
 for key in ['TSC_RS_EMITTER_FINAL_SHARD','TSC_RS_EMITTER_FINAL_CASE_FILTER','TSC_RS_EMITTER_FINAL_CASE_SET']:env.pop(key,None)
 env['TSC_RS_EMITTER_FINAL_CASE_FILTER']=case;env['TSC_RS_EMITTER_FINAL_CASE_SET']='all';env['TSC_RS_EMITTER_FINAL_CAPTURE_DIR']=str(out/'captures');env['TSC_RS_EMITTER_FINAL_FAILURE_DIR']=str(out/'failures')
 target='plan_base_rows_match_complete_production_commands' if 'plan-base' in row['fixture'] else 'universe_rows_match_complete_production_commands';label=f'retired-parse-{index:02}-r224';run(label,[artifacts['emitter_final_universe'],'--exact',target,'--nocapture','--test-threads=1'],env)
 log=(b/f'records/local/{label}.log').read_text();assert len(re.findall(r'selected 1/(?:217|1798) / exact 1 / known 0 / failed 0',log))==1;manifest['steps'][-1]['case_id']=case;save()
prior=r/'target/h2-8c'
if prior.exists():shutil.copytree(prior,out/'prior-transpile-evidence')
run('retired-transpile-r224',[artifacts['transpile_routes_contract'],'--test-threads=1'])
shutil.copytree(prior,out/'transpile-evidence')
run('final-source-clippy-r224',['cargo','clippy','--workspace','--all-targets','--','-D','warnings'])
for item in manifest['artifacts']:assert sha(Path(item['binary']).read_bytes())==item['sha256']
manifest['qualified']=True;save();print(json.dumps(manifest['steps']),flush=True)
