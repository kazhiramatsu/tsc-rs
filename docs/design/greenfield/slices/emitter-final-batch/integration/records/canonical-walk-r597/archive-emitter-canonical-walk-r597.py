"""Archive one completed sanctioned walk and a verifiable certificate; never mint."""
import argparse,datetime,gzip,hashlib,json,subprocess
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--label',required=True);p.add_argument('--out-name',required=True);a=p.parse_args()
r=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');b=r/'docs/design/greenfield/slices/emitter-final-batch/integration'
assert a.label.replace('-','').replace('_','').isalnum();assert a.out_name.replace('-','').replace('_','').isalnum()
sha=lambda x:hashlib.sha256(x).hexdigest()
def git(*args):return subprocess.check_output(['git',*args],cwd=r)
head=git('rev-parse','HEAD').decode().strip()
receipt_path=b/'records/local'/f'{a.label}.json';receipt=json.loads(receipt_path.read_bytes())
assert receipt['head']==head and receipt['tracked_clean'] and receipt['exit']==0
argv=receipt['argv'];at=argv.index('bash');assert argv[at:]==['bash','scripts/chain-walk.sh']
for name in ['WALK_EXPECT_OBS','TSRS_H2_5G_FRESH','SKIP_PREFLIGHT']:
 assert name not in receipt['env'],name
raw=gzip.decompress(receipt_path.with_suffix('.log.gz').read_bytes());assert sha(raw)==receipt['log_sha256'] and len(raw)==receipt['log_bytes']
assert b'chain walk: converged and green' in raw
assert b'RECORDED OVERRIDE' not in raw
run_id=(r/'target/chain-walk/converged-run-id').read_text().strip();run=r/'target/chain-walk/runs'/run_id
assert run.resolve()==(r/'target/chain-walk/runs/latest').resolve()
summary=(run/'summary.log').read_text();assert f'certificate run {run_id}' in summary
certificate_hash=(r/'target/chain-walk/converged-crates.sha256').read_text().strip()
paths=sorted(p for p in git('ls-tree','-r','--name-only',head,'--','crates').decode().splitlines() if p.endswith('.rs'))
for path in paths:assert (r/path).read_bytes()==git('show',f'{head}:{path}'),path
assert not git('ls-files','--others','--exclude-standard','--','crates').strip(),'No untracked crate inputs'
actual=sha(''.join(f'{sha((r/path).read_bytes())}  {path}\n' for path in paths).encode());assert actual==certificate_hash
out=b/'records'/a.out_name;out.mkdir(exist_ok=False);files=[]
def put(name,data):
 p=out/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(data);files.append({'path':name,'sha256':sha(data),'bytes':len(data)})
put(receipt_path.name,receipt_path.read_bytes());put(receipt_path.with_suffix('.log.gz').name,receipt_path.with_suffix('.log.gz').read_bytes());put('archive-emitter-canonical-walk-r597.py',Path(__file__).read_bytes())
for path in sorted(run.rglob('*')):
 if path.is_file():
  assert not path.is_symlink()
  data=path.read_bytes();name='run/'+str(path.relative_to(run))
  if path.suffix=='.log':name+='.gz';data=gzip.compress(data,mtime=0)
  put(name,data)
# Preserve each prior round's completed log before the driver's path reuse.
history=Path('/tmp/emitter-final-walk-log-history-r597')
history_manifest=json.loads((history/'manifest.json').read_bytes())
assert history_manifest['run_id']==run_id and not history_manifest['walk_active']
assert not history_manifest['errors'],history_manifest['errors']
events=[json.loads(line) for line in (run/'events.jsonl').read_text().splitlines() if line.strip()]
expected={f"round-{x['round']}/{x['rung']}" for x in events if x.get('phase')=='check'}
assert set(history_manifest['entries'])==expected
for key,row in history_manifest['entries'].items():
 assert row['captured'],key
 data=(history/row['path']).read_bytes();assert sha(data)==row['stored_sha256']
 raw_history=gzip.decompress(data);assert sha(raw_history)==row['raw_sha256'] and len(raw_history)==row['raw_bytes']
 put('log-history/'+row['path'],data)
put('log-history/manifest.json',(history/'manifest.json').read_bytes())
put('log-history/capture.py',Path('/tmp/emitter-final-walk-log-history-r597.py').read_bytes())

review_path=Path('/tmp/emitter-final-walk-review-r597.json')
review=json.loads(review_path.read_bytes())
assert review['walk_source_head']==head and review['walk_receipt_success']
assert review['command_fixture_proof']['passed'] and review['command_fixture_proof']['fixture_count']==5
put('generated-diff-review.json',review_path.read_bytes())
put('generated-diff-review.py',Path('/tmp/emitter-final-review-walk-r597.py').read_bytes())
put('command-fixture-diff-review.mjs',Path('/tmp/emitter-final-review-fixtures-r597.mjs').read_bytes())

# Preserve the complete read-only JSON leaf classification for the manual review.
detail_path=Path('/tmp/emitter-final-generated-leaves-r597.json')
detail=json.loads(detail_path.read_bytes())
assert detail['source_head']==head and detail['qualification_claim'] is False
put('generated-diff-leaves.json',detail_path.read_bytes())
put('generated-diff-leaves.py',Path('/tmp/emitter-final-generated-leaves-r597.py').read_bytes())


# Preserve the exact driver-generated certificate bytes and the resulting tracked diff.
put('converged-crates.sha256',(r/'target/chain-walk/converged-crates.sha256').read_bytes())
put('converged-run-id',(r/'target/chain-walk/converged-run-id').read_bytes())
put('walk-generated.patch.gz',gzip.compress(git('diff','--binary','--full-index','HEAD','--','crates/oracle','ratchets','.github/ci/contracts','.github/ci/qualification-policy.v2.json', 'crates/compiler/tests/fixtures/emitter-final-universe.json', 'crates/compiler/tests/fixtures/emitter-final-universe-plan-base.json.zst', 'crates/compiler/tests/fixtures/emitter-jsdoc-original-command.json', 'crates/compiler/tests/fixtures/h2-5h-parameter-temporaries.json', 'crates/compiler/tests/fixtures/utf16-original-rows-complete.json', 'docs/design/greenfield/slices/h2-5h-parameter-temporaries-selection.v1.json'),mtime=0))
finished=datetime.datetime.fromisoformat(receipt['started_at'])+datetime.timedelta(seconds=receipt['seconds'])
certificate={'qualified':True,'qualification_scope':'One sanctioned canonical walk and exact unchanged Rust bytes; not final unsplit CI or delivery qualification.','exit':0,'source_head_before_walk':head,'run_id':run_id,'green_tail':'chain walk: converged and green','finished_at':finished.isoformat(),'converged_crates_sha256':certificate_hash,'rust_file_count':len(paths),'five_g_summary':[l for l in summary.splitlines() if '5g:' in l or '5g enforcement:' in l],'overrides':[]}
put('certificate.json',(json.dumps(certificate,indent=2)+'\n').encode())
(out/'manifest.json').write_text(json.dumps({'source_head':head,'scope':certificate['qualification_scope'],'certificate':certificate,'files':files},indent=2)+'\n')
print(json.dumps({'archive':str(out),'run_id':run_id,'files':len(files),'crate_tree':certificate_hash}))
