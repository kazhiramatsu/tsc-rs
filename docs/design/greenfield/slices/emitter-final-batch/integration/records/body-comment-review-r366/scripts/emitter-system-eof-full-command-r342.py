from pathlib import Path
import json,os,subprocess,time,gzip,hashlib
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-leading-binding-prototype')
C=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep')
T=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target')
O=T/'emitter-system-eof-full-command-r342';O.mkdir(exist_ok=False)
sha=lambda b:hashlib.sha256(b).hexdigest()
def git(*args):return subprocess.check_output(['git',*args],cwd=R)
head=git('rev-parse','HEAD').decode().strip()
product_diff=git('diff','HEAD')
product_paths=set(git('diff','HEAD','--name-only').decode().splitlines())
assert product_paths=={'crates/emitter/src/printer.rs','crates/emitter/src/builtins/system.rs','crates/emitter/tests/unit/source_map/tests.rs'}
assert 'test result: ok.' in Path('/tmp/emitter-system-eof-lib-r343.log').read_text()
paths=['crates/compiler/tests/fixtures/emitter-context-recovery.json','crates/compiler/tests/integration/h2_8a_import_helpers.rs','crates/compiler/tests/fixtures/export-name-syntax-maps.json','crates/compiler/tests/integration/h2_8a_export_name_syntax_maps.rs','crates/syntax/tests/emitter_recovery.rs']
paths+=['crates/compiler/tests/integration/'+p for p in ['source_map_recording_witness_contract.rs','source_map_emit_witness_contract.rs']]
rows_path='crates/compiler/tests/emitter_final_rows.rs'
paths.append(rows_path)
before={p:(R/p).read_bytes() for p in paths}
after={p:(C/p).read_bytes() for p in paths[:5]}
for p in paths[5:7]:after[p]=(Path('/tmp/emitter-empty-variable-witness-controls-r334')/Path(p).name).read_bytes()
after[rows_path]=before[rows_path]+b'''
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

assert len(json.loads(after[paths[0]])['cases'])==684
assert len(json.loads(after[paths[0]])['refused_cases'])==72
assert len(json.loads(after[paths[2]])['cases'])==80
report={'head':head,'qualified':False,'scope':'Private System execute-array and EOF map fix, full context684 supported commands x2 plus72 syntax-refusal controls, B80 completecommands x2, source-map witnesses and adjacent map APIs. Test-only copies from canonical official mints. Syntax330 proof applies unchanged.','product_diff_sha256':sha(product_diff),'source_files':{p:sha((R/p).read_bytes()) for p in sorted(product_paths)},'test_sources':{p:sha(data) for p,data in after.items()},'steps':[]}
(O/'product.diff.gz').write_bytes(gzip.compress(product_diff,mtime=0))
def save():(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n')
env=os.environ.copy();env.update(CARGO_BUILD_JOBS='2',CARGO_TARGET_DIR=str(T))
for name in ['TSC_RS_IMPORT_HELPERS_CASE_FILTER','TSC_RS_EMITTER_FINAL_CASE_FILTER','TSC_RS_EMITTER_FINAL_CAPTURE_DIR','TSC_RS_RECOVERY_FACTS_OUTPUT']:env.pop(name,None)
def run(label,package,target,filter=None):
 args=['taskpolicy','-b','nice','-n','15','cargo','test','--offline','-p',package,'--test',target]
 if filter:args.append(filter)
 args+=['--','--nocapture','--test-threads=1']
 start=time.monotonic();log=O/(label+'.log')
 with log.open('xb') as f:code=subprocess.run(args,cwd=R,env=env,stdout=f,stderr=subprocess.STDOUT).returncode
 raw=log.read_bytes();(O/(label+'.log.gz')).write_bytes(gzip.compress(raw,mtime=0));row={'label':label,'argv':args,'exit':code,'seconds':time.monotonic()-start,'log_sha256':sha(raw)};report['steps'].append(row);save();print(json.dumps(row),flush=True);return code
save();codes=[]
try:
 for p,data in after.items():(R/p).write_bytes(data)
 assert set(git('diff','HEAD','--name-only').decode().splitlines())==product_paths|set(paths)
 (O/'with-test-copies.diff.gz').write_bytes(gzip.compress(git('diff','HEAD'),mtime=0))
 codes.append(run('syntax-controls','tsc-rs-syntax','emitter_recovery'))
 codes.append(run('context684','tsc-rs-compiler','contracts','h2_8a_import_helpers::context_recovery_matches_complete_typescript_observations'))
 codes.append(run('leading80','tsc-rs-compiler','contracts','h2_8a_export_name_syntax_maps::'))
 codes.append(run('recording-witnesses','tsc-rs-compiler','contracts','source_map_recording_witness_contract::'))
 codes.append(run('production-witnesses','tsc-rs-compiler','contracts','source_map_emit_witness_contract::'))
 codes.append(run('w4a-controls','tsc-rs-compiler','contracts','h2_7b_w4a_controls::'))
 for target in ['emitter_final_rows','h2_6a_map_option_projection','h2_7e_declaration_maps','h2_7e_declaration_map_apis']:
  codes.append(run(target,'tsc-rs-compiler',target))
 report['qualified']=not any(codes);save()
finally:
 for p,data in before.items():
  assert (R/p).read_bytes()==after[p],('concurrent edit',p)
  (R/p).write_bytes(data)
 assert git('diff','HEAD')==product_diff
 report['test_files_restored']=True;save()
print(json.dumps({'head':head,'qualified':report['qualified'],'steps':report['steps']}),flush=True)
raise SystemExit(max(codes))
