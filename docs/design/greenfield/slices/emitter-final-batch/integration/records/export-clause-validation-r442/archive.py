from pathlib import Path
import gzip,hashlib,json,subprocess
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');T=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target');B=R/'docs/design/greenfield/slices/emitter-final-batch/integration';S=T/'emitter-module-validation-r441';O=B/'records/export-clause-validation-r442';report=json.loads((S/'manifest.json').read_bytes());h=lambda b:hashlib.sha256(b).hexdigest()
assert len(report['steps'])==13 or report['steps'] and not report['steps'][-1]['checks_passed'];assert h(subprocess.check_output(['git','diff','HEAD'],cwd=R))==report['candidate_diff_sha256'];assert h(gzip.decompress((S/'candidate.patch.gz').read_bytes()))==report['candidate_diff_sha256'];O.mkdir(exist_ok=False);files=[]
def put(name,data):
 p=O/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(data);files.append({'path':name,'bytes':len(data),'sha256':h(data)})
put('validation-manifest.json',(S/'manifest.json').read_bytes());put('candidate.patch.gz',(S/'candidate.patch.gz').read_bytes());put('runner.py',Path('/tmp/emitter-module-validation-r441.py').read_bytes());put('retained-archive-audit-r446.json',Path('/tmp/emitter-retained-archive-audit-r446.json').read_bytes());put('archive.py',Path(__file__).read_bytes())
labels=[s['label'] for s in report['steps']]+['export-clause-oracle-r439','export-clause-format-r439','export-clause-build-r440','export-clause-resume-guard-r440','export-clause-resume-guard-r443']
for label in labels:
 rp=B/'records/local'/(label+'.json');r=json.loads(rp.read_bytes());raw=gzip.decompress(rp.with_suffix('.log.gz').read_bytes());assert h(raw)==r['log_sha256'] and len(raw)==r['log_bytes']
 if label.endswith('-r441'):assert r['head']==report['head'] and r['diff_sha256']==report['candidate_diff_sha256'];assert r['exit']==next(s['exit'] for s in report['steps'] if s['label']==label)
 put('local/'+rp.name,rp.read_bytes());put('local/'+rp.with_suffix('.log.gz').name,rp.with_suffix('.log.gz').read_bytes())
for directory in ['emitter-namespace-prefix-review-r436','emitter-export-clause-r437','emitter-export-clause-controls-r438','emitter-export-clause-apply-r439','emitter-export-clause-focused-r440','emitter-export-clause-guard-r443']:
 root=Path('/tmp')/directory
 for p in sorted(root.rglob('*')):
  if not p.is_file() or p.name=='probe':continue
  data=p.read_bytes();name=directory+'/'+p.relative_to(root).as_posix()
  if len(data)>100000 and p.suffix!='.gz':name+='.gz';data=gzip.compress(data,mtime=0)
  put(name,data)
for name in ['prepare-emitter-export-clause-r437.py','prepare-emitter-export-clause-controls-r438.py','apply-emitter-export-clause-r439.py','emitter-export-clause-focused-r440.py','emitter-export-clause-guard-r443.py']:
 put('scripts/'+name,(Path('/tmp')/name).read_bytes())
(O/'manifest.json').write_text(json.dumps({'head':report['head'],'candidate_diff_sha256':report['candidate_diff_sha256'],'qualified_focused_scope':report['qualified'],'qualified_final':False,'prior_failed_validation':'../module-boundary-validation-r433/manifest.json','scope':'Fresh export-clause comment owner projection only; global strict resume checks unchanged. Includes actual Opus195, corrected review rationale, 20 appended complete TypeScript cases preserving all prior245, focused36 complete native commandsx2 plus72fullcaptures, zero-test guard attempt440 refused and correct single guard443 passed, and frozen successor validation. Original422407/409 with2failures remains retained and unqualified. Final corpus, walk and unsplit CI remain required.','files':files},indent=2)+'\n');print(json.dumps({'archive':str(O),'files':len(files),'qualified_focused':report['qualified']}))
