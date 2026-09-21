from pathlib import Path
import json,gzip,hashlib,subprocess
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');T=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target');O=R/'docs/design/greenfield/slices/emitter-final-batch/integration/records/body-comment-validation-r374';items=[]
assert json.loads((T/'emitter-body-tests-followup-r370/manifest.json').read_text())['qualified'];assert json.loads((T/'emitter-body-measurement-r372/manifest.json').read_text())['qualified']
def add(p,name):
 p=Path(p);data=p.read_bytes()
 if p.suffix=='.log':data=gzip.compress(data,mtime=0);name+='.gz'
 items.append((name,data,str(p)))
for n in [186,187]:
 p=Path(f'/tmp/emitter-claude-review-round{n}-opus.json');assert json.loads(p.read_text()).get('result');add(p,'reviews/'+p.name);p=Path(f'/tmp/emitter-round{n}-request.md');add(p,'reviews/'+p.name)
for folder in [T/'emitter-body-validation-r364',T/'emitter-body-tests-followup-r370',T/'emitter-body-measurement-r372',Path('/tmp/emitter-current-registration-r367'),Path('/tmp/emitter-body-success-mint-r369')]:
 for p in sorted(folder.rglob('*')):
  if p.is_file() and p.suffix in ['.json','.log','.gz','.mjs']:
   if p.suffix=='.log' and p.with_suffix('.log.gz').exists():continue
   add(p,folder.name+'/'+p.relative_to(folder).as_posix())
for name in ['emitter-body-validation-r364.py','emitter-system-if-oracle-r368.mjs','emitter-system-if-oracle-r368.json','emitter-body-success-mint-r369.py','emitter-body-tests-followup-r370.py','emitter-body-measurement-r372.py','emitter-integrate-final-r371.py','emitter-canonical-corpus-r375.py','emitter-canonical-focused-r349.py']:
 add('/tmp/'+name,'scripts/'+name)
add(__file__,'archive.py');O.mkdir(exist_ok=False);index=[]
for name,data,source in items:
 p=O/name;assert not p.exists();p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(data);index.append({'path':name,'source':source,'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()})
report={'canonical_head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip(),'qualified_final':False,'scope':'Private515libPASS370,172flagsALLexactx2(364), full10failurecontractsPASS372incl22reviewrowsx2(success/fault/sharedreuse); actualOpus186187 reviews. 364firstfailures preserved:oneoldSystemunitliteral(TS368confirmsnewJS), oneadapterquantitymismatch(text/eventsidentical, physicalstringcolumn0vswriterpendingindent4). RepairsTESTONLY; no furtherproductionchange. 367historicalreadinessfail andcurrentgeneratorregistrationcoverage explicit. 371/375/349 are PREPARATION ONLY. 365completecompiler748/System160/witnesses stillrunning; no canonical/finalqualificationclaim.','files':index}
(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({'archive':str(O),'files':len(index),'bytes':sum(r['bytes'] for r in index)}))
