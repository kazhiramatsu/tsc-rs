//! `printNode` (P5-5a, P5-5c): nodes the client encoded, decoded and printed
//! as tsgo prints a decoded tree. A source file prints its statements with
//! its comments, a node of none prints without source text or a current
//! source file; a parentless literal prints from its node (its quote, a
//! regular expression closed on request); parsed shapes take tsgo's
//! print-time parentheses. Pinned to tsgo's answers at 19dadef8 for the
//! same encodings.
use base64::Engine;

use super::*;
use crate::encoder::{encode_node, encode_source_file, ScriptKind, SourceFileFacts};
use tsc_syntax::NodeData;

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

/// Each statement of the file `name` encoded alone, as the client encodes a
/// node of a source file, and printed.
fn print_statements(name: &str, text: &str) -> Vec<String> {
    let file = crate::parse_source_file(name, text, ScriptKind::from_file_name(name));
    let NodeData::SourceFile(root) = &file.arena.node(file.root).data else {
        unreachable!("a parsed file's root is its source file")
    };
    let statements = file
        .arena
        .node_array(root.statements.unwrap())
        .nodes
        .to_vec();
    statements
        .into_iter()
        .map(|statement| {
            let data = base64::engine::general_purpose::STANDARD
                .encode(encode_node(&file, statement, &|_| false).0);
            print(json!({ "data": data })).unwrap()
        })
        .collect()
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
fn a_node_of_no_source_file_prints_without_one() {
    // A statement encoded alone has no current source file: an empty
    // function body takes a line and an object literal drops its trailing
    // comma (the file keeps both forms); an arrow keeps `<T,>`.
    let text =
        "function f() {}\nconst o = { a: 1, };\nconst g = <T,>(x: T) => x;\nlet n = [1].map(x => x);\n";
    assert_eq!(
        print_file("/s.ts", text),
        "function f() { }\nconst o = { a: 1, };\nconst g = <T,>(x: T) => x;\nlet n = [1].map(x => x);\n"
    );
    assert_eq!(
        print_statements("/s.ts", text),
        [
            "function f() {\n}",
            "const o = { a: 1 };",
            "const g = <T,>(x: T) => x;",
            "let n = [1].map(x => x);"
        ]
    );
}

#[test]
fn parsed_operands_take_tsgo_parentheses() {
    // A decoded tree keeps TypeScript operands, heritage expressions and
    // decorator expressions as parsed; tsgo parenthesizes them by precedence
    // while printing (`@new x` and `@x?.y` need none).
    let text = "export const x01 = 1 as number * 2;\nexport const x21 = 1 + 1 as number >> 2;\n<number>temp ** 3;\nx = y satisfies T || z;\n";
    assert_eq!(
        print_file("/p.ts", text),
        "export const x01 = (1 as number) * 2;\nexport const x21 = (1 + 1 as number) >> 2;\n(<number>temp) ** 3;\nx = y satisfies T || z;\n"
    );
    let text = "class C1 extends A?.B {}\na?.b<c>.d;\n@new x class C {}\n@x?.y class D {}\n";
    assert_eq!(
        print_file("/h.ts", text),
        "class C1 extends (A?.B) {\n}\n(a?.b)<c>.d;\n@new x\nclass C {\n}\n@x?.y\nclass D {\n}\n"
    );
    assert_eq!(
        print_statements("/h.ts", text),
        [
            "class C1 extends (A?.B) {\n}",
            "(a?.b)<c>.d;",
            "@new x\nclass C {\n}",
            "@x?.y\nclass D {\n}"
        ]
    );
}

#[test]
fn type_syntax_prints_as_tsgo_prints_it() {
    // A parsed `typeof A[]` keeps its form; an interface's heritage clauses
    // are separated by the list's space besides their own; a mapped type's
    // error-recovery members print inside its braces; a JSDoc `?` takes its
    // operand at non-array precedence; an object literal method keeps its
    // postfix token.
    let text = "var v: typeof A[];\ninterface I extends A extends B {}\ntype After = {\n    [p in P]: void;\n    model: 'hour' | 'day'\n}\nexport type T = [first: string, rest: ...string[]?];\nconst o = { m?() { return 12 }, n!() { return 13 } };\n";
    let expected = [
        "var v: typeof A[];",
        "interface I extends A  extends B {\n}",
        "type After = {\n    [p in P]: void;\n    model: 'hour' | 'day';\n};",
        "export type T = [\n    first: string,\n    rest: ...?(string[])\n];",
        "const o = { m?() { return 12; }, n!() { return 13; } };",
    ];
    assert_eq!(print_file("/t.ts", text), expected.join("\n") + "\n");
    assert_eq!(print_statements("/t.ts", text), expected);
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
