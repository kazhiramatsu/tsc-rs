from pathlib import Path
import json,gzip,hashlib,subprocess
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep')
B=R/'docs/design/greenfield/slices/emitter-final-batch/integration'
T=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target')
O=B/'records/leading-binding-proof-r326'
proof=json.loads((T/'emitter-leading-binding-successor-r323/manifest.json').read_bytes())
assert proof['parser_qualified'] and not proof['commands_qualified'] and proof['new_inputs']==[]
items=[]
def add(p,name=None,compress=False):
 p=Path(p);data=p.read_bytes();name=name or p.parent.name+'/'+p.name
 if compress:data=gzip.compress(data,mtime=0);name+='.gz'
 items.append((name,data,str(p)))
for folder in [Path('/tmp/emitter-leading-binding-panic-r321'),Path('/tmp/emitter-leading-binding-panic-fix-r322'),Path('/tmp/emitter-leading-binding-proof-r323')]:
 for p in sorted(folder.iterdir()):
  if p.is_file():add(p,compress=p.suffix=='.log')
for label in ['emitter-leading-binding-full-command-r309','emitter-leading-binding-next-proofs-r314','emitter-leading-binding-controls-mint-r320']:
 for p in sorted((T/label).iterdir()):
  if p.is_file() and p.suffix!='.log':add(p)
for label in ['emitter-leading-binding-successor-r314','emitter-leading-binding-successor-r323']:
 for name in ['manifest.json','steps.json','resolved-dependencies.json','build.log','test.log','replay.stderr.log','successor.json','selection.json']:
  p=T/label/name
  if p.exists():add(p,compress=name.endswith('.log') or name in ['successor.json','selection.json'])
add('/tmp/emitter-leading-binding-panic-r321.py')
add('/tmp/emitter-leading-binding-controls-mint-r320.py')
add(__file__,'archive.py')
O.mkdir(exist_ok=False);index=[]
for name,data,source in items:
 p=O/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(data)
 index.append({'path':name,'source':source,'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()})
report={'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip(),'prototype_head':proof['head'],'scope':'B panic reproduced before fix;214 syntax units pass after lazy tuple fix;16994 original inputs preserve every core/raw fact and every profile, zero new admissions. Original40 commands passed on previous B head; new40 official oracle controls minted, native80 and current-head selected commands pending. All110 original loader failures remain unqualified. Empty variable repair not covered.','files':index}
(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps({'archive':str(O),'files':len(index),'bytes':sum(x['bytes'] for x in index)}))
