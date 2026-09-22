from pathlib import Path
import hashlib,json,os,shutil,subprocess
root=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');base=root/'docs/design/greenfield/slices/emitter-final-batch/integration';target=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target')
head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip();assert head == Path('/tmp/emitter-corpus-controls-r176-head').read_text().strip()
out=target/'emitter-corpus-controls-r176';out.mkdir(exist_ok=False);manifest={'head':head,'qualified':False,'steps':[],'artifacts':[]}
def save():(out/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
def run(label,args,env=None):
 assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()==head
 assert not subprocess.check_output(['git','diff','HEAD','--name-only'],cwd=root).strip()
 code=subprocess.run(['python3',str(base/'run-local.py'),label,*map(str,args)],cwd=root,env=env).returncode
 manifest['steps'].append({'label':label,'exit':code});save();return code
code=run('corpus-syntax-r176',['cargo','test','--offline','-p','tsc-rs-syntax','--lib','--','--test-threads=1'])
if code:raise SystemExit(code)
label='corpus-controls-compiler-build-r176';code=run(label,['cargo','test','--offline','-p','tsc-rs-compiler','--test','contracts','--no-run','--message-format=json'])
if code:raise SystemExit(code)
found=[]
for line in (base/f'records/local/{label}.log').read_text().splitlines():
 try:event=json.loads(line)
 except json.JSONDecodeError:continue
 if event.get('reason')=='compiler-artifact' and event.get('executable') and event.get('profile',{}).get('test'):
  assert 'tsc-rs-compiler' in event['package_id'];binary=out/'contracts';shutil.copy2(event['executable'],binary);os.chmod(binary,0o555);found.append(binary);manifest['artifacts'].append({'path':str(binary),'sha256':hashlib.sha256(binary.read_bytes()).hexdigest()})
assert len(found)==1;save();binary=found[0]
listing=subprocess.check_output([str(binary),'--list','--format','terse'],text=True);assert len([l for l in listing.splitlines() if l.endswith(': test')])==488
codes=[]
for n in [167,168,171]:
 label=f'corpus-controls-{n}-native-r176';env=os.environ.copy();capture=out/f'captures-{n}';assert not capture.exists();env['TSC_RS_H2_8A_CAPTURE_WRITES_DIR']=str(capture)
 codes.append(run(label,[binary,'--exact',f'emitter_residual_audit::r{n}_corpus_controls_match_complete_typescript_commands','--nocapture','--test-threads=1'],env))
manifest['qualified']=not any(codes);save();raise SystemExit(max(codes))
