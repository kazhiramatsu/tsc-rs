from pathlib import Path
import hashlib,json,os,subprocess,time,tomllib
root=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');probe=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-escaped-keyword-proof');census=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-census');target=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target');base=root/'docs/design/greenfield/slices/emitter-final-batch/integration'
head=Path('/tmp/emitter-corpus-controls-r184-head').read_text().strip();probe_head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=probe,text=True).strip();assert probe_head == Path('/tmp/emitter-keyword-successor-r185-proof-head').read_text().strip()
def check():
 for tree,want in [(root,head),(probe,probe_head)]:
  assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=tree,text=True).strip()==want
  assert not subprocess.check_output(['git','diff','HEAD','--name-only'],cwd=tree).strip()
print('Waiting for r184; both Repair and KeywordProof remain frozen through this proof.',flush=True)
while not (base/'records/local/corpus-controls-pipeline-r184.json').exists():time.sleep(10)
prior=json.loads((target/'emitter-corpus-controls-r184/manifest.json').read_text());assert prior['head']==head
assert next(step['exit'] for step in prior['steps'] if step['label']=='corpus-syntax-r184')==0
check();out=target/'emitter-keyword-successor-r185';out.mkdir(exist_ok=False);manifest={'head':head,'proof_head':probe_head,'parser_qualified':False,'commands_qualified':False,'prior_control_status':prior['qualified'],'steps':[]}
def save():(out/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
def run(label,args):
 check();code=subprocess.run(['python3',str(base/'run-local.py'),label,*map(str,args)],cwd=root).returncode;check();manifest['steps'].append({'label':label,'exit':code});save()
 if code:raise SystemExit(code)
snapshot=target/'emitter-recovery-census-r78/parse-snapshot.json';assert hashlib.sha256(snapshot.read_bytes()).hexdigest()=='1846fce26da956f515874486d43b9ac856fda31456e76a2925455a8488f76d75'
run('keyword-selector-guards-r185',['python3',probe/'scripts/test-select-recovery-parse-corpus.py'])
run('keyword-successor-replay-r185',['python3',probe/'scripts/replay-recovery-parse.py','--parser-tree',root,'--input',snapshot,'--out',out/'successor.json','--build-dir',out/'build-successor','--label','r185-successor','--baseline-kind','successor','--with-recovery-profiles'])
original=target/'emitter-recovery-parser-replays-r131';previous=target/'emitter-variable-successor-r151'
run('keyword-corpus-selection-r185',['python3',probe/'scripts/select-recovery-parse-corpus.py','--snapshot',snapshot,'--current',original/'current.json','--projection',original/'projection.json','--merge-base',original/'merge-base.json','--profiles-dir',target/'emitter-recovery-census-r78/profiles','--successor',out/'successor.json','--previous-successor',previous/'successor.json','--out',out/'selection.json'])
old=json.loads((previous/'selection.json').read_text());selected=json.loads((out/'selection.json').read_text());assert {c['case_id'] for c in old['cases']} <= {c['case_id'] for c in selected['cases']}
r177=json.loads((target/'emitter-keyword-successor-r177/successor.json').read_text());fresh=json.loads((out/'successor.json').read_text())
assert r177['recovery_facts_sha256']==fresh['recovery_facts_sha256'], 'C must not change any committed recovery fact'
assert set(r177['digests'])==set(fresh['digests']);new_context=[]
for key,old in r177['digests'].items():
 new=fresh['digests'][key];assert old['core']==new['core']
 for profile,value in old['profiles'].items():
  if profile!='context_recovery':assert value==new['profiles'][profile],(key,profile)
  else:assert not value or new['profiles'][profile],key
 if old['profiles']!=new['profiles']:
  assert fresh['class_member_body_gaps'][key],key;new_context.append(key)
old48=json.loads((target/'emitter-keyword-successor-r177/selection.json').read_text());assert {c['case_id'] for c in old48['cases']} <= {c['case_id'] for c in selected['cases']}
manifest['r177_raw_recovery_facts_unchanged']=True;manifest['r177_new_class_context_inputs']=new_context
expected={(p['name'],p['version'],p.get('source'),p.get('checksum')) for p in tomllib.loads((census/'Cargo.lock').read_text())['package']};lock=tomllib.loads((out/'build-successor/Cargo.lock').read_text())['package'];assert all((p['name'],p['version'],p.get('source'),p.get('checksum')) in expected for p in lock if p.get('source'))
(out/'resolved-dependencies.json').write_text(json.dumps([p for p in lock if p.get('source')],indent=2)+'\n');manifest['parser_qualified']=True;manifest['selection_summary']=selected['summary'];save()
run('keyword-selected-oracle-r185',['node','scripts/observe-recovery-selected-corpus.mjs',out/'selection.json',out/'oracle.json',census])
print(json.dumps({'parser_qualified':True,'complete_command_qualification':False,'summary':selected['summary']}),flush=True)
