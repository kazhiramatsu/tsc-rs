use super::*;
use crate::nodes::{NodeArray, NodeData};
use crate::relocate::{collect_node_data_ids, remap_node_data_ids};
use std::collections::HashMap;
use tsc_diagnostics::{DocumentVersion, MessageChain, VersionedTextStore};
use tsc_types::{IdentityDomain, IdentitySpace};

#[derive(Clone, Debug, PartialEq)]
struct CanonicalNode {
    kind: SyntaxKind,
    flags: i32,
    numeric_literal_flags: i32,
    multi_line: Option<bool>,
    pos: u32,
    end: u32,
    parent: Option<NodeId>,
    js_doc: Option<crate::NodeArrayId>,
    data: NodeData,
}

#[derive(Clone, Debug, PartialEq)]
struct CanonicalTree {
    nodes: Vec<CanonicalNode>,
    arrays: Vec<NodeArray>,
    external_module_indicator: Option<NodeId>,
}

fn canonical_tree(source: &SourceFile) -> CanonicalTree {
    let mut node_map = HashMap::new();
    let mut array_map = HashMap::new();
    let mut nodes = vec![source.root];
    let mut arrays = Vec::new();
    node_map.insert(source.root, NodeId(0));

    let mut node_index = 0usize;
    let mut array_index = 0usize;
    while node_index < nodes.len() || array_index < arrays.len() {
        while node_index < nodes.len() {
            let node = source.arena.node(nodes[node_index]);
            let mut child_nodes = Vec::new();
            let mut child_arrays = Vec::new();
            collect_node_data_ids(&node.data, &mut child_nodes, &mut child_arrays);
            if let Some(js_doc) = node.js_doc {
                child_arrays.push(js_doc);
            }
            for child in child_nodes {
                if let std::collections::hash_map::Entry::Vacant(entry) = node_map.entry(child) {
                    let canonical = NodeId(nodes.len() as u32);
                    entry.insert(canonical);
                    nodes.push(child);
                }
            }
            for child in child_arrays {
                if let std::collections::hash_map::Entry::Vacant(entry) = array_map.entry(child) {
                    let canonical = crate::NodeArrayId(arrays.len() as u32);
                    entry.insert(canonical);
                    arrays.push(child);
                }
            }
            node_index += 1;
        }
        if array_index < arrays.len() {
            let array = source.arena.node_array(arrays[array_index]);
            for child in &array.nodes {
                if let std::collections::hash_map::Entry::Vacant(entry) = node_map.entry(*child) {
                    let canonical = NodeId(nodes.len() as u32);
                    entry.insert(canonical);
                    nodes.push(*child);
                }
            }
            array_index += 1;
        }
    }

    let canonical_nodes = nodes
        .iter()
        .map(|id| {
            let node = source.arena.node(*id);
            let mut data = node.data.clone();
            remap_node_data_ids(
                &mut data,
                |id| *node_map.get(&id).expect("canonical node reference"),
                |id| *array_map.get(&id).expect("canonical array reference"),
            );
            CanonicalNode {
                kind: node.kind,
                flags: node.flags,
                numeric_literal_flags: node.numeric_literal_flags,
                multi_line: node.multi_line,
                pos: node.pos,
                end: node.end,
                parent: node.parent.map(|id| {
                    *node_map
                        .get(&id)
                        .expect("canonical parent belongs to the root graph")
                }),
                js_doc: node.js_doc.map(|id| {
                    *array_map
                        .get(&id)
                        .expect("canonical JSDoc array belongs to the root graph")
                }),
                data,
            }
        })
        .collect();
    let canonical_arrays = arrays
        .iter()
        .map(|id| {
            let mut array = source.arena.node_array(*id).clone();
            for node in &mut array.nodes {
                *node = *node_map
                    .get(node)
                    .expect("canonical array element belongs to the root graph");
            }
            array
        })
        .collect();
    CanonicalTree {
        nodes: canonical_nodes,
        arrays: canonical_arrays,
        external_module_indicator: source.external_module_indicator.map(|id| {
            *node_map
                .get(&id)
                .expect("external module indicator belongs to the root graph")
        }),
    }
}

fn diagnostic_pins(source: &SourceFile) -> Vec<(u32, Option<u32>, Option<u32>, MessageChain)> {
    source
        .parse_diagnostics
        .iter()
        .map(|diagnostic| {
            (
                diagnostic.code(),
                diagnostic.start,
                diagnostic.length,
                diagnostic.message.clone(),
            )
        })
        .collect()
}

