"""Attach verbatim earlier-round evidence after the official green archiver succeeds."""
from pathlib import Path
import hashlib,json,subprocess
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep')
O=R/'docs/design/greenfield/slices/emitter-final-batch/integration/records/canonical-walk-r562'
P=Path('/tmp/emitter-final-walk-progress-r564')
sha=lambda b:hashlib.sha256(b).hexdigest()
head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip()
assert head=='11d1971dd31a68e05fe18bedb26b6fdef4e9d170'
manifest=json.loads((O/'manifest.json').read_bytes())
cert=json.loads((O/'certificate.json').read_bytes())
assert cert['qualified'] and cert['exit']==0 and cert['run_id']=='20260921-024343-28510'
assert manifest['source_head']==head and manifest['certificate']==cert
for row in manifest['files']:assert sha((O/row['path']).read_bytes())==row['sha256']
for name in ['manifest.json','round1-complete-manifest.json']:
 d=json.loads((P/name).read_text());assert not d['qualified'] and d['run_id']==cert['run_id']
 for row in d['files']:assert sha((P/row['path']).read_bytes())==row['sha256']
review=Path('/tmp/emitter-final-walk-review-r562.json');d=json.loads(review.read_text())
assert d['walk_source_head']==head and d['walk_receipt_success']
for row in d['changes']:
 assert all(row.get('content_checks',{}).values()),row['path']
assert not (O/'earlier-round-evidence').exists()
def put(name,data):
 p=O/name;assert not p.exists();p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(data)
 manifest['files'].append({'path':name,'sha256':sha(data),'bytes':len(data)})
for p in sorted(P.iterdir()):
 assert p.is_file() and not p.is_symlink()
 put('earlier-round-evidence/'+p.name,p.read_bytes())
put('generated-diff-review.json',review.read_bytes())
put('generated-diff-review.py',Path('/tmp/emitter-final-review-walk-r562.py').read_bytes())
put('attach-progress-r565.py',Path(__file__).read_bytes())
manifest['earlier_round_evidence_scope']='Verbatim first-round H2.8a metadata remints and full JSON delta comparison, saved before the official driver overwrote rung logs. These progress snapshots retain their original qualified=false; only the separately preserved actual green invocation certificate qualifies this walk.'
(O/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
for row in manifest['files']:assert sha((O/row['path']).read_bytes())==row['sha256']
print(json.dumps({'archive':str(O),'files':len(manifest['files']),'certificate_unchanged':sha((O/'certificate.json').read_bytes())}))
