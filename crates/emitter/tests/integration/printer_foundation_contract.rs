use tsc_emitter::{
    create_printer, transform_nodes, NewLineKind, PrintRequest, PrinterError, PrinterOptions,
    SourceFileTextMode, StandaloneWriter, TransformArena, TransformBundle, TransformRoot,
    UnsupportedEmitFeature,
};
use tsc_syntax::{parse_source_file, LanguageVariant, NodeData, ParseOptions};

fn transformed(
    text: &str,
) -> (
    tsc_emitter::TransformationResult<'static>,
    tsc_emitter::TransformSourceId,
) {
    let parsed = parse_source_file("unicode.js", text, Default::default(), None);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    let result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        Vec::new(),
        false,
    )
    .expect("identity transform");
    (result, source)
}

#[test]
fn whole_source_pipeline_preserves_text_and_positions() {
    // h2-6a-m-2 §5: the identity arm's hook-event seam is deleted with
    // the recording model (the arm records nothing and is unreachable
    // under compiler emit); the byte/position oracle stands.
    let text = "const astral = \"😀\";\nconst combining = \"e\u{301}\";\n";
    let (mut result, source) = transformed(text);
    let mut printer = create_printer(PrinterOptions::new(NewLineKind::LineFeed));
    let printed = printer
        .print(&mut result, PrintRequest::SourceFile(source), None)
        .expect("whole-source print");

    assert_eq!(printed.text(), text);
    assert_eq!(
        printed.end().position().value(),
        u32::try_from(text.encode_utf16().count()).unwrap()
    );
    assert_eq!(printed.end().line(), 2);
    assert_eq!(printed.end().column(), 0);
    assert!(printed.source_map().is_none());
}

#[test]
fn canonical_source_file_mode_walks_an_unchanged_tree() {
    let text = "export function foo() {\n  console.log(\"foo\");\n}\n";
    let (mut result, source) = transformed(text);
    let mut printer = create_printer(
        PrinterOptions::new(NewLineKind::CarriageReturnLineFeed)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    );
    let printed = printer
        .print(&mut result, PrintRequest::SourceFile(source), None)
        .expect("canonical source-file print");

    assert_eq!(
        printed.text(),
        "export function foo() {\r\n    console.log(\"foo\");\r\n}\r\n"
    );
}

#[test]
fn throw_keyword_and_expression_share_internal_comment_boundaries_once() {
    let text = concat!(
        "/*1*/ try /*2*/ { /*3*/\n",
        "    /*4*/ throw /*5*/ \"no\" /*6*/;\n",
        "/*7*/} /*8*/ catch /*9*/ ( /*10*/ e /*11*/ ) /*12*/ { /*13*/\n",
        "/*14*/} /*15*/ finally /*16*/ { /*17*/\n",
        "/*18*/} /*19*/\n",
    );
    let (mut result, source) = transformed(text);
    let printed = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    )
    .print(&mut result, PrintRequest::SourceFile(source), None)
    .expect("canonical try statement print");

    assert_eq!(printed.text().matches("/*5*/").count(), 1);
    assert_eq!(printed.text().matches("/*6*/").count(), 1);
    assert!(printed.text().contains("throw /*5*/ \"no\" /*6*/;"));
}

#[test]
fn standalone_tsx_printer_emits_type_argument_delimiters() {
    let parsed = parse_source_file(
        "type-arguments.tsx",
        concat!(
            "const selfClosing = <Foo<unknown, Props> />;\n",
            "const opening = <Foo<string>></Foo>;\n",
        ),
        ParseOptions {
            language_variant: LanguageVariant::Jsx,
            ..ParseOptions::default()
        },
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        Vec::new(),
        false,
    )
    .expect("identity TSX transform");
    let printed = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    )
    .print(&mut result, PrintRequest::SourceFile(source), None)
    .expect("print standalone TSX type arguments");

    assert_eq!(
        printed.text(),
        concat!(
            "const selfClosing = <Foo<unknown, Props> />;\n",
            "const opening = <Foo<string>></Foo>;\n",
        )
    );
}

