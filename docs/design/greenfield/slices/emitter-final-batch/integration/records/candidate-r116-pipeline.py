"""Serialize builds; overlap read-only functional comparisons with frozen census.
Neither census nor comparison wall time is performance qualification.
"""
from pathlib import Path
import subprocess,os,signal,json,hashlib,datetime,re,shutil
root=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-recovery-next')
base=root/'docs/design/greenfield/slices/emitter-final-batch/integration';target=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target');pid=56011
head=subprocess.check_output(['git','-C',str(root),'rev-parse','HEAD'],text=True).strip();assert not subprocess.check_output(['git','-C',str(root),'diff','HEAD','--name-only']).strip()
census=json.loads((target/'emitter-census-r78-immutable/manifest.json').read_text());receipt=base/'records/local/census-build-pause-r116.json';assert not receipt.exists()
state={'head':head,'census_manifest':census,'steps':[],'protocol':'Census paused only for Cargo build; immutable native functional replays may overlap the read-only frozen census. All commands demoted; no timing qualifies performance. No xtask rebuild or source mutation.'}
now=lambda:datetime.datetime.now(datetime.timezone.utc).isoformat()
def save():receipt.write_text(json.dumps(state,indent=2)+'\n')
def verify_census():
 assert hashlib.sha256(Path(census['original_executable']).read_bytes()).hexdigest()==census['sha256']
 mapped=subprocess.check_output(['lsof','-a','-p',str(pid),'-d','txt','-Fni'],text=True)
 assert f"i{census['original_inode']}\nn{census['original_executable']}\n" in mapped
verify_census();os.kill(pid,signal.SIGSTOP);state['paused_at']=now();save()
def run(label,command,env=None):
 assert subprocess.check_output(['git','-C',str(root),'rev-parse','HEAD'],text=True).strip()==head
 assert not subprocess.check_output(['git','-C',str(root),'diff','HEAD','--name-only']).strip()
 r=subprocess.run(['python3',str(base/'run-local.py'),label,*map(str,command)],cwd=root,env=env)
 state['steps'].append({'label':label,'exit':r.returncode});save();return r.returncode
try:
 code=run('compiler-bounded-build-r116',['cargo','test','--offline','-p','tsc-rs-compiler','--test','contracts','--no-run'])
finally:
 verify_census();os.kill(pid,signal.SIGCONT);state['resumed_at']=now();state['census_executable_preserved']=True;save()
if code:raise SystemExit(code)
log=(base/'records/local/compiler-bounded-build-r116.log').read_text();matches=re.findall(r'Executable tests/contracts.rs \(([^)]+)\)',log);assert len(matches)==1,matches
original=Path(matches[0]);out=target/'emitter-r116-immutable';out.mkdir(exist_ok=False);binary=out/'contracts';shutil.copy2(original,binary);os.chmod(binary,0o555);digest=hashlib.sha256(binary.read_bytes()).hexdigest();assert digest==hashlib.sha256(original.read_bytes()).hexdigest();stat=original.stat();manifest={'head':head,'built_at':now(),'original_executable':str(original),'original_inode':stat.st_ino,'original_device':stat.st_dev,'immutable_executable':str(binary),'sha256':digest,'source_tree':str(root),'build_receipt':'records/local/compiler-bounded-build-r116.json','qualified_at_copy':False};(out/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
bounded=['emitter_residual_audit::'+n+'_match_complete_typescript_commands' for n in ['r107_declaration_comment_controls','r109_token_neighbours','r111_do_body_controls','r113_type_comment_controls']]
boundaries=['emitter_residual_audit::'+n+'_match_complete_typescript_commands' for n in ['r77_recovery_regressions','recovery_boundary_neighbours','heritage_factory_boundaries','helper_diagnostic_boundaries','nested_parenthesis_recovery','class_helper_gates','r95_emitter_neighbours','r95_wrapper_and_rest_neighbours','r95_statement_callee_boundaries','r104_object_rest_controls']]
# These three established test names use singular matches.
for i,n in enumerate(boundaries):
 if any(x in n for x in ['nested_parenthesis_recovery_match','r77_recovery_regressions_match']):
  # r77 is plural regressions and still uses match; preserve the actual spelling.
  if 'nested_parenthesis_recovery_match' in n:boundaries[i]=n.replace('_match_complete','_matches_complete')
original_tests=['emitter_residual_audit::system_binding_publication_matches_complete_typescript_commands','emitter_residual_audit::await_flag_boundaries_match_complete_typescript_commands','emitter_residual_audit::exported_destructuring_comments_match_complete_typescript_commands']+['h2_8a_import_helpers::'+n for n in ['missing_await_recovery_matches_complete_typescript_observations','missing_declaration_recovery_matches_complete_typescript_observations','missing_declaration_effects_match_complete_typescript_observations','missing_declaration_scripts_match_complete_typescript_observations','missing_declaration_binding_matches_complete_typescript_observations','parameter_gap_recovery_matches_complete_typescript_observations','statement_gap_recovery_matches_complete_typescript_observations','context_recovery_matches_complete_typescript_observations']]
listed=subprocess.check_output([str(binary),'--list','--format','terse'],text=True);names={line.removesuffix(': test') for line in listed.splitlines() if line.endswith(': test')}
for tests in [bounded,boundaries,original_tests]:assert len(tests)==len(set(tests)) and set(tests)<=names,sorted(set(tests)-names)
for label,tests,case_count in [('recovery-new-controls-r116',bounded,74),('recovery-complete-boundaries-r116',boundaries,1930),('system-recovery-native-r116',original_tests,4088)]:
 capture=target/(label+'-captures');assert not capture.exists();env=os.environ.copy();env['TSC_RS_H2_8A_CAPTURE_WRITES_DIR']=str(capture)
 result=run(label,[binary,'--exact',*tests,'--nocapture','--test-threads=1'],env)
 result_log=(base/'records/local'/ (label+'.log')).read_text()
 summary=re.findall(r'test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;.*?(\d+) filtered out;',result_log)
 assert len(summary)==1,summary
 if result:raise SystemExit(result)
 assert summary[0][:4]==('ok',str(len(tests)),'0','0'),summary
 assert hashlib.sha256(binary.read_bytes()).hexdigest()==digest
 state['steps'][-1]['expected_cases']=case_count;save()
print(json.dumps({'head':head,'steps':state['steps'],'native_executable_sha256':digest,'census_resumed':state['resumed_at']}),flush=True)