fn compare_incremental(
    before: &str,
    start: usize,
    delete_length: usize,
    inserted: &str,
) -> IncrementalParseStats {
    compare_incremental_with_options(
        "incremental.ts",
        before,
        start,
        delete_length,
        inserted,
        ParseOptions::default(),
    )
}

fn compare_incremental_with_options(
    file_name: &str,
    before: &str,
    start: usize,
    delete_length: usize,
    inserted: &str,
    options: ParseOptions,
) -> IncrementalParseStats {
    assert!(before.is_char_boundary(start));
    assert!(before.is_char_boundary(start + delete_length));
    let mut after = before.to_owned();
    after.replace_range(start..start + delete_length, inserted);
    let old_snapshot = TextSnapshot::new(before, DocumentVersion::new("old"));
    let old = create_language_service_source_file(file_name, old_snapshot, options.clone());
    let new_snapshot = TextSnapshot::new(after, DocumentVersion::new("new"));
    let incremental = update_language_service_source_file(
        old,
        Arc::clone(&new_snapshot),
        ByteTextChangeRange {
            span: ByteTextSpan::new(start as u32, delete_length as u32),
            new_length: inserted.len() as u32,
        },
        options.clone(),
        IncrementalParseOptions::default(),
    )
    .expect("valid exact incremental edit");
    let fresh = create_language_service_source_file(file_name, new_snapshot, options);

    assert_eq!(canonical_tree(&fresh), canonical_tree(&incremental.source));
    assert_eq!(
        diagnostic_pins(&fresh),
        diagnostic_pins(&incremental.source)
    );
    assert_eq!(
        fresh.js_doc_diagnostics,
        incremental.source.js_doc_diagnostics
    );
    assert_eq!(fresh.referenced_files, incremental.source.referenced_files);
    assert_eq!(
        fresh.type_reference_directives,
        incremental.source.type_reference_directives
    );
    assert_eq!(
        fresh.lib_reference_directives,
        incremental.source.lib_reference_directives
    );
    assert_eq!(fresh.amd_dependencies, incremental.source.amd_dependencies);
    assert_eq!(fresh.module_name, incremental.source.module_name);
    assert_eq!(
        fresh.comment_directives,
        incremental.source.comment_directives
    );
    incremental.stats
}

#[test]
fn amd_pragma_edits_are_fresh_equivalent() {
    let before = concat!(
        "/// <amd-module name=\"before\" />\n",
        "/// <amd-dependency path=\"dep-a\" />\n",
        "export {};\n",
    );
    let start = before.find("before").expect("module name");
    compare_incremental(before, start, "before".len(), "after");

    let insertion = before.find("export").expect("export statement");
    compare_incremental(
        before,
        insertion,
        0,
        "/// <amd-dependency path=\"dep-b\" name=\"alias\" />\n",
    );
}

#[test]
fn dynamic_import_and_import_meta_edits_are_fresh_equivalent() {
    let before = concat!(
        "export const load = () => import(\"./before\");\n",
        "export const url = import.meta.url;\n",
    );
    let specifier = before.find("./before").expect("dynamic import specifier");
    compare_incremental(before, specifier, "./before".len(), "./after");

    let meta = before.rfind("meta").expect("import.meta name");
    compare_incremental(before, meta, "meta".len(), "metal");
}

#[test]
fn import_attribute_and_typescript_extension_edits_are_fresh_equivalent() {
    let before = concat!(
        "import value from \"./data.ts\" with { type: \"javascript\" };\n",
        "export const load = () => import(\"./lazy.cts\", { with: { type: \"javascript\" } });\n",
    );
    let extension = before.find("data.ts").expect("static TypeScript extension");
    compare_incremental(before, extension, "data.ts".len(), "data.mts");

    let attribute = before
        .rfind("javascript")
        .expect("dynamic import attribute");
    compare_incremental(before, attribute, "javascript".len(), "json");
}

