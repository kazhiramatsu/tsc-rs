from pathlib import Path
import hashlib,json,os,subprocess,time,shutil
root=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-comma-prep')
base=root/'docs/design/greenfield/slices/emitter-final-batch/integration'
target=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target')
def alive(pid):
 try:os.kill(pid,0)
 except ProcessLookupError:return False
 return True
while alive(96491):time.sleep(10)
manifest=json.loads((target/'emitter-variable-type-r150-native/manifest.json').read_text())
artifacts={a['target']:a for a in manifest['artifacts']}
for a in artifacts.values():assert hashlib.sha256(Path(a['binary']).read_bytes()).hexdigest()==a['sha256']
results=[]
groups=[('variable-comma-native-r152',['variable_comma_recovery_matches_complete_typescript_commands']),('variable-type-neighbours-r152',[n+'_match_complete_typescript_commands' for n in ['r104_object_rest_controls','r107_declaration_comment_controls','r113_type_comment_controls','r117_type_comment_controls','r109_token_neighbours','r111_do_body_controls','r119_type_comment_controls']])]
for label,names in groups:
 assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()==manifest['head']
 env=os.environ.copy();capture=target/(label+'-captures');assert not capture.exists();env['TSC_RS_H2_8A_CAPTURE_WRITES_DIR']=str(capture)
 code=subprocess.run(['python3',str(base/'run-local.py'),label,artifacts['contracts']['binary'],'--exact',*['emitter_residual_audit::'+n for n in names],'--nocapture','--test-threads=1'],cwd=root,env=env).returncode
 results.append({'label':label,'exit':code})
# Independent measurements continue after a failure; every raw exit is retained.
label='variable-transpile-r152'
code=subprocess.run(['python3',str(base/'run-local.py'),label,artifacts['transpile_routes_contract']['binary'],'--test-threads=1'],cwd=root).returncode
results.append({'label':label,'exit':code})
out=target/'emitter-variable-tail-r152';out.mkdir(exist_ok=False)
(out/'summary.json').write_text(json.dumps({'source_head':manifest['head'],'steps':results},indent=2)+'\n')
p=root/'target/h2-8c'
if p.exists():shutil.copytree(p,out/'transpile-evidence')
print(json.dumps(results),flush=True)
raise SystemExit(1 if any(r['exit'] for r in results) else 0)
