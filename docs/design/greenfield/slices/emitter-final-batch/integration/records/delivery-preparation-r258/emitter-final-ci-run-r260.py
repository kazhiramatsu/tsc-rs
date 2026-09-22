"""Run the authorized unsplit final gate with evidence outside its monitored inputs."""
import argparse, datetime, gzip, hashlib, json, os, re, subprocess, time
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--head',required=True);p.add_argument('--label',required=True);p.add_argument('--normal-priority',action='store_true');p.add_argument('--previous',type=Path)
a=p.parse_args();r=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');out=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target/emitter-final-unsplit-ci')
assert re.fullmatch(r'[a-zA-Z0-9_-]+',a.label)
assert not out.resolve().is_relative_to(r.resolve())
sha=lambda x:hashlib.sha256(x).hexdigest()
def git(*args):return subprocess.check_output(['git',*args],cwd=r)
assert git('rev-parse','HEAD').decode().strip()==a.head
assert not git('diff','HEAD','--name-only').strip()
assert not git('status','--porcelain','--','crates').strip()
if a.normal_priority:
 assert a.previous,'Require the preserved prior failed gate receipt; review sole performance failure before using this switch'
 prev=json.loads(a.previous.read_bytes());assert prev['head']==a.head and prev['exit']!=0 and prev['tracked_clean']
 assert sha(gzip.decompress(a.previous.with_suffix('.log.gz').read_bytes()))==prev['log_sha256']
out.mkdir(parents=True,exist_ok=True);stem=out/a.label
for suffix in ['.log','.log.gz','.json','.journal-before.json','.journal-after.json']:
 assert not stem.with_suffix(suffix).exists(),stem.with_suffix(suffix)
command=['cargo','xtask','ci','--baseline','3b1f5fe87fd31e3b303bb44bd257342735452ed9']
prefix=[] if a.normal_priority else ['taskpolicy','-b','nice','-n','15']
env=os.environ.copy();env['CARGO_BUILD_JOBS']='2';env['CARGO_TARGET_DIR']='/Users/hiramatsu/dev/tsc-rs-emitter-final/target'
for key in ['SKIP_PREFLIGHT','WALK_EXPECT_OBS','TSRS_H2_5G_FRESH','WALK_PREFLIGHT_RECEIPT','WALK_PLAN','WALK_DRY']:
 assert key not in env,(key,'No walk overrides are permitted for this final gate')
journal=r/'target/local-ci-resume/v1/journal.json'
receipt={'argv':prefix+command,'cwd':str(r),'head':a.head,'tracked_clean':True,'started_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'env':{k:v for k,v in env.items() if k.startswith(('CARGO_','TSC_RS_','TSRS_'))},'normal_priority':a.normal_priority,'previous_receipt':str(a.previous) if a.previous else None}
if journal.exists():
 data=journal.read_bytes();stem.with_suffix('.journal-before.json').write_bytes(data);receipt['journal_before_sha256']=sha(data)
started=time.monotonic()
with stem.with_suffix('.log').open('xb') as log:
 process=subprocess.Popen(prefix+command,cwd=r,env=env,stdout=log,stderr=subprocess.STDOUT)
 print(f'{a.label}: pid={process.pid}; log={stem.with_suffix(".log")}',flush=True)
 code=process.wait()
raw=stem.with_suffix('.log').read_bytes();stem.with_suffix('.log.gz').write_bytes(gzip.compress(raw,mtime=0));text=raw.decode(errors='replace')
receipt.update(exit=code,seconds=round(time.monotonic()-started,3),finished_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),log_sha256=sha(raw),log_bytes=len(raw),head_after=git('rev-parse','HEAD').decode().strip(),tracked_clean_after=not git('diff','HEAD','--name-only').strip(),phase_runs=re.findall(r'^local CI phase: run (.+)$',text,re.M),phase_recorded=re.findall(r'^local CI checkpoint: recorded (.+)$',text,re.M),phase_reused=re.findall(r'^local CI resume: reuse (.+) \(exact inputs and outputs\)$',text,re.M),completion=re.findall(r'^local CI resume: complete; cleared failed-run journal \(reused=(\d+) recorded=(\d+)\)$',text,re.M))
if journal.exists():
 data=journal.read_bytes();stem.with_suffix('.journal-after.json').write_bytes(data);receipt['journal_after_sha256']=sha(data)
receipt['qualified']=code==0 and receipt['head_after']==a.head and receipt['tracked_clean_after']
stem.with_suffix('.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(text[-5000:]);print(json.dumps({'label':a.label,'exit':code,'qualified':receipt['qualified'],'seconds':receipt['seconds'],'receipt':str(stem.with_suffix('.json'))}),flush=True)
raise SystemExit(code if code else 0 if receipt['qualified'] else 2)
