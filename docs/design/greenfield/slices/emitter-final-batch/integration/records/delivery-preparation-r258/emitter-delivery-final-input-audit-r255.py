"""Read final-ref bytes for the D-only qualification record; never qualify by itself."""
import argparse, hashlib, json, subprocess
from pathlib import Path

p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--tree',type=Path,required=True)
p.add_argument('--validation',required=True)
p.add_argument('--merge')
p.add_argument('--out',type=Path,required=True)
a=p.parse_args()
def git(*args):return subprocess.check_output(['git','-C',str(a.tree),*args],stderr=subprocess.PIPE)
v=git('rev-parse',a.validation+'^{commit}').decode().strip()
cache={}
def contents(path):
 assert not Path(path).is_absolute() and '..' not in Path(path).parts,path
 if path not in cache:cache[path]=git('show',f'{v}:{path}')
 return cache[path]
def sha(data):return hashlib.sha256(data).hexdigest()
base='docs/design/greenfield/slices/emitter-final-batch/integration'
# This preparation is archived into V before its results may be used for D.
owner_path=base+'/records/delivery-preparation-r251/emitter-delivery-owner-identities-r249.json'
try:
 owners=json.loads(contents(owner_path))
except subprocess.CalledProcessError:
 # Pre-walk preparation only; mark this distinction explicitly.
 owners=json.loads(Path('/tmp/emitter-delivery-owner-identities-r249.json').read_bytes())
 owner_path='scratch-only /tmp/emitter-delivery-owner-identities-r249.json'
source=contents(owners['source_path'])
assert sha(source)==owners['source_sha256']
units=source.decode('utf-8').encode('utf-16-le')
def verify_owner(owner):
 for key,digest in [('source_range','declaration_sha256'),('body_range','body_sha256')]:
  span=owner[key];start=span['start']['offset'];end=span['end']['offset']
  text=units[start*2:end*2].decode('utf-16-le')
  assert sha(text.encode('utf-8'))==owner[digest],(owner['lexical_path'],key)
 return {'name':owner['name'],'lexical_path':owner['lexical_path'],'source_range':owner['source_range'],'body_range':owner['body_range'],'body_sha256':owner['body_sha256'],'declaration_sha256':owner['declaration_sha256']}
rows={k:[verify_owner(x) for x in xs] for k,xs in owners['selected_rows'].items()}
assert len(rows)==18
adjacent=verify_owner(owners['adjacent_semantic_correction'])
plan=json.loads(Path('/tmp/emitter-delivery-plan-r227.json').read_text())
profiles=[]
for path in plan['profile_candidates']:
 data=contents(path);artifact=json.loads(data)
 historical=path=='ratchets/h1-emit-profile.v1.json'
 inputs=artifact.get('runtime_inputs',[])
 assert isinstance(inputs,list)
 stale=[]
 for inp in inputs:
  try: digest=sha(contents(inp['path']))
  except subprocess.CalledProcessError: digest=None
  if digest!=inp['sha256']:stale.append({'path':inp['path'],'expected':inp['sha256'],'actual':digest,'reason':'missing at validation ref' if digest is None else 'hash changed'})
 profiles.append({'path':path,'sha256':sha(data),'classification':'historical H1 lineage' if historical else 'H2 transition' if path.endswith('h2-profile-transition.v1.json') else 'current H2 runtime profile','runtime_input_count':len(inputs),'stale_runtime_inputs':stale})
assert len(profiles)==24
assert sum(x['classification']=='current H2 runtime profile' for x in profiles)==22
result={'qualification_claim':False,'reason':'Input identity preparation alone is not final CI, hosted acceptance, or delivery qualification.','validation_ref':v,'validation_tree':git('rev-parse',v+'^{tree}').decode().strip(),'owner_identity_evidence':owner_path,'typescript_source':owners['source_path'],'typescript_source_sha256':sha(source),'selected_rows':rows,'adjacent_checker_owner':adjacent,'profiles':profiles}
if a.merge:
 m=git('rev-parse',a.merge+'^{commit}').decode().strip()
 subprocess.run(['git','-C',str(a.tree),'merge-base','--is-ancestor',v,m],check=True)
 result['merge_ref']=m;result['merge_tree']=git('rev-parse',m+'^{tree}').decode().strip()
 assert result['merge_tree']==result['validation_tree'],'Delivery tree changed'
 assert not owner_path.startswith('scratch-only'),'Archive owner evidence in V first'
 assert not any(x['stale_runtime_inputs'] for x in profiles),'Current H2 runtime input stale'
 assert next(x for x in profiles if x['path']=='ratchets/h2-5g-profile.v1.json')['runtime_input_count']==921
with a.out.open('x') as f:json.dump(result,f,indent=2);f.write('\n')
print(json.dumps({'validation_ref':v,'row_count':len(rows),'owner_instances':sum(map(len,rows.values())),'profile_count':len(profiles),'current_h2_profile_count':22,'stale_runtime_input_references':sum(len(x['stale_runtime_inputs']) for x in profiles),'qualification_claim':False,'output':str(a.out)}))