#[test]
fn standalone_tsx_printer_preserves_attribute_and_child_comments() {
    let text = concat!(
        "const value = (<div\n",
        "/* block attribute comment */\n",
        "attr=\"ok\">\n",
        "  <span // line attribute comment\n",
        "  title=\"ok\" /> // first child comment\n",
        "  // second child comment\n",
        "  <span />\n",
        "</div>);\n",
    );
    let parsed = parse_source_file(
        "comments.tsx",
        text,
        ParseOptions {
            language_variant: LanguageVariant::Jsx,
            ..ParseOptions::default()
        },
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        Vec::new(),
        false,
    )
    .expect("identity TSX transform");
    let printed = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    )
    .print(&mut result, PrintRequest::SourceFile(source), None)
    .expect("print standalone TSX comments");

    for comment in ["/* block attribute comment */", "// line attribute comment"] {
        assert_eq!(printed.text().matches(comment).count(), 1, "{printed:?}");
    }
    // JSX text includes the source spelling after the first child while that
    // child also owns its same-line trailing comment, matching tsc's ordinary
    // comments phase around a NoInterveningComments child list.
    assert_eq!(
        printed.text().matches("// first child comment").count(),
        2,
        "{printed:?}"
    );
    assert_eq!(
        printed.text().matches("// second child comment").count(),
        1,
        "{printed:?}"
    );
}

#[test]
fn standalone_tsx_tag_name_line_comment_uses_the_configured_newline() {
    let text = concat!(
        "const value = <span // a double-slash comment\n",
        "  attr2=\"bar\"\n",
        "/>;\n",
    );
    let parsed = parse_source_file(
        "tag-name-line-comment.tsx",
        text,
        ParseOptions {
            language_variant: LanguageVariant::Jsx,
            ..ParseOptions::default()
        },
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        Vec::new(),
        false,
    )
    .expect("identity TSX transform");
    let printed = create_printer(
        PrinterOptions::new(NewLineKind::CarriageReturnLineFeed)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    )
    .print(&mut result, PrintRequest::SourceFile(source), None)
    .expect("print TSX tag-name line comment");

    assert_eq!(
        printed.text(),
        "const value = <span // a double-slash comment\r\n attr2=\"bar\"/>;\r\n"
    );
}

#[test]
fn standalone_tsx_child_keeps_comment_newline_separate_from_raw_jsx_text() {
    let text = concat!(
        "const value = <div>\n",
        "  <Item />  // error\n",
        "  <Item />\n",
        "</div>;\n",
    );
    let parsed = parse_source_file(
        "child-line-comment.tsx",
        text,
        ParseOptions {
            language_variant: LanguageVariant::Jsx,
            ..ParseOptions::default()
        },
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        Vec::new(),
        false,
    )
    .expect("identity TSX transform");
    let printed = create_printer(
        PrinterOptions::new(NewLineKind::CarriageReturnLineFeed)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    )
    .print(&mut result, PrintRequest::SourceFile(source), None)
    .expect("print TSX child line comment");

    assert_eq!(
        printed.text(),
        concat!(
            "const value = <div>\n",
            "  <Item /> // error\r\n",
            "  // error\n",
            "  <Item />\n",
            "</div>;\r\n",
        )
    );
}

#[test]
fn empty_case_block_uses_the_multiline_case_block_list_format() {
    let text = concat!("switch (compact) { }\n", "switch (multiline) {\n", "}\n",);
    let (mut result, source) = transformed(text);
    let printed = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    )
    .print(&mut result, PrintRequest::SourceFile(source), None)
    .expect("print empty switch case-block layouts");

    assert_eq!(
        printed.text(),
        "switch (compact) {\n}\nswitch (multiline) {\n}\n"
    );
}

#[test]
fn template_span_expression_owns_comments_after_the_substitution_open() {
    let text = concat!(
        "const a = 1;\n",
        "const f = () => `${\n",
        "// span comment\n",
        "a}tail`;\n",
    );
    let (mut result, source) = transformed(text);
    let printed = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    )
    .print(&mut result, PrintRequest::SourceFile(source), None)
    .expect("print template-span comments");

    assert_eq!(printed.text(), text);
}

