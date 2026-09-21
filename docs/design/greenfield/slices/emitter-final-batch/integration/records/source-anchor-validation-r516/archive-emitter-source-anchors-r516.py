import gzip,hashlib,json,subprocess
from pathlib import Path
r=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');b=r/'docs/design/greenfield/slices/emitter-final-batch/integration';prep=Path('/tmp/emitter-l0-anchor-repair-r511')
sha=lambda b:hashlib.sha256(b).hexdigest()
head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=r,text=True).strip();assert head=='fa06ae275710e40278240d0b8f9737749a230b0b'
assert not subprocess.check_output(['git','diff','HEAD','--name-only','--','crates',':(exclude)crates/oracle'],cwd=r).strip()
assert not subprocess.check_output(['git','diff','HEAD','--name-only','--','ratchets'],cwd=r).strip()
assert not (r/'target/chain-walk/converged-crates.sha256').exists()
for n in [208,209,210,211]:assert json.loads((prep/f'round{n}-opus.json').read_text()).get('is_error') is False
failed=json.loads((prep/'final-canonical-walk-r510.json').read_text());assert failed['head']==head and failed['tracked_clean'] and failed['exit']==1
proof=json.loads((prep/'final-source-proof-r515.json').read_text())
for x in proof['inputs']:assert sha((r/x['path']).read_bytes())==x['sha256']
expected={'l0-anchor-controls-r512':0,'l0-static-preconditions-r512':0,'l0-workspace-audit-r512':0,'all-source-anchor-controls-r514':1,'all-source-anchor-controls-r515':0,'all-source-static-preconditions-r515':0,'all-source-workspace-audit-r515':0}
for label,code in expected.items():
 p=b/'records/local'/(label+'.json');j=json.loads(p.read_text());raw=gzip.decompress(p.with_suffix('.log.gz').read_bytes());assert j['head']==head and j['exit']==code and sha(raw)==j['log_sha256']
 if label.endswith('r515'):assert j['diff_sha256']==sha(subprocess.check_output(['git','diff','HEAD'],cwd=r))
out=b/'records/source-anchor-validation-r516';out.mkdir(exist_ok=False);files=[]
def put(name,data):
 p=out/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(data);files.append({'path':name,'bytes':len(data),'sha256':sha(data)})
for p in sorted(prep.rglob('*')):
 if p.is_file():
  name=str(p.relative_to(prep));data=p.read_bytes()
  if p.suffix=='.log':name+='.gz';data=gzip.compress(data,mtime=0)
  put('preparation/'+name,data)
for label in expected:
 for suffix in ['.json','.log.gz']:
  p=b/'records/local'/(label+suffix);put('local/'+p.name,p.read_bytes())
for x in proof['inputs']:put('final-source/'+x['path'],(r/x['path']).read_bytes())
put('archive-emitter-source-anchors-r516.py',Path(__file__).read_bytes())
put('tracked-changes.patch.gz',gzip.compress(subprocess.check_output(['git','diff','HEAD'],cwd=r),mtime=0))
manifest={'source_head':head,'qualified':False,'scope':'Failed first official walk before any successful mint; L0 and H2 source-reference repair and real preflight controls. No Rust or generated-artifact changes. A successful official walk, final unsplit CI and hosted checks are still required.','failed_walk':{'label':'final-canonical-walk-r510','run_id':'20260920-190844-72391','exit':1},'earlier_static_limit':'512 static exit0 did not recognize the H2 anchor error families; it is not proof those source anchors were valid.','final_validation_labels':[x for x in expected if x.endswith('r515')],'source_proof':proof,'files':files}
(out/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
for x in files:assert sha((out/x['path']).read_bytes())==x['sha256']
print(json.dumps({'archive':str(out),'files':len(files),'bytes':sum(x['bytes'] for x in files)}))
