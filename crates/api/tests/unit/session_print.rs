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
fn comments_around_typescript_syntax() {
    // A namespace's braces carry the comments on the line of `{` and before
    // `}`; a member without `;` writes its comment after the `;` it gets; a
    // type's trailing comments follow it (annotations, an index signature,
    // a conditional type's parts, an arrow's return type), as do a
    // variable name's before its type; an enum's `{` keeps no comment on
    // its line.
    assert_eq!(
        print_file(
            "/m.ts",
            "namespace M { // c1\n    var x;\n    // c2\n}\nnamespace N {\n    // only\n}\n"
        ),
        "namespace M { // c1\n    var x;\n    // c2\n}\nnamespace N {\n    // only\n}\n"
    );
    assert_eq!(
        print_file(
            "/i.ts",
            "interface I {\n    a: string; // one\n    b: number // two\n}\ninterface J {\n    [k: string]: number /* c8 */;\n}\nclass K {\n    p: number // c9\n}\n"
        ),
        "interface I {\n    a: string; // one\n    b: number; // two\n}\ninterface J {\n    [k: string]: number /* c8 */;\n}\nclass K {\n    p: number; // c9\n}\n"
    );
    assert_eq!(
        print_file(
            "/c.ts",
            "type X<P> = P extends any // c3\n    ? A\n    : B;\nlet e /*c6*/: any = {};\nvar f = (a: number): number /* c7 */ => a;\n"
        ),
        "type X<P> = P extends any // c3\n ? A : B;\nlet e /*c6*/: any = {};\nvar f = (a: number): number /* c7 */ => a;\n"
    );
    assert_eq!(
        print_file("/e.ts", "enum E { // c4\n    A = 1,\n    B // c5\n}\n"),
        "enum E {\n    A = 1,\n    B // c5\n}\n"
    );
}

#[test]
fn comments_of_templates_and_recovered_statements() {
    // A tagged template's template keeps the comment before it; a file's
    // detached comment is found at the statement list's start, before a
    // token the parser skipped; a missing declaration prints nothing, not
    // even its comments.
    assert_eq!(
        print_file("/t.ts", "tag\n// c10\n`x`;\n"),
        "tag \n// c10\n`x`;\n"
    );
    assert_eq!(
        print_file("/h.ts", "// header\n\nimport 10;"),
        "// header\n10;\n"
    );
    assert_eq!(print_file("/d.ts", "// c11\n@dec\n"), "");
}

#[test]
fn preserve_source_newlines_prints_as_tsgo() {
    // P5-5b, tsgo's answers at aa814927. A decoded tree has no parents, so
    // tsgo writes no line before a list's first item (its leading count
    // reads the next token) nor between items (siblings without a parent
    // are not comparable); the closing lines are the source's. An item after
    // no line writes the comments at its position first, so tsgo writes some
    // comments twice; a `//` comment always ends its line.
    let preserve = |text: &str| {
        print(json!({ "data": file_data("/p.ts", text), "preserveSourceNewlines": true })).unwrap()
    };
    for (text, expected) in [
        ("a;\nb;\n", "a;b;\n"),
        ("a;\n\nb;", "a;b;"),
        ("// c\na;", "// c\n// c\na;"),
        ("a; // x", "a; // x\n"),
        (
            "class C {\n  a(); // x\n  b();\n}",
            "class C {a(); // x\n    // x\n    b();\n}",
        ),
        ("enum E { A, B }", "enum E {A,B}"),
        ("type a = [any];", "type a = [any];"),
        (
            "type b = [\n  any,\n  string\n];",
            "type b = [any, string\n];",
        ),
        (
            "function f() {\n  while (x) {}\n}",
            "function f() {while (x) { }\n}",
        ),
        ("if (x)\n  /* c */ a;", "if (x) \n/* c */ a;"),
        (
            "class A {\n  @d() m() {}\n  @e\n  n(@p x) {}\n}\n",
            "class A {@d() m() { }@e\n    n(@p x) { }\n}\n",
        ),
        (
            "switch (x) {\n  case 0: // zero\n    a;\n  default:\n    b;\n}\n",
            "switch (x) {case 0: // zero\n        // zero\n        a;default:b;\n}\n",
        ),
        (
            "function f() {\n  \"use strict\";\n  a;\n}\n",
            "function f() {\n    \"use strict\";a;\n}\n",
        ),
        ("if (true) {\n", "if (true) {}\n"),
    ] {
        assert_eq!(preserve(text), expected, "{text:?}");
    }
}

