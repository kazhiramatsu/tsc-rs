from pathlib import Path
import gzip,hashlib,json
rel=Path('docs/design/greenfield/slices/emitter-final-batch/integration')
prep=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-declaration-prep')/rel
root=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-recovery-next')/rel
target=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target/emitter-recovery-parser-replays-r131')
out=prep/'records/parser-replays-r131-complete';out.mkdir(exist_ok=False)
sha=lambda b:hashlib.sha256(b).hexdigest();artifacts=[]
def archive(src,dest,compress=False):
 dest.parent.mkdir(parents=True,exist_ok=True)
 data=src.read_bytes();content=gzip.compress(data,compresslevel=6,mtime=0) if compress else data
 with dest.open('xb') as f:f.write(content)
 if compress:assert gzip.decompress(content)==data
 artifacts.append({'source':str(src),'archive':str(dest.relative_to(prep)),'original_sha256':sha(data),'original_bytes':len(data),'archive_sha256':sha(content),'archive_bytes':len(content),'lossless_gzip':compress})
for label in ['current','projection','merge-base','successor']:
 replay=json.loads((target/f'{label}.json').read_bytes());assert len(replay['digests'])==16994
 archive(target/f'{label}.json',out/f'{label}.json.gz',True)
 archive(target/f'{label}.build.log',out/f'{label}.build.log.gz',True)
 for name in ['Cargo.toml','Cargo.lock']:archive(target/f'build-{label}'/name,out/f'build-{label}'/name)
for name in ['selection.json','resolved-dependencies.json']:archive(target/name,out/f'{name}.gz',True)
archive(target/'oracle-r135.json',out/'oracle-r135.json.gz',True)
for label in ['corpus-replay-pipeline-r131','corpus-oracle-r135']:
 for suffix in ['.json','.log.gz']:archive(root/f'records/local/{label}{suffix}',out/f'{label}{suffix}')
for name in ['emitter-corpus-replay-pipeline-r131.py','emitter-parser-replays-r131.py']:archive(Path('/tmp')/name,out/name)
selection=json.loads((target/'selection.json').read_bytes())
summary={'original_snapshot_sha256':'1846fce26da956f515874486d43b9ac856fda31456e76a2925455a8488f76d75','parser_inputs_per_replay':16994,'replays':4,'probe_head':'33fddc898e1086dad95830e3a9ef99fe816dadef','successor_head':'d0edc1a74a6249318b19aa449ab2735983a8df46','successor_core_and_recovery_events_unchanged':True,'selection':selection['summary'],'native_command_qualification':'pending','artifacts':artifacts}
(out/'manifest.json').write_text(json.dumps(summary,indent=2)+'\n');print(json.dumps({k:v for k,v in summary.items() if k!='artifacts'},indent=2));print('total archive bytes',sum(x['archive_bytes'] for x in artifacts))
