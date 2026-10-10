//! `printNode` (P5-5a): nodes the client encoded, decoded and printed as
//! tsgo prints a decoded tree. A source file prints its statements with
//! its comments, a node of none prints without source text; a parentless
//! literal prints from its node (its quote, a regular expression closed on
//! request). Pinned to tsgo's answers at 19dadef8 for the same encodings.
use base64::Engine;

use super::*;
use crate::encoder::{encode_node, encode_source_file, ScriptKind, SourceFileFacts};

/// The base64 encoding tsgo's API gives the file `name`.
fn file_data(name: &str, text: &str) -> String {
    let script_kind = ScriptKind::from_file_name(name);
    let file = crate::parse_source_file(name, text, script_kind);
    let facts = SourceFileFacts {
        path: Some(name),
        script_kind,
        ..SourceFileFacts::default()
    };
    base64::engine::general_purpose::STANDARD.encode(encode_source_file(&file, &facts).0)
}

fn print(params: Value) -> Result<String, String> {
    let (session, _) = session(&[]);
    call(&session, "printNode", params).map(|text| text.as_str().unwrap().to_owned())
}

fn print_file(name: &str, text: &str) -> String {
    print(json!({ "data": file_data(name, text) })).unwrap()
}

#[test]
fn a_source_file_prints_its_tree_with_its_comments() {
    assert_eq!(
        print_file(
            "/a.ts",
            "export type Pair = [string, number];\nexport const obj = { m: 1, s: \"hi\", b: true };\n"
        ),
        "export type Pair = [\n    string,\n    number\n];\nexport const obj = { m: 1, s: \"hi\", b: true };\n"
    );
    assert_eq!(
        print_file("/b.ts", "/**\n * A doc.\n */\ndoThing(); // trailing\n"),
        "/**\n * A doc.\n */\ndoThing(); // trailing\n"
    );
    assert_eq!(print_file("/d.json", r#"{"x": 1}"#), "{ \"x\": 1 }\n");
}

#[test]
fn parentless_nodes_print_from_themselves() {
    // `1.` prints as `1` with the access's extra dot; the non-null and
    // type assertion operands take tsgo's parentheses; call and `new`
    // type arguments stay; a single quote and a nested namespace keep
    // their forms.
    assert_eq!(
        print_file(
            "/c.ts",
            "1..toString();\nx!.y;\n<any>(<any>a);\nf<T>(x);\nnew Map<string, number>();\n"
        ),
        "1..toString();\n(x!).y;\n<any>(<any>a);\nf<T>(x);\nnew Map<string, number>();\n"
    );
    assert_eq!(
        print_file("/e.ts", "const s = 'q';\nnamespace A.B { }\n"),
        "const s = 'q';\nnamespace A.B { }\n"
    );
}

#[test]
fn a_node_of_no_source_file_and_the_literal_options() {
    let file = crate::parse_source_file("/r.ts", "let r = /re\n", ScriptKind::Ts);
    let literal = file
        .arena
        .node_ids()
        .find(|&id| file.arena.node(id).kind == tsc_syntax::SyntaxKind::RegularExpressionLiteral)
        .unwrap();
    let data =
        base64::engine::general_purpose::STANDARD.encode(encode_node(&file, literal, &|_| false).0);
    assert_eq!(print(json!({ "data": data })).unwrap(), "/re");
    assert_eq!(
        print(json!({ "data": data, "terminateUnterminatedLiterals": true })).unwrap(),
        "/re/"
    );
}

#[test]
fn bad_data_is_a_client_error() {
    assert_eq!(
        print(json!({ "data": "a!b=" })).unwrap_err(),
        "api: client error: invalid base64 data: illegal base64 data at input byte 1"
    );
    assert_eq!(
        print(json!({ "data": "AAAA" })).unwrap_err(),
        "api: client error: failed to decode AST: data too short for header: 3 bytes"
    );
}
