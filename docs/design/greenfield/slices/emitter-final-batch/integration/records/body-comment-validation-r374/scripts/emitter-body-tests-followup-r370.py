from pathlib import Path
import json,subprocess,os,time,gzip,hashlib
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-leading-binding-prototype');C=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');T=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target');O=T/'emitter-body-tests-followup-r370';prior=json.loads((T/'emitter-body-validation-r364/manifest.json').read_text());assert len(prior['steps'])==4
assert [(r['label'],r['exit']) for r in prior['steps'] if r['exit']]==[('emitter-lib',101),('failure',101)], 'inspect all other failures before proceeding'
failurelog=(T/'emitter-body-validation-r364/failure.log').read_text();assert 'REVIEW exact 21/22' in failurelog and failurelog.count('REVIEW MISMATCH ')==1
raw=(T/'emitter-body-validation-r364/emitter-lib.log').read_text();assert '514 passed; 1 failed;' in raw and 'builtins::tests::system_module_hoists_uninitialized_export_from_source_owned_if_statement' in raw
oracle=json.loads(Path('/tmp/emitter-system-if-oracle-r368.json').read_text());assert oracle['observations'][0]['alwaysStrict']==False
expected='        execute: function () {// https://github.com/microsoft/TypeScript/issues/59373\n';assert expected in oracle['observations'][0]['outputText']
O.mkdir(exist_ok=False);p=R/'crates/emitter/tests/unit/builtins/tests.rs';before=p.read_bytes();s=before.decode();at=s.index('fn system_module_hoists_uninitialized_export_from_source_owned_if_statement()');end=s.index('\n#[test]',at);chunk=s[at:end];x='''            "        execute: function () {\\n",''';y='''            "        execute: function () {// https://github.com/microsoft/TypeScript/issues/59373\\n",''';assert chunk.count(x)==1;s=s[:at]+chunk.replace(x,y)+s[end:];p.write_text(s)
fixture='crates/emitter/tests/fixtures/printer-failure-review.json';(O/'previous-failure-fixture.json.gz').write_bytes(gzip.compress((R/fixture).read_bytes(),mtime=0));(R/fixture).write_bytes((C/fixture).read_bytes())
report={'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip(),'qualified':False,'scope':'No productionchange. Correct one stale System unit JS expectation using pinnedTypeScript368(explicitalwaysStrictfalse), extend newserializedfailurecase with2successfulsame-printer operations peractual186. Original21failurecasesunchanged. ThisrunqualifiesONLYemitterlib; failurecontracthasmeasurementadaptermismatch (actualtext/eventsallmatch but physicalstringcolumn0vswriterpendingindent4), review187pending.','steps':[]};h=lambda b:hashlib.sha256(b).hexdigest()
report['unit_before_sha256']=h(before);report['unit_after_sha256']=h(p.read_bytes());report['oracle_sha256']=h(Path('/tmp/emitter-system-if-oracle-r368.json').read_bytes())
def save():(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n')
env=os.environ.copy();env.update(CARGO_BUILD_JOBS='2',CARGO_TARGET_DIR=str(T),TSC_RS_PRINTER_FAILURE_REVIEW_ACTUAL=str(O/'failure-review-actual.json'))
def run(label,args):
 log=O/(label+'.log');start=time.monotonic()
 with log.open('xb') as f:c=subprocess.run(args,cwd=R,env=env,stdout=f,stderr=subprocess.STDOUT).returncode
 b=log.read_bytes();(O/(label+'.log.gz')).write_bytes(gzip.compress(b,mtime=0));row={'label':label,'argv':args,'exit':c,'seconds':time.monotonic()-start,'log_sha256':h(b)};report['steps'].append(row);save();print(json.dumps(row),flush=True);return c
save();codes=[run('fmt',['cargo','fmt','--all'])];assert not codes[-1];base=['taskpolicy','-b','nice','-n','15','cargo','test','--offline','-p','tsc-rs-emitter']
for label,args in [('emitter-lib',['--lib'])]:codes.append(run(label,base+args+['--','--nocapture','--test-threads=1']))
report['qualified']=not any(codes);diff=subprocess.check_output(['git','diff','HEAD'],cwd=R);(O/'private.diff.gz').write_bytes(gzip.compress(diff,mtime=0));report['diff_sha256']=h(diff);save();raise SystemExit(max(codes))
