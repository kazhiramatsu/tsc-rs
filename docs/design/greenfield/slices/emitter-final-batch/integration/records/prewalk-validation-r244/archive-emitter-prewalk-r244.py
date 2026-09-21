from pathlib import Path
import gzip,hashlib,json,subprocess,shutil,re
r=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');b=r/'docs/design/greenfield/slices/emitter-final-batch/integration';t=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target/emitter-prewalk-r244');head='628562c31a5102348a7e8769f3a2aa88deef70f1';sha=lambda x:hashlib.sha256(x).hexdigest()
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=r,text=True).strip()==head
assert not subprocess.check_output(['git','diff','HEAD','--name-only'],cwd=r).strip()
assert not subprocess.check_output(['git','status','--porcelain','--','crates'],cwd=r).strip()
m=json.loads((t/'manifest.json').read_text());outer=json.loads((b/'records/local/final-prewalk-validation-pipeline-r244.json').read_text());assert m['head']==head==outer['head'] and outer['tracked_clean'];assert m['qualified']==(len(m['steps'])==7 and all(s['exit']==0 for s in m['steps']))
if m['qualified']:assert outer['exit']==0
else:assert outer['exit']!=0
out=b/'records/prewalk-validation-r244';out.mkdir(exist_ok=False);files=[];results=[]
def put(name,data):
 (out/name).write_bytes(data);files.append({'path':name,'sha256':sha(data),'bytes':len(data)})
for p in sorted((b/'records/local').glob('*-r244.json')):
 j=json.loads(p.read_text());assert j['head']==head and j['tracked_clean'];raw=gzip.decompress(p.with_suffix('.log.gz').read_bytes());assert sha(raw)==j['log_sha256'] and len(raw)==j['log_bytes']
 put(p.name,p.read_bytes());put(p.with_suffix('.log.gz').name,p.with_suffix('.log.gz').read_bytes())
 results.append({'label':p.stem,'exit':j['exit'],'seconds':j['seconds'],'libtest_summaries':re.findall(r'^test result: .*$',raw.decode(),re.M),'scope_summaries':[line for line in raw.decode().splitlines() if 'SUMMARY' in line]})
put('pipeline-manifest.json',(t/'manifest.json').read_bytes());put('emitter-final-prewalk-validation-r244.py',Path('/tmp/emitter-final-prewalk-validation-r244.py').read_bytes());put('archive-emitter-prewalk-r244.py',Path(__file__).read_bytes())
(out/'manifest.json').write_text(json.dumps({'source_head':head,'qualified':m['qualified'],'scope':'Format and workspace all-targets Clippy; six full compiler test targets, full harness contracts and xtask bin tests, baseline example build and static walk preconditions. This is prewalk validation, not final unsplit CI or canonical walk. All raw failures/ignored counts remain visible; no successful raw command capture archive is claimed for default in-memory fixture comparators.','results':results,'files':files},indent=2)+'\n')
print('Archived',len(files),'files;qualified',m['qualified'])
