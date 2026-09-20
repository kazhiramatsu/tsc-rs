from pathlib import Path
import json,gzip,hashlib,subprocess,re
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');T=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target');O=R/'docs/design/greenfield/slices/emitter-final-batch/integration/records/body-comment-complete-r379';items=[]
report376=json.loads((T/'emitter-body-commands-r376/manifest.json').read_text());assert report376['qualified'] and report376['test_files_restored'] and len(report376['steps'])==6
assert json.loads((T/'emitter-body-commands-r365/manifest.json').read_text())['test_files_restored']
report378=json.loads((T/'emitter-empty-body-check-r378/manifest.json').read_text());assert report378['qualified']
for label,count in [('system160',1),('syntax-controls',2),('context748',1),('rows-and-frozen-bundle',2),('recording-witnesses',4),('production-witnesses',4)]:
 s=(T/'emitter-body-commands-r376'/(label+'.log')).read_text();assert re.search(r'test result: ok\. '+str(count)+r' passed; 0 failed; 0 ignored;',s),label
s=(T/'emitter-empty-body-check-r378/native.log').read_text();assert 'SUMMARY exact=72 failed=0 selected=72' in s and 'test result: ok. 1 passed; 0 failed; 0 ignored;' in s
for label,count in [('system160',160),('context748',748)]:
 s=(T/'emitter-body-commands-r376'/(label+'.log')).read_text();assert f'SUMMARY exact={count} failed=0 selected={count}' in s;assert len(re.findall('context recovery EXACT x2 ',s))==count

def add(p,name):
 p=Path(p);data=p.read_bytes()
 if p.suffix=='.log':data=gzip.compress(data,mtime=0);name+='.gz'
 items.append((name,data,str(p)))
for folder in [T/'emitter-body-commands-r365',T/'emitter-body-commands-r376',T/'emitter-empty-body-check-r378',Path('/tmp/emitter-integration-drycheck-r377')]:
 for p in sorted(folder.rglob('*')):
  if p.is_file() and p.suffix in ['.json','.log','.gz']:
   if p.suffix=='.log' and p.with_suffix('.log.gz').exists():continue
   add(p,folder.name+'/'+p.relative_to(folder).as_posix())
for name in ['emitter-body-commands-r365.py','emitter-body-commands-r376.py','emitter-empty-body-check-r378.py','emitter-integrate-final-r371.py','emitter-canonical-focused-r349.py','emitter-canonical-corpus-r375.py']:
 add('/tmp/'+name,'scripts/'+name)
add(__file__,'archive.py');O.mkdir(exist_ok=False);index=[]
for name,data,source in items:
 p=O/name;assert not p.exists();p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(data);index.append({'path':name,'source':source,'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()})
report={'canonical_head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip(),'private_head':report376['head'],'qualified_private_scope':True,'qualified_final':False,'scope':'Final private System/currentarray + EOFmap/typedguard + functionbodycomments production:160focusedthenall748completecommandsx2,72refusalcontrols,21originalrows+2frozenSystemoutFile,8mapwitness testsALLPASS376. Existing72empty-block completecommandsx2PASS378. Original515lib/172flags/22failureproofs in374. 365temporaryselector compilefailureZEROcommands retained honestly. 371integration+349canonicalfocused+375freshcanonicalcorpus are DRAFTONLY, notrun; canonical/walk/unsplitCI/hosted/mergepending. No source/ASTbound relaxation.','files':index};(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({'archive':str(O),'files':len(index),'bytes':sum(r['bytes'] for r in index)}))
