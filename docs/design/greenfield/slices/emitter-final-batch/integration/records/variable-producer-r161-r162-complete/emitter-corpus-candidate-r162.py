from pathlib import Path
import hashlib,json,os,shutil,subprocess,time
root=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep')
base=root/'docs/design/greenfield/slices/emitter-final-batch/integration';target=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target')
head=Path('/tmp/emitter-variable-producer-r161-head').read_text().strip()
def alive(pid):
 try:os.kill(pid,0)
 except ProcessLookupError:return False
 return True
print('Wait for r161; keep the same candidate frozen through this proof.',flush=True)
while not (base/'records/local/variable-producer-pipeline-r161.json').exists():time.sleep(15)
prior=json.loads((target/'emitter-variable-type-r161-native/manifest.json').read_text())
assert prior['head']==head
steps={s['label']:s for s in prior['steps']}
for label in ['variable-type-native-r161','variable-comma-native-r161','variable-type-neighbours-r161']:assert steps[label]['exit']==0
assert len([s for s in prior['steps'] if s['label'].startswith('variable-emitter-') and s['exit']==0])==23
assert all(s['exit']==0 for s in prior['steps'] if s['label']!='variable-type-transpile-r161')
# An old KNOWN retire assertion cannot conceal a new native difference.
for p in (target/'emitter-variable-type-r161-native/transpile-evidence').glob('native-*.json'):
 d=json.loads(p.read_text())
 if 'unexpected_open' in d:assert not d['unexpected_open']
out=target/'emitter-corpus-candidate-r162';out.mkdir(exist_ok=False)
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
run('corpus-xtask-build-r162',['cargo','build','--offline','-p','tsc-rs-xtask','--bin','xtask','--message-format=json'],env)
native=collect('corpus-xtask-build-r162',False)
run('corpus-xtask-tests-build-r162',['cargo','test','--offline','-p','tsc-rs-xtask','--bin','xtask','--no-run','--message-format=json'],env)
tests=collect('corpus-xtask-tests-build-r162',True)
run('corpus-native-guards-r162',[tests,'recovery_corpus_native::','--nocapture','--test-threads=1'])
selection=target/'emitter-variable-successor-r151/selection.json';oracle=target/'emitter-variable-successor-r151/oracle-r155.json';assert oracle.exists()
run('corpus-native-selected-r162',[native,'recovery-corpus-native',selection,out/'native.json','/Users/hiramatsu/dev/tsc-rs-emitter-final-census'])
run('corpus-comparison-r162',['python3','scripts/compare-recovery-selected-corpus.py','--selection',selection,'--native',out/'native.json','--oracle',oracle,'--out',out/'comparison.json'])
roster=base/'records/project-projection-roster-r139.json'
run('project-native-108-r162',[native,'project-command-supplement',roster,out/'project-native.json','/Users/hiramatsu/dev/tsc-rs-emitter-final-census'])
run('project-oracle-108-r162',['node','scripts/observe-project-command-supplement.mjs',out/'project-native.json',roster,out/'project-oracle.json'])
run('project-comparison-108-r162',['python3','scripts/compare-project-command-supplement.py','--native',out/'project-native.json','--oracle',out/'project-oracle.json','--out',out/'project-comparison.json'])
for a in manifest['artifacts']:assert sha(Path(a['path']).read_bytes())==a['sha256']
manifest['qualified']=True;save();print(json.dumps(manifest['steps']),flush=True)
