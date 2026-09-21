"""Layer the reviewed context-only extension onto immutable r185 evidence."""
from pathlib import Path
import argparse,copy,gzip,hashlib,importlib.util,json,os,re,subprocess,time,tomllib
HERE=Path(__file__).resolve().parent
PINS=json.loads((HERE/'pins.json').read_bytes())
ROOT=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-leading-binding-prototype')
PROOF=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-escaped-keyword-proof')
TARGET=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target')
CENSUS=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-census')
SNAPSHOT=TARGET/'emitter-recovery-census-r78/parse-snapshot.json'
PREVIOUS=TARGET/'emitter-keyword-successor-r185'
KEYS={'literal','missing_await','missing_declaration','parameter_gaps','statement_gaps','context_recovery'}
def sha(data):return hashlib.sha256(data).hexdigest()
def git(root,*args):return subprocess.check_output(['git',*args],cwd=root)
def load(path):return json.loads(Path(path).read_bytes())
def module(name,path):
 spec=importlib.util.spec_from_file_location(name,path);m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m);return m

def compare_inputs(before,after,snapshot):
 ids=set(snapshot['inputs']);assert len(ids)==16994
 assert set(snapshot['digests'])==set(before['digests'])==set(after['digests'])==ids
 assert before['input_artifact_sha256']==after['input_artifact_sha256']==PINS['snapshot_sha256']
 assert before['digest_code_sha256']==after['digest_code_sha256']==snapshot['digest_code_sha256']
 changed=[]
 for key in ids:
  old=before['digests'][key];new=after['digests'][key]
  assert old['core']==new['core']==snapshot['digests'][key]['core'],('core',key)
  for row in [old,new]:
   assert isinstance(row['profiles'],dict) and set(row['profiles'])==KEYS,('profiles',key)
   assert all(type(v) is bool for v in row['profiles'].values()),('profile types',key)
  for profile in KEYS-{'context_recovery'}:
   assert old['profiles'][profile]==new['profiles'][profile],('predecessor',key,profile)
  assert not old['profiles']['context_recovery'] or new['profiles']['context_recovery'],('lost admission',key)
  if old['profiles']['context_recovery']!=new['profiles']['context_recovery']:changed.append(key)
 for name in ['recovery_facts_sha256','legacy_recovery_facts_sha256']:
  for artifact in [before,after]:
   assert set(artifact[name])==ids
   assert all(isinstance(h,str) and re.fullmatch('[0-9a-f]{64}',h) for h in artifact[name].values())
  assert before[name]==after[name],name
 # This equality also covers every causal map, schema field, and future field.
 # Only the explicitly checked context predicate and build metadata may differ.
 normalized={k:v for k,v in after.items() if k!='build'}
 normalized['digests']={key:{**after['digests'][key],'profiles':{**after['digests'][key]['profiles'],'context_recovery':before['digests'][key]['profiles']['context_recovery']}} for key in ids}
 assert normalized=={k:v for k,v in before.items() if k!='build'},'non-context replay field changed'
 return sorted(changed)

