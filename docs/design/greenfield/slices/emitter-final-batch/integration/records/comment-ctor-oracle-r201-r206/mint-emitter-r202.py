from pathlib import Path
import json,gzip,subprocess,hashlib
r=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');b=r/'docs/design/greenfield/slices/emitter-final-batch/integration';archive=b/'records/comment-ctor-controls-r194-r196-complete/predecessor';changes=[]
for n,count in [(168,672),(171,344)]:
 p=r/f'crates/compiler/tests/fixtures/emitter-r{n}-corpus-controls.json';raw=gzip.decompress((archive/(p.name+'.gz')).read_bytes());assert p.read_bytes()==raw;old=json.loads(raw);p.unlink()
 subprocess.run(['python3',str(b/'run-local.py'),f'comment-ctor-oracle-{n}-r202','node',f'scripts/observe-emitter-r{n}-corpus-controls.mjs','--write'],cwd=r,check=True)
 new=json.loads(p.read_text());by={c['case_id']:c for c in new['cases']};assert len(by)==count
 for c in old['cases']:
  after=by[c['case_id']]
  if c==after:continue
  assert n==168 and c['case_id'].endswith('/detached-bom-unicode') and '/remove-false/' in c['case_id'],c['case_id']
  assert {k:v for k,v in c.items() if k!='typescript_observation'}=={k:v for k,v in after.items() if k!='typescript_observation'}
  before=c['typescript_observation'];observed=after['typescript_observation'];assert {k:v for k,v in before.items() if k not in ['writes','emit_result']}=={k:v for k,v in observed.items() if k not in ['writes','emit_result']}
  assert {k:v for k,v in before['emit_result'].items() if k!='source_maps'}=={k:v for k,v in observed['emit_result'].items() if k!='source_maps'}
  assert len(before['writes'])==len(observed['writes'])
  for a,z in zip(before['writes'],observed['writes']):
   if a==z:continue
   assert a['path'].endswith('/main.js.map')
   keys={'callback_utf8_base64','callback_utf8_bytes','materialized_utf8_base64','materialized_utf8_bytes'}
   assert {k:v for k,v in a.items() if k not in keys}=={k:v for k,v in z.items() if k not in keys}
  changes.append({'case_id':c['case_id'],'before':before,'after':observed})
 print(f'r{n}: retained {len(old["cases"])} input tuples, added {count-len(old["cases"])}; observation corrections total {len(changes)}',flush=True)
assert len(changes)==4
Path('/tmp/emitter-bom-observer-correction-r202.json').write_text(json.dumps({'reason':'Match TypeScript sys.readFile and native byte-host decode; strip exactly one initial BOM','proof':'bom-host-protocol-r201','changes':changes},indent=2)+'\n')
