#![allow(dead_code)]
#[path = "/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep/crates/compiler/tests/integration/h2_7b_w4a_controls.rs"] mod h2_7b_w4a_controls;
#[path = "/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep/crates/compiler/tests/integration/h2_7c_declaration_blocking.rs"] mod h2_7c_declaration_blocking;
fn main() {
 let artifact:serde_json::Value=serde_json::from_slice(&std::fs::read("/tmp/emitter-module-boundary-observe-r408/oracle.json").unwrap()).unwrap();
 
 h2_7c_declaration_blocking::assert_cases_with_inspection(&artifact,true,|id,prepared,_expected|{
  let mut sink=tsc_compiler::MemoryOutputSink::new();
  let result=tsc_compiler::ProgramSession::new(prepared.clone()).emit_command_for_harness(&mut sink);
  println!("case={id} command_ok={}",result.is_ok());
  for artifact in sink.writes() {println!("{}: {}",artifact.path().to_string_lossy(),String::from_utf8_lossy(artifact.callback_bytes()));}
 });
}
