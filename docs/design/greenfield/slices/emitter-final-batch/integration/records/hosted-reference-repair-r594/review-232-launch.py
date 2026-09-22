from pathlib import Path
import subprocess,json,time
b=Path('/tmp/emitter-hosted-reference-repair-r578')
args=['claude', '--resume', '124932ff-57f1-4cad-baa4-57875f2bfe2c', '--model', 'opus', '--permission-mode', 'dontAsk', '--allowedTools', 'Read,Grep,Glob,Bash(git *),Bash(rg *),Bash(sed *),Bash(python3 *)', '--output-format', 'json', '--print']+[(b/"review-232-request.md").read_text()]
s=time.monotonic()
with (b/"review-232-response.json").open("x") as out,(b/"review-232-stderr.log").open("x") as err:
 r=subprocess.run(args,cwd='/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep',stdout=out,stderr=err)
print(json.dumps({"review":232,"exit":r.returncode,"seconds":round(time.monotonic()-s,3)}),flush=True)
raise SystemExit(r.returncode)
