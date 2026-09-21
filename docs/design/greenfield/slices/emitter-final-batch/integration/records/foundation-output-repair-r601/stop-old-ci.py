from pathlib import Path
import subprocess,os,signal,json,datetime
ROOT_PID=15797
OUT=Path('/tmp/emitter-final-hosted-failure-r600')
def process_table():
 rows={}
 for line in subprocess.check_output(['ps','-axo','pid,ppid,args'],text=True).splitlines()[1:]:
  p=line.split(None,2)
  if len(p)==3:rows[int(p[0])]={'pid':int(p[0]),'ppid':int(p[1]),'command':p[2]}
 return rows
rows=process_table()
assert rows[ROOT_PID]['command']=='/Users/hiramatsu/dev/tsc-rs-emitter-final/target/debug/xtask ci --baseline 3b1f5fe87fd31e3b303bb44bd257342735452ed9'
owned={ROOT_PID:rows[ROOT_PID]}
os.kill(ROOT_PID,signal.SIGSTOP)
# Freeze descendants before termination, so no new observer/git children start.
for _ in range(8):
 rows=process_table();new=[]
 for pid,row in rows.items():
  if row['ppid'] in owned and pid not in owned:
   try:os.kill(pid,signal.SIGSTOP)
   except ProcessLookupError:continue
   owned[pid]=row;new.append(pid)
 if not new:break
record={'when':datetime.datetime.now(datetime.timezone.utc).isoformat(),'head':'5b28906cf5544c63bb55f4c97e40f714672fd722','reason':'Intentional cancellation before the hosted foundation-output repair; not a successful complete gate or natural CI failure.','owned_processes':list(owned.values())}
(OUT/'local-ci-stop.json').write_text(json.dumps(record,indent=2)+'\n')
for pid in reversed(list(owned)):
 try:
  os.kill(pid,signal.SIGTERM)
  os.kill(pid,signal.SIGCONT)
 except ProcessLookupError:pass
print(json.dumps(record,indent=2))