#[test]
fn template_span_comment_ownership_covers_open_expression_and_close_boundaries() {
    let text = concat!(
        "`head${ // opening comment\n",
        "10}\n",
        "middle${\n",
        "/* expression comment */\n",
        "20\n",
        "// closing comment\n",
        "}\n",
        "tail`;\n",
    );
    let (mut result, source) = transformed(text);
    let printed = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    )
    .print(&mut result, PrintRequest::SourceFile(source), None)
    .expect("print all template-span comment boundaries");

    assert_eq!(printed.text(), text);
}

#[test]
fn canonical_string_line_continuation_keeps_its_source_newline() {
    let text = "var x = {'text\\\n':'hello'};\nx.text = \"bar\";\n";
    let (mut result, source) = transformed(text);
    let mut printer = create_printer(
        PrinterOptions::new(NewLineKind::CarriageReturnLineFeed)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    );
    let printed = printer
        .print(&mut result, PrintRequest::SourceFile(source), None)
        .expect("canonical string-literal line-continuation print");

    assert_eq!(
        printed.text(),
        "var x = { 'text\\\n': 'hello' };\r\nx.text = \"bar\";\r\n"
    );
}

#[test]
fn canonical_object_literals_preserve_same_line_property_groups() {
    let text = "const options = {\n    first: true, second: true,\n    third: 1, fourth: 2\n};\n";
    let (mut result, source) = transformed(text);
    let mut printer = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    );
    let printed = printer
        .print(&mut result, PrintRequest::SourceFile(source), None)
        .expect("canonical source-file print");

    assert_eq!(printed.text(), text);
}

#[test]
fn canonical_property_access_preserves_a_line_break_after_the_dot() {
    let text = "const files = fs.read().\n    filter(value => value);\n";
    let (mut result, source) = transformed(text);
    let mut printer = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    );
    let printed = printer
        .print(&mut result, PrintRequest::SourceFile(source), None)
        .expect("canonical source-file print");

    assert_eq!(printed.text(), text);
}

#[test]
fn canonical_binary_and_prefix_operators_preserve_lexical_separation() {
    let text = "var x = 1;\nvar y = 1;\nvar z =\nx\n+\n+\n+\ny;\nvar c =\nx\n-\n-\n-\ny;\n";
    let (mut result, source) = transformed(text);
    let mut printer = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    );
    let printed = printer
        .print(&mut result, PrintRequest::SourceFile(source), None)
        .expect("canonical source-file print");

    assert_eq!(
        printed.text(),
        "var x = 1;\nvar y = 1;\nvar z = x\n    +\n        + +y;\nvar c = x\n    -\n        - -y;\n"
    );
}

#[test]
fn canonical_nested_conditionals_preserve_source_line_structure() {
    let text = "var v = a \n  ? b\n    ? d\n    : e\n  : c\n    ? f\n    : g;\n";
    let (mut result, source) = transformed(text);
    let mut printer = create_printer(
        PrinterOptions::new(NewLineKind::CarriageReturnLineFeed)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    );
    let printed = printer
        .print(&mut result, PrintRequest::SourceFile(source), None)
        .expect("canonical source-file print");

    assert_eq!(
        printed.text(),
        "var v = a\r\n    ? b\r\n        ? d\r\n        : e\r\n    : c\r\n        ? f\r\n        : g;\r\n"
    );
}

#[test]
fn canonical_continue_statement_writes_its_asi_safe_terminator() {
    let text = "while (true) continue\n";
    let (mut result, source) = transformed(text);
    let mut printer = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    );
    let printed = printer
        .print(&mut result, PrintRequest::SourceFile(source), None)
        .expect("canonical source-file print");

    assert_eq!(printed.text(), "while (true)\n    continue;\n");
}

