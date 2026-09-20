from pathlib import Path
import argparse,subprocess,json,time,re
p=argparse.ArgumentParser();p.add_argument('--head',required=True);a=p.parse_args()
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');T=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target');B=R/'docs/design/greenfield/slices/emitter-final-batch/integration';O=T/'emitter-canonical-focused-r396';O.mkdir(exist_ok=False)
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip()==a.head
assert not subprocess.check_output(['git','diff','HEAD','--name-only','--','crates','scripts'],cwd=R).strip()
steps=[('format',['cargo','fmt','--all','--','--check']),('emitter-all',['cargo','test','--offline','-p','tsc-rs-emitter','--no-fail-fast','--','--test-threads=1'])]
compiler=['emitter_residual_audit::r77_recovery_regressions_match_complete_typescript_commands','h2_7a_m4_controls::','h2_7b_w3a_controls::w4_a0_reused_type_references_apply_factory_parenthesization','program_session_contract::programmatic_node_module_resolution_relationships_keep_exact_module_names','h2_8a_token_comment_phases::','h2_8a_ellipsis_comment_owners::','h2_8a_import_helpers::import_helpers_matches_complete_typescript_observations','h2_8a_meta_property_token_maps::','h2_8a_export_name_syntax_maps::','h2_8a_import_helpers::context_recovery_matches_complete_typescript_observations','emitter_residual_audit::empty_block_comments_match_complete_typescript_commands']
for i,f in enumerate(compiler):steps.append(('compiler-'+str(i),['cargo','test','--offline','-p','tsc-rs-compiler','--test','contracts',f,'--','--nocapture','--test-threads=1']))
steps.append(('syntax-context',['cargo','test','--offline','-p','tsc-rs-syntax','--test','emitter_recovery','--','--nocapture','--test-threads=1']))
steps.append(('utf16-boundaries',['cargo','test','--offline','-p','tsc-rs-compiler','--test','h2_8a_utf16_identity_recovery_controls','--','--nocapture','--test-threads=1']))
xtask=['h2_1a_acceptance::','h2_2c_acceptance::h2_6c_de_legacy_collector::nonbundle_declaration_maps_dispose_javascript_parse_metadata','h2_2c_acceptance::h2_7b_tests::h2_6c_current_manifest_keeps_only_unclosed_historical_refusals','h2_3d_acceptance::tests::pinned_h2_3d_acceptance_is_exact']
for i,f in enumerate(xtask):steps.append(('xtask-'+str(i),['cargo','test','--offline','-p','tsc-rs-xtask','--bin','xtask',f,'--','--nocapture','--test-threads=1']))
steps.append(('clippy',['cargo','clippy','--offline','--workspace','--all-targets','--','-D','warnings']))
report={'head':a.head,'scope':'Retry349 after reviewed independent comment endpoints and empty function-body ownership repair. Full emitter all targets with no-fail-fast; original red compiler bands plus B80/context788/empty72 and syntax boundaries; final unsplit CI remains mandatory.','qualified':False,'steps':[]}
def save():(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n')
save()
for name,cmd in steps:
 label='canonical-focused-'+name+'-r396';start=time.monotonic();code=subprocess.run(['python3',str(B/'run-local.py'),label,*cmd],cwd=R).returncode
 raw=(B/'records/local'/(label+'.log')).read_text()
 summaries=[tuple(map(int,m)) for m in re.findall(r'test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;',raw)]
 is_test=cmd[:2]==['cargo','test'];passed=sum(m[0] for m in summaries);ignored=sum(m[2] for m in summaries)
 checked=code==0 and (not is_test or passed>0)
 report['steps'].append({'label':label,'exit':code,'checks_passed':checked,'seconds':time.monotonic()-start,'argv':cmd,'passed_tests':passed if is_test else None,'ignored_tests':ignored if is_test else None,'test_summaries':summaries});save();print(json.dumps(report['steps'][-1]),flush=True)
 if not checked and name in ['format','emitter-all']:break
report['qualified']=len(report['steps'])==len(steps) and all(s['checks_passed'] for s in report['steps']);save();raise SystemExit(0 if report['qualified'] else 1)
