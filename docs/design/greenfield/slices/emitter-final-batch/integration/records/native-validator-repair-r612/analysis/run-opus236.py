from pathlib import Path
import subprocess,time,json
out=Path('/tmp/emitter-final-native-failure-r611')
args=['claude','--resume','124932ff-57f1-4cad-baa4-57875f2bfe2c','--model','opus','--permission-mode','dontAsk','--allowedTools','Read,Grep,Glob,Bash(git *),Bash(rg *),Bash(sed *),Bash(python3 *)','--output-format','json','--print',(out/'opus236-request.md').read_text()]
started=time.monotonic()
with (out/'opus236-response.json').open('xb') as stdout,(out/'opus236-stderr.log').open('xb') as stderr:
 p=subprocess.run(args,cwd='/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep',stdout=stdout,stderr=stderr)
(out/'opus236-launcher.json').write_text(json.dumps({'exit':p.returncode,'seconds':time.monotonic()-started,'model':'opus','scope':'Actual read-only review; no native tests or mint authorized'},indent=2)+'\n')
print(json.dumps({'exit':p.returncode,'seconds':time.monotonic()-started,'response':str(out/'opus236-response.json')}),flush=True)
raise SystemExit(p.returncode)