#[test]
fn pinned_incremental_parser_core_cases_are_fresh_equivalent() {
    let cases = [
        (
            "insert into method",
            "class C {\n public foo1() {}\n public foo2() { return 1; }\n public foo3() {}\n}",
            ";",
            0,
            " + 1",
        ),
        (
            "delete from method",
            "class C {\n public foo1() {}\n public foo2() { return 1 + 1; }\n public foo3() {}\n}",
            "+ 1",
            3,
            "",
        ),
        (
            "regular expression",
            "class C { public foo1() { /; } public foo2() { return 1;} public foo3() { } }",
            ";}",
            0,
            "/",
        ),
        (
            "line comment",
            "class C { public foo1() { /; } public foo2() { return 1; } public foo3() { } }",
            ";",
            0,
            "/",
        ),
        (
            "parameter reuse",
            "class C { public foo2(a, b, c, d) { return 1; } }",
            ";",
            0,
            " + 1",
        ),
        (
            "type member",
            "interface I { a: number; b: string; (c): d; new (e): f; g(): h }",
            ": string",
            0,
            "?",
        ),
        (
            "enum member",
            "enum E { a = 1, b = 1 << 1, c = 3, e = 4, f = 5, g = 7 }",
            "<<",
            2,
            "+",
        ),
        (
            "strict mode transition",
            "foo1();\nfoo1();\nfoo1();\npackage();",
            "foo1",
            0,
            "'use strict';\n",
        ),
        (
            "parenthesized to arrow",
            "var v = (a, b, c, d, e)",
            ", b",
            0,
            ":",
        ),
        ("assertion to arrow", "var v = <T>(a);", ";", 0, " => 1"),
        (
            "yield context",
            "function foo() {\nyield(foo1);\n}",
            "foo",
            0,
            "*",
        ),
        (
            "class to interface",
            "class A { public M1() { } public M2() { } p1 = 0; p2 = 0 }",
            "class",
            5,
            "interface",
        ),
        (
            "object literal to class",
            "var v = { public A() { } public B() { } public C() { } }",
            "var v =",
            7,
            "class C",
        ),
        (
            "incomplete comment",
            "function bug(\n test /** */ true = test test 123\n) {}",
            "/",
            1,
            "",
        ),
    ];

    for (name, source, needle, delete_length, insert) in cases {
        let start = source.find(needle).expect(name);
        let delete_length = if delete_length == 0 { 0 } else { delete_length };
        let stats = compare_incremental(source, start, delete_length, insert);
        assert!(stats.incremental, "{name}: {stats:#?}");
    }
}

