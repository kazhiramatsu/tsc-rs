from pathlib import Path
import json,os,subprocess,time,gzip,hashlib
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-leading-binding-prototype')
T=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target')
O=T/'emitter-empty-variable-full-command-r335';O.mkdir(exist_ok=False)
HEAD='4aeb1640156f291f4b5189b8ad9ed370cf17cf9d'
sha=lambda b:hashlib.sha256(b).hexdigest()
def git(*args):return subprocess.check_output(['git',*args],cwd=R)
while True:
 j=json.loads((T/'emitter-leading-binding-full-command-r332/manifest.json').read_bytes())
 if j.get('test_files_restored'):break
 time.sleep(10)
assert j['qualified'],'resolve B command divergence before more admission'
assert git('rev-parse','HEAD').decode().strip()==HEAD
assert not git('diff','HEAD','--name-only').strip()
fixture='crates/compiler/tests/fixtures/emitter-context-recovery.json'
consumer='crates/compiler/tests/integration/h2_8a_import_helpers.rs'
paths=[fixture,consumer]+['crates/compiler/tests/integration/'+p for p in ['source_map_recording_witness_contract.rs','source_map_emit_witness_contract.rs']]
before={p:(R/p).read_bytes() for p in paths}
after={fixture:gzip.decompress((T/'emitter-empty-variable-controls-mint-r331/previous600.json.gz').read_bytes())}
mint=json.loads((T/'emitter-empty-variable-controls-mint-r328/manifest.json').read_bytes())
assert sha(after[fixture])==mint['fixture_sha256']
artifact=json.loads(after[fixture]);assert len(artifact['cases'])==528
selected=[c['case_id'] for c in artifact['cases'] if '/empty-variable/' in c['case_id']]
assert len(selected)==96
s=before[consumer].decode();needle='    assert_eq!(cases.len(), 432);'
assert s.count(needle)==1
s=s.replace(needle,'''    assert_eq!(cases.len(), 528);
    let cases: Vec<_> = cases.iter().filter(|case| {
        case["case_id"].as_str().unwrap().contains("/empty-variable/")
    }).collect();
    assert_eq!(cases.len(), 96);''')
after[consumer]=s.encode()
for p in paths[2:]:after[p]=(Path('/tmp/emitter-empty-variable-witness-controls-r334')/Path(p).name).read_bytes()
report={'head':HEAD,'qualified':False,'scope':'96 new empty-variable complete commands, exactly twice each, using canonical official r328 observations; old432 controls and additional60 r331 controls remain pending. Also existing frozen source-map recording/production witnesses with parity assertions. Test-only fixture/consumer modifications; no product mutation.','selected_case_ids':selected,'source_files':{p:sha(data) for p,data in after.items()},'steps':[]}
def save():(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n')
env=os.environ.copy();env.update(CARGO_BUILD_JOBS='2',CARGO_TARGET_DIR=str(T))
def run(label,filter):
 args=['taskpolicy','-b','nice','-n','15','cargo','test','--offline','-p','tsc-rs-compiler','--test','contracts',filter,'--','--nocapture','--test-threads=1']
 start=time.monotonic();log=O/(label+'.log')
 with log.open('xb') as f:code=subprocess.run(args,cwd=R,env=env,stdout=f,stderr=subprocess.STDOUT).returncode
 raw=log.read_bytes();(O/(label+'.log.gz')).write_bytes(gzip.compress(raw,mtime=0));row={'label':label,'argv':args,'exit':code,'seconds':time.monotonic()-start,'log_sha256':sha(raw)};report['steps'].append(row);save();print(json.dumps(row),flush=True);return code
save()
try:
 for p,data in after.items():(R/p).write_bytes(data)
 assert set(git('diff','HEAD','--name-only').decode().splitlines())==set(paths)
 (O/'test-only.diff.gz').write_bytes(gzip.compress(git('diff','HEAD'),mtime=0))
 codes=[]
 codes.append(run('empty96','h2_8a_import_helpers::context_recovery_matches_complete_typescript_observations'))
 # Inspect both frozen APIs even if new command controls find an output mismatch.
 codes.append(run('recording-witnesses','source_map_recording_witness_contract::'))
 codes.append(run('production-witnesses','source_map_emit_witness_contract::'))
 report['qualified']=not any(codes);save()
finally:
 for p,data in before.items():
  assert (R/p).read_bytes()==after[p],('concurrent edit',p)
  (R/p).write_bytes(data)
 assert not git('diff','HEAD','--name-only').strip()
 report['test_files_restored']=True;save()
print(json.dumps({'head':HEAD,'qualified':report['qualified'],'steps':report['steps']}),flush=True)
raise SystemExit(max(codes))
