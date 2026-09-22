from pathlib import Path
import gzip,hashlib,json,shutil,subprocess
r=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');src=Path('/tmp/emitter-hosted-catalog-repair-r570');dst=r/'docs/design/greenfield/slices/emitter-final-batch/integration/records/hosted-catalog-repair-r573'
sha=lambda data:hashlib.sha256(data).hexdigest()
assert not dst.exists()
for review in (223,224):
 x=json.loads((src/f'review-{review}-response.json').read_bytes());assert not x.get('is_error',False) and x.get('result')
checks={name:json.loads((src/'checks-r572'/f'{name}.json').read_bytes()) for name in ['planner','pin-preflight','qualification-tests','qualification-cli']}
assert all(x['exit']==0 for x in checks.values())
for name,receipt in checks.items():
 raw=gzip.decompress((src/'checks-r572'/f'{name}.log.gz').read_bytes());assert sha(raw)==receipt['log_sha256']
assert json.loads((src/'checks-r571/planner.json').read_bytes())['exit']==1
stop=json.loads((src/'stopped-local-ci/final-unsplit-ci-r569.json').read_bytes());assert stop['exit']==-15 and not stop['qualified']
proof=json.loads((src/'final-catalog-proof.json').read_bytes());assert proof['all_suite_counts_match'] and len(proof['catalog_51'])==51
files=[];dst.mkdir(parents=True)
for p in sorted(src.rglob('*')):
 if not p.is_file() or '__pycache__' in p.parts:continue
 rel=p.relative_to(src)
 if p.suffix=='.log' and p.with_suffix('.log.gz').exists():continue
 raw=p.read_bytes();data=raw;stored=rel
 if p.name.endswith('.journal-after.json') or p.name.endswith('.journal-before.json'):
  data=gzip.compress(raw,mtime=0);stored=Path(str(rel)+'.gz')
 dest=dst/stored;dest.parent.mkdir(parents=True,exist_ok=True);dest.write_bytes(data)
 files.append({'path':str(stored),'sha256':sha(data),'bytes':len(data),'source_sha256':sha(raw),'source_bytes':len(raw),'compression':'gzip' if stored!=rel else None})
patch=subprocess.check_output(['git','diff','--','.github/ci/qualification-policy.v2.json','.github/ci/test_replay.py','scripts/emitter_final_witnesses.py','scripts/witness.py'],cwd=r)
(dst/'repair.diff').write_bytes(patch);files.append({'path':'repair.diff','sha256':sha(patch),'bytes':len(patch)})
manifest={'qualified':False,'scope':'Hosted catalog repair, actual failed hosted plans, intentionally stopped obsolete CI and focused checks; not a completed final gate or delivery.','source_head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=r,text=True).strip(),'hosted_failed_runs':[35532008563,35532008570],'obsolete_ci_exit':-15,'first_local_planner_exit':1,'focused_checks':checks,'walk_certificate_unchanged':True,'walk_certificate_sha256':proof['certificate_sha256'],'unchanged_crates_sha256':proof['unchanged_crates_sha256'],'files':files}
(dst/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
print(json.dumps({'directory':str(dst),'files':len(files),'bytes':sum(x['bytes'] for x in files),'qualified':False},indent=2))
