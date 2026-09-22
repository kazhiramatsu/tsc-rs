from pathlib import Path
import datetime,gzip,hashlib,json,os,subprocess,time
r=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');base=Path(__file__).parent;out=base/'ef7-refresh-r580';out.mkdir(exist_ok=False)
sha=lambda b:hashlib.sha256(b).hexdigest();env=dict(os.environ,CARGO_BUILD_JOBS='2',CARGO_TARGET_DIR='/Users/hiramatsu/dev/tsc-rs-emitter-final/target')
versions={cmd[0]:subprocess.check_output(cmd,text=True).strip() for cmd in [['node','--version'],['zstd','--version']]};(out/'versions.json').write_text(json.dumps(versions,indent=2)+'\n')
assert versions['node']=='v'+(r/'.node-version').read_text().strip()
for name,file,count in [('217','emitter-final-universe.json',217),('plan-base','emitter-final-universe-plan-base.json.zst',1798)]:
 rel='crates/compiler/tests/fixtures/'+file;p=r/rel;before=(base/'before'/rel).read_bytes();assert p.read_bytes()==before
 decode=lambda data:subprocess.check_output(['zstd','-d','-c'],input=data) if file.endswith('.zst') else data
 old=json.loads(decode(before));assert len(old['cases'])==count
 argv=['taskpolicy','-b','nice','-n','15','node','scripts/observe-emitter-final-universe.mjs','--write','--set',name]
 started=datetime.datetime.now(datetime.timezone.utc).isoformat();t=time.monotonic();log=out/(name+'.log');print('START',name,flush=True)
 with log.open('xb') as f:result=subprocess.run(argv,cwd=r,env=env,stdout=f,stderr=subprocess.STDOUT)
 raw=log.read_bytes();log.with_suffix('.log.gz').write_bytes(gzip.compress(raw,mtime=0));rec={'argv':argv,'cwd':str(r),'started_at':started,'seconds':round(time.monotonic()-t,3),'exit':result.returncode,'log_sha256':sha(raw),'before_sha256':sha(before),'after_sha256':sha(p.read_bytes())};(out/(name+'.json')).write_text(json.dumps(rec,indent=2)+'\n');print(json.dumps(rec),flush=True)
 if result.returncode:print(raw.decode(errors='replace')[-4000:]);raise SystemExit(result.returncode)
 new=json.loads(decode(p.read_bytes()));changes=[]
 def compare(a,b,path=''):
  if isinstance(a,dict) and isinstance(b,dict):
   assert a.keys()==b.keys(),path
   for k in a:compare(a[k],b[k],path+'/'+k)
  elif isinstance(a,list) and isinstance(b,list):
   assert len(a)==len(b),path
   for i,(x,y) in enumerate(zip(a,b)):compare(x,y,path+'/'+str(i))
  elif a!=b:changes.append({'path':path,'before':a,'after':b})
 compare(old,new);proof={'cases':count,'case_payloads_identical':old['cases']==new['cases'],'changes':changes};(out/(name+'-delta.json')).write_text(json.dumps(proof,indent=2)+'\n')
 assert proof['case_payloads_identical'];assert {c['path'] for c in changes} in [{'/inputs/1/sha256'},{'/inputs/1/sha256','/source_commit'}],changes
 assert new['inputs'][1]['path']=='ratchets/h2-candidate-dispositions.v1.json';assert new['inputs'][1]['sha256']==sha((r/new['inputs'][1]['path']).read_bytes())
 print('VERIFIED',name,json.dumps(proof),flush=True)
