"""Read-only guard tests using immutable observations; no new parser qualification."""
from pathlib import Path
import copy,hashlib,importlib.util,json,subprocess,time
ROOT=Path(__file__).resolve().parent
spec=importlib.util.spec_from_file_location('layered_replay',ROOT/'replay.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
snapshot=m.load(m.SNAPSHOT);before=m.load(m.PREVIOUS/'successor.json')
assert m.sha(m.SNAPSHOT.read_bytes())==m.PINS['snapshot_sha256']
true_id=next(k for k,v in before['digests'].items() if v['profiles']['context_recovery'])
false_id=next(k for k,v in before['digests'].items() if not v['profiles']['context_recovery'])
rows=[]
def trial(name,key,mutate,allowed=False):
 after={**before,'digests':dict(before['digests'])}
 after['digests'][key]=copy.deepcopy(before['digests'][key]);mutate(after,key)
 try:
  delta=m.compare_inputs(before,after,snapshot)
 except AssertionError as e:
  assert not allowed,(name,str(e));rows.append({'case':name,'result':'rejected','reason':str(e)[:180]})
 else:
  assert allowed,(name,'unexpectedly accepted');assert delta==[key];rows.append({'case':name,'result':'accepted synthetic context-only flip, not qualification'})
trial('lost context admission',true_id,lambda a,k:a['digests'][k]['profiles'].__setitem__('context_recovery',False))
trial('predecessor profile change',false_id,lambda a,k:a['digests'][k]['profiles'].__setitem__('statement_gaps',not a['digests'][k]['profiles']['statement_gaps']))
trial('null profiles',false_id,lambda a,k:a['digests'][k].__setitem__('profiles',None))
trial('nonboolean profile',false_id,lambda a,k:a['digests'][k]['profiles'].__setitem__('literal',0))
trial('missing parse input',false_id,lambda a,k:a['digests'].pop(k))
trial('core drift',false_id,lambda a,k:a['digests'][k]['core'].__setitem__('ast_shape_sha256','0'*64))
def change_facts(a,k):
 a['recovery_facts_sha256']=dict(before['recovery_facts_sha256']);a['recovery_facts_sha256'][k]='0'*64
trial('raw fact drift',false_id,change_facts)
trial('unexpected replay field',false_id,lambda a,k:a.__setitem__('unreviewed_extra',True))
trial('context-only positive guard',false_id,lambda a,k:a['digests'][k]['profiles'].__setitem__('context_recovery',True),True)
# A new input that is not accounted for by any row must never disappear.
selector,prior=m.original_selection(snapshot,before)
fresh=copy.copy(before);fresh['build']={**before['build'],'parser_head':m.PINS['prototype_head']}
try:m.select_extension(snapshot,prior,fresh,['unmapped-synthetic-input'],selector)
except AssertionError as e:rows.append({'case':'unmapped new input','result':'rejected','reason':str(e)[:180]})
else:raise AssertionError('unmapped input was dropped')
assert len(rows)==10
result={'scope':'guard tests only; mutated observations are never saved as qualification','results':rows,'wrapper_sha256':m.sha((ROOT/'replay.py').read_bytes()),'test_sha256':m.sha(Path(__file__).read_bytes())}
(ROOT/'guard-results.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps({'guards':len(rows),'result':'PASS','runtime_replay':'NOT RUN'}))
