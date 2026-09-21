from pathlib import Path
import gzip,hashlib,json,shutil
rel=Path('docs/design/greenfield/slices/emitter-final-batch/integration')
prep=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-declaration-prep')/rel
census=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-census')/rel
root=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-recovery-next')/rel
var=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-recovery-prep')/rel
target=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target')
out=prep/'records/census-r78-complete';out.mkdir(exist_ok=False)
sha=lambda b:hashlib.sha256(b).hexdigest()
artifacts=[]
def archive(src,dest,compress=False):
 dest.parent.mkdir(parents=True,exist_ok=True)
 h=hashlib.sha256();count=0
 with src.open('rb') as rd,dest.open('xb') as raw:
  if compress:
   with gzip.GzipFile(fileobj=raw,mode='wb',mtime=0,filename='',compresslevel=6) as wr:
    while data:=rd.read(1024*1024):h.update(data);count+=len(data);wr.write(data)
  else:
   while data:=rd.read(1024*1024):h.update(data);count+=len(data);raw.write(data)
 if compress:
  h2=hashlib.sha256();n=0
  with gzip.open(dest,'rb') as f:
   while data:=f.read(1024*1024):h2.update(data);n+=len(data)
  assert n==count and h2.hexdigest()==h.hexdigest()
 artifacts.append({'source':str(src),'archive':str(dest.relative_to(prep)),'original_sha256':h.hexdigest(),'original_bytes':count,'archive_sha256':sha(dest.read_bytes()),'archive_bytes':dest.stat().st_size,'lossless_gzip':compress})
archive(target/'emitter-recovery-census-r78/parse-snapshot.json',out/'parse-snapshot.json.gz',True)
assert artifacts[-1]['original_sha256']=='1846fce26da956f515874486d43b9ac856fda31456e76a2925455a8488f76d75'
for p in sorted((target/'emitter-recovery-census-r78/profiles').glob('*.json')):archive(p,out/'profiles'/f'{p.name}.gz',True)
archive(target/'emitter-census-r78-immutable/manifest.json',out/'original-binary-manifest.json')
for suffix in ['.json','.log.gz']:archive(census/f'records/local/recovery-full-census-r78{suffix}',out/f'recovery-full-census-r78{suffix}')
archive(Path('/tmp/emitter-census-coverage-r126.json'),out/'coverage.json')
for suffix in ['.json','.log.gz']:archive(root/f'records/local/corpus-replay-pipeline-r126{suffix}',out/f'failed-replay-r126/corpus-replay-pipeline-r126{suffix}')
for p in ['current.build.log','build-current/Cargo.toml','build-current/Cargo.lock']:archive(target/'emitter-recovery-parser-replays-r107'/p,out/'failed-replay-r126'/p)
for path in ['/tmp/emitter-corpus-replay-pipeline-r126.py','/tmp/emitter-parser-replays-after-census.py','/tmp/emitter-variable-type-native-r129.py']:archive(Path(path),out/'failed-replay-r126'/Path(path).name)
for suffix in ['.json','.log.gz']:archive(var/f'records/local/variable-type-pipeline-r129{suffix}',out/f'failed-replay-r126/variable-type-pipeline-r129{suffix}')
summary={'census_head':'67df86615a45f9595025064bcc12fde93063fedf','claimed_ids':14329,'loaded_rows':14219,'explicit_load_failures':110,'unique_parse_inputs':16994,'coverage_complete':True,'emit_qualified_by_this_artifact':False,'replay_r126':'failed before first parse: standalone serde_json omitted preserve_order; original snapshot not rewritten','variable_r129':'blocked before any build by failed original replay','artifacts':artifacts}
(out/'manifest.json').write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps({k:v for k,v in summary.items() if k!='artifacts'},indent=2));print('archived',sum(x['archive_bytes'] for x in artifacts),'bytes')