def source_preflight():
 assert git(ROOT,'rev-parse','HEAD').decode().strip()==PINS['prototype_head']
 assert git(ROOT,'rev-parse','HEAD^').decode().strip()==PINS['canonical_base']
 assert not git(ROOT,'diff','HEAD','--name-only').strip()
 assert not git(ROOT,'status','--porcelain','--','crates').strip()
 assert git(PROOF,'rev-parse','HEAD').decode().strip()==PINS['proof_head']
 assert not git(PROOF,'diff','HEAD','--name-only').strip()
 changed=set(git(ROOT,'diff','--name-only',PINS['canonical_base'],'HEAD').decode().splitlines())
 assert changed==set(PINS['file_pairs']),changed
 for path,(old,new) in PINS['file_pairs'].items():
  assert sha(git(ROOT,'show',PINS['canonical_base']+':'+path))==old
  assert sha((ROOT/path).read_bytes())==new
 for path,h in PINS['evidence_sha256'].items():assert sha(Path(path).read_bytes())==h,path
 assert sha((ROOT/'Cargo.lock').read_bytes())==PINS['canonical_Cargo_lock_sha256']
 assert (ROOT/'Cargo.lock').read_bytes()==(CENSUS/'Cargo.lock').read_bytes()
 inspection=Path('/tmp/emitter-leading-binding-proof-inspection-r298/scope.json')
 assert sha(inspection.read_bytes())==PINS['scope_inspection_sha256']
 replay=module('frozen_replay',PROOF/'scripts/replay-recovery-parse.py')
 identities=replay.source_identity(ROOT)
 older=load(PREVIOUS/'successor.json')['build']['source_files_sha256']
 deltas={p for p in older.keys()|identities.keys() if older.get(p)!=identities.get(p)}
 expected={r['path']:(r['before'],r['after']) for r in load(inspection)['previous_185_source_deltas']}
 assert deltas==set(expected)
 for p,(old,new) in expected.items():assert (older[p],identities[p])==(old,new)
 assert {p for p in deltas if '/src/' in p}=={'crates/syntax/src/recovery.rs','crates/syntax/src/recovery/context.rs'}
 return identities

def original_selection(snapshot,previous):
 selector=module('frozen_selector',PROOF/'scripts/select-recovery-parse-corpus.py')
 old_replays=TARGET/'emitter-recovery-parser-replays-r131'
 reports={name:load(TARGET/'emitter-recovery-census-r78/profiles'/(name+'.json')) for name in selector.PROFILES}
 replay=selector.select(snapshot,PINS['snapshot_sha256'],load(old_replays/'current.json'),{k:load(old_replays/(k+'.json')) for k in ['projection','merge-base']},reports,previous)
 old=load(PREVIOUS/'selection.json')
 groups=selector.compare_previous_successor(load(TARGET/'emitter-variable-successor-r151/successor.json'),previous)
 # The frozen selector's main adds these counts after select() returns.
 replay['summary']['keyword_extension_changed_inputs']={key:len(value) for key,value in groups.items()}
 for key,value in replay.items():assert old[key]==value,('previous selection drift',key)
 assert groups==old['keyword_extension']
 assert len(old['cases'])==48 and len(old['load_failures'])==110
 return selector,old

def select_extension(snapshot,prior,fresh,new_inputs,selector):
 originals={r['case_id']:r for r in snapshot['rows']};old={r['case_id']:r for r in prior['cases']}
 assert len(originals)==len(snapshot['rows'])==14219 and len(old)==len(prior['cases'])==48
 new_set=set(new_inputs);mapping={k:[] for k in new_inputs};cases=[];documents={}
 def collect(value):
  if isinstance(value,dict):
   for name,child in value.items():
    if name=='content_sha256' and child is not None:documents[child]=snapshot['documents'][child]
    else:collect(child)
  elif isinstance(value,list):
   for child in value:collect(child)
 for id,row in originals.items():
  units=[unit for unit in row['units'] if unit['input_id'] in new_set]
  if id not in old and not units:continue
  reasons=copy.deepcopy(old[id]['reasons']) if id in old else []
  if id in old:assert {k:v for k,v in old[id].items() if k!='reasons'}==row
  for unit in units:
   mapping[unit['input_id']].append({'case_id':id,'path':unit['path'],'role':unit['role']})
   reasons.append({'context_extension':'leading-binding-composite','successor':fresh['build']['parser_head'],'input_id':unit['input_id'],'path':unit['path'],'role':unit['role']})
  if row['loader'] in ['load_compiler_no_emit','load_project_no_emit']:
   assert row['emit_load_error'] and row['emit_disposition']=='parse-admission-only; emit-not-qualified'
  else:assert row['emit_load_error'] is None and row['emit_disposition']=='pending-complete-command-comparison'
  assert row['command_input'] is not None
  selector.validate_input_numbers(row['command_input']);collect(row['command_input']);cases.append({**row,'reasons':reasons})
 assert all(mapping.values()),('unmapped new inputs',[k for k,v in mapping.items() if not v])
 selected={r['case_id'] for r in cases};assert set(old)<=selected and len(cases)==len(selected)
 for key,value in prior['documents'].items():assert documents[key]==value
 assert prior['load_failures']==snapshot['load_failures'] and len(snapshot['load_failures'])==110
 result={**prior,'cases':cases,'documents':documents,'summary':{**prior['summary'],'selected_rows':len(cases),'unchanged_rows':14219-len(cases),'selected_no_emit_fallbacks':sum(r['loader'] in ['load_compiler_no_emit','load_project_no_emit'] for r in cases),'leading_binding_new_inputs':len(new_inputs),'leading_binding_new_rows':len(selected-set(old))}}
 result['successor']={k:fresh['build'][k] for k in ['parser_head','syntax_tree_hash','source_files_sha256','binary_sha256','predicate_diff_sha256']}
 result['leading_binding_extension']={'new_inputs':new_inputs,'input_rows':mapping,'prior_selection_sha256':sha((PREVIOUS/'selection.json').read_bytes()),'retained_case_ids':sorted(old),'qualified_commands':False,'limitation':'110 original load failures remain unqualified; nonselected rows have parse-level identity, not reexecuted command-output qualification'}
 result['evidence']={**prior['evidence'],'leading_binding_wrapper_sha256':sha(Path(__file__).read_bytes()),'leading_binding_pins_sha256':sha((HERE/'pins.json').read_bytes())}
 return result

