from pathlib import Path
import datetime,gzip,hashlib,json,subprocess,time
r=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');b=Path(__file__).parent;out=b/'heritage-refresh-r588';out.mkdir(exist_ok=False)
rel='crates/compiler/tests/fixtures/emitter-heritage-boundaries.json';p=r/rel;before=(b/'before'/rel).read_bytes();assert p.read_bytes()==before;old=json.loads(before);assert len(old['cases'])==132
sha=lambda x:hashlib.sha256(x).hexdigest();argv=['taskpolicy','-b','nice','-n','15','node','scripts/observe-emitter-heritage-boundaries.mjs','--write'];start=datetime.datetime.now(datetime.timezone.utc).isoformat();t=time.monotonic();log=out/'heritage.log';p.unlink()
try:
 with log.open('xb') as f:run=subprocess.run(argv,cwd=r,stdout=f,stderr=subprocess.STDOUT)
except BaseException:
 if p.exists():(out/'partial-fixture.json').write_bytes(p.read_bytes())
 p.write_bytes(before);raise
raw=log.read_bytes();log.with_suffix('.log.gz').write_bytes(gzip.compress(raw,mtime=0));rec={'argv':argv,'cwd':str(r),'started_at':start,'seconds':round(time.monotonic()-t,3),'exit':run.returncode,'log_sha256':sha(raw),'before_sha256':sha(before),'after_sha256':sha(p.read_bytes()) if p.exists() else None};(out/'heritage.json').write_text(json.dumps(rec,indent=2)+'\n');print(json.dumps(rec),flush=True)
if run.returncode:
 if p.exists():(out/'partial-fixture.json').write_bytes(p.read_bytes())
 p.write_bytes(before);print(raw.decode(errors='replace')[-3000:]);raise SystemExit(run.returncode)
new=json.loads(p.read_bytes());assert len(new['cases'])==140;assert old['cases']==new['cases'][:132];assert old.keys()==new.keys();assert {k for k in old if old[k]!=new[k]}=={'observer_sha256','cases'};assert new['observer_sha256']==sha((r/'scripts/observe-emitter-heritage-boundaries.mjs').read_bytes());ids=[x['case_id'] for x in new['cases'][132:]];assert len(set(ids))==8;assert all(x.startswith('emitter-heritage-boundaries/access-recovery/') for x in ids)
proof={'old_cases':132,'old_case_payloads_identical':True,'added_cases':8,'new_cases':140,'added_ids':ids,'metadata_change':'observer_sha256 only','all_other_fields_identical':True};(out/'delta.json').write_text(json.dumps(proof,indent=2)+'\n');print(json.dumps(proof),flush=True)
