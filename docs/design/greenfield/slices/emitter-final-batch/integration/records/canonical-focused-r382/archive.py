"""Archive actual canonical focused outcomes, including failures; never replace CI."""
from pathlib import Path
import gzip,hashlib,json,shutil,subprocess
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');T=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target');B=R/'docs/design/greenfield/slices/emitter-final-batch/integration';source=T/'emitter-canonical-focused-r349';report=json.loads((source/'manifest.json').read_bytes());O=B/'records/canonical-focused-r382'
assert len(report['steps'])==16 or any(not s['checks_passed'] and s['label'] in ['canonical-focused-format-r349','canonical-focused-emitter-all-r349'] for s in report['steps']), 'pipeline not complete'
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip()==report['head']
assert not subprocess.check_output(['git','diff','HEAD','--name-only'],cwd=R).strip()
O.mkdir(exist_ok=False);files=[]
def put(name,data):
 p=O/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(data);files.append({'path':name,'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()})
put('focused-manifest.json',(source/'manifest.json').read_bytes());put('runner.py',Path('/tmp/emitter-canonical-focused-r349.py').read_bytes());put('archive.py',Path(__file__).read_bytes())
for step in report['steps']:
 label=step['label'];rp=B/'records/local'/(label+'.json');r=json.loads(rp.read_bytes());raw=gzip.decompress(rp.with_suffix('.log.gz').read_bytes())
 assert r['head']==report['head'] and r['exit']==step['exit']
 assert hashlib.sha256(raw).hexdigest()==r['log_sha256'] and len(raw)==r['log_bytes']
 put('local/'+rp.name,rp.read_bytes());put('local/'+rp.with_suffix('.log.gz').name,rp.with_suffix('.log.gz').read_bytes())
for path in [Path('/tmp/emitter-round188-request.md'),Path('/tmp/emitter-claude-review-round188-opus.json'),*sorted(Path('/tmp/emitter-corpus-proof-review-r385').iterdir()),*sorted(Path('/tmp/emitter-remote-state-r383').iterdir()),*sorted(Path('/tmp/emitter-canonical-pin-precheck-r386').iterdir())]:
 if path.is_file():put('preparation/'+path.parent.name+'/'+path.name,path.read_bytes())
put('preparation/emitter-canonical-corpus-r375.py',Path('/tmp/emitter-canonical-corpus-r375.py').read_bytes())
(O/'manifest.json').write_text(json.dumps({'head':report['head'],'qualified_focused_scope':report['qualified'],'qualified_final':False,'files':files},indent=2)+'\n')
print(json.dumps({'archive':str(O),'steps':len(report['steps']),'qualified_focused':report['qualified'],'files':len(files)}))
