"""Review actual official writer outputs; not a new-key check certificate."""
import gzip,hashlib,json,re
from pathlib import Path
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep')
P=Path('/tmp/emitter-5g-repair-r536')
B=R/'docs/design/greenfield/slices/emitter-final-batch/integration/records/local'
receipt=json.loads((B/'g5-library-reuse-write-r539.json').read_text())
raw=gzip.decompress((B/'g5-library-reuse-write-r539.log.gz').read_bytes())
assert receipt['exit']==0 and receipt['head']=='bb01b86df0a2d266910d25124c4bfbade334c826'
assert hashlib.sha256(raw).hexdigest()==receipt['log_sha256'] and len(raw)==receipt['log_bytes']
assert b'candidates=9027 admitted=8511 deferred=516 reused_observations=9023' in raw
old_bytes=gzip.decompress((P/'before/ratchets/h2-5g-qualification.v1.json.gz').read_bytes())
new_bytes=(R/'ratchets/h2-5g-qualification.v1.json').read_bytes()
a,b=json.loads(old_bytes),json.loads(new_bytes)
assert len(a['cases'])==len(b['cases'])==9027
assert a['summary']==b['summary'] and a['execution_contract']==b['execution_contract']
assert b['generator']['sha256']==hashlib.sha256((R/'crates/oracle/h2-5g-qualification.mjs').read_bytes()).hexdigest()
expected={row['case_id']:row for row in json.loads(Path('/tmp/emitter-5g-journal-comparison-r532.json').read_text())['differences']}
changed=[];leaves=[]
def differences(a,b,p=''):
 if type(a)!=type(b): return [(p,a,b)]
 if isinstance(a,dict):
  assert a.keys()==b.keys(),p
  return [z for k in a for z in differences(a[k],b[k],p+'/'+k)]
 if isinstance(a,list):
  assert len(a)==len(b),p
  return [z for i,(x,y) in enumerate(zip(a,b)) for z in differences(x,y,p+'/'+str(i))]
 return [] if a==b else [(p,a,b)]
for x,y in zip(a['cases'],b['cases']):
 assert x['case_id']==y['case_id']
 if x==y:
  assert json.dumps(x)==json.dumps(y)
  continue
 assert y['case_id'] in expected
 assert y==expected[y['case_id']]['new'],y['case_id']
 diff=differences(x,y)
 for path,left,right in diff:
  if re.fullmatch(r'/typescript_observation/reported_diagnostics/\d+/file',path):
   assert left=='/Users/hiramatsu/dev/tsc-rs/vendor/typescript-6.0.3/lib/lib.es5.d.ts'
   assert right==str(R/'vendor/typescript-6.0.3/lib/lib.es5.d.ts')
   leaves.append({'case_id':y['case_id'],'path':path,'before':left,'after':right})
  else:
   assert path in ['/case_fingerprint_sha256','/typescript_observation/run_fingerprint_sha256','/typescript_run_fingerprints/0','/typescript_run_fingerprints/1'],path
 changed.append({'case_id':y['case_id'],'differing_leaves':len(diff)})
assert len(changed)==4 and len(leaves)==17
assert set(a)==set(b)
assert [k for k in a if a[k]!=b[k]]==['generator','cases','qualification_fingerprint_sha256']
result={'qualified':False,'meaning':'Actual official --write freshly observed four cases and reused 9023; only 17 raw lib diagnostic path leaves and derived hashes changed. Full new-key 9027 double observation remains required. Agreement with old-key failed-run journals is comparison evidence only, never new-key adoption.','source_head':receipt['head'],'writer_receipt':receipt,'old_artifact_sha256':hashlib.sha256(old_bytes).hexdigest(),'new_artifact_sha256':hashlib.sha256(new_bytes).hexdigest(),'generator_sha256':b['generator']['sha256'],'compared_cases':9027,'unchanged_cases':9023,'changed_cases':changed,'path_leaves':leaves,'summary_unchanged':True,'execution_contract_unchanged':True}
(P/'official-write-review-r540.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps({k:result[k] for k in ['qualified','compared_cases','unchanged_cases','changed_cases','summary_unchanged','execution_contract_unchanged']},indent=2))
