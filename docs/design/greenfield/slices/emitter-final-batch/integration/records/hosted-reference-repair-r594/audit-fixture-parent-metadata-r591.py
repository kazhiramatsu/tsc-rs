from pathlib import Path
import json,subprocess,hashlib,datetime
r=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');out=Path(__file__).with_suffix('.json')
sha=lambda b:hashlib.sha256(b).hexdigest();checked=[]
def visit(value,consumer,pointer=''):
 if isinstance(value,dict):
  for name,hash_name in [('path','sha256'),('artifact','artifact_sha256'),('universe','universe_sha256'),('universe','artifact_sha256')]:
   path=value.get(name);pin=value.get(hash_name)
   if isinstance(path,str) and path.startswith('ratchets/') and isinstance(pin,str):
    checked.append({'consumer':consumer,'pointer':pointer+'/'+hash_name,'input':path,'pinned':pin,'current':sha((r/path).read_bytes())})
  for key,child in value.items():
   if key not in ('cases','observations'):visit(child,consumer,pointer+'/'+key)
 elif isinstance(value,list):
  for i,child in enumerate(value):visit(child,consumer,pointer+'/'+str(i))
for crate in ('compiler','emitter','program'):
 for p in sorted((r/'crates'/crate/'tests/fixtures').rglob('*')):
  if p.is_file() and (p.name.endswith('.json') or p.name.endswith('.json.zst')):
   raw=subprocess.check_output(['zstd','-q','-d','--stdout',str(p)]) if p.suffix=='.zst' else p.read_bytes()
   visit(json.loads(raw),str(p.relative_to(r)))
stale=[row for row in checked if row['pinned']!=row['current']]
report={'scope':'Current ratchet path/artifact/universe + associated sha256 metadata outside cases/observations in compiler/emitter/program fixture JSON and JSON.zst; not native execution or exhaustive arbitrary metadata schema validation.','time':datetime.datetime.now(datetime.timezone.utc).isoformat(),'checked':len(checked),'stale':stale,'entries':checked}
out.write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({'checked':len(checked),'stale':len(stale)}));assert len(checked)==11 and not stale
