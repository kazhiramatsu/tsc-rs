from pathlib import Path
import argparse,json,base64,collections
p=argparse.ArgumentParser();p.add_argument('directory',type=Path);a=p.parse_args();groups={}
for f in sorted(a.directory.glob('*.json')):
 try:d=json.loads(f.read_text())
 except json.JSONDecodeError:continue
 groups.setdefault(d['case_id'],[]).append(d)
fails={i:rs for i,rs in groups.items() if any(r['error'] is not None or r['actual']!=r['expected'] for r in rs)}
print(json.dumps({'captured_cases':len(groups),'exact_twice':sum(len(rs)==2 and all(r['error'] is None and r['actual']==r['expected'] for r in rs) for rs in groups.values()),'failed_cases':len(fails),'failure_shapes':dict(collections.Counter(i.split('/')[-1] for i in fails))},indent=2))
seen=set()
for i,rs in fails.items():
 shape=i.split('/')[-1]
 if shape in seen:continue
 seen.add(shape);r=next(r for r in rs if r['error'] is not None or r['actual']!=r['expected']);print('CASE',i,'ERROR',r['error'])
 if r['error'] is not None:continue
 actual,expected=r['actual'],r['expected'];print('different top-level keys',[k for k in expected if expected[k]!=actual.get(k)])
 for k in ['reported_diagnostics','emit_refused','exit_code']:
  if actual.get(k)!=expected[k]:print(k,'native',actual.get(k),'ts',expected[k])
 aw=actual['writes'];ew=expected['writes']
 if len(aw)!=len(ew):print('write counts',len(aw),len(ew));continue
 for x,y in zip(aw,ew):
  if x==y:continue
  print('WRITE',y['path'],'keys',[k for k in y if x.get(k)!=y[k]])
  if x.get('callback_utf8_base64')!=y['callback_utf8_base64']:
   print('NATIVE',base64.b64decode(x['callback_utf8_base64']).decode(errors='replace'))
   print('TS',base64.b64decode(y['callback_utf8_base64']).decode(errors='replace'))
