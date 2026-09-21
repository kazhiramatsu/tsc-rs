from pathlib import Path
import subprocess,os,time,json,datetime,hashlib,gzip,sys
root=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep')
out=Path('/tmp/emitter-final-precondition-r615')
label=sys.argv[1]; cmd=sys.argv[2:];assert cmd
log=out/(label+'.log');receipt=out/(label+'.json')
assert not log.exists() and not receipt.exists()
env=dict(os.environ,CARGO_BUILD_JOBS='2',CARGO_TARGET_DIR='/Users/hiramatsu/dev/tsc-rs-emitter-final/target')
meta={'argv':cmd,'cwd':str(root),'started_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=root).decode().strip(),'env':{k:env[k] for k in ('CARGO_BUILD_JOBS','CARGO_TARGET_DIR')}}
meta['diff_sha256']=hashlib.sha256(subprocess.check_output(['git','diff','--binary','HEAD'],cwd=root)).hexdigest()
paths=sorted(p for p in subprocess.check_output(['git','ls-files','crates'],cwd=root,text=True).splitlines() if p.endswith('.rs'))
meta['rust_files']=len(paths)
meta['rust_sha256']=hashlib.sha256(''.join(f'{hashlib.sha256((root/p).read_bytes()).hexdigest()}  {p}\n' for p in paths).encode()).hexdigest()
start=time.monotonic()
with log.open('xb') as output:p=subprocess.run(cmd,cwd=root,env=env,stdout=output,stderr=subprocess.STDOUT)
raw=log.read_bytes();(out/(label+'.log.gz')).write_bytes(gzip.compress(raw,mtime=0))
meta.update(exit=p.returncode,seconds=round(time.monotonic()-start,3),finished_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),log_sha256=hashlib.sha256(raw).hexdigest(),log_bytes=len(raw))
receipt.write_text(json.dumps(meta,indent=2)+'\n');print(json.dumps(meta),flush=True)
raise SystemExit(p.returncode)
