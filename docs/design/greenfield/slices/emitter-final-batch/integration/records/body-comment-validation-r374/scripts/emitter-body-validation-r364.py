from pathlib import Path
import json,subprocess,os,hashlib,gzip,time
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-leading-binding-prototype');C=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');T=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target');O=T/'emitter-body-validation-r364'
prior=json.loads((T/'emitter-system-eof-full-command-r342/manifest.json').read_bytes());assert prior.get('test_files_restored'), '342 must restore frozen files'
assert [(r['label'],r['exit']) for r in prior['steps'] if r['exit']] == [('context684',101)]
O.mkdir(exist_ok=False);sha=lambda b:hashlib.sha256(b).hexdigest();draft=Path('/tmp/emitter-system-eof-final-r351');proposal=json.loads((draft/'manifest.json').read_bytes())
for row in proposal['paths']:assert sha((R/row['path']).read_bytes())==row['original_sha256']
for row in proposal['paths']:
 p=row['path'];data=(Path('/tmp/emitter-body-final-r361/printer.rs') if p.endswith('/src/printer.rs') else draft/p).read_bytes();(R/p).write_bytes(data)
paths=['crates/emitter/tests/fixtures/meta-property-token-map-invariants.json','crates/emitter/tests/fixtures/list-comment-flags.json','crates/emitter/tests/list_comment_flags_contract.rs','crates/emitter/tests/fixtures/printer-failure-review.json','crates/emitter/tests/printer_failure_contract.rs']
for p in paths:(R/p).write_bytes((C/p).read_bytes())
report={'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip(),'scope':'Private361 typed EOF guard + range-owned bodycomments + body-only NoNested extent, official172flags/22failure +12MetaProperty and515+emitterlib controls. Prior342 failed4Systemtrivia; not claimedqualified. Newfull748compiler controls tofollow.','qualified':False,'steps':[]}
def save():(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n')
env=os.environ.copy();env.update(CARGO_BUILD_JOBS='2',CARGO_TARGET_DIR=str(T),TSC_RS_PRINTER_FAILURE_REVIEW_ACTUAL=str(O/'failure-review-actual.json'))
def run(label,args):
 log=O/(label+'.log');start=time.monotonic()
 with log.open('xb') as f:code=subprocess.run(args,cwd=R,env=env,stdout=f,stderr=subprocess.STDOUT).returncode
 raw=log.read_bytes();(O/(label+'.log.gz')).write_bytes(gzip.compress(raw,mtime=0));row={'label':label,'argv':args,'exit':code,'seconds':time.monotonic()-start,'log_sha256':sha(raw)};report['steps'].append(row);save();print(json.dumps(row),flush=True);return code
save();codes=[];codes.append(run('fmt',['cargo','fmt','--all']));assert not codes[-1]
base=['taskpolicy','-b','nice','-n','15','cargo','test','--offline','-p','tsc-rs-emitter']
for label,args in [('emitter-lib',['--lib']),('flags',['--test','list_comment_flags_contract']),('failure',['--test','printer_failure_contract'])]:
 codes.append(run(label,base+args+['--','--nocapture','--test-threads=1']))
report['qualified']=not any(codes);diff=subprocess.check_output(['git','diff','HEAD'],cwd=R);(O/'private.diff.gz').write_bytes(gzip.compress(diff,mtime=0));report['diff_sha256']=sha(diff);report['source_files']={p:sha((R/p).read_bytes()) for p in subprocess.check_output(['git','diff','HEAD','--name-only'],cwd=R,text=True).splitlines()};save();raise SystemExit(max(codes))
