import hashlib,json,os,subprocess,time
from pathlib import Path
out=Path('target/ops-cover-3d')
selected=['declaration-specifiers','declaration-comments','jsdoc-return']
targets=['h2_8a_declaration_specifiers','h2_8a_declaration_comment_ranges','h2_8a_jsdoc_return']
def binary_hashes():
 result={}
 for name in targets:
  binaries=[p for p in Path('target/debug/deps').glob(name+'-*') if p.is_file() and os.access(p,os.X_OK) and p.suffix != '.d']
  assert len(binaries)==1,(name,binaries)
  p=binaries[0];result[name]={'path':str(p),'sha256':hashlib.sha256(p.read_bytes()).hexdigest()}
 return result
before=binary_hashes()
poisoned={'TSC_RS_DECL_COMMENT_FILTER':'no-matching-case','TSC_RS_JSDOC_RETURN_FILTER':'no-matching-case','TSC_RS_DECL_COMMENT_CAPTURE_DIR':'/nonexistent/ops-cover-stale','TSC_RS_JSDOC_RETURN_CAPTURE_DIR':'/nonexistent/ops-cover-stale','TSC_RS_DECLARATION_SPECIFIER_CAPTURE_DIR':'/nonexistent/ops-cover-stale'}
env=dict(os.environ,**poisoned,CARGO_BUILD_JOBS='2',WITNESS_SUITES=json.dumps(selected))
cmd=['python3','.github/ci/replay.py','witnesses']
start=time.monotonic()
with (out/'after.log').open('w') as log: result=subprocess.run(cmd,env=env,stdout=log,stderr=subprocess.STDOUT)
after=binary_hashes()
record={'argv':cmd,'environment':{'CARGO_BUILD_JOBS':'2','WITNESS_SUITES':env['WITNESS_SUITES'],**poisoned},'exit':result.returncode,'seconds':round(time.monotonic()-start,3),'baseline_binaries':before,'after_binaries':after,'binaries_unchanged':before==after}
(out/'after.json').write_text(json.dumps(record,indent=2)+'\n');print(json.dumps(record),flush=True)
assert before==after
raise SystemExit(result.returncode)
