from pathlib import Path
import json,gzip,hashlib,subprocess
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');B=R/'docs/design/greenfield/slices/emitter-final-batch/integration';O=B/'records/reviewed-repairs-r317';T=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target');review=json.loads(Path('/tmp/emitter-claude-review-round177-opus.json').read_text());assert not review.get('is_error',False) and review['result'];r315=json.loads((T/'emitter-reviewed-fixture-refresh-r315/manifest.json').read_text());assert r315.get('oracle_refresh_complete');files=[]
def add(source,dest):
 p=Path(source);data=p.read_bytes();files.append((dest,data,str(p)))
for name in ['emitter-round177-request.md','emitter-claude-review-round177-opus.json','emitter-xtask-stale-controls-r316.py','emitter-reviewed-fixture-refresh-r315.py']:
 add(Path('/tmp')/name,name)
for folder in [Path('/tmp/emitter-xtask-stale-controls-r316'),T/'emitter-reviewed-fixture-refresh-r315']:
 for p in sorted(folder.iterdir()):
  if p.is_file():add(p,folder.name+'/'+p.name)
for label in ['import-comment-controls-oracle-r315','import-helper-retirement-oracle-r315']:
 p=B/'records/local'/(label+'.json');j=json.loads(p.read_text());assert j['exit']==0;raw=gzip.decompress(p.with_suffix('.log.gz').read_bytes());assert hashlib.sha256(raw).hexdigest()==j['log_sha256'];add(p,'local/'+p.name);add(p.with_suffix('.log.gz'),'local/'+p.with_suffix('.log.gz').name)
add(__file__,'archive.py');O.mkdir(exist_ok=False);index=[]
for name,data,source in files:
 p=O/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(data);index.append({'path':name,'source':source,'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()})
(O/'current-source-diff.patch.gz').write_bytes(gzip.compress(subprocess.check_output(['git','diff','HEAD','--','crates','scripts'],cwd=R),mtime=0))
report={'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip(),'qualified_native':False,'scope':'actual Opus177 reviewed three stale xtask controls;316applied with fullcasefingerprint verification and strictempty312closure.315officialTypeScript36newcommands minted andold129unchanged;importhelperfulloraclecheck passed. Nativefixretests stillpending. ReviewclaimsaboutCIabolitionnotadopted:completeunsplitgate mandatory.','files':index};(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({'archived':str(O),'files':len(index),'qualified_native':False}))
