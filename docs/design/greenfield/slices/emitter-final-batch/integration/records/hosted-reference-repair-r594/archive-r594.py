from pathlib import Path
import gzip,hashlib,json,subprocess
r=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep')
src=Path('/tmp/emitter-hosted-reference-repair-r578')
dst=r/'docs/design/greenfield/slices/emitter-final-batch/integration/records/hosted-reference-repair-r594'
sha=lambda b:hashlib.sha256(b).hexdigest()
assert not dst.exists()
for number in range(225,233):
 d=json.loads((src/f'review-{number}-response.json').read_bytes());assert not d.get('is_error',False) and d.get('result')
required={
 'checks-r583':['acceptance-h2-7de','noemit-commands'],
 'checks-r589':['factory-boundaries','import-original','heritage-complete','foundation-bundle','resolution-cache','parameter-temporaries','utf16-original','jsdoc-original'],
 'checks-r590':['planner','pin-preflight','qualification-tests','qualification-cli'],
 'checks-r592':['controls-full','system-full'],
 'checks-r593':['workspace-clippy'],
 'checks-r595-helper':['helper-controls','driver-syntax'],
 'checks-r595-canonical':['helper-controls','driver-syntax','helper-metadata','workspace-audit','planner','pin-preflight','qualification-tests','qualification-cli'],
}
checks={}
for directory,names in required.items():
 for name in names:
  receipt=json.loads((src/directory/(name+'.json')).read_bytes());assert receipt['exit']==0,(directory,name,receipt)
  raw=gzip.decompress((src/directory/(name+'.log.gz')).read_bytes());assert sha(raw)==receipt['log_sha256'];checks[directory+'/'+name]=receipt
for directory,name,code in [('checks-r584-red','planner',1),('checks-r586-red','import-original',1),('checks-r587-red','factory-boundaries',101)]:
 receipt=json.loads((src/directory/(name+'.json')).read_bytes());assert receipt['exit']==code
 assert sha(gzip.decompress((src/directory/(name+'.log.gz')).read_bytes()))==receipt['log_sha256']
for name in ('217','plan-base'):
 d=json.loads((src/'ef7-refresh-r580'/(name+'-delta.json')).read_bytes());assert d['case_payloads_identical']
for name in ('emitter-jsdoc-original-command','h2-5h-parameter-temporaries','utf16-original-rows-complete'):
 d=json.loads((src/'command-refresh-r581'/(name+'-delta.json')).read_bytes());assert d['case_payloads_identical']
heritage=json.loads((src/'heritage-refresh-r588/delta.json').read_bytes());assert heritage['old_case_payloads_identical'] and heritage['added_cases']==8
stop=json.loads((src/'stopped-local-ci/final-unsplit-ci-r576.json').read_bytes());assert stop['exit']==-15 and not stop['qualified']
protected=json.loads(Path('/tmp/emitter-final-untracked-before-ci-r576.json').read_bytes())['files']
for row in protected:
 p=r/row['path'];assert p.stat().st_mtime_ns==row['mtime_ns'] and sha(p.read_bytes())==row['sha256']
source_state=json.loads((src/'repair-source-state-r595.json').read_bytes())
for row in source_state['files']:
 data=(r/row['path']).read_bytes();assert sha(data)==row['sha256'] and len(data)==row['bytes'],row['path']
files=[];dst.mkdir(parents=True)
for p in sorted(src.rglob('*')):
 if not p.is_file() or '__pycache__' in p.parts:continue
 rel=p.relative_to(src)
 if p.suffix=='.log' and p.with_suffix('.log.gz').exists():continue
 raw=p.read_bytes();data=raw;stored=rel
 # Preserve byte-exact old artifacts and logs without bloating the tracked packet.
 if p.suffix in ('.log','.diff','.patch') or (p.suffix=='.json' and len(raw)>65536) or (p.suffix=='.md' and b'\r' in raw):
  data=gzip.compress(raw,mtime=0);stored=Path(str(rel)+'.gz')
 if rel==Path('resolution-draft.md'):stored=Path('resolution.md')
 dest=dst/stored;dest.parent.mkdir(parents=True,exist_ok=True);dest.write_bytes(data)
 files.append({'path':str(stored),'sha256':sha(data),'bytes':len(data),'source_sha256':sha(raw),'source_bytes':len(raw),'compression':'gzip' if stored!=rel and data!=raw else None})
patch=subprocess.check_output(['git','diff','--binary'],cwd=r)
stored_patch=gzip.compress(patch,mtime=0)
(dst/'repair.diff.gz').write_bytes(stored_patch);files.append({'path':'repair.diff.gz','sha256':sha(stored_patch),'bytes':len(stored_patch),'source_sha256':sha(patch),'source_bytes':len(patch),'compression':'gzip'})
manifest={'qualified':False,'scope':'Actual old-head hosted failures, intentionally stopped obsolete CI, reviewed runtime and replay repairs, reference refresh proofs and focused checks. A NEW normal walk and final exact-head local/hosted gates are still required.','source_head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=r,text=True).strip(),'hosted_failed_runs':[35533163389,35533163416],'obsolete_ci_exit':-15,'focused_checks':checks,'prior_walk_certificate_current':False,'protected_untracked_unchanged':len(protected),'files':files}
(dst/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n');print(json.dumps({'directory':str(dst),'files':len(files),'bytes':sum(x['bytes'] for x in files),'qualified':False},indent=2))
