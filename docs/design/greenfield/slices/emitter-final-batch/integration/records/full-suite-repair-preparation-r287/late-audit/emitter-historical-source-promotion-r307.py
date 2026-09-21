from pathlib import Path
import json,hashlib,subprocess,time
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');D=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target/debug');O=Path('/tmp/emitter-historical-source-promotion-r307');O.mkdir(exist_ok=False)
h=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
cs=json.loads((R/'ratchets/h2-1a-qualification.v1.json').read_text())['cases'];src=(R/'crates/xtask/src/h2_1a_acceptance.rs').read_text();candidates=[c for c in cs if c['disposition']=='deferred-to-slices' and c['required_slices'] in [['H2.8a'],['H2.9']] and json.dumps(c['case_id']) not in src]
items=''.join('    ('+','.join(json.dumps(x) for x in [c['case_id'],c['case_fingerprint_sha256'],c['required_slices'][0]])+'),\n' for c in candidates)
needle='static CURRENT_EXACT_SOURCE_PROMOTIONS: &[(&str, &str, &str)] = &[\n';assert src.count(needle)==1;src=src.replace(needle,needle+items)
src+='''
pub fn probe(workspace: &Path) {
 let artifact: Value = serde_json::from_slice(&fs::read(workspace.join(QUALIFICATION_RELATIVE_PATH)).unwrap()).unwrap();
 let ids: Value = serde_json::from_str(include_str!("candidates.json")).unwrap();
 for id in ids.as_array().unwrap() {
  let case=artifact["cases"].as_array().unwrap().iter().find(|c|c["case_id"]==*id).unwrap();
  println!("START {}",id);
  match execute_observed(workspace,case,DiagnosticExpectation::CurrentExactSourcePromotion) {
   Ok((writes,diagnostics))=>println!("RESULT {}",json!({"case_id":id,"exact":true,"writes":writes,"diagnostics":diagnostics,"repetitions":2})),
   Err(error)=>println!("RESULT {}",json!({"case_id":id,"exact":false,"error":error.to_string()})),
  }
 }
}
'''
(O/'h2_1a_acceptance.rs').write_text(src);(O/'h2_2d_acceptance.rs').write_bytes((R/'crates/xtask/src/h2_2d_acceptance.rs').read_bytes());(O/'candidates.json').write_text(json.dumps([c['case_id'] for c in candidates],indent=2)+'\n');(O/'main.rs').write_text('#![allow(dead_code)]\nmod h2_1a_acceptance;\nmod h2_2d_acceptance;\nfn main(){h2_1a_acceptance::probe(std::path::Path::new("'+str(R)+'"));}\n')
fp=json.loads((D/'.fingerprint/tsc-rs-xtask-b59760cd7fe58a46/test-bin-xtask.json').read_text());names=['base64','serde_json','sha2','tsc_compiler','tsc_diagnostics','tsc_harness','tsc_program'];libs={}
for name in names:
 expected=next(x[3] for x in fp['deps'] if x[1]==name);found=[]
 for p in (D/'.fingerprint').glob('*/lib-'+name):
  if int.from_bytes(bytes.fromhex(p.read_text().strip()),'little')==expected:
   lib=D/'deps'/('lib'+name+'-'+p.parent.name.rsplit('-',1)[1]+'.rlib')
   if lib.is_file():found.append(lib)
 assert len(found)==1,(name,found);p=found[0];libs[name]={'path':str(p),'sha256':h(p),'expected_fingerprint':expected}
args=['rustc','--edition=2021',str(O/'main.rs'),'-L','dependency='+str(D/'deps'),'-o',str(O/'probe')]
for name,v in libs.items():args+=['--extern',name+'='+v['path']]
report={'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip(),'qualified':False,'scope':'scratch copy of original full-observation comparator; only source promotion list extended provisionally for measurement; no repository change','candidates':len(candidates),'libraries':libs,'sources':{str(p):h(p) for p in [R/'crates/xtask/src/h2_1a_acceptance.rs',R/'crates/xtask/src/h2_2d_acceptance.rs',O/'h2_1a_acceptance.rs',O/'h2_2d_acceptance.rs']},'build_argv':args}
with (O/'build.log').open('w') as f:report['build_exit']=subprocess.run(args,stdout=f,stderr=subprocess.STDOUT).returncode
(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n');assert report['build_exit']==0
start=time.monotonic()
with (O/'native.log').open('w') as f:report['native_exit']=subprocess.run(['taskpolicy','-b','nice','-n','15',str(O/'probe')],stdout=f,stderr=subprocess.STDOUT,cwd=R).returncode
report['seconds']=time.monotonic()-start;report['native_sha256']=h(O/'native.log');report['results']=[json.loads(x.removeprefix('RESULT ')) for x in (O/'native.log').read_text().splitlines() if x.startswith('RESULT ')];report['complete']=report['native_exit']==0 and len(report['results'])==len(candidates);(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({k:report[k] for k in ['native_exit','seconds','complete','results']},indent=2))
