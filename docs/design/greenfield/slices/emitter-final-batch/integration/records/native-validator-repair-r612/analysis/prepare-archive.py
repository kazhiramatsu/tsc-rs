from pathlib import Path
import gzip,hashlib,json,subprocess
root=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep')
src=Path('/tmp/emitter-final-native-failure-r611')
out=root/'docs/design/greenfield/slices/emitter-final-batch/integration/records/native-validator-repair-r612'
assert not out.exists()
batch=json.loads((src/'focused-batch.json').read_text());assert batch['complete'] and all(x['exit']==0 for x in batch['results'])
for label in ['conformance-library-fixed','pin-preflight-fixed','clippy-final',*[r['label'] for r in batch['results']]]:
 d=json.loads((src/(label+'.json')).read_text());raw=(src/(label+'.log')).read_bytes();assert d['exit']==0 and hashlib.sha256(raw).hexdigest()==d['log_sha256'] and gzip.decompress((src/(label+'.log.gz')).read_bytes())==raw
for n in [235,236]:
 d=json.loads((src/f'opus{n}-launcher.json').read_text());assert d['exit']==0
 assert json.loads((src/f'opus{n}-response.json').read_text()).get('result')
failed=json.loads((src/'receipt.json').read_text());assert failed['exit']==1 and failed['qualified'] is False
out.mkdir();entries=[]
def put(name,data,raw=None):
 p=out/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(data)
 row={'path':name,'sha256':hashlib.sha256(data).hexdigest(),'bytes':len(data)}
 if raw is not None:row.update(raw_sha256=hashlib.sha256(raw).hexdigest(),raw_bytes=len(raw))
 entries.append(row)
for p in sorted(src.iterdir()):
 if not p.is_file():continue
 if p.suffix=='.log':
  if p.with_suffix('.log.gz').exists():continue
  raw=p.read_bytes();put('analysis/'+p.name+'.gz',gzip.compress(raw,mtime=0),raw)
 else:put('analysis/'+p.name,p.read_bytes())
ci=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target/emitter-final-unsplit-ci')
for suffix in ['json','log.gz','journal-before.json','journal-after.json']:
 p=ci/('final-unsplit-ci-r603.'+suffix);put('failed-ci/'+p.name,p.read_bytes())
raw=(ci/'final-unsplit-ci-r603.log').read_bytes();assert hashlib.sha256(raw).hexdigest()==failed['log_sha256'];assert gzip.decompress((ci/'final-unsplit-ci-r603.log.gz').read_bytes())==raw
put('failed-ci/untracked-before.json',Path('/tmp/emitter-final-untracked-before-ci-r603.json').read_bytes())
changes=subprocess.check_output(['git','diff','--binary','--full-index','HEAD','--','crates/compiler/tests/unit/cli/tests.rs','crates/conformance/src/host_resolution.rs','crates/conformance/tests/unit/host_resolution/tests.rs'],cwd=root)
put('repair.patch.gz',gzip.compress(changes,mtime=0),changes)
put('resolution.md',(src/'resolution.md').read_bytes())
manifest={'source_head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),'scope':'Actual failed full CI, independently reviewed bounded validator/test repair and focused successful checks; not final walk, final CI or merge qualification.','files':entries}
(out/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
for e in entries:assert hashlib.sha256((out/e['path']).read_bytes()).hexdigest()==e['sha256']
print(json.dumps({'archive':str(out),'files':len(entries),'bytes':sum(e['bytes'] for e in entries)}))
