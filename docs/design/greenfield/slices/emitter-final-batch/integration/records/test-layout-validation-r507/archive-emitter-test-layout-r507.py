from pathlib import Path
import datetime,gzip,hashlib,json,re,subprocess
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');B=R/'docs/design/greenfield/slices/emitter-final-batch/integration';O=B/'records/test-layout-validation-r507';h=lambda b:hashlib.sha256(b).hexdigest()
report=json.loads(Path('/tmp/emitter-target-roster-validation-r506/manifest.json').read_bytes());assert report.get('focused_qualified') is True
prior498=json.loads(Path('/tmp/emitter-layout-validation-r498/manifest.json').read_bytes())
prior=json.loads(Path('/tmp/emitter-layout-validation-r495/manifest.json').read_bytes())
lib=json.loads((B/'records/local/488-layout-affected-libraries.json').read_bytes());assert lib['exit']==0
proof=json.loads(Path('/tmp/emitter-test-layout-proof-r484/canonical-after-fmt-proof.json').read_bytes())
for path,digest in proof['canonical_files'].items():assert h((R/path).read_bytes())==digest,path
for path,digest in report['source_inputs'].items():assert h((R/path).read_bytes())==digest,path
assert h((R/'crates/xtask/src/recovery_parse_snapshot.rs').read_bytes())==proof['frozen_source_sha256']
O.mkdir(exist_ok=False);rows=[]
def put(name,b):
 p=O/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(b);rows.append(dict(path=name,sha256=h(b),bytes=len(b)))
def capture(p,name):
 p=Path(p);b=p.read_bytes()
 if p.suffix=='.log' or len(b)>150000:b=gzip.compress(b,mtime=0);name+='.gz'
 put(name,b)
for directory in ['emitter-test-layout-review-r483','emitter-frozen-layout-boundary-review-r487','emitter-layout-code-review-r489','emitter-delivery-layout-successors-r491','emitter-layout-followup-r493','emitter-layout-validation-r495','emitter-layout-policy-pin-r497','emitter-layout-validation-r498','emitter-integration-target-boundary-r501','emitter-target-roster-repair-r502','emitter-target-roster-code-review-r505','emitter-target-roster-validation-r506']:
 for p in sorted((Path('/tmp')/directory).iterdir()):
  if p.is_file():capture(p,directory+'/'+p.name)
d=Path('/tmp/emitter-test-layout-proof-r484')
for p in sorted(d.iterdir()):
 if p.is_file():capture(p,'proof/'+p.name)
capture(d/'src/main.rs','proof/src/main.rs')
for row in json.loads((d/'move-proof.json').read_bytes())['moves']:
 capture(d/'scratch-v2'/row['target'],'proof/formatted-scratch/'+row['target'])
for name in ['emitter-verify-layout-r490.py','emitter-apply-layout-followup-r493.py','emitter-test-preflight-fingerprint-r494.py','emitter-layout-validation-r495.py','emitter-layout-validation-r498.py','commit-emitter-layout-r499.py','emitter-final-layout-to-dry-r500.py','emitter-test-preflight-fingerprint-r503.py','emitter-target-roster-validation-r506.py','commit-emitter-layout-r508.py']:
 capture(Path('/tmp')/name,name)
labels=['488-layout-affected-libraries','492-layout-static-preconditions']+[r['label'] for r in prior['steps'] if r['label'].startswith('495-')]+[r['label'] for r in prior498['steps']]+['504-target-roster-workspace-units']+[r['label'] for r in report['steps']]
counts={}
for label in labels:
 p=B/'records/local'/f'{label}.json';v=json.loads(p.read_bytes());z=p.with_suffix('.log.gz');raw=gzip.decompress(z.read_bytes());assert v['exit']==(1 if label in ['495-layout-qualification','498-layout-workspace-audit'] else 0) and h(raw)==v['log_sha256']
 capture(p,'local/'+p.name);capture(z,'local/'+z.name)
 matches=re.findall(r'test result: ok\. (\d+) passed; 0 failed;',raw.decode())
 if matches:counts[label]=list(map(int,matches));assert min(counts[label])>0
assert len(counts['488-layout-affected-libraries'])==6
assert counts['498-layout-workspace-units']==[25]
assert counts['504-target-roster-workspace-units']==[26]
assert counts['506-target-roster-workspace-units']==[26]
assert counts['498-layout-de-units']==[2]
assert counts['498-layout-native-units']==[6]
put('source.patch.gz',gzip.compress(subprocess.check_output(['git','diff','HEAD'],cwd=R),mtime=0))
# Git diff does not include new files: retain all owned outlined modules and descriptor.
for path in sorted(subprocess.check_output(['git','ls-files','--others','--exclude-standard','--','crates','scripts'],cwd=R,text=True).splitlines()):capture(R/path,'new-source/'+path)
capture(Path(__file__),Path(__file__).name)
manifest=dict(schema=1,created_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),scope='Focused test-layout repair and frozen census identity preservation. Production token streams unchanged in14moved source files;15testmodules relocated,9fixture references unchanged. Gate implementation changes are separately exercised, including a70-file exact integration-target roster preserving alltest addresses and Cargo autodiscovery. Does not claim final walk, unsplitCI or delivery.',source_head=report['source_head'],frozen_source_sha256=proof['frozen_source_sha256'],runtime_inputs=920,tests=counts,retained_failures=['Original drywalk478 refused16layoutfindings beforemint; archive485','First scratch extraction found rustfmt optional punctuation; original failure retained','First canonical-byte proof found14leading-newline removals by cargo fmt; corrected full token proof retained','495 qualification49/50: known current-source hash stale aftertestmove; singlehash497fix;498all50green','498 workspaceaudit failed oldcap2 with33compiler targets; trustedbase already30. Exactroster502preservesall70targets;506actualaudit validates finalrepair'],reviews=[203,204,205,206,207],files=rows)
(O/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
for row in rows:assert h((O/row['path']).read_bytes())==row['sha256']
print(json.dumps(dict(archive=str(O),files=len(rows),bytes=sum(row['bytes'] for row in rows),tests=counts)))