#[test]
fn a_parenthesized_operand_leaves_its_edge_grammar() {
    // tsgo at aa814927: the comma operand of `/` in an initializer takes one
    // pair of parentheses, which discharge the initializer's grammar.
    assert_eq!(
        print_statements("/j.tsx", "const a = <div attr=<span /><span /> />;\n"),
        ["const a = (<div attr=<span />/>, <span />) /  > ;"]
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

/// The first statement of `text` encoded as the client encodes a node it
/// made: without positions. A single-quoted literal keeps its quote.
fn synthesized_statement_data(text: &str) -> String {
    let mut file = crate::parse_source_file("/n.ts", text, ScriptKind::Ts);
    let NodeData::SourceFile(root) = &file.arena.node(file.root).data else {
        unreachable!("a parsed file's root is its source file")
    };
    let statement = file.arena.node_array(root.statements.unwrap()).nodes[0];
    let single_quoted: Vec<_> = file
        .arena
        .node_ids()
        .filter(|&id| {
            let node = file.arena.node(id);
            node.kind == tsc_syntax::SyntaxKind::StringLiteral
                && file.text()[tsc_syntax::skip_trivia(file.text(), node.pos as usize)..]
                    .starts_with('\'')
        })
        .collect();
    let nodes: Vec<_> = file.arena.node_ids().collect();
    for id in nodes {
        let node = file.arena.node_mut(id);
        node.pos = u32::MAX;
        node.end = u32::MAX;
    }
    base64::engine::general_purpose::STANDARD
        .encode(encode_node(&file, statement, &|id| single_quoted.contains(&id)).0)
}

/// A snapshot of a project of `/src/index.ts` (with the snapshot's
/// preferences), and its project.
fn insertion_session(text: &str, preferences: Value) -> (Session, u64, String) {
    let (session, _) = session(&[("/tsconfig.json", "{}"), ("/src/index.ts", text)]);
    let created = call(
        &session,
        "createSnapshot",
        json!({ "openProjects": ["/tsconfig.json"], "userPreferences": preferences }),
    )
    .unwrap();
    let snapshot = created["snapshot"].as_u64().unwrap();
    let project = created["projects"][0]["id"].as_str().unwrap().to_owned();
    (session, snapshot, project)
}

#[test]
fn a_node_formatted_for_insertion_takes_the_indentation_and_settings_there() {
    // tsgo's handleFormatNodeForInsertion at aa8149273 for the same nodes:
    // the indentation of the position's line, the snapshot's format
    // settings (an indent size, a new line), a single-quoted literal kept.
    let text = "function greet(name: string) {\n    console.log(name);\n}\n";
    let position = text.find("\n    console.log").unwrap() + 1;
    let format = |preferences: Value, node: &str| {
        let (session, snapshot, project) = insertion_session(text, preferences);
        call(
            &session,
            "formatNodeForInsertion",
            json!({
                "snapshot": snapshot,
                "project": project,
                "file": "/src/index.ts",
                "position": position,
                "data": synthesized_statement_data(node),
            }),
        )
        .map(|text| text.as_str().unwrap().to_owned())
    };
    let class = "class C { m() { return 1; } p = 2; }";
    assert_eq!(
        format(Value::Null, class).unwrap(),
        "    class C {\n        m() { return 1; }\n        p = 2;\n    }"
    );
    assert_eq!(
        format(json!({ "indentSize": 2 }), class).unwrap(),
        "  class C {\n    m() { return 1; }\n    p = 2;\n  }"
    );
    assert_eq!(
        format(json!({ "newLineCharacter": "\r\n" }), class).unwrap(),
        "    class C {\r\n        m() { return 1; }\r\n        p = 2;\r\n    }"
    );
    assert_eq!(
        format(Value::Null, "const s = 'q';").unwrap(),
        "    const s = 'q';"
    );
}

#[test]
fn formatting_for_insertion_reports_a_missing_file_before_bad_data() {
    // tsgo resolves the snapshot, its program and the file before it
    // decodes the node.
    let (session, snapshot, project) = insertion_session("const a = 1;\n", Value::Null);
    let request = |file: &str, data: &str| {
        call(
            &session,
            "formatNodeForInsertion",
            json!({
                "snapshot": snapshot,
                "project": project,
                "file": file,
                "position": 0,
                "data": data,
            }),
        )
        .unwrap_err()
    };
    assert_eq!(
        request("/src/missing.ts", "!"),
        "api: client error: source file not found: /src/missing.ts"
    );
    assert_eq!(
        request("/src/index.ts", "!"),
        "api: client error: invalid base64 data: illegal base64 data at input byte 0"
    );
}
