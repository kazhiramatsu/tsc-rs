from pathlib import Path
import datetime,gzip,hashlib,json,os,subprocess,time
r=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');base=Path(__file__).parent;out=base/'command-refresh-r581'
sha=lambda b:hashlib.sha256(b).hexdigest();env=dict(os.environ,CARGO_BUILD_JOBS='2',CARGO_TARGET_DIR='/Users/hiramatsu/dev/tsc-rs-emitter-final/target')
# Run only after the EF7 writer has finished successfully; never overlap mints.
for name in ['217','plan-base']:assert json.loads((base/'ef7-refresh-r580'/(name+'.json')).read_bytes())['exit']==0
out.mkdir(exist_ok=False)
selection='docs/design/greenfield/slices/h2-5h-parameter-temporaries-selection.v1.json';old_selection=(base/'before'/selection).read_bytes();assert (r/selection).read_bytes()==old_selection
row_proof=json.loads((base/'parameter-current-row-proof.json').read_bytes());assert row_proof['selected']==8 and not row_proof['changed_rows']
old=json.loads(old_selection);new=json.loads(old_selection);text=old_selection.decode();deltas=[]
for parent in row_proof['parents']:
 name=parent['path'];h=sha((r/name).read_bytes());assert h==parent['current'] and old['files'][name]==parent['old']
 needle=json.dumps(name)+': '+json.dumps(parent['old']);assert text.count(needle)==1;text=text.replace(needle,json.dumps(name)+': '+json.dumps(h));new['files'][name]=h;deltas.append({'path':'/files/'+name,'before':parent['old'],'after':h})
assert json.loads(text)==new;(r/selection).write_text(text);(out/'selection-delta.json').write_text(json.dumps({'changes':deltas,'all_other_fields_identical':True,'selected_row_hashes_unchanged':8},indent=2)+'\n')
for stem,count,allowed in [
 ('emitter-jsdoc-original-command',1,{'/selection/0/artifact_sha256'}),
 ('h2-5h-parameter-temporaries',68,{'/selection_sha256','/parents/0/sha256','/parents/1/sha256'}),
 ('utf16-original-rows-complete',4,{'/parent/sha256'})]:
 rel='crates/compiler/tests/fixtures/'+stem+'.json';p=r/rel;before=(base/'before'/rel).read_bytes();assert p.read_bytes()==before;old=json.loads(before);assert len(old['cases'])==count
 argv=['taskpolicy','-b','nice','-n','15','node','scripts/observe-'+stem+'.mjs','--write'];started=datetime.datetime.now(datetime.timezone.utc).isoformat();t=time.monotonic();log=out/(stem+'.log');print('START',stem,flush=True)
 # The original writers deliberately require a new file. Retain exact old bytes
 # in the external before snapshot and Git history, then invoke them unchanged.
 p.unlink()
 try:
  with log.open('xb') as f:result=subprocess.run(argv,cwd=r,env=env,stdout=f,stderr=subprocess.STDOUT)
 except BaseException:
  if p.exists():(out/(stem+'-incomplete.json')).write_bytes(p.read_bytes())
  p.write_bytes(before);raise
 raw=log.read_bytes();log.with_suffix('.log.gz').write_bytes(gzip.compress(raw,mtime=0));rec={'argv':argv,'cwd':str(r),'started_at':started,'seconds':round(time.monotonic()-t,3),'exit':result.returncode,'log_sha256':sha(raw),'before_sha256':sha(before),'after_sha256':sha(p.read_bytes()) if p.exists() else None};(out/(stem+'.json')).write_text(json.dumps(rec,indent=2)+'\n');print(json.dumps(rec),flush=True)
 if result.returncode:
  if p.exists():(out/(stem+'-incomplete.json')).write_bytes(p.read_bytes())
  p.write_bytes(before);print(raw.decode(errors='replace')[-5000:]);raise SystemExit(result.returncode)
 new=json.loads(p.read_bytes());changes=[]
 def compare(a,b,path=''):
  if isinstance(a,dict) and isinstance(b,dict):
   assert a.keys()==b.keys(),path
   for k in a:compare(a[k],b[k],path+'/'+k)
  elif isinstance(a,list) and isinstance(b,list):
   assert len(a)==len(b),path
   for i,(x,y) in enumerate(zip(a,b)):compare(x,y,path+'/'+str(i))
  elif a!=b:changes.append({'path':path,'before':a,'after':b})
 compare(old,new);proof={'cases':count,'case_payloads_identical':old['cases']==new['cases'],'changes':changes};(out/(stem+'-delta.json')).write_text(json.dumps(proof,indent=2)+'\n');assert proof['case_payloads_identical'];assert {x['path'] for x in changes}==allowed,changes
 print('VERIFIED',stem,json.dumps(proof),flush=True)
