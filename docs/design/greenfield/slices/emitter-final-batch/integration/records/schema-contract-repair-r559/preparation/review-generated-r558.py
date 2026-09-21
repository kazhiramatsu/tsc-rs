import copy,hashlib,json,re,subprocess
from pathlib import Path
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep')
P=Path('/tmp/emitter-schema-cycle-repair-r553')
HEAD='f46dc801560cd8ccaf8ccce8dcec6135615f0244'
git=lambda *a:subprocess.check_output(['git',*a],cwd=R)
assert git('rev-parse','HEAD').decode().strip()==HEAD
assert not git('diff','HEAD','--name-only','--','crates/**/*.rs','crates/oracle/*.mjs','.github/ci/qualification.mjs').strip()
sha=lambda x:hashlib.sha256(x).hexdigest()
def leaves(a,b,p):
 if a==b:return []
 if isinstance(a,dict) and isinstance(b,dict) and a.keys()==b.keys():
  return sum((leaves(a[k],b[k],p+'/'+k) for k in a),[])
 if isinstance(a,list) and isinstance(b,list) and len(a)==len(b):
  return sum((leaves(x,y,p+'/'+str(i)) for i,(x,y) in enumerate(zip(a,b))),[])
 return [{'path':p,'before':a,'after':b}]
changed=git('diff','HEAD','--name-only','--','ratchets').decode().splitlines()
expected=['h2-5g-profile','h2-5h-a-comment-scope-witnesses','h2-5h-a-dispositions','h2-5h-a-es2015-generators-witnesses','h2-5h-a-foundation','h2-5h-a-gap-matrix','h2-5h-a-owner-graph','h2-7a-close','h2-7a-printer-reprint','h2-7a-probe-traces','h2-7a-witnesses','h2-7c-qualification','h2-8a-candidates','h2-8a-observations']
assert changed==sorted('ratchets/'+x+'.v1.json' for x in expected), changed
all_leaves=[];protected=[]
for f in changed:
 before=json.loads(git('show',HEAD+':'+f));after=json.loads((R/f).read_bytes())
 all_leaves+=leaves(before,after,f)
 for key in ['cases','observations','summary','rows','stratum','selection_contract','execution_contract','focused']:
  if key in before:
   assert before[key]==after[key], (f,key)
   protected.append(f+'/'+key)
nonhash=[x for x in all_leaves if not all(isinstance(x[k],str) and re.fullmatch('[0-9a-f]{64}',x[k]) for k in ['before','after'])]
assert nonhash==json.loads(Path('/tmp/emitter-final-provisional-leaves-r551.json').read_text())['non_hash_changes']
for name in ['h2-7c-qualification','h2-5h-a-dispositions']:
 artifact=json.loads((R/f'ratchets/{name}.v1.json').read_bytes())
 assert artifact['contract']['sha256']==sha((R/artifact['contract']['path']).read_bytes())
f='.github/ci/contracts/h2-7c-qualification.schema.json'
before=json.loads(git('show',HEAD+':'+f));after=json.loads((R/f).read_bytes())
expected=copy.deepcopy(before)
child=copy.deepcopy(before['$defs']['diagnostic'])
child['properties']['related_information']={'type':'null'}
expected['$defs']['related_diagnostic']=child
expected['$defs']['diagnostic']['properties']['related_information']['oneOf'][1]['items']['$ref']='#/$defs/related_diagnostic'
assert expected==after
f='.github/ci/contracts/h2-5h-a-dispositions.schema.json'
before=json.loads(git('show',HEAD+':'+f));after=json.loads((R/f).read_bytes())
schema_leaves=leaves(before,after,f)
assert len(schema_leaves)==7
result={'qualified':False,'source_head':HEAD,'scope':'Final failed-walk generated delta plus official two-schema remints; no walk certificate','artifact_files':changed,'changed_leaves':all_leaves,'unchanged_payload_surfaces':protected,'h27c_exact_bounded_clone':True,'dispositions_schema_changes':schema_leaves,'production_sources_unchanged':True}
(P/'generated-review-r558.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps({'artifact_files':len(changed),'changed_leaves':len(all_leaves),'nonhash_leaves':len(nonhash),'unchanged_payload_surfaces':len(protected),'schema_constants':len(schema_leaves),'qualified':False}))
