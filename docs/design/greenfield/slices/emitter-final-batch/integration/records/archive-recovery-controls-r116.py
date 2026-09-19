from pathlib import Path
import collections,json,gzip,hashlib,sys,base64,difflib
root=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-recovery-next')
base=root/'docs/design/greenfield/slices/emitter-final-batch/integration'
target=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target')
label=sys.argv[1]
expected={'recovery-new-controls-r116':74,'recovery-complete-boundaries-r116':1930,'system-recovery-native-r116':4088}[label]
receipt=json.loads((base/f'records/local/{label}.json').read_text())
manifest=json.loads((target/'emitter-r116-immutable/manifest.json').read_text())
assert receipt['head']==manifest['head'] and receipt['tracked_clean']
assert hashlib.sha256(Path(manifest['immutable_executable']).read_bytes()).hexdigest()==manifest['sha256']
assert hashlib.sha256((base/f'records/local/{label}.log').read_bytes()).hexdigest()==receipt['log_sha256']
captures=[json.loads(p.read_text()) for p in sorted((target/(label+'-captures')).glob('*.json'))]
by_case=collections.defaultdict(list)
for c in captures:by_case[c['case_id']].append(c)
assert len(by_case)==expected, (len(by_case),expected)
exact=[];failed=[];diffs=[]
for id,obs in sorted(by_case.items()):
 good=len(obs)==2 and all(o['actual']==o['expected'] and o['error'] is None for o in obs)
 (exact if good else failed).append(id)
 if not good:
  for o in obs:
   fields=[k for k in o['expected'] if (o['actual'] or {}).get(k)!=o['expected'][k]]
   diffs.append({'case_id':id,'capture_index':o['capture_index'],'fields':fields,'error':o['error'],'actual':o['actual'],'expected':o['expected']})
archive=gzip.compress((json.dumps(captures,ensure_ascii=False,separators=(',',':'))+'\n').encode(),mtime=0)
with (base/f'records/{label}-complete-captures.json.gz').open('xb') as f:f.write(archive)
summary={'head':receipt['head'],'binary':manifest,'records':len(captures),'cases':len(by_case),'exact':len(exact),'failed':len(failed),'failed_cases':failed,'counts_by_repetitions':dict(collections.Counter(len(v) for v in by_case.values())),'capture_archive_sha256':hashlib.sha256(archive).hexdigest(),'capture_archive_bytes':len(archive),'receipt_exit':receipt['exit'],'log_sha256':receipt['log_sha256']}
with (base/f'cross-review/{label}-capture-summary.json').open('x') as f:json.dump(summary,f,indent=2);f.write('\n')
with (base/f'cross-review/{label}-differences.json').open('x') as f:json.dump(diffs,f,indent=2,ensure_ascii=False);f.write('\n')
rendered=[]
for d in diffs:
 rendered.append(d['case_id']+' fields='+','.join(d['fields'])+' error='+str(d['error']))
 a={w['path']:w for w in (d['actual'] or {}).get('writes',[])};e={w['path']:w for w in d['expected']['writes']}
 for path in sorted(a.keys()|e.keys()):
  if path.endswith('.map'):continue
  def decode(w):return base64.b64decode(w.get('callback_utf8_base64','')).decode('utf8',errors='replace')
  av=decode(a.get(path,{}));ev=decode(e.get(path,{}))
  if av!=ev:rendered.extend(difflib.unified_diff(ev.splitlines(True),av.splitlines(True),fromfile='expected'+path,tofile='actual'+path))
with (base/f'cross-review/{label}-text-diffs.txt').open('x') as f:f.write('\n'.join(rendered)+'\n')
print(json.dumps(summary,indent=2));print('\n'.join(rendered))
assert bool(failed)==bool(receipt['exit']), 'receipt and complete tuple comparison disagree'