#[test]
fn pinned_incremental_parser_context_and_lookahead_matrix_is_fresh_equivalent() {
    macro_rules! pinned {
        ($name:expr, $source:expr, $start:expr, $delete:expr, $insert:expr, $reuse:expr) => {{
            let source: &str = $source;
            let stats = compare_incremental(source, $start, $delete, $insert);
            assert!(stats.incremental, "{}: {stats:#?}", $name);
            if let Some(expected) = $reuse {
                assert_eq!(
                    stats.reused_list_elements > 0,
                    expected,
                    "{}: unexpected reuse shape: {stats:#?}",
                    $name
                );
            }
        }};
    }

    let source = "class C { public foo1() { ; } public foo2() { return 1/;} public foo3() { } }";
    pinned!(
        "regular expression 2",
        source,
        source.find(';').unwrap(),
        0,
        "/",
        Some(true)
    );
    let source = "class C { public foo1() { /; } public foo2() { return 1; } public foo3() { } }";
    pinned!("comment 2", source, 0, 0, "//", Some(false));
    let source = "//class C { public foo1() { /; } public foo2() { return 1; } public foo3() { } }";
    let stats = compare_incremental(source, 0, 2, "");
    assert!(
        stats.full_parse_fallback,
        "comment-only old files have no reusable list"
    );
    let source =
        "class C { public foo1() { /; } public foo2() { */ return 1; } public foo3() { } }";
    pinned!(
        "comment 4",
        source,
        source.find(';').unwrap(),
        0,
        "*",
        Some(true)
    );

    for (name, source, start, delete, insert, reuse) in [
        (
            "strict mode 1",
            "foo1();\r\nfoo1();\r\nfoo1();\r\npackage();",
            0,
            0,
            "'strict';\r\n",
            true,
        ),
        (
            "strict mode 2",
            "foo1();\r\nfoo1();\r\nfoo1();\r\npackage();",
            0,
            0,
            "'use strict';\r\n",
            true,
        ),
        (
            "parenthesized expression to arrow 2",
            "var v = (a, b) = c",
            "var v = (a, b) = c".find("= c").unwrap() + 1,
            0,
            ">",
            false,
        ),
        (
            "arrow function to parenthesized expression 1",
            "var v = (a:, b, c, d, e)",
            "var v = (a:, b, c, d, e)".find(':').unwrap(),
            1,
            "",
            false,
        ),
        (
            "arrow function to parenthesized expression 2",
            "var v = (a, b) => c",
            "var v = (a, b) => c".find('>').unwrap(),
            1,
            "",
            false,
        ),
        (
            "arrow function to assertion",
            "var v = <T>(a) => 1;",
            "var v = <T>(a) => 1;".find(" =>").unwrap(),
            " => 1".len(),
            "",
            false,
        ),
        (
            "contextual shift to shift-equals",
            "var v = 1 >> = 2",
            "var v = 1 >> = 2".find(">> =").unwrap() + 2,
            1,
            "",
            false,
        ),
        (
            "contextual shift-equals to shift",
            "var v = 1 >>= 2",
            "var v = 1 >>= 2".find(">>=").unwrap() + 2,
            0,
            " ",
            false,
        ),
        (
            "contextual shift to generic invocation",
            "var v = T>>(2)",
            "var v = T>>(2)".find('T').unwrap(),
            0,
            "Foo<Bar<",
            false,
        ),
        (
            "generic invocation to contextual shift",
            "var v = Foo<Bar<T>>(2)",
            "var v = Foo<Bar<T>>(2)".find("Foo<Bar<").unwrap(),
            "Foo<Bar<".len(),
            "",
            false,
        ),
        (
            "contextual shift to generic type and initializer",
            "var v = T>>=2;",
            "var v = T>>=2;".find('=').unwrap(),
            "= ".len(),
            ": Foo<Bar<",
            false,
        ),
        (
            "generic type and initializer to contextual shift",
            "var v : Foo<Bar<T>>=2;",
            "var v : Foo<Bar<T>>=2;".find(':').unwrap(),
            ": Foo<Bar<".len(),
            "= ",
            false,
        ),
        (
            "arithmetic operator to type argument list",
            "var v = new Dictionary<A, B>0",
            "var v = new Dictionary<A, B>0".find('0').unwrap(),
            1,
            "()",
            false,
        ),
        (
            "type argument list to arithmetic operator",
            "var v = new Dictionary<A, B>()",
            "var v = new Dictionary<A, B>()".find("()").unwrap(),
            2,
            "",
            false,
        ),
        (
            "yield context 2",
            "function *foo() {\r\nyield(foo1);\r\n}",
            "function *foo() {\r\nyield(foo1);\r\n}".find('*').unwrap(),
            1,
            "",
            false,
        ),
    ] {
        pinned!(name, source, start, delete, insert, Some(reuse));
    }

    for (name, source) in [
        ("speculative generic lookahead 1", "var v = F<b>e"),
        ("speculative generic lookahead 2", "var v = F<a,b>e"),
        ("speculative generic lookahead 3", "var v = F<a,b,c>e"),
        ("speculative generic lookahead 4", "var v = F<a,b,c,d>e"),
    ] {
        let start = source.find('b').unwrap() + 1;
        pinned!(name, source, start, 0, ",x", None::<bool>);
    }

    for (name, source) in [
        (
            "strict mode 3",
            "'strict';\r\nfoo1();\r\nfoo1();\r\nfoo1();\r\npackage();",
        ),
        (
            "strict mode 4",
            "'use strict';\r\nfoo1();\r\nfoo1();\r\nfoo1();\r\npackage();",
        ),
        (
            "strict mode 7",
            "'use blahhh';\r\nfoo1();\r\nfoo2();\r\nfoo3();\r\nfoo4();\r\nfoo4();\r\nfoo6();\r\nfoo7();\r\nfoo8();\r\nfoo9();\r\n",
        ),
    ] {
        let end = source.find('f').unwrap();
        pinned!(name, source, 0, end, "", Some(true));
    }
    let source = "'use blahhh';\r\nfoo1();\r\nfoo2();\r\nfoo3();\r\nfoo4();\r\nfoo4();\r\nfoo6();\r\nfoo7();\r\nfoo8();\r\nfoo9();\r\n";
    pinned!(
        "strict mode 5",
        source,
        source.find('b').unwrap(),
        6,
        "strict",
        Some(true)
    );
    let source = "'use strict';\r\nfoo1();\r\nfoo2();\r\nfoo3();\r\nfoo4();\r\nfoo4();\r\nfoo6();\r\nfoo7();\r\nfoo8();\r\nfoo9();\r\n";
    pinned!(
        "strict mode 6",
        source,
        source.find('s').unwrap(),
        6,
        "blahhh",
        Some(true)
    );
}

