from pathlib import Path
import hashlib,json,subprocess,copy
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep')
O=Path('/tmp/emitter-final-walk-progress-r564')
HEAD='11d1971dd31a68e05fe18bedb26b6fdef4e9d170'
git=lambda *a:subprocess.check_output(['git',*a],cwd=R)
assert git('rev-parse','HEAD').decode().strip()==HEAD
paths=['ratchets/h2-8a-candidates.v1.json','ratchets/h2-8a-observations.v1.json']
assert git('diff','HEAD','--name-only').decode().splitlines()==paths
rows=[]
for name,index,input_path in [(paths[0],3,'ratchets/h2-7c-qualification.v1.json'),(paths[1],1,paths[0])]:
 before=json.loads(git('show',HEAD+':'+name));after=json.loads((R/name).read_bytes())
 assert before['inputs'][index]['path']==after['inputs'][index]['path']==input_path
 expected=copy.deepcopy(before)
 expected['inputs'][index]['sha256']=hashlib.sha256((R/input_path).read_bytes()).hexdigest()
 assert expected==after,(name,'Only the named parent digest may change')
 assert expected!=before
 rows.append({'path':name,'parent_path':input_path,'before_sha256':before['inputs'][index]['sha256'],'after_sha256':after['inputs'][index]['sha256'],'cases_identical':before['cases']==after['cases'],'case_count':len(after['cases']),'all_other_fields_identical':True})
result={'qualified_walk':False,'scope':'Read-only first-round delta review; actual walk still must finish','source_head':HEAD,'run_id':'20260921-024343-28510','changes':rows}
(O/'h28a-delta-review.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result,indent=2))
