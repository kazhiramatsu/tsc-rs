from pathlib import Path
import gzip,hashlib,json,subprocess
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');B=R/'docs/design/greenfield/slices/emitter-final-batch/integration';O=B/'records/module-boundary-review-r419';head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip()
assert head=='8859338f1c761d867cbc7f5ef58987dc3777f944'
assert not subprocess.check_output(['git','diff','HEAD','--name-only'],cwd=R).strip()
for name in ['r404','r412']:
 d=json.loads(Path('/tmp/emitter-import-map-review-'+name+'/response.json').read_bytes());assert d.get('result') and not d.get('is_error'),name
O.mkdir(exist_ok=False);files=[]
def put(name,data):
 p=O/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(data);files.append({'path':name,'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()})
roots=['emitter-import-map-review-r404','emitter-import-map-review-r412','emitter-import-command-probe-r405','emitter-import-command-probe-r406','emitter-import-command-probe-r409','emitter-module-boundary-observe-r408','emitter-retained-modifier-probe-r407','emitter-module-boundary-final-r416','emitter-module-controls-r414','emitter-declaration-route-test-r401','emitter-utf16-capture-r423','emitter-utf16-retirement-r424','emitter-historical-source-promotion-r426']
for name in roots:
 base=Path('/tmp')/name
 for p in sorted(base.rglob('*')):
  if not p.is_file() or p.name=='probe' or (name=='emitter-module-boundary-final-r416' and 'crates' in p.relative_to(base).parts):continue
  data=p.read_bytes();rel=name+'/'+str(p.relative_to(base))
  if p.suffix=='.log' or p.name=='oracle.json':rel+='.gz';data=gzip.compress(data,mtime=0)
  put(rel,data)
for name in ['emitter-historical-source-promotion-r426.py','emitter-utf16-capture-r423.py','prepare-emitter-utf16-retirement-r424.py','apply-emitter-utf16-retirement-r425.py','emitter-final-prerequisites-r402.json','emitter-module-boundary-summary-r410.json','emitter-leading-comment-boundary-r415.json','emitter-module-boundary-next-r417.md','emitter-import-command-probe-r405.py','emitter-import-command-probe-r406.py','emitter-module-command-probe-r409.py','emitter-module-boundary-observe-r408.py','emitter-retained-modifier-probe-r407.mjs','prepare-emitter-module-boundary-repair-r413.py','prepare-emitter-module-boundary-final-r416.py','prepare-emitter-module-controls-r414.py']:
 put('prepared/'+name,(Path('/tmp')/name).read_bytes())
put('archive.py',Path(__file__).read_bytes())
(O/'manifest.json').write_text(json.dumps({'head':head,'qualified_final':False,'qualified_native_repair':False,'scope':'Preserved failures and proposed module declaration repair. Canonical396 discovered duplicate side-effect import comments; adjacent12 complete TS commands x2 versus same-library native capture all fail. SourceLeading and declaration-specific modifier/anchor proposal not yet applied or qualified. Opus191 mistaken collector/parameter proposal is explicitly retracted in192 using actual TS415; typed export factory is adopted as an explicit upstream contract, not evidence that returning the unchanged current node would drop earlier-pass children. Probe405 setup failure preserved;406/409 actual comparator exit101. No parser/checker/shared map changes proposed. AdditionalH2.1a decoratorOnUsing sourcepromotionscope remainsunderactivityreview;426retainsalleightremainingcandidatesanddoesnotqualifytheactivitymismatch.','files':files},indent=2)+'\n')
print(json.dumps({'archive':str(O),'files':len(files),'bytes':sum(f['bytes'] for f in files)}))