#[test]
fn pinned_incremental_parser_reusable_context_matrix_is_fresh_equivalent() {
    fn check(
        name: &str,
        source: &str,
        start: usize,
        delete: usize,
        insert: &str,
        expected_reuse: Option<bool>,
    ) {
        let stats = compare_incremental(source, start, delete, insert);
        assert!(stats.incremental, "{name}: {stats:#?}");
        if let Some(expected) = expected_reuse {
            assert_eq!(
                stats.reused_list_elements > 0,
                expected,
                "{name}: unexpected reuse shape: {stats:#?}"
            );
        }
    }

    let source = "export class Foo {\r\n}\r\n\r\nexport var foo = new Foo();\r\n\r\n    export function test(foo: Foo) {\r\n        return true;\r\n    }\r\n";
    check(
        "delete semicolon",
        source,
        source.rfind(';').unwrap(),
        1,
        "",
        Some(true),
    );
    let source = "class Dictionary<> { }\r\nvar y;\r\n";
    check(
        "edit after empty type parameter list",
        source,
        source.len(),
        0,
        "var x;",
        Some(true),
    );
    let source = "function fn(/* comment! */ a: number, c) { }";
    check(
        "delete parameter after comment",
        source,
        source.find("a:").unwrap(),
        "a: number,".len(),
        "",
        Some(false),
    );
    let source = "class C { set Bar(bar:string) {} } var o2 = { set Foo(val:number) { } };";
    check(
        "modifier added to accessor",
        source,
        source.find("set").unwrap(),
        0,
        "public ",
        Some(true),
    );
    let source = "alert(100); class OverloadedMonster { constructor(); constructor(name) { } }";
    check(
        "insert parameter ahead of parameter",
        source,
        source.find("100").unwrap(),
        0,
        "'1', ",
        Some(true),
    );
    let source = "module mAmbient { module m3 { } }";
    check(
        "insert declare before module",
        source,
        0,
        0,
        "declare ",
        Some(false),
    );
    let source = "() =>\n   // do something\n0;";
    check(
        "insert function above arrow with comment",
        source,
        0,
        0,
        "function Foo() { }",
        Some(false),
    );
    let source = "while (true) /3; return;";
    check(
        "finish incomplete regular expression",
        source,
        source.len() - 1,
        0,
        "/",
        Some(false),
    );
    let source = "return;\r\nwhile (true) /3/g;";
    check(
        "regular expression to divide operation",
        source,
        source.find("while").unwrap(),
        "while ".len(),
        "",
        Some(false),
    );
    let source = "return;\r\n(true) /3/g;";
    check(
        "divide operation to regular expression",
        source,
        source.find('(').unwrap(),
        0,
        "while ",
        Some(false),
    );
    check(
        "unterminated comment after keyword converted to identifier",
        "return; a.public /*",
        0,
        0,
        "",
        // The pinned test supplies an unchanged range and tsc returns the
        // same mutable SourceFile object. Rust publishes a new immutable
        // snapshot/version and therefore deliberately does not count this as
        // copied-subtree reuse.
        Some(false),
    );

    for (name, source, delete, insert, expected_reuse) in [
        (
            "interface to class",
            "interface A { M1?(); M2?(); M3?(); p1?; p2?; p3? }",
            "interface",
            "class",
            false,
        ),
        (
            "move methods from class to object literal",
            "class C { public A() { } public B() { } public C() { } }",
            "class C",
            "var v =",
            false,
        ),
        (
            "move methods from object literal to class",
            "var v = { public A() { } public B() { } public C() { } }",
            "var v =",
            "class C",
            true,
        ),
        (
            "do not move constructors from class to object literal",
            "class C { public constructor() { } public constructor() { } public constructor() { } }",
            "class C",
            "var v =",
            false,
        ),
        (
            "do not move constructor-named methods from object literal to class",
            "var v = { public constructor() { } public constructor() { } public constructor() { } }",
            "var v =",
            "class C",
            false,
        ),
        (
            "index signatures class to interface",
            "class C { public [a: number]: string; public [a: number]: string; public [a: number]: string }",
            "class",
            "interface",
            true,
        ),
        (
            "index signatures interface to class",
            "interface C { public [a: number]: string; public [a: number]: string; public [a: number]: string }",
            "interface",
            "class",
            true,
        ),
        (
            "accessors class to object literal",
            "class C { public get A() { } public get B() { } public get C() { } }",
            "class C",
            "var v =",
            false,
        ),
        (
            "accessors object literal to class",
            "var v = { public get A() { } public get B() { } public get C() { } }",
            "var v =",
            "class C",
            true,
        ),
    ] {
        check(
            name,
            source,
            source.find(delete).unwrap(),
            delete.len(),
            insert,
            Some(expected_reuse),
        );
    }

    for (name, source, start, delete, insert) in [
        (
            "surround function declarations with block",
            "declare function F1() { } export function F2() { } declare export function F3() { }",
            0,
            0,
            "{",
        ),
        (
            "remove block around function declarations",
            "{ declare function F1() { } export function F2() { } declare export function F3() { }",
            0,
            1,
            "",
        ),
        (
            "object literal to class in strict mode",
            "\"use strict\"; var v = { public A() { } public B() { } public C() { } }",
            14,
            "var v =".len(),
            "class C",
        ),
        (
            "index signatures class to interface in strict mode",
            "\"use strict\"; class C { public [a: number]: string; public [a: number]: string; public [a: number]: string }",
            14,
            "class".len(),
            "interface",
        ),
        (
            "index signatures interface to class in strict mode",
            "\"use strict\"; interface C { public [a: number]: string; public [a: number]: string; public [a: number]: string }",
            14,
            "interface".len(),
            "class",
        ),
        (
            "accessors object literal to class in strict mode",
            "\"use strict\"; var v = { public get A() { } public get B() { } public get C() { } }",
            14,
            "var v =".len(),
            "class C",
        ),
    ] {
        check(name, source, start, delete, insert, Some(true));
    }

    let source = "class Greeter { constructor(element: HTMLElement) { } }";
    check("reuse permanent subtree flags", source, 15, 0, "\n", None);
}

