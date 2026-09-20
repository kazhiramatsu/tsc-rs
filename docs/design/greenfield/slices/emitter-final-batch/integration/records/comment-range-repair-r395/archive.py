from pathlib import Path
import json,gzip,hashlib,subprocess
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');T=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target');B=R/'docs/design/greenfield/slices/emitter-final-batch/integration';O=B/'records/comment-range-repair-r395';h=lambda b:hashlib.sha256(b).hexdigest()
for name in ['emitter-comment-range-mint-r390','emitter-empty-function-apply-r394']:
 d=json.loads((T/name/'manifest.json').read_bytes());assert d['oracle_qualified'] and not d['native_qualified'];assert all(x['exit']==0 for x in d['steps'])
for n in [189,190]:assert not json.loads(Path(f'/tmp/emitter-claude-review-round{n}-opus.json').read_bytes())['is_error']
O.mkdir(exist_ok=False);files=[]
def put(name,data):
 p=O/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(data);files.append({'path':name,'bytes':len(data),'sha256':h(data)})
for root in [Path('/tmp/emitter-function-range-repair-r388'),Path('/tmp/emitter-comment-range-controls-r389'),Path('/tmp/emitter-synthetic-body-probe-r391'),Path('/tmp/emitter-empty-function-final-r393'),T/'emitter-comment-range-mint-r390',T/'emitter-empty-function-apply-r394']:
 for p in sorted(root.rglob('*')):
  if not p.is_file() or p.name=='probe':continue
  name=root.name+'/'+str(p.relative_to(root));data=p.read_bytes()
  if p.suffix=='.log':name+='.gz';data=gzip.compress(data,mtime=0)
  put(name,data)
for n in [189,190]:
 for name in [f'emitter-round{n}-request.md',f'emitter-claude-review-round{n}-opus.json']:put('reviews/'+name,Path('/tmp',name).read_bytes())
for name in ['emitter-body-range-probe-r387.mjs','emitter-body-range-probe-r387.json','prepare-emitter-comment-range-controls-r389.py','emitter-comment-range-mint-r390.py','emitter-synthetic-body-probe-r391.py','emitter-empty-function-apply-r394.py','emitter-canonical-focused-r396.py','emitter-canonical-corpus-r375.py','archive-emitter-canonical-corpus-r384.py']:put('scripts/'+name,Path('/tmp',name).read_bytes())
for label in ['body-endpoint-flags-r390','body-endpoint-commands-r390','empty-body-final-oracle-r394','empty-body-final-format-r394']:
 for suffix in ['.json','.log.gz']:put('local/'+label+suffix,(B/'records/local'/(label+suffix)).read_bytes())
put('candidate-changes.patch.gz',gzip.compress(subprocess.check_output(['git','diff','HEAD','--','crates','scripts'],cwd=R),mtime=0));put('archive.py',Path(__file__).read_bytes())
(O/'manifest.json').write_text(json.dumps({'head_before_repair':subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip(),'qualified_final':False,'native_qualified':False,'oracle_qualified':True,'scope':'349 regression and391 synthetic-fallback reproduction; actual189190 reviewed local comment fixes; official340 printer and788 full-command expectations, all old172/244 and748+72 preserved. Native rerun pending.','review_correction':'Actual190 suggested adjacent newline comment might disappear; authoritative394 TypeScript observations retain both adjacent and detached comments for Original ranges. That unmeasured expectation was not adopted.','files':files},indent=2)+'\n');print(json.dumps({'archive':str(O),'files':len(files),'bytes':sum(x['bytes'] for x in files)}))
