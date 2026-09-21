#![allow(dead_code,unused_imports)]
#[path = "/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep/crates/program/tests/support/scalar_json.rs"] mod utf16_scalar_json;
#[path = "/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep/crates/compiler/tests/integration/h2_7b_w4a_controls.rs"] mod h2_7b_w4a_controls;
#[path = "/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep/crates/compiler/tests/integration/h2_7c_declaration_blocking.rs"] mod h2_7c_declaration_blocking;
mod capture;
fn main(){let artifact:serde_json::Value=serde_json::from_str(include_str!("cases.json")).unwrap();let mut failures=Vec::new();for case in artifact["cases"].as_array().unwrap(){let id=case["case_id"].as_str().unwrap();let result=std::panic::catch_unwind(||h2_7c_declaration_blocking::assert_cases_with_inspection(&serde_json::json!({"cases":[case]}),true,capture::capture_complete_command));if result.is_ok(){println!("EXACT x2 {id}");}else{failures.push(id);}}assert!(failures.is_empty(),"{failures:?}");}