#[test]
fn pinned_incremental_parser_simulated_typing_sequences_are_fresh_equivalent() {
    fn run_sequence(initial: &str, edits: impl IntoIterator<Item = (usize, usize, &'static str)>) {
        let mut text = initial.to_owned();
        let mut source = create_language_service_source_file(
            "typing.ts",
            TextSnapshot::new(&text, DocumentVersion::new("0")),
            ParseOptions::default(),
        );
        for (ordinal, (start, delete, insert)) in edits.into_iter().enumerate() {
            text.replace_range(start..start + delete, insert);
            let snapshot = TextSnapshot::new(&text, DocumentVersion::new(ordinal.to_string()));
            let updated = update_language_service_source_file(
                source,
                Arc::clone(&snapshot),
                ByteTextChangeRange {
                    span: ByteTextSpan::new(start as u32, delete as u32),
                    new_length: insert.len() as u32,
                },
                ParseOptions::default(),
                IncrementalParseOptions::default(),
            )
            .unwrap();
            let fresh =
                create_language_service_source_file("typing.ts", snapshot, ParseOptions::default());
            assert_eq!(canonical_tree(&fresh), canonical_tree(&updated.source));
            assert_eq!(diagnostic_pins(&fresh), diagnostic_pins(&updated.source));
            source = updated.source;
        }
    }

    let source = "interface IFoo<T> { }\r\ninterface Array<T> extends IFoo<T> { }";
    let start = source.find("extends").unwrap();
    run_sequence(source, (0.."extends IFoo<T>".len()).map(|_| (start, 1, "")));

    let source = concat!(
        "function foo() {\r\n",
        " function getOccurrencesAtPosition() {\r\n",
        "  switch (node) { enum \r\n  }\r\n",
        "  return undefined;\r\n",
        "  function keywordToReferenceEntry() {}\r\n",
        " }\r\n",
        " return { getEmitOutput: (fileName): Bar => null };\r\n",
        "}",
    );
    let start = source.find("enum ").unwrap() + "enum ".len();
    run_sequence(source, [(start, 0, "F"), (start + 1, 0, "o")]);
}

#[test]
fn comment_directives_remain_fresh_equivalent_across_reused_regions() {
    for directive in ["// @ts-ignore", "/* @ts-ignore */", "/*\n @ts-ignore */"] {
        let source = format!(
            "const x = 10;\nfunction one() {{\n {directive}\n let a: string = x;\n return a;\n}}\nfunction two() {{\n {directive}\n let b: string = x;\n return b;\n}}\none();\ntwo();"
        );
        let second = source.rfind(directive).unwrap();
        compare_incremental(&source, second, directive.len(), "");
        compare_incremental(&source, second + directive.find('@').unwrap(), 1, "blah ");
        let kind = second + directive.find("ignore").unwrap();
        compare_incremental(&source, kind, "ignore".len(), "expect-error");

        let without_second = format!(
            "{}{}",
            &source[..second],
            &source[second + directive.len()..]
        );
        compare_incremental(&without_second, second, 0, directive);
    }
}

#[test]
fn reused_jsdoc_attachments_and_diagnostics_are_fresh_equivalent() {
    let source = "/**\n * @typedef Name\n * @type {string}\n * @type {Oops}\n */\nfunction first() {}\n/** @param {number} x */\nfunction second(x) { return x + 1; }\n";
    let options = ParseOptions {
        javascript_file: true,
        ..ParseOptions::default()
    };
    let probe = create_language_service_source_file(
        "incremental.js",
        TextSnapshot::new(source, DocumentVersion::new("probe")),
        options.clone(),
    );
    assert!(!probe.js_doc_diagnostics.is_empty());
    let edit = source.rfind("+ 1").unwrap();
    let stats = compare_incremental_with_options("incremental.js", source, edit, 3, "+ 2", options);
    assert!(stats.reused_nodes > 0);
    let first_body = source.find("first() {").unwrap() + "first() {".len();
    compare_incremental_with_options(
        "incremental.js",
        source,
        first_body,
        0,
        " return; ",
        ParseOptions {
            javascript_file: true,
            ..ParseOptions::default()
        },
    );
}

#[test]
fn external_module_and_top_level_await_reparse_remain_fresh_equivalent() {
    let source = "const before = 1;\nawait load();\nconst after = 2;\n";
    let stats = compare_incremental(source, 0, 0, "export {};\n");
    assert!(stats.incremental);

    let source = "export {};\nconst before = 1;\nawait load();\nconst after = 2;\n";
    compare_incremental(source, 0, "export {};\n".len(), "");
    let edit = source.rfind("2;").unwrap();
    let stats = compare_incremental(source, edit, 1, "3");
    assert!(stats.reused_list_elements > 0);

    let options = ParseOptions {
        force_external_module: true,
        ..ParseOptions::default()
    };
    compare_incremental_with_options(
        "forced-module.ts",
        "const before = 1;\nawait load();\nconst after = 2;\n",
        "const before = 1;\nawait load();\nconst after = 2;\n"
            .rfind('2')
            .unwrap(),
        1,
        "4",
        options,
    );
}

#[test]
fn language_service_snapshot_versions_are_owned_and_old_versions_drop_promptly() {
    let domain = IdentityDomain::reclaiming();
    let old_snapshot = TextSnapshot::new(
        "const before = 1;\nconst value = 2;\nconst after = 3;",
        DocumentVersion::new("old"),
    );
    let old_snapshot_weak = Arc::downgrade(&old_snapshot);
    let source = create_language_service_source_file_in_identity_domain(
        "versions.ts",
        Arc::clone(&old_snapshot),
        ParseOptions::default(),
        &domain,
    )
    .unwrap();
    drop(old_snapshot);
    assert!(old_snapshot_weak.upgrade().is_some());

    let edit = source.text().find("2;").unwrap();
    let new_snapshot = TextSnapshot::new(
        "const before = 1;\nconst value = 4;\nconst after = 3;",
        DocumentVersion::new("new"),
    );
    let updated = update_language_service_source_file_in_identity_domain(
        source,
        Arc::clone(&new_snapshot),
        ByteTextChangeRange {
            span: ByteTextSpan::new(edit as u32, 1),
            new_length: 1,
        },
        ParseOptions::default(),
        IncrementalParseOptions::default(),
        &domain,
    )
    .unwrap();
    assert!(old_snapshot_weak.upgrade().is_none());
    assert!(Arc::ptr_eq(updated.source.snapshot(), &new_snapshot));
    assert_eq!(updated.source.snapshot().document_version().as_str(), "new");
    assert_eq!(
        domain
            .stats()
            .unwrap()
            .space(IdentitySpace::Node)
            .active_ranges,
        1
    );

    drop(updated);
    assert_eq!(
        domain
            .stats()
            .unwrap()
            .space(IdentitySpace::Node)
            .active_ranges,
        0
    );
}

#[test]
fn deterministic_unicode_edit_sequence_is_fresh_equivalent_and_reclaims_old_ranges() {
    let initial = "// UTF-16 境界😀\ninterface Box { value: string; next?: Box; }\nconst first = { value: \"α😀\" };\nfunction read(x: Box) { return x.value; }\nconst last = read({ value: \"終\" });\n";
    let mut store = VersionedTextStore::new(initial, DocumentVersion::new("0"));
    let domain = IdentityDomain::reclaiming();
    let mut source = Arc::new(
        crate::parse_source_file_from_snapshot_in_identity_domain(
            "unicode.ts",
            store.current_snapshot(),
            ParseOptions::default(),
            None,
            &domain,
        )
        .unwrap(),
    );
    let insertions = ["", "x", "😀", "日本", "\n", "/*c*/", "=>", "?", "終😀"];
    let mut state = 0x1a2b_3c4d_5e6f_7788u64;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };

    for edit in 0..200u32 {
        let text = source.text().to_owned();
        let boundaries = text
            .char_indices()
            .map(|(position, _)| position)
            .chain(std::iter::once(text.len()))
            .collect::<Vec<_>>();
        let mut start_index = next() as usize % boundaries.len();
        let mut end_index = next() as usize % boundaries.len();
        if start_index > end_index {
            std::mem::swap(&mut start_index, &mut end_index);
        }
        if edit % 41 != 0 {
            end_index = end_index.min(start_index.saturating_add(3));
        }
        let start = boundaries[start_index];
        let end = boundaries[end_index];
        let inserted = insertions[next() as usize % insertions.len()];
        let outcome = store
            .edit_bytes(
                ByteTextSpan::new(start as u32, (end - start) as u32),
                inserted,
                DocumentVersion::new(edit.to_string()),
            )
            .unwrap();
        let snapshot = store.snapshot();
        let updated = update_language_service_source_file_in_identity_domain(
            source,
            Arc::clone(&snapshot),
            outcome.byte_change(),
            ParseOptions::default(),
            IncrementalParseOptions::default(),
            &domain,
        )
        .unwrap();
        let fresh =
            create_language_service_source_file("unicode.ts", snapshot, ParseOptions::default());
        assert_eq!(
            canonical_tree(&fresh),
            canonical_tree(&updated.source),
            "tree mismatch after edit {edit}: start={start} end={end} inserted={inserted:?}"
        );
        assert_eq!(
            diagnostic_pins(&fresh),
            diagnostic_pins(&updated.source),
            "diagnostic mismatch after edit {edit}"
        );
        assert_eq!(
            fresh.comment_directives, updated.source.comment_directives,
            "comment directives mismatch after edit {edit}"
        );
        source = updated.source;

        let stats = domain.stats().unwrap();
        assert_eq!(stats.space(IdentitySpace::Node).active_ranges, 1);
        assert_eq!(stats.space(IdentitySpace::NodeArray).active_ranges, 1);
    }
}

#[test]
fn unicode_change_boundaries_fail_closed_without_rounding() {
    let snapshot = TextSnapshot::new("const emoji = \"😀\";", DocumentVersion::new("1"));
    let source = create_language_service_source_file(
        "unicode-boundary.ts",
        snapshot,
        ParseOptions::default(),
    );
    let emoji = source.text().find('😀').unwrap() as u32;
    let replacement = TextSnapshot::new("const emoji = \"x\";", DocumentVersion::new("2"));
    let error = update_language_service_source_file(
        source,
        replacement,
        ByteTextChangeRange {
            span: ByteTextSpan::new(emoji + 1, 1),
            new_length: 1,
        },
        ParseOptions::default(),
        IncrementalParseOptions::default(),
    )
    .unwrap_err();
    assert_eq!(
        error,
        IncrementalParseError::InvalidOldScalarBoundary {
            position: emoji + 1
        }
    );
}
