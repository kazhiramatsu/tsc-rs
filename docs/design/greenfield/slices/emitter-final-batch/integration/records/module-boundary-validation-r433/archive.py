from pathlib import Path
import argparse,gzip,hashlib,json,subprocess
p=argparse.ArgumentParser();p.add_argument('--out-name',default='module-boundary-validation-r433');a=p.parse_args()
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');T=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target');B=R/'docs/design/greenfield/slices/emitter-final-batch/integration';S=T/'emitter-module-validation-r422';report=json.loads((S/'manifest.json').read_bytes());O=B/'records'/a.out_name
assert len(report['steps'])==13 or report['steps'] and not report['steps'][-1]['checks_passed'],'pipeline still active'
h=lambda b:hashlib.sha256(b).hexdigest();diff=subprocess.check_output(['git','diff','HEAD'],cwd=R);assert h(diff)==report['candidate_diff_sha256'];assert gzip.decompress((S/'candidate.patch.gz').read_bytes())==diff
O.mkdir(exist_ok=False);files=[]
def put(name,data):
 dest=O/name;dest.parent.mkdir(parents=True,exist_ok=True);dest.write_bytes(data);files.append({'path':name,'bytes':len(data),'sha256':h(data)})
put('validation-manifest.json',(S/'manifest.json').read_bytes());put('candidate.patch.gz',(S/'candidate.patch.gz').read_bytes());put('runner.py',Path('/tmp/emitter-module-validation-r422.py').read_bytes());put('archive.py',Path(__file__).read_bytes())
labels=[s['label'] for s in report['steps']]+['module-boundary-oracle-r420','module-boundary-format-r420','utf16-retirement-oracle-r425','utf16-retirement-oracle-r430','source-promotion-format-r432']
for label in labels:
 rp=B/'records/local'/(label+'.json');receipt=json.loads(rp.read_bytes());raw=gzip.decompress(rp.with_suffix('.log.gz').read_bytes());assert h(raw)==receipt['log_sha256'] and len(raw)==receipt['log_bytes']
 if label.startswith('module-validation-'):
  assert receipt['head']==report['head'] and receipt['diff_sha256']==report['candidate_diff_sha256'];assert receipt['exit']==next(s['exit'] for s in report['steps'] if s['label']==label)
 put('local/'+rp.name,rp.read_bytes());put('local/'+rp.with_suffix('.log.gz').name,rp.with_suffix('.log.gz').read_bytes())
for directory in ['emitter-module-boundary-apply-r420','emitter-utf16-retirement-apply-r425','emitter-acceptance-activity-review-r427','emitter-decorator-provenance-r428','emitter-source-promotion-r429','emitter-utf16-retirement-resume-r430','emitter-source-promotion-review-r431','emitter-source-promotion-apply-r432','emitter-static-readiness-r434']:
 root=Path('/tmp')/directory;assert root.is_dir()
 for path in sorted(root.rglob('*')):
  if not path.is_file() or path.name=='probe':continue
  data=path.read_bytes();name=directory+'/'+path.relative_to(root).as_posix()
  if len(data)>100000 and path.suffix!='.gz':name+='.gz';data=gzip.compress(data,mtime=0)
  put(name,data)
for name in ['emitter-decorator-provenance-r428.py','prepare-emitter-source-promotion-r429.py','emitter-utf16-retirement-resume-r430.py','apply-emitter-source-promotion-r432.py']:
 put('scripts/'+name,(Path('/tmp')/name).read_bytes())
put('comment-range-migration-note.md',Path('/tmp/emitter-comment-range-migration-note-r397.md').read_bytes())
manifest={'head':report['head'],'candidate_diff_sha256':report['candidate_diff_sha256'],'qualified_focused_scope':report['qualified'],'qualified_final':False,'scope':'Actual applied import/export repair and original refusal retirements, same frozen working diff across all validation stages. Records source provenance, actual Opus193/194, full TypeScript observations, failed exclusive-create mint425 and successful430. Prior396failures remain in398. Final corpus replay, sanctioned walk and unsplit CI remain separate obligations.','files':files};(O/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n');print(json.dumps({'archive':str(O),'files':len(files),'qualified_focused':report['qualified']}))
