from pathlib import Path
import hashlib,json,os,shutil,subprocess,time
root=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep')
base=root/'docs/design/greenfield/slices/emitter-final-batch/integration';target=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target')
head=Path('/tmp/emitter-corpus-controls-r203-head').read_text().strip()
print('Wait for r203 controls before native corpus/project comparison; candidate stays frozen.',flush=True)
while not (base/'records/local/corpus-controls-pipeline-r203.json').exists():time.sleep(10)
receipt=json.loads((base/'records/local/keyword-successor-pipeline-r185.json').read_text());assert receipt['exit']==0,receipt
prior=json.loads((target/'emitter-keyword-successor-r185/manifest.json').read_text());assert prior['head']=='3e28cb21344eae8980cfe66992913f2d8758ede2' and prior['parser_qualified']
selection_proof=json.loads((target/'emitter-keyword-successor-r185/selection.json').read_text())
assert subprocess.check_output(['git','rev-parse','HEAD:crates/syntax'],cwd=root,text=True).strip()==selection_proof['successor']['syntax_tree_hash']
controls=json.loads((target/'emitter-corpus-controls-r203/manifest.json').read_text());assert controls['head']==head
assert controls['qualified'], 'r203 controls must qualify before the final corpus/project rerun'
assert next(s['exit'] for s in controls['steps'] if s['label']=='corpus-controls-compiler-build-r203')==0
out=target/'emitter-corpus-candidate-r204';out.mkdir(exist_ok=False)
manifest={'head':head,'qualified':False,'steps':[],'artifacts':[]}
sha=lambda b:hashlib.sha256(b).hexdigest()
def save():(out/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
def check():
 assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()==head
 assert not subprocess.check_output(['git','diff','HEAD','--name-only'],cwd=root).strip()
def run(label,args,env=None):
 check();code=subprocess.run(['python3',str(base/'run-local.py'),label,*map(str,args)],cwd=root,env=env).returncode;check()
 manifest['steps'].append({'label':label,'exit':code});save()
 if code:raise SystemExit(code)
def collect(label,test):
 found=[]
 for line in (base/f'records/local/{label}.log').read_text().splitlines():
  try:event=json.loads(line)
  except json.JSONDecodeError:continue
  if event.get('reason')=='compiler-artifact' and event.get('executable') and event.get('profile',{}).get('test')==test:
   assert 'tsc-rs-xtask' in event['package_id'];binary=out/('xtask-tests' if test else 'xtask');shutil.copy2(event['executable'],binary);os.chmod(binary,0o555);found.append(binary);manifest['artifacts'].append({'path':str(binary),'sha256':sha(binary.read_bytes())})
 assert len(found)==1;save();return found[0]
# Functional evidence only: align dev debug info with the repository test profile.
env=os.environ.copy();env['CARGO_PROFILE_DEV_DEBUG']='0'
run('corpus-xtask-build-r204',['cargo','build','--offline','-p','tsc-rs-xtask','--bin','xtask','--message-format=json'],env)
native=collect('corpus-xtask-build-r204',False)
run('corpus-xtask-tests-build-r204',['cargo','test','--offline','-p','tsc-rs-xtask','--bin','xtask','--no-run','--message-format=json'],env)
tests=collect('corpus-xtask-tests-build-r204',True)
run('corpus-native-guards-r204',[tests,'recovery_corpus_native::','--nocapture','--test-threads=1'])
selection=target/'emitter-keyword-successor-r185/selection.json';oracle=out/'oracle.json'
run('corpus-selected-oracle-r204',['node','scripts/observe-recovery-selected-corpus.mjs',selection,oracle,'/Users/hiramatsu/dev/tsc-rs-emitter-final-census'])
run('corpus-native-selected-r204',[native,'recovery-corpus-native',selection,out/'native.json','/Users/hiramatsu/dev/tsc-rs-emitter-final-census'])
run('corpus-comparison-r204',['python3','scripts/compare-recovery-selected-corpus.py','--selection',selection,'--native',out/'native.json','--oracle',oracle,'--out',out/'comparison.json'])
roster=base/'records/project-projection-roster-r139.json'
run('project-native-108-r204',[native,'project-command-supplement',roster,out/'project-native.json','/Users/hiramatsu/dev/tsc-rs-emitter-final-census'])
run('project-oracle-108-r204',['node','scripts/observe-project-command-supplement.mjs',out/'project-native.json',roster,out/'project-oracle.json'])
run('project-comparison-108-r204',['python3','scripts/compare-project-command-supplement.py','--native',out/'project-native.json','--oracle',out/'project-oracle.json','--out',out/'project-comparison.json'])
for a in manifest['artifacts']:assert sha(Path(a['path']).read_bytes())==a['sha256']
manifest['qualified']=True;save();print(json.dumps(manifest['steps']),flush=True)
