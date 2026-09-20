from pathlib import Path
import json,gzip,hashlib,subprocess
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');T=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target');O=R/'docs/design/greenfield/slices/emitter-final-batch/integration/records/body-comment-review-r366';items=[]
assert json.loads((T/'emitter-system-eof-full-command-r342/manifest.json').read_text())['test_files_restored']
def add(p,name):
 p=Path(p);data=p.read_bytes()
 if p.suffix=='.log':data=gzip.compress(data,mtime=0);name+='.gz'
 if p.suffix=='.rs':data=gzip.compress(data,mtime=0);name+='.gz'
 items.append((name,data,str(p)))
for n in [184,185]:
 p=Path(f'/tmp/emitter-claude-review-round{n}-opus.json');assert json.loads(p.read_text()).get('result');add(p,'reviews/'+p.name)
 p=Path(f'/tmp/emitter-round{n}-request.md');add(p,'reviews/'+p.name)
for folder in [T/'emitter-system-eof-full-command-r342',T/'emitter-system-comments-controls-mint-r353']+[Path('/tmp')/name for name in ['emitter-function-body-flags-probe-r356','emitter-function-body-flags-probe-r357','emitter-function-body-flags-probe-r358','emitter-body-comments-r359','emitter-body-flags-controls-mint-r360','emitter-body-final-r361','emitter-body-failure-mint-r362','emitter-body-consumers-r363']]:
 for p in sorted(folder.rglob('*')):
  if p.is_file() and p.suffix in ['.json','.gz','.rs','.mjs','.patch','.py','.log']:
   if p.suffix=='.log' and p.with_suffix('.log.gz').exists():continue
   add(p,folder.name+'/'+p.relative_to(folder).as_posix())
for p in [Path('/tmp')/name for name in ['emitter-system-eof-full-command-r342.py','emitter-system-comments-controls-mint-r353.py','emitter-function-body-flags-probe-r356.py','emitter-function-body-flags-probe-r357.py','emitter-function-body-flags-probe-r358.py','prepare-emitter-body-comments-r359.py','emitter-body-flags-controls-mint-r360.py','prepare-emitter-body-final-r361.py','emitter-body-failure-mint-r362.py','emitter-body-consumers-r363.py','emitter-pr561-status-r355.json','emitter-remote-main-r355.txt']]:add(p,'scripts/'+p.name)
add(__file__,'archive.py');O.mkdir(exist_ok=False);index=[]
for name,data,source in items:
 p=O/name;assert not p.exists();p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(data);index.append({'path':name,'source':source,'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()})
report={'canonical_head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip(),'qualified':False,'scope':'342complete10steps:context680/684exactx2+4Systemtrivia failures, other9bands green (allold8mapfailuresresolved). 353TSadditional64controls,360TS25flags,362TS1serializedfailure allpreserveoldobservations. 356 rawpreservefastpath and357 unsupportedrecording are unsuccessfulsetups, NOTflagsproof;358Canonicalrecording witnessesbodyNoTrailing+NoNested differences (JScomparisononly). Actual184/185 review and359/361drafts,363consumers; fixednative364/365 andactual186 NOTpartofthisarchive. No finalqualificationclaim.','files':index}
(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({'archive':str(O),'files':len(index),'bytes':sum(r['bytes'] for r in index)}))
