import hashlib,json,os,subprocess,time
from pathlib import Path
out=Path('target/ops-cover-3d')
groups=[('specifiers','h2_8a_declaration_specifiers',['focused_declaration_specifiers_match_complete_commands','composition_declaration_specifiers_match_complete_commands']),('comments','h2_8a_declaration_comment_ranges',['declaration_comment_range_focused_complete_commands','declaration_comment_detached_prefix_complete_commands','declaration_comment_parameter_tags_complete_commands']),('jsdoc','h2_8a_jsdoc_return',['jsdoc_return_controls_match_complete_commands_twice'])]
env=dict(os.environ,CARGO_BUILD_JOBS='2')
for key in list(env):
 if key.startswith('TSC_RS_'): del env[key]
records=[]
for name,target,tests in groups:
 cmd=['cargo','test','--manifest-path','crates/compiler/Cargo.toml','--test',target,tests[0],'--','--exact',*tests[1:],'--nocapture','--test-threads=1']
 start=time.monotonic()
 with (out/f'baseline-{name}.log').open('w') as log: result=subprocess.run(cmd,env=env,stdout=log,stderr=subprocess.STDOUT)
 record={'name':name,'argv':cmd,'exit':result.returncode,'seconds':round(time.monotonic()-start,3)}
 records.append(record); print(json.dumps(record),flush=True)
 (out/'baseline.json').write_text(json.dumps(records,indent=2)+'\n')
 if result.returncode: raise SystemExit(result.returncode)
