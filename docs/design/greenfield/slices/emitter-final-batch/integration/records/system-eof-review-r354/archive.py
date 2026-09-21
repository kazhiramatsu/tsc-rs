from pathlib import Path
import json,gzip,hashlib,subprocess
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');O=R/'docs/design/greenfield/slices/emitter-final-batch/integration/records/system-eof-review-r354';items=[]
def add(p,name=None):
 p=Path(p);data=p.read_bytes();name=name or p.parent.name+'/'+p.name
 if p.suffix=='.log':data=gzip.compress(data,mtime=0);name+='.gz'
 items.append((name,data,str(p)))
for n in [181,182,183]:
 review=Path(f'/tmp/emitter-claude-review-round{n}-opus.json');assert json.loads(review.read_bytes()).get('result');add(review,'reviews/'+review.name);add(f'/tmp/emitter-round{n}-request.md','reviews/'+f'emitter-round{n}-request.md')
for name in ['emitter-system-eof-review-r345','emitter-synthetic-meta-probe-r346','emitter-synthetic-meta-controls-mint-r347','emitter-synthetic-meta-tests-r348','emitter-synthetic-owner-controls-mint-r350','emitter-system-eof-final-r351']:
 folder=Path('/tmp')/name
 for p in sorted(folder.rglob('*')):
  if p.is_file() and p.suffix in ['.rs','.py','.mjs','.json','.gz','.patch','.log']:
   add(p,name+'/'+p.relative_to(folder).as_posix())
for name in ['prepare-emitter-system-eof-review-r345.py','emitter-synthetic-meta-probe-r346.py','emitter-synthetic-meta-controls-mint-r347.py','emitter-synthetic-owner-controls-mint-r350.py','prepare-emitter-system-eof-final-r351.py','emitter-system-eof-final-validation-r352.py']:
 add(Path('/tmp')/name,'scripts/'+name)
for n in [340,341,343]:add(f'/tmp/emitter-system-eof-lib-r{n}.log','lib-tests/'+f'emitter-system-eof-lib-r{n}.log')
add(__file__,'archive.py');O.mkdir(exist_ok=False);index=[]
for name,data,source in items:
 p=O/name;assert not p.exists();p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(data);index.append({'path':name,'source':source,'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()})
report={'canonical_head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip(),'scope':'Actual Opus181-183, initial EOF/System implementation unit verification (340compilefail,341513pass1testexpectationfail,343514pass), independent syntheticMetaProperty panic346, canonical internal TypeScript12controls347/350. 345/348/351/352 are PREPARATION ONLY, not applied/qualified. Original4Systemcomments remainfailed in342 fullcommands, and new64TScontrols353 are being observed; no final map/command/integration qualification claim. No AST or position constraints relaxed.','qualified':False,'files':index}
(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({'archive':str(O),'files':len(index),'bytes':sum(x['bytes'] for x in index)}))
