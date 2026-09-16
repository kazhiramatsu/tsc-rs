import hashlib, json, os, subprocess, time
from pathlib import Path
root=Path.cwd()
out=root/'target/literal-update-integration'
record={'base': subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(), 'runs':[]}
env=dict(os.environ,CARGO_BUILD_JOBS='2',CARGO_INCREMENTAL='0',CARGO_PROFILE_TEST_DEBUG='0',CARGO_NET_OFFLINE='true',RUSTC_WRAPPER='')
record['env']={k:env[k] for k in ('CARGO_BUILD_JOBS','CARGO_INCREMENTAL','CARGO_PROFILE_TEST_DEBUG','CARGO_NET_OFFLINE','RUSTC_WRAPPER')}
record['source_sha256']={str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(root.glob('crates/**/src/**/*.rs'))}
def run(name,cmd):
 start=time.monotonic()
 with (out/(name+'.log')).open('w') as log:
  p=subprocess.run(['taskpolicy','-b','nice','-n','15',*cmd],cwd=root,env=env,stdout=log,stderr=subprocess.STDOUT)
 row={'name':name,'argv':cmd,'exit_code':p.returncode,'seconds':round(time.monotonic()-start,3),'log':name+'.log','sha256':hashlib.sha256((out/(name+'.log')).read_bytes()).hexdigest()}
 record['runs'].append(row)
 (out/'local.v1.json').write_text(json.dumps(record,indent=2)+'\n')
 print(json.dumps(row),flush=True)
 if p.returncode: raise SystemExit(p.returncode)
run('fmt',['cargo','fmt','--all','--','--check'])
run('emitter-witnesses',['python3','-c',"import sys; sys.path.insert(0,'scripts'); import witness; witness.run_emitter_direct(['literal-update','literal-value-provenance','literal-parent-provenance','string-literal-identifier-source','utf16-literal-escaping'])"])
run('compiler-witnesses',['python3','-c',"import sys; sys.path.insert(0,'scripts'); import witness; witness.run_compiler_direct(['literal-update-pipeline','require-rewrite','utf16-tagged-template','utf16-literal-witnesses'])"])
run('emitter-lib',['cargo','test','--manifest-path','crates/emitter/Cargo.toml','--lib','--','--test-threads=1'])
record['binary_sha256']={str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for stem in ('literal_update_contract','literal_update_pipeline_contract','h2_8a_require_rewrite') for p in (root/'target/debug/deps').glob(stem+'-*') if p.is_file() and p.suffix==''}
assert all(hashlib.sha256(Path(p).read_bytes()).hexdigest()==h for p,h in record['source_sha256'].items())
record['source_unchanged_during_validation']=True
(out/'local.v1.json').write_text(json.dumps(record,indent=2)+'\n')
print('All focused local checks passed.',flush=True)
