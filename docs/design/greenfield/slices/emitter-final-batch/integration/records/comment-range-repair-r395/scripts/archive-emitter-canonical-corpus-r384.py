"""Archive completed fresh canonical parser and command evidence; no mint or gate."""
from pathlib import Path
import gzip,hashlib,json,subprocess
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');T=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target');B=R/'docs/design/greenfield/slices/emitter-final-batch/integration';S=T/'emitter-canonical-corpus-r375';O=B/'records/canonical-corpus-r384'
h=lambda b:hashlib.sha256(b).hexdigest()
def load(p):return json.loads(p.read_bytes())
r=load(S/'manifest.json');assert all(r[k] for k in ['parser_qualified','commands_qualified','projects_qualified']);assert len(r['steps'])==9 and all(s['exit']==0 for s in r['steps'])
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip()==r['head']
assert not subprocess.check_output(['git','diff','HEAD','--name-only'],cwd=R).strip()
sel=load(S/'selection.json');succ=load(S/'successor.json');assert len(sel['cases'])==48 and len(sel['load_failures'])==110
assert succ['build']['parser_head']==r['head'] and succ['build']['wrapper_sha256']==h(Path('/tmp/emitter-canonical-corpus-r375.py').read_bytes())
a=load(S/'comparison.json');b=load(S/'projects-comparison.json');assert a['summary']=={'selected':48,'dispositions':{'complete-command-exact':46,'no-emit-command-exact; emit-not-qualified':2},'unloaded':110},a['summary'];assert b['summary']=={'selected':108,'dispositions':{'complete-command-exact':108}},b['summary']
assert h((S/'xtask').read_bytes())==r['native_sha256']
O.mkdir(exist_ok=False);files=[]
def put(name,data):
 p=O/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(data);files.append({'path':name,'bytes':len(data),'sha256':h(data)})
for p in sorted(S.iterdir()):
 if not p.is_file() or p.name=='xtask' or p.suffix=='.log':continue
 data=p.read_bytes();name=p.name
 if p.suffix=='.json' and p.name not in ['manifest.json','resolved-dependencies.json']:
  name+='.gz';data=gzip.compress(data,mtime=0)
 put('run/'+name,data)
for step in r['steps']:
 data=gzip.decompress((S/(step['label']+'.log.gz')).read_bytes());assert h(data)==step['log_sha256']
for name in ['Cargo.toml','Cargo.lock']:put('build/'+name,(S/'build'/name).read_bytes())
put('runner.py',Path('/tmp/emitter-canonical-corpus-r375.py').read_bytes());put('archive.py',Path(__file__).read_bytes())
(O/'manifest.json').write_text(json.dumps({'head':r['head'],'qualified_parser_scope':True,'qualified_selected_commands_scope':True,'qualified_project_supplement_scope':True,'qualified_final':False,'limits':['110 original load failures remain unqualified.','Two selected noEmit controls are not emit proof.','Project supplement does not replace original failed loads.','Final unsplit CI and hosted merge remain required.'],'files':files},indent=2)+'\n')
print(json.dumps({'archive':str(O),'files':len(files),'bytes':sum(x['bytes'] for x in files)}))