#[test]
fn canonical_jump_statements_preserve_keyword_and_label_comments() {
    let text = concat!(
        "foo: for (;;) {\n",
        "    /*1*/ continue /*2*/ foo /*3*/;\n",
        "    /*4*/ break /*5*/ foo /*6*/;\n",
        "}\n",
    );
    let (mut result, source) = transformed(text);
    let mut printer = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    );
    let printed = printer
        .print(&mut result, PrintRequest::SourceFile(source), None)
        .expect("canonical jump-statement print");

    assert_eq!(printed.text(), text);
}

#[test]
fn canonical_asi_jump_comments_follow_the_synthesized_terminator() {
    let text = concat!(
        "while (true) {\n",
        "    break // break reason\n",
        "}\n",
        "foo: while (true) {\n",
        "    continue foo // continue reason\n",
        "}\n",
    );
    let (mut result, source) = transformed(text);
    let mut printer = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    );
    let printed = printer
        .print(&mut result, PrintRequest::SourceFile(source), None)
        .expect("canonical ASI jump-comment print");

    assert_eq!(
        printed.text(),
        concat!(
            "while (true) {\n",
            "    break; // break reason\n",
            "}\n",
            "foo: while (true) {\n",
            "    continue foo; // continue reason\n",
            "}\n",
        )
    );
}

#[test]
fn canonical_switch_tokens_own_internal_comments() {
    let text = concat!(
        "/*-1*/ foo /*0*/ : /*1*/ switch /*2*/ ( /*3*/ false /*4*/ ) /*5*/ {\n",
        "    /*6*/ case /*7*/ false /*8*/ : /*9*/\n",
        "        /*10*/ break /*11*/ foo /*12*/;\n",
        "    /*13*/ default /*14*/ : /*15*/\n",
        "    /*16*/ case /*17*/ false /*18*/ : /*19*/ { /*20*/\n",
        "    /*21*/ } /*22*/\n",
        "}\n",
    );
    let (mut result, source) = transformed(text);
    let mut printer = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    );
    let printed = printer
        .print(&mut result, PrintRequest::SourceFile(source), None)
        .expect("canonical switch token-comment print");

    assert_eq!(
        printed.text(),
        concat!(
            "/*-1*/ foo /*0*/: /*1*/ switch /*2*/ ( /*3*/false /*4*/) /*5*/ {\n",
            "    /*6*/ case /*7*/ false /*8*/: /*9*/\n",
            "        /*10*/ break /*11*/ foo /*12*/;\n",
            "    /*13*/ default /*14*/: /*15*/\n",
            "    /*16*/ case /*17*/ false /*18*/: /*19*/ { /*20*/\n",
            "        /*21*/ } /*22*/\n",
            "}\n",
        )
    );
}

#[test]
fn canonical_do_statement_ends_a_non_block_body_before_while() {
    let text = "do\n    const value = 0;\nwhile (true);\n";
    let (mut result, source) = transformed(text);
    let mut printer = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    );
    let printed = printer
        .print(&mut result, PrintRequest::SourceFile(source), None)
        .expect("canonical source-file print");

    assert_eq!(printed.text(), text);
}

#[test]
fn canonical_multiline_comments_preserve_empty_lines() {
    let text = "/*\n\nparagraph one\n\nparagraph two\n*/\nconst value = 1;\n";
    let (mut result, source) = transformed(text);
    let mut printer = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    );
    let printed = printer
        .print(&mut result, PrintRequest::SourceFile(source), None)
        .expect("canonical source-file print");

    assert_eq!(printed.text(), text);
}

#[test]
fn canonical_leading_block_comment_separates_the_following_token() {
    let text = "function f(value) { }\nf(/*\n    */() => { });\n";
    let (mut result, source) = transformed(text);
    let mut printer = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    );
    let printed = printer
        .print(&mut result, PrintRequest::SourceFile(source), None)
        .expect("canonical source-file print");

    assert_eq!(
        printed.text(),
        "function f(value) { }\nf(/*\n    */ () => { });\n"
    );
}

