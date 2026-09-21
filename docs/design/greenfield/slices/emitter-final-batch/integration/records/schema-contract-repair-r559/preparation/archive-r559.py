"""Archive the actual failed r543 and bounded schema repair, never certify a walk."""
import datetime,gzip,hashlib,json,subprocess
from pathlib import Path
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep')
P=Path('/tmp/emitter-schema-cycle-repair-r553')
B=R/'docs/design/greenfield/slices/emitter-final-batch/integration'
O=B/'records/schema-contract-repair-r559'
HEAD='f46dc801560cd8ccaf8ccce8dcec6135615f0244'
sha=lambda b:hashlib.sha256(b).hexdigest()
git=lambda *a:subprocess.check_output(['git',*a],cwd=R)
assert git('rev-parse','HEAD').decode().strip()==HEAD
assert not git('diff','HEAD','--name-only','--','crates/**/*.rs','crates/oracle/*.mjs','.github/ci/qualification.mjs').strip()
for row in json.loads((P/'before-manifest.json').read_text())['files']:
 assert sha((P/row['path']).read_bytes())==row['sha256'],row['path']
failed=json.loads((B/'records/local/final-canonical-walk-r543.json').read_text())
assert failed['exit']==1 and failed['head']==HEAD and failed['seconds']==10741.332
proof=json.loads((P/'generated-review-r558.json').read_text())
assert proof['production_sources_unchanged'] and len(proof['artifact_files'])==14
for number in [221,222]:
 d=json.loads((P/f'review-{number}-response.json').read_text())
 assert not d['is_error'] and d['result'].strip()
labels=['schema-dispositions-write-r554','schema-qualification-write-r555','schema-qualification-tests-r556','schema-full-check-r557']
for label in labels:
 d=json.loads((B/f'records/local/{label}.json').read_text())
 raw=gzip.decompress((B/f'records/local/{label}.log.gz').read_bytes())
 assert d['exit']==0 and d['head']==HEAD and sha(raw)==d['log_sha256'] and len(raw)==d['log_bytes'],label
 if label.endswith('r556'):assert b'tests 66' in raw and b'pass 66' in raw
 if label.endswith('r557'):assert b'27' in raw
O.mkdir(exist_ok=False);files=[]
def put(name,data):
 q=O/name;q.parent.mkdir(parents=True,exist_ok=True);q.write_bytes(data)
 files.append({'path':name,'bytes':len(data),'sha256':sha(data)})
for q in sorted(P.rglob('*')):
 if q.is_file():
  assert not q.is_symlink()
  name='preparation/'+q.relative_to(P).as_posix();data=q.read_bytes()
  if q.suffix in ['.log','.diff']:name+='.gz';data=gzip.compress(data,mtime=0)
  put(name,data)
progress=Path('/tmp/emitter-final-walk-progress-r552')
for row in json.loads((progress/'manifest.json').read_text())['files']:
 assert sha((progress/row['copy']).read_bytes())==row['sha256']
for q in sorted(progress.iterdir()):
 assert q.is_file() and not q.is_symlink()
 data=q.read_bytes();name='failed-walk-progress/'+q.name
 if q.suffix=='.log':name+='.gz';data=gzip.compress(data,mtime=0)
 put(name,data)
for label in labels:
 for ext in ['.json','.log.gz']:put('validation/'+label+ext,(B/f'records/local/{label}{ext}').read_bytes())
for f in ['.github/ci/contracts/h2-7c-qualification.schema.json','.github/ci/contracts/h2-5h-a-dispositions.schema.json','.github/ci/bounded-artifact-schemas.test.mjs','.github/ci/qualification.test.mjs']:
 put('sources/'+f,(R/f).read_bytes())
put('reviewed-generated-and-schema.patch.gz',gzip.compress(git('diff','HEAD','--','ratchets','.github/ci'),mtime=0))
manifest={'recorded_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'source_head':HEAD,'qualified':False,'failed_official_walk':{'label':'final-canonical-walk-r543','run_id':'20260920-232502-12419','exit':1,'seconds':10741.332,'reason':'Final schema check rejected cyclic H27c ref; independent all27 audit also found stale H25ha roster constants. No green certificate.'},'scope':'Actual r543 failure, H25g fresh9027 receipt and later cache hit, bounded schema repairs, actual Opus221-222, official writers,66 registered tests and full27-contract plusFCI CLIcheck. New sanctioned walk, final exact-head unsplit CI, hosted checks and merge remain required.','generated_delta_review':proof,'files':files}
(O/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
for row in files:assert sha((O/row['path']).read_bytes())==row['sha256']
print(json.dumps({'archive':str(O),'files':len(files),'bytes':sum(x['bytes'] for x in files),'qualified':False}))
