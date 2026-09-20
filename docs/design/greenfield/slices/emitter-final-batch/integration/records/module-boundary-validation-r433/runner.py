from pathlib import Path
import argparse,gzip,hashlib,json,os,re,subprocess,time
p=argparse.ArgumentParser();p.add_argument('--head',required=True);p.add_argument('--diff-sha256');a=p.parse_args();R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');T=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target');B=R/'docs/design/greenfield/slices/emitter-final-batch/integration';O=T/'emitter-module-validation-r422'
def diff():return subprocess.check_output(['git','diff','HEAD'],cwd=R)
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip()==a.head
candidate_diff=diff();candidate_diff_sha256=hashlib.sha256(candidate_diff).hexdigest()
if a.diff_sha256:assert candidate_diff_sha256==a.diff_sha256
else:assert not candidate_diff
assert not subprocess.check_output(['git','ls-files','--others','--exclude-standard','--','crates','scripts'],cwd=R).strip()
assert len(json.loads((R/'crates/compiler/tests/fixtures/token-comment-phases.json').read_bytes())['cases'])==245
for k in ['TSC_RS_IMPORT_HELPERS_CASE_FILTER','TSC_RS_EMITTER_FINAL_CASE_FILTER','EMITTER_R376_SYSTEM_ONLY']:assert k not in os.environ,k
O.mkdir(exist_ok=False);(O/'candidate.patch.gz').write_bytes(gzip.compress(candidate_diff,mtime=0));steps=[('format',['cargo','fmt','--all','--','--check'],None),('emitter-all',['cargo','test','--offline','-p','tsc-rs-emitter','--no-fail-fast','--','--test-threads=1'],None)]
for label,f,count in [('m4','h2_7a_m4_controls::',None),('token','h2_8a_token_comment_phases::',409),('ellipsis','h2_8a_ellipsis_comment_owners::',514),('helpers','h2_8a_import_helpers::import_helpers_matches_complete_typescript_observations',551),('export-names','h2_8a_export_name_syntax_maps::',80),('context','h2_8a_import_helpers::context_recovery_matches_complete_typescript_observations',788),('recording','source_map_recording_witness_contract::',None),('production','source_map_emit_witness_contract::',None)]:
 steps.append((label,['cargo','test','--offline','-p','tsc-rs-compiler','--test','contracts',f,'--','--nocapture','--test-threads=1'],count))
steps.append(('utf16',['cargo','test','--offline','-p','tsc-rs-compiler','--test','h2_8a_utf16_identity_recovery_controls','--','--nocapture','--test-threads=1'],None))
steps.append(('h2-1a',['cargo','test','--offline','-p','tsc-rs-xtask','h2_1a_acceptance::tests::','--','--nocapture','--test-threads=1'],None))
steps.append(('clippy',['cargo','clippy','--offline','--workspace','--all-targets','--','-D','warnings'],None))
report={'head':a.head,'tracked_clean':not candidate_diff,'candidate_diff_sha256':candidate_diff_sha256,'qualified':False,'scope':'Reviewed import/export producer and printer corrections: whole emitter, previously failingM4/token/ellipsis, all affected helper/export/context commands, bothsource-map witness lanes, UTF16 boundaries and all H2.1a acceptance tests, workspaceclippy. Unsplit finalCI remainsmandatory. No temporary selectors or modified comparators.','steps':[]}
def save():(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n')
save()
for name,cmd,expected in steps:
 assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip()==a.head
 assert diff()==candidate_diff
 label='module-validation-'+name+'-r422';t=time.monotonic();code=subprocess.run(['python3',str(B/'run-local.py'),label,*cmd],cwd=R).returncode;log=(B/'records/local'/(label+'.log')).read_text();summaries=[tuple(map(int,m)) for m in re.findall(r'test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;',log)];passed=sum(m[0] for m in summaries);exact=len(re.findall(r'^.* EXACT x2 ',log,re.M));good=code==0 and (cmd[:2]!=['cargo','test'] or passed>0) and (expected is None or exact==expected)
 assert diff()==candidate_diff
 report['steps'].append({'label':label,'exit':code,'checks_passed':good,'seconds':time.monotonic()-t,'argv':cmd,'test_summaries':summaries,'passed_tests':passed,'expected_complete_commands':expected,'exact_x2_lines':exact});save();print(json.dumps(report['steps'][-1]),flush=True)
 if not good:break
report['qualified']=len(report['steps'])==len(steps) and all(s['checks_passed'] for s in report['steps']);save();raise SystemExit(0 if report['qualified'] else 1)
