from pathlib import Path
import os,hashlib,json,subprocess,time
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');D=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target/debug');B=R/'docs/design/greenfield/slices/emitter-final-batch/integration';O=Path('/tmp/emitter-export-clause-focused-r440');h=lambda b:hashlib.sha256(b).hexdigest();a=json.loads(Path('/tmp/emitter-export-clause-apply-r439/manifest.json').read_bytes());assert a['original_observations_unchanged'];diff=subprocess.check_output(['git','diff','HEAD'],cwd=R);assert h(diff)==a['candidate_diff_sha256'];O.mkdir(exist_ok=False)
report={'head':a['head'],'candidate_diff_sha256':h(diff),'qualified':False,'scope':'Bounded 36 complete commands x2 using unmodified existing full comparator and separately captured complete command tuples; freshly built canonical libraries; strict resume owner guard remains required.','steps':[]}
def save():(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n')
def run(label,cmd):
 assert subprocess.check_output(['git','diff','HEAD'],cwd=R)==diff;t=time.monotonic();code=subprocess.run(['python3',str(B/'run-local.py'),label,*cmd],cwd=R).returncode;report['steps'].append({'label':label,'exit':code,'seconds':time.monotonic()-t});save();assert subprocess.check_output(['git','diff','HEAD'],cwd=R)==diff;assert code==0
save();run('export-clause-build-r440',['cargo','test','--offline','-p','tsc-rs-compiler','--test','contracts','--no-run','--message-format=json'])
artifacts=[]
for line in (B/'records/local/export-clause-build-r440.log').read_text().splitlines():
 try:v=json.loads(line)
 except json.JSONDecodeError:continue
 if v.get('reason')=='compiler-artifact' and v.get('executable') and v.get('target',{}).get('name')=='contracts' and 'tsc-rs-compiler' in v['package_id']:artifacts.append(v['executable'])
assert len(artifacts)==1;binary=Path(artifacts[0]);fp_path=D/'.fingerprint'/('tsc-rs-compiler-'+binary.name.rsplit('-',1)[1])/'test-integration-test-contracts.json';fp=json.loads(fp_path.read_bytes());report['canonical_test_binary']={'path':str(binary),'sha256':h(binary.read_bytes()),'fingerprint':str(fp_path),'fingerprint_sha256':h(fp_path.read_bytes())}
libs={}
for name in ['tsc_compiler','tsc_diagnostics','tsc_emitter','tsc_host','tsc_program','serde_json','sha2','base64','tsc_syntax','tsc_harness']:
 expected=next(d[3] for d in fp['deps'] if d[1]==name);found=[]
 for p in (D/'.fingerprint').glob('*/lib-'+name):
  if int.from_bytes(bytes.fromhex(p.read_text().strip()),'little')==expected:
   lib=D/'deps'/('lib'+name+'-'+p.parent.name.rsplit('-',1)[1]+'.rlib')
   if lib.is_file():found.append(lib)
 assert len(found)==1,(name,found);lib=found[0];libs[name]={'path':str(lib),'sha256':h(lib.read_bytes()),'fingerprint':expected}
report['libraries']=libs
fixture=R/'crates/compiler/tests/fixtures/token-comment-phases.json';cs=json.loads(fixture.read_bytes())['cases'];selected=[c for c in cs if '/export-clause-owner/' in c['case_id'] or any('/'+prefix+'/' in c['case_id'] for prefix in ['module-recovery-owner/namespace-export','module-recovery-owner/namespace-declare','module-recovery-owner/export-star-export','recovery-import-owner/side-effect'])];assert len(selected)==36,len(selected);(O/'cases.json').write_text(json.dumps({'cases':selected},indent=2)+'\n');report['case_ids']=[c['case_id'] for c in selected];report['fixture_sha256']=h(fixture.read_bytes())
source=(R/'crates/compiler/tests/integration/h2_8a_token_comment_phases.rs').read_text();capture='''use base64::Engine as _;
use serde_json::{json,Value};
use tsc_diagnostics::{Diagnostic,MessageChain};
use tsc_emitter::{EmitArtifact,EmitArtifactKind,EmitWriteMetadata};
'''+source[source.index('// Supplemental executions'):];assert capture.count('fn capture_complete_command(')==1;capture=capture.replace('fn capture_complete_command(','pub fn capture_complete_command(');(O/'capture.rs').write_text(capture)
(O/'main.rs').write_text('''#![allow(dead_code,unused_imports)]
#[path = "'''+str(R)+'''/crates/program/tests/support/scalar_json.rs"] mod utf16_scalar_json;
#[path = "'''+str(R)+'''/crates/compiler/tests/integration/h2_7b_w4a_controls.rs"] mod h2_7b_w4a_controls;
#[path = "'''+str(R)+'''/crates/compiler/tests/integration/h2_7c_declaration_blocking.rs"] mod h2_7c_declaration_blocking;
mod capture;
fn main(){let artifact:serde_json::Value=serde_json::from_str(include_str!("cases.json")).unwrap();let mut failures=Vec::new();for case in artifact["cases"].as_array().unwrap(){let id=case["case_id"].as_str().unwrap();let result=std::panic::catch_unwind(||h2_7c_declaration_blocking::assert_cases_with_inspection(&serde_json::json!({"cases":[case]}),true,capture::capture_complete_command));if result.is_ok(){println!("EXACT x2 {id}");}else{failures.push(id);}}assert!(failures.is_empty(),"{failures:?}");}
''')
args=['rustc','--edition=2021',str(O/'main.rs'),'-L','dependency='+str(D/'deps'),'-o',str(O/'probe')]
for name,v in libs.items():args+=['--extern',name+'='+v['path']]
env=os.environ.copy();env['CARGO_MANIFEST_DIR']=str(R/'crates/compiler');env['TSC_RS_H2_8A_CAPTURE_WRITES_DIR']=str(O/'captures');report['source_sha256']={p.name:h(p.read_bytes()) for p in [O/'main.rs',O/'capture.rs',O/'cases.json']};save()
with (O/'build.log').open('w') as f:report['probe_build_exit']=subprocess.run(['taskpolicy','-b','nice','-n','15']+args,cwd=R,env=env,stdout=f,stderr=subprocess.STDOUT).returncode
save();assert report['probe_build_exit']==0
with (O/'native.log').open('w') as f:report['native_exit']=subprocess.run(['taskpolicy','-b','nice','-n','15',str(O/'probe')],cwd=R,env=env,stdout=f,stderr=subprocess.STDOUT).returncode
report['native_log_sha256']=h((O/'native.log').read_bytes());report['native_binary_sha256']=h((O/'probe').read_bytes());captures=[json.loads(p.read_bytes()) for p in sorted((O/'captures').glob('*.json'))];report['capture_count']=len(captures);report['captured_all_exact']=len(captures)==72 and all(c['actual']==c['expected'] and c['error'] is None for c in captures);save();print(json.dumps({k:report.get(k) for k in ['native_exit','capture_count','captured_all_exact']}),flush=True);assert report['native_exit']==0 and report['captured_all_exact'];assert subprocess.check_output(['git','diff','HEAD'],cwd=R)==diff
run('export-clause-resume-guard-r440',['cargo','test','--offline','-p','tsc-rs-emitter','--lib','token_cursor::tests::comment_resume_can_only_merge_progress_for_the_same_owner_boundary','--','--exact','--nocapture','--test-threads=1']);log=(B/'records/local/export-clause-resume-guard-r440.log').read_text();assert 'test result: ok. 1 passed; 0 failed;' in log;report['qualified']=True;save();print(json.dumps({'qualified':True,'commands':36,'repetitions':2,'strict_resume_guard':True}))
