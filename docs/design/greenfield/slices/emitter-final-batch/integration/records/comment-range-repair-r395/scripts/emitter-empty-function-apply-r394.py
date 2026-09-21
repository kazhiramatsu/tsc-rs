from pathlib import Path
import json,gzip,hashlib,subprocess
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');B=R/'docs/design/greenfield/slices/emitter-final-batch/integration';T=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target');O=T/'emitter-empty-function-apply-r394';O.mkdir(exist_ok=False);D=Path('/tmp/emitter-empty-function-final-r393')
assert json.loads((T/'emitter-comment-range-mint-r390/manifest.json').read_bytes())['oracle_qualified']
assert not json.loads(Path('/tmp/emitter-claude-review-round190-opus.json').read_bytes())['is_error']
report={'source_head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip(),'native_qualified':False,'scope':'Apply actual190 empty-function ownership correction and observe96 adjacent/detached factory controls; no native or final qualification yet.','steps':[]}
for dst,src in [('crates/emitter/src/printer.rs','printer.rs'),('scripts/observe-list-comment-flags.mjs','observe-list-comment-flags.mjs'),('crates/emitter/tests/list_comment_flags_contract.rs','list_comment_flags_contract.rs')]:
 (O/(src+'.before.gz')).write_bytes(gzip.compress((R/dst).read_bytes(),mtime=0));(R/dst).write_bytes((D/src).read_bytes())
f=R/'crates/emitter/tests/fixtures/list-comment-flags.json';raw=f.read_bytes();old=json.loads(raw);assert len(old['cases'])==244;(O/'flags244-before.json.gz').write_bytes(gzip.compress(raw,mtime=0))
for name,cmd in [('empty-body-final-oracle',['node','scripts/observe-list-comment-flags.mjs','--write']),('empty-body-final-format',['cargo','fmt','--all'])]:
 code=subprocess.run(['python3',str(B/'run-local.py'),name+'-r394',*cmd],cwd=R).returncode;report['steps'].append({'label':name,'exit':code});(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n');assert code==0,(name,code)
 if name=='empty-body-final-oracle':
  fresh=json.loads(f.read_bytes());assert len(fresh['cases'])==340 and fresh['cases'][:244]==old['cases'];assert {k:v for k,v in old.items() if k not in ['observer_sha256','cases']}=={k:v for k,v in fresh.items() if k not in ['observer_sha256','cases']}
  for shape in ['empty-body-adjacent-line','empty-body-detached-line']:
   row=next(r for r in fresh['cases'] if r['case_id']==shape+'/Original/None/retained');print(row['case_id'],repr(row['output']),flush=True)
  report['old244_unchanged']=True;report['flags340_sha256']=hashlib.sha256(f.read_bytes()).hexdigest()
report['oracle_qualified']=True;(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report))
