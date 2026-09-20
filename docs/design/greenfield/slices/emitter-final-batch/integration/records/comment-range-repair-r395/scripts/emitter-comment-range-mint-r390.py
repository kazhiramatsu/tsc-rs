from pathlib import Path
import subprocess,json,hashlib,gzip,time
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');B=R/'docs/design/greenfield/slices/emitter-final-batch/integration';D=Path('/tmp/emitter-comment-range-controls-r389');P=Path('/tmp/emitter-function-range-repair-r388');O=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target/emitter-comment-range-mint-r390');O.mkdir(exist_ok=False)
h=lambda b:hashlib.sha256(b).hexdigest();report={'source_head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip(),'oracle_qualified':False,'native_qualified':False,'scope':'Official TypeScript fixture extension plus reviewed comment endpoint repair; native comparisons pending.','steps':[]}
def save():(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n')
assert report['source_head']=='70696be105e0c1f8009e2ac0a57236ace99c798d'
assert not subprocess.check_output(['git','diff','HEAD','--name-only'],cwd=R).strip()
assert not json.loads((Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target/emitter-canonical-focused-r349/manifest.json')).read_bytes())['qualified']
assert json.loads(Path('/tmp/emitter-claude-review-round189-opus.json').read_bytes())['is_error'] is False
report['before']={};save()
for path,src in [('crates/emitter/src/printer.rs',P/'printer.rs'),('crates/emitter/tests/integration/active_transform_contract.rs',P/'active_transform_contract.rs'),*[(p,D/p) for p in json.loads((D/'manifest.json').read_bytes())['files']]]:
 raw=(R/path).read_bytes();report['before'][path]=h(raw);copy=O/'before'/path;copy.parent.mkdir(parents=True,exist_ok=True);copy.write_bytes(raw);(R/path).write_bytes(src.read_bytes())
for name,script,fixture,expected,oldcount in [
 ('body-endpoint-flags','scripts/observe-list-comment-flags.mjs','crates/emitter/tests/fixtures/list-comment-flags.json',244,172),
 ('body-endpoint-commands','scripts/observe-emitter-context-recovery.mjs','crates/compiler/tests/fixtures/emitter-context-recovery.json',788,748)]:
 raw=(R/fixture).read_bytes();old=json.loads(raw);(O/(name+'-before.json.gz')).write_bytes(gzip.compress(raw,mtime=0));assert len(old['cases'])==oldcount
 if name=='body-endpoint-commands':(R/fixture).unlink()
 start=time.monotonic()
 try:
  code=subprocess.run(['python3',str(B/'run-local.py'),name+'-r390','node',script,'--write'],cwd=R).returncode
  assert code==0,(name,code)
  fresh=json.loads((R/fixture).read_bytes());assert len(fresh['cases'])==expected
  assert fresh['cases'][:oldcount]==old['cases']
  assert len({x['case_id'] for x in fresh['cases']})==expected
  if 'refused_cases' in old:assert fresh['refused_cases']==old['refused_cases'] and len(fresh['refused_cases'])==72
  assert {k:v for k,v in old.items() if k not in ['observer_sha256','cases']}=={k:v for k,v in fresh.items() if k not in ['observer_sha256','cases']}
 except BaseException:
  (R/fixture).write_bytes(raw);raise
 report['steps'].append({'label':name,'exit':code,'seconds':time.monotonic()-start,'old_cases_preserved':oldcount,'new_total':expected,'fixture_sha256':h((R/fixture).read_bytes())});save()
for path,before,after in [('crates/compiler/tests/integration/h2_8a_import_helpers.rs','assert_eq!(cases.len(), 748);','assert_eq!(cases.len(), 788);'),('crates/syntax/tests/emitter_recovery.rs','("cases", true, 748)','("cases", true, 788)')]:
 p=R/path;s=p.read_text();assert s.count(before)==1;p.write_text(s.replace(before,after))
p=R/'crates/compiler/tests/fixtures/emitter-r77-regressions.json';old=json.loads(p.read_bytes());new=json.loads(p.read_bytes());new['fixtures']['emitter-context-recovery.json']=h((R/'crates/compiler/tests/fixtures/emitter-context-recovery.json').read_bytes());assert old['cases']==new['cases'];p.write_text(json.dumps(new,indent=2,ensure_ascii=False)+'\n')
report['oracle_qualified']=True;save();print(json.dumps(report))
