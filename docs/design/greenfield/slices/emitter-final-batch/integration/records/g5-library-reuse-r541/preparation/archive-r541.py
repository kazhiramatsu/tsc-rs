"""Archive actual failed walk and bounded repair; does not certify convergence."""
import datetime,gzip,hashlib,json,subprocess
from pathlib import Path
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep')
P=Path('/tmp/emitter-5g-repair-r536')
B=R/'docs/design/greenfield/slices/emitter-final-batch/integration'
O=B/'records/g5-library-reuse-r541'
HEAD='bb01b86df0a2d266910d25124c4bfbade334c826'
sha=lambda data:hashlib.sha256(data).hexdigest()
git=lambda *args:subprocess.check_output(['git',*args],cwd=R)
assert git('rev-parse','HEAD').decode().strip()==HEAD
assert not git('diff','HEAD','--name-only','--','crates/**/*.rs').strip()
proof=json.loads((P/'official-write-review-r540.json').read_text())
assert proof['compared_cases']==9027 and proof['unchanged_cases']==9023 and len(proof['path_leaves'])==17
assert proof['generator_sha256']==sha((R/'crates/oracle/h2-5g-qualification.mjs').read_bytes())
failed=json.loads((P/'failed-walk/final-canonical-walk-r526.json').read_text());assert failed['exit']==-15 and failed['head']==HEAD
for row in json.loads((P/'failed-walk/manifest.json').read_text())['files']:
 data=(P/'failed-walk'/row['path']).read_bytes();assert sha(data)==row['stored_sha256']
for stem in ['r533','r535','r537']:
 response=json.loads(Path(f'/tmp/emitter-final-5g-path-review-{stem}-response.json').read_text())
 assert response['is_error'] is False and response['result'].strip()
labels=['g5-library-reuse-qualification-r538','g5-library-reuse-write-r539']
for label in labels:
 receipt=json.loads((B/f'records/local/{label}.json').read_text())
 raw=gzip.decompress((B/f'records/local/{label}.log.gz').read_bytes())
 assert receipt['exit']==0 and receipt['head']==HEAD and sha(raw)==receipt['log_sha256'] and len(raw)==receipt['log_bytes']
 if label.endswith('r538'):assert b'tests 61' in raw and b'pass 61' in raw
O.mkdir(exist_ok=False);files=[]
def put(name,data):
 out=O/name;out.parent.mkdir(parents=True,exist_ok=True);out.write_bytes(data)
 files.append({'path':name,'bytes':len(data),'sha256':sha(data)})
for p in sorted(P.rglob('*')):
 if p.is_file():
  assert not p.is_symlink()
  name='preparation/'+p.relative_to(P).as_posix();data=p.read_bytes()
  if p.suffix in ('.log','.diff'):name+='.gz';data=gzip.compress(data,mtime=0)
  put(name,data)
put('preparation/failed-journal-case-comparison-r532.json',Path('/tmp/emitter-5g-journal-comparison-r532.json').read_bytes())
put('review/initial-proposal-r534.md',Path('/tmp/emitter-final-5g-write-guard-r534-proposal.md').read_bytes())
for number,stem in [(218,'r533'),(219,'r535'),(220,'r537')]:
 for suffix in ['request.md','response.json','stderr.log']:
  p=Path(f'/tmp/emitter-final-5g-path-review-{stem}-{suffix}');put(f'review/round{number}-{suffix}',p.read_bytes())
for label in labels:
 for suffix in ['.json','.log.gz']:put('validation/'+label+suffix,(B/('records/local/'+label+suffix)).read_bytes())
for relative in ['crates/oracle/h2-5g-qualification.mjs','scripts/h2-5g-library-paths.test.mjs','.github/ci/qualification.test.mjs']:
 put('sources/'+relative,(R/relative).read_bytes())
put('bounded-repair.patch.gz',gzip.compress(git('diff','HEAD','--','crates/oracle/h2-5g-qualification.mjs','.github/ci/qualification.test.mjs','ratchets/h2-5g-qualification.v1.json'),mtime=0))
manifest={'recorded_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'source_head':HEAD,'qualified':False,'failed_official_walk':{'label':'final-canonical-walk-r526','exit':-15,'run_id':'20260920-213559-96586','disposition':'Integrator stopped owned tree after confirmed nonconvergent H2.5g check/write loop; no certificate'},'scope':'Bounded raw-library-diagnostic reuse invalidation, failed-run journals, actual Opus218-220, 61 focused tests and official writer. A full new-key 9027 double check, green sanctioned walk, final exact-head unsplit CI, hosted checks and merge remain required.','writer_review':proof,'files':files}
(O/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
for row in files:assert sha((O/row['path']).read_bytes())==row['sha256']
print(json.dumps({'archive':str(O),'files':len(files),'bytes':sum(x['bytes'] for x in files),'qualified':False}))