#[test]
fn canonical_call_list_keeps_comments_before_the_closing_delimiter() {
    let text = "function f(value) { }\nf(() => {\n    // inside\n}\n// after argument\n);\n";
    let (mut result, source) = transformed(text);
    let mut printer = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    );
    let printed = printer
        .print(&mut result, PrintRequest::SourceFile(source), None)
        .expect("canonical source-file print");

    assert_eq!(printed.text(), text);
}

#[test]
fn canonical_call_arguments_keep_line_comment_spacing_without_indent_drift() {
    let text = concat!(
        "f(a, b, c, // between arguments\n",
        "d // after final argument\n",
        ");\n",
    );
    let (mut result, source) = transformed(text);
    let mut printer = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    );
    let printed = printer
        .print(&mut result, PrintRequest::SourceFile(source), None)
        .expect("canonical source-file print");

    assert_eq!(printed.text(), text);
}

#[test]
fn canonical_binding_patterns_emit_omitted_expressions_as_empty_slots() {
    let text = "let [, b, , a] = results;\nfunction f([, a, , b]) { }\n";
    let (mut result, source) = transformed(text);
    let mut printer = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    );
    let printed = printer
        .print(&mut result, PrintRequest::SourceFile(source), None)
        .expect("canonical source-file print");

    assert_eq!(printed.text(), text);
}

#[test]
fn canonical_template_spans_do_not_preserve_incidental_source_spaces() {
    let text = "f `\\x0D${ \"Interrupted CRLF\" }\\x0A`;\n";
    let (mut result, source) = transformed(text);
    let mut printer = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    );
    let printed = printer
        .print(&mut result, PrintRequest::SourceFile(source), None)
        .expect("canonical template expression print");

    assert_eq!(printed.text(), "f `\\x0D${\"Interrupted CRLF\"}\\x0A`;\n");
}

#[test]
fn canonical_source_file_mode_emits_deferred_import_phase() {
    let (mut result, source) =
        transformed("import defer * as namespace from \"./dependency.js\";\nnamespace.run();\n");
    let mut printer = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    );
    let printed = printer
        .print(&mut result, PrintRequest::SourceFile(source), None)
        .expect("deferred import print");

    assert_eq!(
        printed.text(),
        "import defer * as namespace from \"./dependency.js\";\nnamespace.run();\n"
    );
}

#[test]
fn recording_absent_uses_the_same_pipeline_and_dormant_roots_fail_typed() {
    let text = "export const value = 1;\n";
    let (mut result, source) = transformed(text);
    let mut printer = create_printer(PrinterOptions::default());
    assert_eq!(
        printer
            .print(&mut result, PrintRequest::SourceFile(source), None)
            .unwrap()
            .text(),
        text
    );

    let root = result.arena().root(source).unwrap();
    let statements = match &result.arena().node(root).unwrap().data {
        NodeData::SourceFile(data) => result
            .arena()
            .node_array_ref(source, data.statements.unwrap())
            .unwrap(),
        _ => unreachable!(),
    };
    assert_eq!(
        printer
            .print(
                &mut result,
                PrintRequest::StandaloneNode {
                    node: root,
                    writer: StandaloneWriter::MultiLine,
                },
                None,
            )
            .unwrap()
            .text(),
        text
    );
    assert_eq!(
        printer.print(&mut result, PrintRequest::JavaScriptMap(source), None),
        Err(PrinterError::Unsupported(
            UnsupportedEmitFeature::JavaScriptMap
        ))
    );
    assert_eq!(
        printer.print(&mut result, PrintRequest::NodeList(statements), None),
        Err(PrinterError::Unsupported(
            UnsupportedEmitFeature::NodeListPrinting
        ))
    );
    assert_eq!(
        printer.print(
            &mut result,
            PrintRequest::Bundle(TransformBundle::new(vec![source])),
            None
        ),
        Err(PrinterError::Unsupported(
            UnsupportedEmitFeature::BundleRoot
        ))
    );
    assert_eq!(
        printer
            .print(&mut result, PrintRequest::Declaration(source), None)
            .unwrap()
            .text(),
        text
    );
}
