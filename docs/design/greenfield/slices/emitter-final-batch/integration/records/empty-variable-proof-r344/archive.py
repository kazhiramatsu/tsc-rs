from pathlib import Path
import json,gzip,hashlib,subprocess
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep')
T=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target')
O=R/'docs/design/greenfield/slices/emitter-final-batch/integration/records/empty-variable-proof-r344'
proof=json.loads((T/'emitter-combined-recovery-successor-r330/manifest.json').read_bytes())
assert proof['parser_qualified'] and not proof['commands_qualified'] and not proof['new_inputs']
r336=json.loads((T/'emitter-empty-variable-full-command-r336/manifest.json').read_bytes())
assert r336['test_files_restored'] and not r336['qualified']
assert [s['exit'] for s in r336['steps']]==[101,0,0]
assert json.loads((T/'emitter-system-range-controls-mint-r339/manifest.json').read_bytes())['old588_and72_unchanged']
items=[]
def add(p,name=None):
 p=Path(p);data=p.read_bytes();name=name or p.parent.name+'/'+p.name
 if p.suffix=='.log' or (p.suffix=='.json' and len(data)>500000):data=gzip.compress(data,mtime=0);name+='.gz'
 items.append((name,data,str(p)))
for name in ['emitter-empty-variable-prototype-r325','emitter-combined-recovery-proof-r327','emitter-combined-recovery-proof-r330','emitter-empty-variable-controls-r331','emitter-empty-variable-witness-controls-r334','emitter-system-range-controls-r339']:
 folder=Path('/tmp')/name
 for p in sorted(folder.iterdir()):
  if p.is_file():add(p)
for label in ['emitter-empty-variable-flags-r324','emitter-empty-variable-controls-mint-r328','emitter-empty-variable-controls-mint-r331','emitter-leading-binding-full-command-r332','emitter-empty-variable-full-command-r335','emitter-empty-variable-full-command-r336','emitter-system-range-controls-mint-r339']:
 for p in sorted((T/label).iterdir()):
  if p.is_file() and (p.suffix in ['.json','.gz','.rs','.mjs','.py'] or p.suffix=='.log' and not p.with_suffix('.log.gz').exists()):add(p)
for label in ['emitter-combined-recovery-successor-r327','emitter-combined-recovery-successor-r330']:
 for name in ['manifest.json','steps.json','resolved-dependencies.json','build.log','test.log','replay.stderr.log','successor.json','selection.json']:
  p=T/label/name
  if p.exists():add(p)
for label in [178,179,180]:
 for name in [f'emitter-round{label}-request.md',f'emitter-claude-review-round{label}-opus.json']:
  add(Path('/tmp')/name,'reviews/'+name)
for name in ['emitter-empty-variable-flags-r324.py','emitter-empty-variable-controls-mint-r328.py','emitter-empty-variable-controls-mint-r331.py','emitter-leading-binding-full-command-r332.py','emitter-empty-variable-full-command-r335.py','emitter-empty-variable-full-command-r336.py','emitter-system-execute-ranges-r338.mjs','emitter-system-execute-ranges-r338.json','emitter-system-range-controls-mint-r339.py']:
 add(Path('/tmp')/name,'scripts/'+name)
add(__file__,'archive.py')
O.mkdir(exist_ok=False);index=[]
for name,data,source in items:
 p=O/name;assert not p.exists();p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(data)
 index.append({'path':name,'source':source,'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()})
report={'canonical_head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip(),'prototype_head':proof['head'],'scope':'Combined B/C syntax16994 proof preserves original AST/diagnostics/raw recovery and all profiles, zero new corpus admissions. 110 original loader failures remain unqualified. B80 full commands x2 pass. Empty-variable96 commands have88 exact and8 System JS map failures (r336), both frozen recording/production suites pass. Official context mint684+72 keeps original rows unchanged; new System/EOF fix and full684 native qualification PENDING and not covered by this archive. Actual Opus178-180 reviews included; AST probes338 are metadata exploration only. Failed335 test preparation retained.','qualified':False,'files':index}
(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({'archive':str(O),'files':len(index),'bytes':sum(x['bytes'] for x in index)}))