def main():
 p=argparse.ArgumentParser();p.add_argument('--check-preflight',action='store_true');p.add_argument('--out',type=Path);a=p.parse_args()
 identities=source_preflight();raw=SNAPSHOT.read_bytes();assert sha(raw)==PINS['snapshot_sha256'];snapshot=json.loads(raw);previous=load(PREVIOUS/'successor.json')
 selector,old_selection=original_selection(snapshot,previous)
 assert compare_inputs(previous,previous,snapshot)==[]
 if a.check_preflight:
  print(json.dumps({'preflight':'PASS','source_head':PINS['prototype_head'],'inputs':16994,'old_selection_reproduced':48,'load_failures':110,'runtime_replay':'NOT RUN'}));return
 assert a.out is not None
 r244=load(TARGET/'emitter-prewalk-r244/manifest.json');assert len(r244['steps'])==7,'finish canonical r244 before this build'
 out=a.out.resolve();out.mkdir(exist_ok=False);build=out/'build';build.mkdir()
 probe=PROOF/'scripts/replay-recovery-parse.rs'
 manifest=['[package]','name = "leading-binding-parse-replay"','version = "0.0.0"','edition = "2021"','','[workspace]','','[[bin]]','name = "leading-binding-parse-replay"','path = '+json.dumps(str(probe)),'','[dependencies]','base64 = "0.22"','serde_json = { version = "1.0", features = ["preserve_order"] }','sha2 = "0.10"']
 for name in ['syntax','types','diagnostics']:manifest.append(f'tsc-{name} = {{ package = "tsc-rs-{name}", path = {json.dumps(str(ROOT/"crates"/name))} }}')
 manifest+=['','[features]','current-recovery-profiles = []','','[profile.dev]','opt-level = 3','debug = 0','incremental = false','']
 (build/'Cargo.toml').write_text('\n'.join(manifest));(build/'Cargo.lock').write_bytes((CENSUS/'Cargo.lock').read_bytes())
 env=os.environ.copy();env['CARGO_BUILD_JOBS']='2';env['CARGO_TARGET_DIR']=str(build/'target');prefix=['taskpolicy','-b','nice','-n','15'];started=time.monotonic();steps=[]
 for action in ['build','test']:
  args=prefix+['cargo',action,'--offline','--manifest-path',str(build/'Cargo.toml'),'--features','current-recovery-profiles'];log=out/(action+'.log')
  with log.open('xb') as f:code=subprocess.run(args,cwd=PROOF,env=env,stdout=f,stderr=subprocess.STDOUT).returncode
  steps.append({'argv':args,'exit':code,'log_sha256':sha(log.read_bytes())});(out/'steps.json').write_text(json.dumps(steps,indent=2)+'\n');assert code==0,(action,code)
 packages=tomllib.loads((build/'Cargo.lock').read_text())['package'];registry=[p for p in packages if p.get('source')]
 def tuples(rows):return {(p['name'],p['version'],p.get('source'),p.get('checksum')) for p in rows}
 assert tuples(registry)==tuples(load(PREVIOUS/'resolved-dependencies.json'))
 (out/'resolved-dependencies.json').write_text(json.dumps(registry,indent=2)+'\n')
 binary=build/'target/debug/leading-binding-parse-replay'
 with (out/'raw-replay.json').open('xb') as f,(out/'replay.stderr.log').open('xb') as err:
  run=subprocess.run(prefix+[str(binary)],input=raw,stdout=f,stderr=err,check=True)
 fresh=load(out/'raw-replay.json');changed=compare_inputs(previous,fresh,snapshot)
 assert source_preflight()==identities
 probe_paths=[PROOF/p for p in ['scripts/replay-recovery-parse.rs','crates/xtask/src/recovery_parse_snapshot.rs','scripts/replay-recovery-parse.py','scripts/select-recovery-parse-corpus.py']]
 probe_hashes={str(p):sha(p.read_bytes()) for p in probe_paths};assert probe_hashes==previous['build']['probe_files_sha256']
 fresh['build']={'label':'leading-binding-context-extension-r299','baseline_kind':'successor','parser_head':PINS['prototype_head'],'parser_diff_sha256':sha(git(ROOT,'diff','HEAD','--','crates/syntax','crates/types','crates/diagnostics')),'source_files_sha256':identities,'syntax_tree_hash':git(ROOT,'rev-parse','HEAD:crates/syntax').decode().strip(),'predicate_diff_sha256':sha(git(ROOT,'diff',PINS['canonical_base'],'HEAD','--',*PINS['file_pairs'])),'probe_files_sha256':probe_hashes,'binary_sha256':sha(binary.read_bytes()),'manifest_sha256':sha((build/'Cargo.toml').read_bytes()),'lock_sha256':sha((build/'Cargo.lock').read_bytes()),'original_lock_sha256':PINS['canonical_Cargo_lock_sha256'],'wrapper_sha256':sha(Path(__file__).read_bytes()),'rustc':subprocess.check_output(['rustc','--version','--verbose'],text=True),'seconds':round(time.monotonic()-started,3)}
 (out/'successor.json').write_text(json.dumps(fresh,separators=(',',':'))+'\n')
 selected=select_extension(snapshot,old_selection,fresh,changed,selector);selected['evidence']['leading_binding_successor_sha256']=sha((out/'successor.json').read_bytes())
 (out/'selection.json').write_text(json.dumps(selected,ensure_ascii=False,separators=(',',':'))+'\n')
 report={'head':PINS['prototype_head'],'parser_qualified':True,'commands_qualified':False,'input_count':16994,'source_files_sha256':identities,'new_inputs':changed,'selection_summary':selected['summary'],'snapshot_sha256':PINS['snapshot_sha256'],'previous_successor_sha256':sha((PREVIOUS/'successor.json').read_bytes()),'previous_selection_sha256':sha((PREVIOUS/'selection.json').read_bytes()),'successor_sha256':sha((out/'successor.json').read_bytes()),'selection_sha256':sha((out/'selection.json').read_bytes()),'limits':['The eight export-name fixture commands are absent from the original input snapshot and need independent complete-command qualification.','No command output is qualified by this parse-only replay; all selected commands must be observed twice and compared completely.','All110 original load failures remain explicit and unqualified.']}
 (out/'manifest.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({'parser_qualified':True,'commands_qualified':False,'new_inputs':len(changed),'selection':selected['summary']}))
if __name__=='__main__':main()
