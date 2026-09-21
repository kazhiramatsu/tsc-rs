use serde_json::{json, Value};
use tsc_syntax::{NodeData, ParseOptions};
use tsc_types::{NodeFlags, ScriptTarget};
fn main() {
    let fixture: Value = serde_json::from_reader(std::io::stdin()).unwrap();
    let cases: Vec<_> = fixture["cases"].as_array().unwrap().iter().map(|case| {
        let text = case["text"].as_str().unwrap();
        let source = tsc_syntax::parse_source_file("main.ts", text, ParseOptions {script_target: ScriptTarget::ES_NEXT, ..ParseOptions::default()}, None);
        let diagnostics: Vec<_> = source.parse_diagnostics.iter().map(|d| json!({"code": d.code(), "start": d.start, "length": d.length, "message": d.message.text.to_string_lossy()})).collect();
        let NodeData::SourceFile(data) = &source.arena.node(source.root).data else { unreachable!() };
        let flags: Vec<_> = source.arena.node_array(data.statements.unwrap()).nodes.iter().map(|id| NodeFlags::from_bits(source.arena.node(*id).flags).contains(NodeFlags::AWAIT_CONTEXT)).collect();
        let expected: Vec<_> = case["statements"].as_array().unwrap().iter().map(|s| s["await_context"].as_bool().unwrap()).collect();
        json!({"text": text, "exact": diagnostics == *case["diagnostics"].as_array().unwrap() && flags == expected, "diagnostics": diagnostics, "await_context": flags, "expected_diagnostics":case["diagnostics"], "expected_await_context":expected})
    }).collect();
    println!("{}", serde_json::to_string_pretty(&json!({"cases":cases})).unwrap());
}
