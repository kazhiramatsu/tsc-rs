from pathlib import Path
import json,subprocess,hashlib,time
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');O=Path('/tmp/emitter-json-owner-activity-r310');O.mkdir(exist_ok=False);D=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target/debug/deps');s=(R/'crates/xtask/src/h2_3d_acceptance.rs').read_text();s+='''
pub fn probe(workspace: &Path) {
 let artifact: Value=serde_json::from_slice(&fs::read(workspace.join(OWNER_CONTROLS_RELATIVE_PATH)).unwrap()).unwrap();
 for c in artifact["controls"].as_array().unwrap() {
  let id=c["control_id"].as_str().unwrap();
  let result=execute_owner_control(c).map(|(w,d)|json!({"exact":true,"writes":w,"diagnostics":d})).unwrap_or_else(|e|json!({"exact":false,"error":e.to_string()}));
  let mut sink=MemoryOutputSink::new();let (out,_)=ProgramSession::new(owner_input(c).unwrap()).emit_with_reported_diagnostics_for_harness(&mut sink).unwrap();
  let active=H2RuntimeSlice::ALL.into_iter().filter_map(|s|{let n=out.h2_activity().runtime_slice(s);(n>0).then(||(s.name().to_string(),json!(n)))}).collect::<serde_json::Map<_,_>>();
  println!("RESULT {}",json!({"id":id,"comparator":result,"activity":active,"writes":sink.writes().len()}));
 }
}
''';(O/'h2_3d_acceptance.rs').write_text(s);(O/'main.rs').write_text('#![allow(dead_code)]\nmod h2_3d_acceptance;\nfn main(){h2_3d_acceptance::probe(std::path::Path::new("'+str(R)+'"));}\n');libs=json.loads(Path('/tmp/emitter-historical-source-promotion-r307/manifest.json').read_text())['libraries'];args=['rustc','--edition=2021',str(O/'main.rs'),'-L','dependency='+str(D),'-o',str(O/'probe')]
for n,v in libs.items():assert hashlib.sha256(Path(v['path']).read_bytes()).hexdigest()==v['sha256'];args+=['--extern',n+'='+v['path']]
report={'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip(),'scope':'original full owner comparator, all14 frozen controls twice; third read-only emit only to display actual activity','qualified':False,'build_argv':args,'libraries':libs};start=time.monotonic()
with (O/'build.log').open('w') as f:report['build_exit']=subprocess.run(args,stdout=f,stderr=subprocess.STDOUT).returncode
assert report['build_exit']==0
with (O/'native.log').open('w') as f:report['native_exit']=subprocess.run(['taskpolicy','-b','nice','-n','15',str(O/'probe')],cwd=R,stdout=f,stderr=subprocess.STDOUT).returncode
report['seconds']=time.monotonic()-start;report['results']=[json.loads(x.removeprefix('RESULT ')) for x in (O/'native.log').read_text().splitlines() if x.startswith('RESULT ')];report['source_sha256']=hashlib.sha256(s.encode()).hexdigest();(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report['results'],indent=2))
