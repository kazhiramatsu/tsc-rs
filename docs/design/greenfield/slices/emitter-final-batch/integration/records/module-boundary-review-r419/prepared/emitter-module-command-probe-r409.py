from pathlib import Path
import hashlib,json,os,subprocess
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');D=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target/debug/deps');O=Path('/tmp/emitter-import-command-probe-r409');O.mkdir(exist_ok=False)
text=f'''#![allow(dead_code)]
#[path = "{R}/crates/compiler/tests/integration/h2_7b_w4a_controls.rs"] mod h2_7b_w4a_controls;
#[path = "{R}/crates/compiler/tests/integration/h2_7c_declaration_blocking.rs"] mod h2_7c_declaration_blocking;
fn main() {{
 let artifact:serde_json::Value=serde_json::from_slice(&std::fs::read("/tmp/emitter-module-boundary-observe-r408/oracle.json").unwrap()).unwrap();
 
 h2_7c_declaration_blocking::assert_cases_with_inspection(&artifact,true,|id,prepared,_expected|{{
  let mut sink=tsc_compiler::MemoryOutputSink::new();
  let result=tsc_compiler::ProgramSession::new(prepared.clone()).emit_command_for_harness(&mut sink);
  println!("case={{id}} command_ok={{}}",result.is_ok());
  for artifact in sink.writes() {{println!("{{}}: {{}}",artifact.path().to_string_lossy(),String::from_utf8_lossy(artifact.callback_bytes()));}}
 }});
}}
''';(O/'main.rs').write_text(text)
args=['rustc','--edition=2021',str(O/'main.rs'),'-L','dependency='+str(D),'-o',str(O/'probe')];libs={}
for name in ['tsc_emitter','tsc_syntax','tsc_compiler','tsc_program','tsc_host','tsc_harness','tsc_diagnostics','serde_json','base64']:
 p=max(D.glob('lib'+name+'-*.rlib'),key=lambda p:p.stat().st_mtime_ns);libs[name]={'path':str(p),'sha256':hashlib.sha256(p.read_bytes()).hexdigest()};args+=['--extern',name+'='+str(p)]
env=os.environ.copy();env['CARGO_MANIFEST_DIR']=str(R/'crates/compiler');prefix=['taskpolicy','-b','nice','-n','15'];report={'scope':'Supplemental same-library diagnostic probe; full existing command comparator is retained and expected to fail. No source changes. Not a replacement for canonical Cargo tests.','head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip(),'libraries':libs,'argv':args}
with (O/'build.log').open('w') as f:report['build_exit']=subprocess.run(prefix+args,cwd=R,env=env,stdout=f,stderr=subprocess.STDOUT).returncode
if report['build_exit']==0:
 with (O/'native.log').open('w') as f:report['native_exit']=subprocess.run(prefix+[str(O/'probe')],cwd=R,env=env,stdout=f,stderr=subprocess.STDOUT).returncode
(O/'manifest.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({k:v for k,v in report.items() if k!='libraries' and k!='argv'}))
