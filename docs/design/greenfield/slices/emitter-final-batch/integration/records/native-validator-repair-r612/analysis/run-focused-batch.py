from pathlib import Path
import subprocess,json,sys
out=Path('/tmp/emitter-final-native-failure-r611')
prefix=['taskpolicy','-b','nice','-n','15']
cases=[
 ('live-anchor-final-controls',['cargo','test','-p','tsc-rs-conformance','--lib','host_resolution::tests::live_anchor','--','--test-threads=1']),
 ('host-resolution-final',['cargo','xtask','host-resolution','check','--baseline','3b1f5fe87fd31e3b303bb44bd257342735452ed9']),
 ('cli-library-final',['cargo','test','-p','tsc-rs-compiler','--lib','--','--test-threads=1']),
 ('cli-option-commands-final',['cargo','test','-p','tsc-rs-compiler','--test','contracts','cli_contract::implemented_emit_option_names_match_typescript_cli_and_config','--','--exact','--test-threads=1']),
 ('cli-diagnostic-order-final',['cargo','test','-p','tsc-rs-compiler','--test','contracts','cli_contract::filesystem_config_and_source_diagnostic_order_matches_typescript','--','--exact','--test-threads=1'])
]
results=[]
for label,cmd in cases:
 p=subprocess.run([sys.executable,str(out/'run-check.py'),label,*prefix,*cmd])
 results.append({'label':label,'exit':p.returncode})
 (out/'focused-batch.json').write_text(json.dumps({'results':results,'complete':len(results)==len(cases) and all(x['exit']==0 for x in results)},indent=2)+'\n')
 if p.returncode:raise SystemExit(p.returncode)
