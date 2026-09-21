"""Read-only live capture of completed per-rung logs before next-round reuse."""
from pathlib import Path
import datetime,gzip,hashlib,json,os,time
root=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep')
run=root/'target/chain-walk/runs/20260921-070717-92633'
out=Path('/tmp/emitter-final-walk-log-history-r597');out.mkdir(exist_ok=False)
pid=92633
entries={};errors=[]
def alive():
 try:os.kill(pid,0);return True
 except ProcessLookupError:return False
while True:
 active=alive();events=[];p=run/'events.jsonl'
 if p.exists():
  for line in p.read_text().splitlines():
   try:events.append(json.loads(line))
   except json.JSONDecodeError:continue
 starts=[x for x in events if x.get('phase')=='check']
 latest={x['rung']:x['round'] for x in starts}
 candidates=starts[:-1] if active else starts
 for event in candidates:
  key=f"round-{event['round']}/{event['rung']}"
  if key in entries:continue
  if latest[event['rung']]!=event['round']:
   errors.append({'key':key,'error':'missed before next-round reuse'});entries[key]={'captured':False};continue
  src=run/(event['rung']+'.log')
  try:
   st=src.stat();raw=src.read_bytes();after=src.stat()
   if (st.st_size,st.st_mtime_ns)!=(after.st_size,after.st_mtime_ns):continue
   dest=out/(key+'.log.gz');dest.parent.mkdir(parents=True,exist_ok=True)
   data=gzip.compress(raw,mtime=0);dest.write_bytes(data)
   entries[key]={'captured':True,'path':str(dest.relative_to(out)),'raw_sha256':hashlib.sha256(raw).hexdigest(),'raw_bytes':len(raw),'stored_sha256':hashlib.sha256(data).hexdigest(),'captured_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'source_mtime_ns':st.st_mtime_ns,'events':[x for x in events if x.get('round')==event['round'] and x.get('rung')==event['rung']]}
  except FileNotFoundError as e:errors.append({'key':key,'error':str(e)})
 (out/'manifest.json').write_text(json.dumps({'scope':'Read-only live snapshots of completed per-rung logs; not gate qualification','run_id':run.name,'walk_active':active,'entries':entries,'errors':errors},indent=2)+'\n')
 if not active:break
 time.sleep(2)
print(json.dumps({'captured':sum(x.get('captured',False) for x in entries.values()),'errors':errors}),flush=True)
