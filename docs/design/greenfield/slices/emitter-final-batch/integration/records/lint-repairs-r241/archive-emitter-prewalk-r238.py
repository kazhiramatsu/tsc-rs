from pathlib import Path
import gzip,hashlib,json,subprocess
r=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');b=r/'docs/design/greenfield/slices/emitter-final-batch/integration';t=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target/emitter-prewalk-r238');head='34dea8e69de2219e9f1f9b1649c610e0b18938bc';sha=lambda data:hashlib.sha256(data).hexdigest()
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=r,text=True).strip()==head;assert not subprocess.check_output(['git','diff','HEAD','--name-only'],cwd=r).strip();m=json.loads((t/'manifest.json').read_text());assert m['head']==head and not m['qualified'] and len(m['steps'])==5 and [x['exit'] for x in m['steps']]==[0,0,0,0,101];outer=json.loads((b/'records/local/final-prewalk-validation-pipeline-r238.json').read_text());assert outer['head']==head and outer['exit']==101
out=b/'records/prewalk-validation-r238';out.mkdir(exist_ok=False);files=[]
def put(name,data):
 (out/name).write_bytes(data);files.append({'path':name,'sha256':sha(data),'bytes':len(data)})
for p in sorted((b/'records/local').glob('*-r238.*')):
 if p.suffix=='.log':continue
 if p.suffix=='.json':
  j=json.loads(p.read_text());assert j['head']==head and j['exit'] in [0,101];raw=gzip.decompress(p.with_suffix('.log.gz').read_bytes());assert sha(raw)==j['log_sha256'] and len(raw)==j['log_bytes']
 put(p.name,p.read_bytes())
put('pipeline-manifest.json',(t/'manifest.json').read_bytes());put('emitter-final-prewalk-validation-r238.py',Path('/tmp/emitter-final-prewalk-validation-r238.py').read_bytes())
(out/'manifest.json').write_text(json.dumps({'head':head,'qualified':False,'scope':'Format, template escape flags5, compiler-host contracts14 and project-loader contracts4 PASS. Workspace all-targets Clippy keep-going -D warnings failed with test-support duplicate modules and redundant code after type migration; xtask census has one needless borrow. Static walk preconditions not run. No complete prewalk/full CI/canonical walk qualification.','files':files},indent=2)+'\n');print('Archived',len(files),'files',out)
