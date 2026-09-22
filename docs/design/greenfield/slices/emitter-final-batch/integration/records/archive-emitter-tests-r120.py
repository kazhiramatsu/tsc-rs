from pathlib import Path
import json,hashlib,re,gzip
root=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-recovery-next')
base=root/'docs/design/greenfield/slices/emitter-final-batch/integration'
manifest=json.loads(Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target/emitter-r120-immutable/emitter-manifest.json').read_text())
rows=[]
for artifact in manifest['artifacts']:
 label='emitter-'+artifact['target'].replace('_','-')+'-r120'
 receipt=json.loads((base/f'records/local/{label}.json').read_text())
 assert receipt['head']==manifest['head'] and receipt['tracked_clean'] and receipt['exit']==0
 assert hashlib.sha256(Path(artifact['immutable']).read_bytes()).hexdigest()==artifact['sha256']
 log=gzip.decompress((base/f'records/local/{label}.log.gz').read_bytes())
 assert hashlib.sha256(log).hexdigest()==receipt['log_sha256']
 counts=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;.*?(\d+) filtered out;',log.decode())
 assert counts==[(str(artifact['tests']),'0','0','0')],(label,counts)
 rows.append({**artifact,'receipt':f'records/local/{label}.json','log_sha256':receipt['log_sha256'],'passed':artifact['tests'],'failed':0,'ignored':0,'filtered':0})
assert len(rows)==23 and sum(r['passed'] for r in rows)==1015
summary={'head':manifest['head'],'suite':'emitter --tests','binaries':len(rows),'passed':1015,'failed':0,'ignored':0,'filtered':0,'runs':rows,'performance_qualified':False,'note':'All binaries copied immutably after a serialized build; functional runs may overlap the frozen census. This qualifies these emitter tests, not the remaining complete-command corpus or final gate.'}
p=base/'cross-review/r120-emitter-all-tests-summary.json'
with p.open('x') as f:json.dump(summary,f,indent=2);f.write('\n')
print(json.dumps({k:v for k,v in summary.items() if k!='runs'},indent=2))
