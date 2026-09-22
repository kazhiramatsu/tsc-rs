from pathlib import Path
import json,os,subprocess,time,gzip,hashlib
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-leading-binding-prototype');C=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');T=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target');O=T/'emitter-body-commands-r365'
assert json.loads((T/'emitter-body-tests-followup-r370/manifest.json').read_text())['qualified']
assert json.loads((T/'emitter-body-measurement-r372/manifest.json').read_text())['qualified']
assert next(r for r in json.loads((T/'emitter-body-validation-r364/manifest.json').read_text())['steps'] if r['label']=='flags')['exit']==0
O.mkdir(exist_ok=False);sha=lambda b:hashlib.sha256(b).hexdigest()
def git(*a):return subprocess.check_output(['git',*a],cwd=R)
productdiff=git('diff','HEAD');paths=['crates/compiler/tests/fixtures/emitter-context-recovery.json','crates/compiler/tests/integration/h2_8a_import_helpers.rs','crates/syntax/tests/emitter_recovery.rs','crates/compiler/tests/integration/source_map_recording_witness_contract.rs','crates/compiler/tests/integration/source_map_emit_witness_contract.rs','crates/compiler/tests/emitter_final_rows.rs'];before={p:(R/p).read_bytes() for p in paths};after={p:(C/p).read_bytes() for p in paths[:3]}
p=paths[1];s=after[p].decode();start=s.index('fn context_recovery_matches_complete_typescript_observations()');head=s[:start];tail=s[start:]
a='    assert_eq!(cases.len(), 748);';assert tail.count(a)==1;tail=tail.replace(a,a+'''
    // Temporary edit-loop selector; the full phase removes the environment variable.
    let focused = std::env::var_os("EMITTER_R365_SYSTEM_ONLY").is_some();
    let cases: Vec<_> = cases.iter().filter(|case| !focused || case["case_id"].as_str().unwrap().starts_with("emitter-context-recovery/system-")).collect();
    assert_eq!(cases.len(), if focused { 160 } else { 748 });
''');after[p]=(head+tail).encode()
for p in paths[3:5]:after[p]=(Path('/tmp/emitter-empty-variable-witness-controls-r334')/Path(p).name).read_bytes()
after[paths[5]]=before[paths[5]]+b'''
#[test]
fn eof_map_system_bundle_frozen_controls() {
    let rows = [
        "typescript-6.0.3/compiler/outModuleConcatSystem.ts#target%3Des5",
        "typescript-6.0.3/compiler/outModuleConcatSystem.ts#target%3Des2015",
    ];
    let (exact, diverging) = replay(H2_6C, &rows, &mut None);
    assert!(diverging.is_empty(), "{diverging:?}");
    assert_eq!(exact.len(), 2);
}
'''
report={'head':git('rev-parse','HEAD').decode().strip(),'qualified':False,'scope':'Private final bodycomments: focused160System then all748completecommands x2 plus72refusals, frozen2SystemoutFilecontrols and8mapwitnesses; temporaryharnessselector recorded and cleared forfullphase, alltestcopies restored.','product_diff_sha256':sha(productdiff),'test_sources':{p:sha(b) for p,b in after.items()},'steps':[]}
(O/'product.diff.gz').write_bytes(gzip.compress(productdiff,mtime=0))
def save():(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n')
env=os.environ.copy();env.update(CARGO_BUILD_JOBS='2',CARGO_TARGET_DIR=str(T))
for k in ['TSC_RS_IMPORT_HELPERS_CASE_FILTER','TSC_RS_EMITTER_FINAL_CASE_FILTER','EMITTER_R365_SYSTEM_ONLY']:env.pop(k,None)
def run(label,package,target,filter=None,focused=False):
 args=['taskpolicy','-b','nice','-n','15','cargo','test','--offline','-p',package,'--test',target]
 if filter:args.append(filter)
 args+=['--','--nocapture','--test-threads=1'];e=env.copy()
 if focused:e['EMITTER_R365_SYSTEM_ONLY']='1'
 start=time.monotonic();log=O/(label+'.log')
 with log.open('xb') as f:code=subprocess.run(args,cwd=R,env=e,stdout=f,stderr=subprocess.STDOUT).returncode
 raw=log.read_bytes();(O/(label+'.log.gz')).write_bytes(gzip.compress(raw,mtime=0));row={'label':label,'argv':args,'focused160':focused,'exit':code,'seconds':time.monotonic()-start,'log_sha256':sha(raw)};report['steps'].append(row);save();print(json.dumps(row),flush=True);return code
save();codes=[]
try:
 for p,b in after.items():(R/p).write_bytes(b)
 (O/'with-test-copies.diff.gz').write_bytes(gzip.compress(git('diff','HEAD'),mtime=0))
 codes.append(run('system160','tsc-rs-compiler','contracts','h2_8a_import_helpers::context_recovery_matches_complete_typescript_observations',True))
 if not codes[-1]:
  codes.append(run('syntax-controls','tsc-rs-syntax','emitter_recovery'))
  codes.append(run('context748','tsc-rs-compiler','contracts','h2_8a_import_helpers::context_recovery_matches_complete_typescript_observations'))
  codes.append(run('rows-and-frozen-bundle','tsc-rs-compiler','emitter_final_rows'))
  codes.append(run('recording-witnesses','tsc-rs-compiler','contracts','source_map_recording_witness_contract::'))
  codes.append(run('production-witnesses','tsc-rs-compiler','contracts','source_map_emit_witness_contract::'))
 report['qualified']=len(codes)==6 and not any(codes);save()
finally:
 for p,b in before.items():
  assert (R/p).read_bytes()==after[p],p
  (R/p).write_bytes(b)
 assert git('diff','HEAD')==productdiff
 report['test_files_restored']=True;save()
raise SystemExit(max(codes))
