use super::*;

fn nodes_of_kind(source: &crate::SourceFile, kind: SyntaxKind) -> Vec<NodeId> {
    (0..source.arena.len() as u32)
        .map(NodeId)
        .filter(|&id| source.arena.node(id).kind == kind)
        .collect()
}

fn parse_with_target(text: &str, target: ScriptTarget) -> SourceFile {
    parse_source_file(
        "a.ts".to_owned(),
        text.to_owned(),
        ParseOptions {
            script_target: target,
            ..ParseOptions::default()
        },
        None,
    )
}

#[test]
fn source_file_stores_default_explicit_and_json_language_versions() {
    let default_source = parse_source_file(
        "a.ts".to_owned(),
        String::new(),
        ParseOptions::default(),
        None,
    );
    assert_eq!(default_source.language_version, ScriptTarget::ES2025);

    let es5_source = parse_with_target("", ScriptTarget::ES5);
    assert_eq!(es5_source.language_version, ScriptTarget::ES5);

    let json_source = parse_json_text("a.json".to_owned(), "{}".to_owned());
    assert_eq!(json_source.language_version, ScriptTarget::ES2015);
}

#[test]
fn amd_pragmas_are_source_owned_and_duplicate_module_names_report_exactly() {
    let source = parse_with_target(
        concat!(
            "/// <amd-module name=\"first\" />\n",
            "/// <amd-dependency path=\"dep-a\" />\n",
            "/// <amd-dependency name=\"alias\" path=\"dep-b\" />\n",
            "/// <amd-module name=\"second\" />\n",
            "export {};\n",
        ),
        ScriptTarget::ES_NEXT,
    );

    assert_eq!(source.module_name.as_deref(), Some("second"));
    assert_eq!(
        source.amd_dependencies,
        [
            crate::AmdDependency {
                path: "dep-a".to_owned(),
                name: None,
            },
            crate::AmdDependency {
                path: "dep-b".to_owned(),
                name: Some("alias".to_owned()),
            },
        ]
    );
    assert_eq!(
        source
            .parse_diagnostics
            .iter()
            .map(tsc_diagnostics::Diagnostic::code)
            .collect::<Vec<_>>(),
        [2458]
    );

    let missing_required_attributes = parse_with_target(
        concat!(
            "/// <amd-module />\n",
            "/// <amd-dependency name=\"alias\" />\n",
            "export {};\n",
        ),
        ScriptTarget::ES_NEXT,
    );
    assert_eq!(missing_required_attributes.module_name, None);
    assert!(missing_required_attributes.amd_dependencies.is_empty());
}

#[test]
fn numeric_literals_retain_the_scanner_flags_used_by_the_printer() {
    let source = parse_with_target("0x10; 1e2; 1;", ScriptTarget::ES2015);
    let literals = nodes_of_kind(&source, SyntaxKind::NumericLiteral);
    assert_eq!(
        literals
            .iter()
            .map(|&literal| {
                let node = source.arena.node(literal);
                let NodeData::NumericLiteral(data) = &node.data else {
                    unreachable!()
                };
                (data.text.as_str(), node.numeric_literal_flags)
            })
            .collect::<Vec<_>>(),
        [("16", 64), ("100", 16), ("1", 0)]
    );
}

#[test]
fn literal_and_block_nodes_retain_the_printer_multiline_bit() {
    let source = parse_with_target(
        "const a = [\n1]; const b = [1]; const c = {\na: 1}; function f() {\nreturn;}",
        ScriptTarget::ES2015,
    );
    assert_eq!(
        nodes_of_kind(&source, SyntaxKind::ArrayLiteralExpression)
            .into_iter()
            .map(|node| source.arena.node(node).multi_line)
            .collect::<Vec<_>>(),
        [Some(true), Some(false)]
    );
    assert_eq!(
        nodes_of_kind(&source, SyntaxKind::ObjectLiteralExpression)
            .into_iter()
            .map(|node| source.arena.node(node).multi_line)
            .collect::<Vec<_>>(),
        [Some(true)]
    );
    assert_eq!(
        nodes_of_kind(&source, SyntaxKind::Block)
            .into_iter()
            .map(|node| source.arena.node(node).multi_line)
            .collect::<Vec<_>>(),
        [Some(true)]
    );
}

#[test]
fn throw_line_break_recovery_ends_at_the_keyword_boundary() {
    let source = parse_with_target("throw\na;", ScriptTarget::ES2015);
    let NodeData::SourceFile(root) = &source.arena.node(source.root).data else {
        unreachable!()
    };
    let statements = source
        .arena
        .node_array(root.statements.expect("source statements"));
    assert_eq!(statements.nodes.len(), 2);

    let throw_statement = source.arena.node(statements.nodes[0]);
    let NodeData::ThrowStatement(throw_data) = &throw_statement.data else {
        unreachable!()
    };
    let expression = source
        .arena
        .node(throw_data.expression.expect("throw recovery expression"));
    let NodeData::Identifier(identifier) = &expression.data else {
        unreachable!()
    };
    assert!(identifier.text.is_empty());
    assert_eq!((expression.pos, expression.end), (5, 5));
    assert_eq!((throw_statement.pos, throw_statement.end), (0, 5));

    let following_statement = source.arena.node(statements.nodes[1]);
    assert_eq!((following_statement.pos, following_statement.end), (5, 8));
    assert!(source.parse_diagnostics.is_empty());
}

#[test]
fn regex_flag_extent_is_target_aware() {
    let es5 = parse_with_target("/a/\u{08a1};", ScriptTarget::ES5);
    let es2015 = parse_with_target("/a/\u{08a1};", ScriptTarget::ES2015);

    let es5_regex = nodes_of_kind(&es5, SyntaxKind::RegularExpressionLiteral);
    let es2015_regex = nodes_of_kind(&es2015, SyntaxKind::RegularExpressionLiteral);
    let NodeData::RegularExpressionLiteral(es5_data) = &es5.arena.node(es5_regex[0]).data else {
        unreachable!()
    };
    let NodeData::RegularExpressionLiteral(es2015_data) = &es2015.arena.node(es2015_regex[0]).data
    else {
        unreachable!()
    };

    assert_eq!(es5_data.text, "/a/");
    assert_eq!(es5.arena.node(es5_regex[0]).end, 3);
    assert_eq!(es2015_data.text, "/a/\u{08a1}");
    assert_eq!(
        es2015.arena.node(es2015_regex[0]).end,
        "/a/\u{08a1}".len() as u32
    );
    assert_eq!(
        es5.parse_diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code(), diagnostic.start, diagnostic.length))
            .collect::<Vec<_>>(),
        vec![(gen::Invalid_character.code, Some(3), Some(1))]
    );
    assert!(es2015.parse_diagnostics.is_empty());
}

#[test]
fn invalid_identifier_rescan_preserves_target_rejected_property_name() {
    let source = parse_with_target("foo.\u{08a1};", ScriptTarget::ES5);
    let recovered = nodes_of_kind(&source, SyntaxKind::Identifier)
        .into_iter()
        .find(|&node| {
            source
                .arena
                .node(node)
                .data
                .as_identifier()
                .is_some_and(|data| data.text == "\u{08a1}")
        })
        .expect("ESNext retry preserves the property identifier");
    let recovered = source.arena.node(recovered);

    assert_eq!((recovered.pos, recovered.end), (4, 7));
    assert!(is_identifier_text("\u{08a1}"));
    assert!(!is_identifier_text_for_target(
        "\u{08a1}",
        ScriptTarget::ES5
    ));
    assert!(is_identifier_text_for_target(
        "\u{08a1}",
        ScriptTarget::ES2015
    ));
    assert_eq!(
        source
            .parse_diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code(), diagnostic.start, diagnostic.length))
            .collect::<Vec<_>>(),
        vec![(gen::Invalid_character.code, Some(4), Some(1))]
    );
}

#[test]
fn regex_literal_stores_only_true_unterminated_state() {
    let terminated = parse_with_target("/a/;", ScriptTarget::ES5);
    let unterminated = parse_with_target("/a", ScriptTarget::ES5);
    let terminated_regex = nodes_of_kind(&terminated, SyntaxKind::RegularExpressionLiteral)[0];
    let unterminated_regex = nodes_of_kind(&unterminated, SyntaxKind::RegularExpressionLiteral)[0];
    let NodeData::RegularExpressionLiteral(terminated_data) =
        &terminated.arena.node(terminated_regex).data
    else {
        unreachable!()
    };
    let NodeData::RegularExpressionLiteral(unterminated_data) =
        &unterminated.arena.node(unterminated_regex).data
    else {
        unreachable!()
    };

    assert_eq!(terminated_data.is_unterminated, None);
    assert_eq!(unterminated_data.is_unterminated, Some(true));
    assert_eq!(
        unterminated.parse_diagnostics[0].code(),
        gen::Unterminated_regular_expression_literal.code
    );
}

/// tsc parseHeritageClause (34277): the clause token is stored on
/// the node (createHeritageClause 24106), extends vs implements.
#[test]
fn heritage_clause_stores_its_token() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "class A extends B implements I {}\n".to_owned(),
        ParseOptions::default(),
        None,
    );
    let tokens: Vec<SyntaxKind> = nodes_of_kind(&source, SyntaxKind::HeritageClause)
        .into_iter()
        .map(|id| match &source.arena.node(id).data {
            NodeData::HeritageClause(data) => data.token,
            _ => unreachable!(),
        })
        .collect();
    assert_eq!(
        tokens,
        vec![SyntaxKind::ExtendsKeyword, SyntaxKind::ImplementsKeyword]
    );
}

/// tsc createImportClause (23491): phaseModifier is the storage,
/// isTypeOnly is derived (true exactly for the TypeKeyword phase);
/// `import defer` is a value import with the DeferKeyword phase.
#[test]
fn import_clause_stores_phase_modifier_and_derives_is_type_only() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "import type { A } from \"m\";\n\
         import defer * as ns from \"n\";\n\
         import x from \"o\";\n"
            .to_owned(),
        ParseOptions::default(),
        None,
    );
    let clauses: Vec<(Option<SyntaxKind>, bool)> = nodes_of_kind(&source, SyntaxKind::ImportClause)
        .into_iter()
        .map(|id| match &source.arena.node(id).data {
            NodeData::ImportClause(data) => (data.phase_modifier, data.is_type_only),
            _ => unreachable!(),
        })
        .collect();
    assert_eq!(
        clauses,
        vec![
            (Some(SyntaxKind::TypeKeyword), true),
            (Some(SyntaxKind::DeferKeyword), false),
            (None, false),
        ]
    );
}

/// tsc getTemplateLiteralRawText (30644): every template fragment
/// stores the raw source slice alongside the cooked text — escapes
/// stay escaped, CRLF stays CRLF.
#[test]
fn template_literals_store_raw_text() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "const a = `\\n`;\nconst b = `x\r\ny`;\nconst c = `h${1}m${2}t`;\n".to_owned(),
        ParseOptions::default(),
        None,
    );
    let fragments = |kind: SyntaxKind| -> Vec<(String, Option<String>)> {
        nodes_of_kind(&source, kind)
            .into_iter()
            .map(|id| match &source.arena.node(id).data {
                NodeData::NoSubstitutionTemplateLiteral(data) => {
                    (data.text.clone(), data.raw_text.clone())
                }
                NodeData::TemplateHead(data) => (data.text.clone(), data.raw_text.clone()),
                NodeData::TemplateMiddle(data) => (data.text.clone(), data.raw_text.clone()),
                NodeData::TemplateTail(data) => (data.text.clone(), data.raw_text.clone()),
                _ => unreachable!(),
            })
            .collect()
    };
    assert_eq!(
        fragments(SyntaxKind::NoSubstitutionTemplateLiteral),
        vec![
            ("\n".to_owned(), Some("\\n".to_owned())),
            ("x\ny".to_owned(), Some("x\r\ny".to_owned())),
        ]
    );
    assert_eq!(
        fragments(SyntaxKind::TemplateHead),
        vec![("h".to_owned(), Some("h".to_owned()))]
    );
    assert_eq!(
        fragments(SyntaxKind::TemplateMiddle),
        vec![("m".to_owned(), Some("m".to_owned()))]
    );
    assert_eq!(
        fragments(SyntaxKind::TemplateTail),
        vec![("t".to_owned(), Some("t".to_owned()))]
    );
}

/// tsc getTemplateLiteralRawText: an unterminated literal keeps its
/// tail (nothing stripped past the opening delimiter).
#[test]
fn unterminated_template_raw_text_keeps_tail() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "`ab".to_owned(),
        ParseOptions::default(),
        None,
    );
    let raws: Vec<(String, Option<String>)> =
        nodes_of_kind(&source, SyntaxKind::NoSubstitutionTemplateLiteral)
            .into_iter()
            .map(|id| match &source.arena.node(id).data {
                NodeData::NoSubstitutionTemplateLiteral(data) => {
                    (data.text.clone(), data.raw_text.clone())
                }
                _ => unreachable!(),
            })
            .collect();
    assert_eq!(raws, vec![("ab".to_owned(), Some("ab".to_owned()))]);
}

/// tsc createMetaProperty (23009): keywordToken disambiguates
/// `import.meta` from `new.target` without re-reading source text.
#[test]
fn meta_property_stores_its_keyword_token() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "const u = import.meta.url;\nfunction f() { return new.target; }\n".to_owned(),
        ParseOptions::default(),
        None,
    );
    let tokens: Vec<SyntaxKind> = nodes_of_kind(&source, SyntaxKind::MetaProperty)
        .into_iter()
        .map(|id| match &source.arena.node(id).data {
            NodeData::MetaProperty(data) => data.keyword_token,
            _ => unreachable!(),
        })
        .collect();
    assert_eq!(
        tokens,
        vec![SyntaxKind::ImportKeyword, SyntaxKind::NewKeyword]
    );
}

/// tsc parseImportType (31291): isTypeOf records the leading
/// `typeof` (createImportTypeNode 22332).
#[test]
fn import_type_stores_is_type_of() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "type T = typeof import(\"m\");\ntype U = import(\"m\").X;\n".to_owned(),
        ParseOptions::default(),
        None,
    );
    let flags: Vec<bool> = nodes_of_kind(&source, SyntaxKind::ImportType)
        .into_iter()
        .map(|id| match &source.arena.node(id).data {
            NodeData::ImportType(data) => data.is_type_of,
            _ => unreachable!(),
        })
        .collect();
    assert_eq!(flags, vec![true, false]);
}

/// tsc parseJsxText (32400): the flag is the scanner's
/// JsxTextAllWhiteSpaces verdict — line-break-carrying all-whitespace
/// text only; inline-only whitespace stays a semantic child.
#[test]
fn jsx_text_stores_contains_only_trivia_white_spaces() {
    let source = parse_source_file(
        "a.tsx".to_owned(),
        "const x = <div>\n  <span> hi </span>\n</div>;\n".to_owned(),
        ParseOptions {
            language_variant: crate::LanguageVariant::Jsx,
            ..ParseOptions::default()
        },
        None,
    );
    let flags: Vec<(String, bool)> = nodes_of_kind(&source, SyntaxKind::JsxText)
        .into_iter()
        .map(|id| match &source.arena.node(id).data {
            NodeData::JsxText(data) => (data.text.clone(), data.contains_only_trivia_white_spaces),
            _ => unreachable!(),
        })
        .collect();
    assert_eq!(
        flags,
        vec![
            ("\n  ".to_owned(), true),
            (" hi ".to_owned(), false),
            ("\n".to_owned(), true),
        ]
    );
}

/// tsc isDeclarationFileName (36180): the `.d.` probe runs on the
/// BASENAME under both separators — a `.d.` in a directory name
/// must not mark the file ambient (PR #5 review find).
#[test]
fn declaration_file_name_probe_uses_the_basename_on_both_separators() {
    let is_decl = |name: &str| {
        parse_source_file(
            name.to_owned(),
            String::new(),
            ParseOptions::default(),
            None,
        )
        .is_declaration_file
    };
    assert!(is_decl("a.d.ts"));
    assert!(is_decl("pkg/index.d.cts"));
    assert!(is_decl("pkg/component.d.html.ts"));
    assert!(!is_decl("C:\\types.d.cache\\index.ts"));
    assert!(!is_decl("types.d.cache/index.ts"));
}

/// tsc sourceFlags (29208) reach the SourceFile root via
/// createSourceFile2 (29214): a declaration file's root carries
/// Ambient (the binder's setExportContextFlag reads it), a JS
/// file's root and children carry JavaScriptFile.
#[test]
fn source_flags_stamp_the_source_file_root() {
    let dts = parse_source_file(
        "a.d.ts".to_owned(),
        "declare const x: 0;\n".to_owned(),
        ParseOptions::default(),
        None,
    );
    assert!(NodeFlags::from_bits(dts.arena.node(dts.root).flags).intersects(NodeFlags::AMBIENT));
    let ts = parse_source_file(
        "a.ts".to_owned(),
        "const x = 1;\n".to_owned(),
        ParseOptions::default(),
        None,
    );
    assert_eq!(ts.arena.node(ts.root).flags, NodeFlags::NONE.bits());

    let js = parse_source_file(
        "a.js".to_owned(),
        "var x = 1;\n".to_owned(),
        ParseOptions {
            javascript_file: true,
            ..ParseOptions::default()
        },
        None,
    );
    assert!(
        NodeFlags::from_bits(js.arena.node(js.root).flags).intersects(NodeFlags::JAVA_SCRIPT_FILE)
    );
    let statement = (0..js.arena.len() as u32)
        .map(NodeId)
        .find(|&id| js.arena.node(id).kind == SyntaxKind::VariableStatement)
        .expect("statement");
    assert!(NodeFlags::from_bits(js.arena.node(statement).flags)
        .intersects(NodeFlags::JAVA_SCRIPT_FILE));
}

#[test]
fn forced_external_module_uses_the_root_indicator_and_reparses_await() {
    let source = parse_source_file(
        "a.mts".to_owned(),
        "await work();\n".to_owned(),
        ParseOptions {
            force_external_module: true,
            ..ParseOptions::default()
        },
        None,
    );
    assert_eq!(source.external_module_indicator, Some(source.root));
    assert!(
        source
            .arena
            .nodes()
            .iter()
            .any(|node| node.kind == SyntaxKind::AwaitExpression),
        "forced modules must take createSourceFile2's top-level-await reparse"
    );
}

#[test]
fn automatic_jsx_module_detection_uses_the_jsx_tag() {
    let source = parse_source_file(
        "a.tsx".to_owned(),
        "const element = <div />;\n".to_owned(),
        ParseOptions {
            language_variant: LanguageVariant::Jsx,
            detect_external_module_from_jsx: true,
            ..ParseOptions::default()
        },
        None,
    );
    let indicator = source
        .external_module_indicator
        .expect("React JSX tags make the file a module in Auto mode");
    assert_eq!(
        source.arena.node(indicator).kind,
        SyntaxKind::JsxSelfClosingElement
    );
}

/// tsc accumulates sourceFlags during the parse and they reach the
/// root: dynamic import (parseLeftHandSideExpressionOrHigher 32285,
/// parseImportType 31292), import.meta (32296), and `import.defer`
/// only when immediately called (32291-32294).
#[test]
fn dynamic_import_and_import_meta_reach_root_source_flags() {
    let root_flags = |text: &str| {
        let file = parse_source_file(
            "a.ts".to_owned(),
            text.to_owned(),
            ParseOptions::default(),
            None,
        );
        NodeFlags::from_bits(file.arena.node(file.root).flags)
    };
    let either =
        NodeFlags::POSSIBLY_CONTAINS_DYNAMIC_IMPORT | NodeFlags::POSSIBLY_CONTAINS_IMPORT_META;

    assert!(!root_flags("const x = 1;\n").intersects(either));

    let dynamic = root_flags("const p = import(\"m\");\n");
    assert!(dynamic.intersects(NodeFlags::POSSIBLY_CONTAINS_DYNAMIC_IMPORT));
    assert!(!dynamic.intersects(NodeFlags::POSSIBLY_CONTAINS_IMPORT_META));

    let import_type = root_flags("type T = import(\"m\").X;\n");
    assert!(import_type.intersects(NodeFlags::POSSIBLY_CONTAINS_DYNAMIC_IMPORT));
    assert!(!import_type.intersects(NodeFlags::POSSIBLY_CONTAINS_IMPORT_META));

    let meta = root_flags("const u = import.meta.url;\n");
    assert!(meta.intersects(NodeFlags::POSSIBLY_CONTAINS_IMPORT_META));
    assert!(!meta.intersects(NodeFlags::POSSIBLY_CONTAINS_DYNAMIC_IMPORT));

    let defer_call = root_flags("const p = import.defer(\"m\");\n");
    assert!(defer_call.intersects(NodeFlags::POSSIBLY_CONTAINS_DYNAMIC_IMPORT));
    assert!(!defer_call.intersects(NodeFlags::POSSIBLY_CONTAINS_IMPORT_META));

    // Bare `import.defer` (not called) sets neither flag.
    assert!(!root_flags("const d = import.defer;\n").intersects(either));
}

/// tsc speculationHelper (29538) restores the token, the
/// diagnostics length, and (assert-unchanged) contextFlags —
/// never sourceFlags — and parseImportType (31292) sets
/// PossiblyContainsDynamicImport before consuming a token: an
/// import type parsed inside a speculation leaks the bit even
/// when the speculation rewinds.
#[test]
fn speculation_rewind_keeps_accumulated_source_flags() {
    let text = "import(\"m\")";

    // lookAhead rewinds unconditionally.
    let mut parser = Parser::new("a.ts".to_owned(), text, LanguageVariant::Standard, false);
    parser.next_token();
    parser.look_ahead(|parser| parser.parse_import_type());
    assert_eq!(parser.token(), SyntaxKind::ImportKeyword);
    assert!(parser
        .source_flags
        .intersects(NodeFlags::POSSIBLY_CONTAINS_DYNAMIC_IMPORT));

    // tryParse rewinds on a falsy result (the
    // tryParseConstraintOfInferType shape: parse, then give the
    // parse up).
    let mut parser = Parser::new("a.ts".to_owned(), text, LanguageVariant::Standard, false);
    parser.next_token();
    let rewound = parser.try_parse(|parser| {
        parser.parse_import_type();
        Option::<NodeId>::None
    });
    assert_eq!(rewound, None);
    assert_eq!(parser.token(), SyntaxKind::ImportKeyword);
    assert!(parser
        .source_flags
        .intersects(NodeFlags::POSSIBLY_CONTAINS_DYNAMIC_IMPORT));
}

fn parse_tsx(text: &str) -> SourceFile {
    parse_source_file(
        "a.tsx".to_owned(),
        text.to_owned(),
        ParseOptions {
            language_variant: LanguageVariant::Jsx,
            javascript_file: false,
            ..ParseOptions::default()
        },
        None,
    )
}

fn first_initializer(source: &SourceFile) -> NodeId {
    let root = source
        .arena
        .node(source.root)
        .data
        .as_source_file()
        .expect("source file root");
    let statements = source
        .arena
        .node_array(root.statements.expect("statements"));
    let statement = source
        .arena
        .node(statements.nodes[0])
        .data
        .as_variable_statement()
        .expect("variable statement");
    let list = source
        .arena
        .node(statement.declaration_list.expect("declaration list"))
        .data
        .as_variable_declaration_list()
        .expect("declaration list data")
        .declarations
        .expect("declarations");
    let declaration = source
        .arena
        .node(source.arena.node_array(list).nodes[0])
        .data
        .as_variable_declaration()
        .expect("variable declaration");
    declaration.initializer.expect("initializer")
}

fn diagnostic_pins(source: &SourceFile) -> Vec<(u32, Option<u32>, Option<u32>)> {
    source
        .parse_diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code(), diagnostic.start, diagnostic.length))
        .collect()
}

#[test]
fn jsx_element_attributes_and_children_oracle_pins() {
    let source = parse_tsx("const a = <div className=\"x\" {...props}>hello{world}</div>;");
    assert!(
        source.parse_diagnostics.is_empty(),
        "{:?}",
        source.parse_diagnostics
    );

    let element = source
        .arena
        .node(first_initializer(&source))
        .data
        .as_jsx_element()
        .expect("jsx element");
    let opening = source
        .arena
        .node(element.opening_element.expect("opening"))
        .data
        .as_jsx_opening_element()
        .expect("opening element");
    let attributes = source
        .arena
        .node(opening.attributes.expect("attributes"))
        .data
        .as_jsx_attributes()
        .expect("attributes data")
        .properties
        .expect("properties");
    let attribute_kinds: Vec<_> = source
        .arena
        .node_array(attributes)
        .nodes
        .iter()
        .map(|id| source.arena.node(*id).kind)
        .collect();
    assert_eq!(
        attribute_kinds,
        [SyntaxKind::JsxAttribute, SyntaxKind::JsxSpreadAttribute]
    );

    let child_kinds: Vec<_> = source
        .arena
        .node_array(element.children.expect("children"))
        .nodes
        .iter()
        .map(|id| source.arena.node(*id).kind)
        .collect();
    assert_eq!(
        child_kinds,
        [SyntaxKind::JsxText, SyntaxKind::JsxExpression]
    );
}

#[test]
fn jsx_fragment_oracle_pins() {
    let source = parse_tsx("const b = <>text{1}<br/></>;");
    assert!(
        source.parse_diagnostics.is_empty(),
        "{:?}",
        source.parse_diagnostics
    );

    let fragment = source
        .arena
        .node(first_initializer(&source))
        .data
        .as_jsx_fragment()
        .expect("jsx fragment");
    assert_eq!(
        source
            .arena
            .node(fragment.opening_fragment.expect("opening fragment"))
            .kind,
        SyntaxKind::JsxOpeningFragment
    );
    assert_eq!(
        source
            .arena
            .node(fragment.closing_fragment.expect("closing fragment"))
            .kind,
        SyntaxKind::JsxClosingFragment
    );
    let child_kinds: Vec<_> = source
        .arena
        .node_array(fragment.children.expect("children"))
        .nodes
        .iter()
        .map(|id| source.arena.node(*id).kind)
        .collect();
    assert_eq!(
        child_kinds,
        [
            SyntaxKind::JsxText,
            SyntaxKind::JsxExpression,
            SyntaxKind::JsxSelfClosingElement
        ]
    );
}

#[test]
fn jsx_tag_and_attribute_name_shapes() {
    let source = parse_tsx("const c = <Foo.Bar a:b=\"1\" this-prop={2} />;");
    assert!(
        source.parse_diagnostics.is_empty(),
        "{:?}",
        source.parse_diagnostics
    );

    let element = source
        .arena
        .node(first_initializer(&source))
        .data
        .as_jsx_self_closing_element()
        .expect("self-closing element");
    assert_eq!(
        source.arena.node(element.tag_name.expect("tag name")).kind,
        SyntaxKind::PropertyAccessExpression
    );
    let attributes = source
        .arena
        .node(element.attributes.expect("attributes"))
        .data
        .as_jsx_attributes()
        .expect("attributes data")
        .properties
        .expect("properties");
    let attribute_nodes = &source.arena.node_array(attributes).nodes;
    let namespaced = source
        .arena
        .node(attribute_nodes[0])
        .data
        .as_jsx_attribute()
        .expect("first attribute");
    assert_eq!(
        source.arena.node(namespaced.name.expect("name")).kind,
        SyntaxKind::JsxNamespacedName
    );
    let dashed = source
        .arena
        .node(attribute_nodes[1])
        .data
        .as_jsx_attribute()
        .expect("second attribute");
    let dashed_name = source
        .arena
        .node(dashed.name.expect("name"))
        .data
        .as_identifier()
        .expect("identifier name");
    assert_eq!(dashed_name.escaped_text, "this-prop");
}

#[test]
fn jsx_this_tag_name() {
    let source = parse_tsx("function g() { return <this.Component />; }");
    assert!(
        source.parse_diagnostics.is_empty(),
        "{:?}",
        source.parse_diagnostics
    );
}

#[test]
fn jsx_closing_tag_mismatch_oracle_pins() {
    let source = parse_tsx("const d = <div></span>;");
    assert_eq!(diagnostic_pins(&source), [(17002, Some(17), Some(4))]);
}

#[test]
fn jsx_sibling_elements_glued_with_synthetic_comma() {
    let source = parse_tsx("const e = <div/><span/>;");
    assert_eq!(diagnostic_pins(&source), [(2657, Some(10), Some(13))]);
    assert_eq!(
        source.arena.node(first_initializer(&source)).kind,
        SyntaxKind::BinaryExpression
    );
}

#[test]
fn jsx_rebalances_closing_tag_owned_by_outer_element() {
    let source = parse_tsx("const f = <div><b>text</div>;");
    assert_eq!(diagnostic_pins(&source), [(17008, Some(16), Some(1))]);

    let outer = source
        .arena
        .node(first_initializer(&source))
        .data
        .as_jsx_element()
        .expect("outer element");
    let children = source.arena.node_array(outer.children.expect("children"));
    let inner = source
        .arena
        .node(*children.nodes.last().expect("inner child"))
        .data
        .as_jsx_element()
        .expect("inner element");
    let synthetic_closing = source
        .arena
        .node(inner.closing_element.expect("synthetic closing"));
    assert_eq!(synthetic_closing.pos, synthetic_closing.end);
}

#[test]
fn jsx_unclosed_element_at_eof_oracle_pins() {
    let source = parse_tsx("const h = <div>");
    assert_eq!(
        diagnostic_pins(&source),
        [(17008, Some(11), Some(3)), (1005, Some(15), Some(0))]
    );
}

#[test]
fn for_of_expression_initializer_stays_an_expression() {
    // tsc: `for (x of a)` initializer is an Identifier, not a
    // VariableDeclarationList (the using-declaration lookahead must not
    // fire on `x of`).
    let source = parse_source_file(
        "a.ts".to_owned(),
        "declare var x: string, a: string[]; for (x of a) { }".to_owned(),
        ParseOptions::default(),
        None,
    );
    assert!(
        source.parse_diagnostics.is_empty(),
        "{:?}",
        source.parse_diagnostics
    );
    let root = source
        .arena
        .node(source.root)
        .data
        .as_source_file()
        .expect("source file root");
    let statements = source
        .arena
        .node_array(root.statements.expect("statements"));
    let for_of = source
        .arena
        .node(statements.nodes[1])
        .data
        .as_for_of_statement()
        .expect("for-of statement");
    assert_eq!(
        source
            .arena
            .node(for_of.initializer.expect("initializer"))
            .kind,
        SyntaxKind::Identifier
    );
}

#[test]
fn using_with_bracket_is_an_expression_statement() {
    // tsc: `using [a] = null` is element-access assignment, not a using
    // declaration (the lookahead accepts identifiers and `{` only).
    let source = parse_source_file(
        "a.ts".to_owned(),
        "declare var using: any[], a: number; function f() { using [a] = null; }".to_owned(),
        ParseOptions::default(),
        None,
    );
    assert!(
        source.parse_diagnostics.is_empty(),
        "{:?}",
        source.parse_diagnostics
    );
    let root = source
        .arena
        .node(source.root)
        .data
        .as_source_file()
        .expect("source file root");
    let statements = source
        .arena
        .node_array(root.statements.expect("statements"));
    let function = source
        .arena
        .node(statements.nodes[1])
        .data
        .as_function_declaration()
        .expect("function declaration");
    let body = source
        .arena
        .node(function.body.expect("body"))
        .data
        .as_block()
        .expect("function body");
    let body_statements = source
        .arena
        .node_array(body.statements.expect("body statements"));
    assert_eq!(
        source.arena.node(body_statements.nodes[0]).kind,
        SyntaxKind::ExpressionStatement
    );
}

#[test]
fn export_before_bare_identifier_reports_declaration_expected() {
    // tsc: `export i` is not a statement start (isStartOfStatement
    // ExportKeyword arm → isStartOfDeclaration false), so the list
    // machinery reports 1128 at `export`, not 1434.
    let source = parse_source_file(
        "a.ts".to_owned(),
        "declare module \"*.foo\" {\n  export i\n".to_owned(),
        ParseOptions::default(),
        None,
    );
    let pins: Vec<(u32, u32, u32)> = source
        .parse_diagnostics
        .iter()
        .map(|d| (d.code(), d.start.unwrap_or(0), d.length.unwrap_or(0)))
        .collect();
    assert_eq!(pins, [(1128, 27, 6), (1005, 36, 0)]);
}

#[test]
fn binding_patterns_support_computed_names_and_nesting() {
    // tsc parses both clean: computed property names in object binding
    // elements, nested patterns in array binding elements.
    let source = parse_source_file(
        "a.ts".to_owned(),
        "declare var f: any; let [{ [f(1)]: x } = f(0)] = []; let [[a], { b: [c] }] = f;"
            .to_owned(),
        ParseOptions::default(),
        None,
    );
    assert!(
        source.parse_diagnostics.is_empty(),
        "{:?}",
        source.parse_diagnostics
    );
}

#[test]
fn parse_json_text_oracle_pins() {
    // Pins collected from ts.parseJsonText (vendor 6.0.3).
    type JsonPin = (&'static str, &'static [(u32, u32, u32)], Option<SyntaxKind>);
    let cases: &[JsonPin] = &[
        (
            "{ \"name\": \"p\", \"exports\": { \".\": \"./i.js\" } }",
            &[],
            Some(SyntaxKind::ObjectLiteralExpression),
        ),
        ("-5", &[], Some(SyntaxKind::PrefixUnaryExpression)),
        (
            "1 2",
            &[(1012, 2, 1)],
            Some(SyntaxKind::ArrayLiteralExpression),
        ),
        ("", &[], None),
        ("\"hello\"", &[], Some(SyntaxKind::StringLiteral)),
        (
            "[1, true, null]",
            &[],
            Some(SyntaxKind::ArrayLiteralExpression),
        ),
        // Unquoted keys and trailing commas are checker errors, not
        // parse errors.
        (
            "{ name: 1, }",
            &[],
            Some(SyntaxKind::ObjectLiteralExpression),
        ),
    ];
    for (text, diagnostics, expression_kind) in cases {
        let source = parse_json_text("a.json".to_owned(), (*text).to_owned());
        let pins: Vec<(u32, u32, u32)> = source
            .parse_diagnostics
            .iter()
            .map(|d| (d.code(), d.start.unwrap_or(0), d.length.unwrap_or(0)))
            .collect();
        assert_eq!(&pins, diagnostics, "diagnostics for {text:?}");

        let root = source
            .arena
            .node(source.root)
            .data
            .as_source_file()
            .expect("source file root");
        let statements = source
            .arena
            .node_array(root.statements.expect("statements"));
        match expression_kind {
            None => assert!(statements.nodes.is_empty(), "statements for {text:?}"),
            Some(kind) => {
                let statement = source
                    .arena
                    .node(statements.nodes[0])
                    .data
                    .as_expression_statement()
                    .expect("expression statement");
                assert_eq!(
                    source
                        .arena
                        .node(statement.expression.expect("expression"))
                        .kind,
                    *kind,
                    "expression kind for {text:?}"
                );
            }
        }
    }
}

#[test]
fn parse_json_text_does_not_publish_source_file_pragmas() {
    let source = parse_json_text(
        "a.json".to_owned(),
        concat!(
            "/// <reference path=\"./dependency.ts\" />\n",
            "/// <reference types=\"pkg\" resolution-mode=\"invalid\" />\n",
            "/// <reference lib=\"es2023\" />\n",
            "/** @jsxRuntime automatic */\n",
            "// @ts-ignore\n",
            "{}",
        )
        .to_owned(),
    );

    assert!(source.parse_diagnostics.is_empty());
    assert!(source.referenced_files.is_empty());
    assert!(source.type_reference_directives.is_empty());
    assert!(source.lib_reference_directives.is_empty());
    assert!(!source.has_jsx_import_source_pragma);
    assert!(!source.has_jsx_runtime_pragma);
    assert!(source.comment_directives.is_empty());

    let unterminated = parse_json_text(
        "unterminated.json".to_owned(),
        "/* @jsxRuntime automatic".to_owned(),
    );
    assert!(!unterminated.has_jsx_import_source_pragma);
    assert!(!unterminated.has_jsx_runtime_pragma);
}

#[test]
fn type_assertion_still_parses_in_standard_variant() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "const e = <string>x;".to_owned(),
        ParseOptions::default(),
        None,
    );
    assert!(
        source.parse_diagnostics.is_empty(),
        "{:?}",
        source.parse_diagnostics
    );
}

#[test]
fn parse_source_file_drains_scanner_errors() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "\"unterminated".to_owned(),
        ParseOptions::default(),
        None,
    );

    assert_eq!(source.parse_diagnostics.len(), 1);
    assert_eq!(source.parse_diagnostics[0].code(), 1002);
    assert_eq!(source.parse_diagnostics[0].start, Some(13));
    assert_eq!(source.parse_diagnostics[0].length, Some(0));
}

#[test]
fn parse_source_file_builds_statement_tree() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "let x = 1; const y = 2; if (x) { debugger; }".to_owned(),
        ParseOptions::default(),
        None,
    );

    assert!(source.parse_diagnostics.is_empty());
    let root = source
        .arena
        .node(source.root)
        .data
        .as_source_file()
        .expect("source file root");
    let statements = source
        .arena
        .node_array(root.statements.expect("statements"));
    assert_eq!(statements.nodes.len(), 3);

    let variable_statement = source.arena.node(statements.nodes[0]);
    let NodeData::VariableStatement(variable_statement_data) = &variable_statement.data else {
        panic!("expected variable statement");
    };
    let declaration_list = variable_statement_data
        .declaration_list
        .expect("declaration list");
    assert!(
        NodeFlags::from_bits(source.arena.node(declaration_list).flags).contains(NodeFlags::LET)
    );
    let declaration_list_data = source
        .arena
        .node(declaration_list)
        .data
        .as_variable_declaration_list()
        .expect("variable declaration list");
    let declarations = source
        .arena
        .node_array(declaration_list_data.declarations.expect("declarations"));
    assert_eq!(declarations.nodes.len(), 1);
    let declaration = source
        .arena
        .node(declarations.nodes[0])
        .data
        .as_variable_declaration()
        .expect("variable declaration");
    assert_eq!(
        source.arena.node(declaration.name.expect("name")).kind,
        SyntaxKind::Identifier
    );
    assert_eq!(
        source
            .arena
            .node(declaration.initializer.expect("initializer"))
            .kind,
        SyntaxKind::NumericLiteral
    );

    let const_statement = source.arena.node(statements.nodes[1]);
    let NodeData::VariableStatement(const_statement_data) = &const_statement.data else {
        panic!("expected const variable statement");
    };
    let const_declaration_list = const_statement_data
        .declaration_list
        .expect("const declaration list");
    assert!(
        NodeFlags::from_bits(source.arena.node(const_declaration_list).flags)
            .contains(NodeFlags::CONST)
    );

    let if_statement = source
        .arena
        .node(statements.nodes[2])
        .data
        .as_if_statement()
        .expect("if statement");
    let then_block = source
        .arena
        .node(if_statement.then_statement.expect("then statement"))
        .data
        .as_block()
        .expect("then block");
    let block_statements = source
        .arena
        .node_array(then_block.statements.expect("block statements"));
    assert_eq!(block_statements.nodes.len(), 1);
    assert_eq!(
        source.arena.node(block_statements.nodes[0]).kind,
        SyntaxKind::DebuggerStatement
    );
}

#[test]
fn parse_import_and_ambient_function_declarations() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "import {foo, baz} from \"foobarbaz\";\nfoo(baz);\ndeclare function fn7(x, y?, ...z);\ndeclare function fn9(...q: {}[]);\n".to_owned(),
        ParseOptions::default(),
        None,
    );

    assert!(
        source.parse_diagnostics.is_empty(),
        "{:?}",
        source.parse_diagnostics
    );
    let root = source
        .arena
        .node(source.root)
        .data
        .as_source_file()
        .expect("source file root");
    let statements = source
        .arena
        .node_array(root.statements.expect("statements"));
    let kinds: Vec<SyntaxKind> = statements
        .nodes
        .iter()
        .map(|&statement| source.arena.node(statement).kind)
        .collect();
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::ImportDeclaration,
            SyntaxKind::ExpressionStatement,
            SyntaxKind::FunctionDeclaration,
            SyntaxKind::FunctionDeclaration,
        ]
    );
    let NodeData::FunctionDeclaration(ambient) = &source.arena.node(statements.nodes[2]).data
    else {
        panic!("expected function declaration");
    };
    assert!(ambient.modifiers.is_some());
    assert!(ambient.body.is_none());
}

#[test]
fn parse_primary_expression_shapes() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "const arr = [1,,...x]; const obj = {a: 1, b, ...c, [d.e]: 2}; new.target; /x/g; const t = `a${b}c`;".to_owned(),
        ParseOptions::default(),
        None,
    );

    assert!(source.parse_diagnostics.is_empty());
    let root = source
        .arena
        .node(source.root)
        .data
        .as_source_file()
        .expect("source file root");
    let statements = source
        .arena
        .node_array(root.statements.expect("statements"));
    assert_eq!(statements.nodes.len(), 5);

    let initializer = |statement: NodeId| -> NodeId {
        let variable_statement = source
            .arena
            .node(statement)
            .data
            .as_variable_statement()
            .expect("variable statement");
        let declaration_list = source
            .arena
            .node(
                variable_statement
                    .declaration_list
                    .expect("declaration list"),
            )
            .data
            .as_variable_declaration_list()
            .expect("declaration list data");
        let declarations = source
            .arena
            .node_array(declaration_list.declarations.expect("declarations"));
        source
            .arena
            .node(declarations.nodes[0])
            .data
            .as_variable_declaration()
            .expect("declaration")
            .initializer
            .expect("initializer")
    };

    let arr = initializer(statements.nodes[0]);
    let arr_data = source
        .arena
        .node(arr)
        .data
        .as_array_literal_expression()
        .expect("array literal");
    let arr_elements = source
        .arena
        .node_array(arr_data.elements.expect("array elements"));
    assert_eq!(
        arr_elements
            .nodes
            .iter()
            .map(|id| source.arena.node(*id).kind)
            .collect::<Vec<_>>(),
        vec![
            SyntaxKind::NumericLiteral,
            SyntaxKind::OmittedExpression,
            SyntaxKind::SpreadElement,
        ]
    );

    let obj = initializer(statements.nodes[1]);
    let obj_data = source
        .arena
        .node(obj)
        .data
        .as_object_literal_expression()
        .expect("object literal");
    let properties = source
        .arena
        .node_array(obj_data.properties.expect("properties"));
    assert_eq!(
        properties
            .nodes
            .iter()
            .map(|id| source.arena.node(*id).kind)
            .collect::<Vec<_>>(),
        vec![
            SyntaxKind::PropertyAssignment,
            SyntaxKind::ShorthandPropertyAssignment,
            SyntaxKind::SpreadAssignment,
            SyntaxKind::PropertyAssignment,
        ]
    );
    let computed_property = source
        .arena
        .node(properties.nodes[3])
        .data
        .as_property_assignment()
        .expect("computed property assignment")
        .name
        .expect("computed name");
    assert_eq!(
        source.arena.node(computed_property).kind,
        SyntaxKind::ComputedPropertyName
    );

    let new_target = source
        .arena
        .node(statements.nodes[2])
        .data
        .as_expression_statement()
        .expect("new.target statement")
        .expression
        .expect("new.target expression");
    assert_eq!(source.arena.node(new_target).kind, SyntaxKind::MetaProperty);

    let regex = source
        .arena
        .node(statements.nodes[3])
        .data
        .as_expression_statement()
        .expect("regex statement")
        .expression
        .expect("regex expression");
    assert_eq!(
        source.arena.node(regex).kind,
        SyntaxKind::RegularExpressionLiteral
    );

    let template = initializer(statements.nodes[4]);
    assert_eq!(
        source.arena.node(template).kind,
        SyntaxKind::TemplateExpression
    );
}

#[test]
fn parse_member_and_call_expression_shapes() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "foo.bar(1, ...xs); obj?.prop?.[key]?.(arg); tag<T>`x${y}z`; new Foo<T>(arg); x!.y;"
            .to_owned(),
        ParseOptions::default(),
        None,
    );

    assert!(source.parse_diagnostics.is_empty());
    let root = source
        .arena
        .node(source.root)
        .data
        .as_source_file()
        .expect("source file root");
    let statements = source
        .arena
        .node_array(root.statements.expect("statements"));
    assert_eq!(statements.nodes.len(), 5);

    let call_statement = source
        .arena
        .node(statements.nodes[0])
        .data
        .as_expression_statement()
        .expect("call statement");
    let call = source
        .arena
        .node(call_statement.expression.expect("call expression"))
        .data
        .as_call_expression()
        .expect("call expression");
    assert_eq!(
        source.arena.node(call.expression.expect("callee")).kind,
        SyntaxKind::PropertyAccessExpression
    );
    let call_arguments = source
        .arena
        .node_array(call.arguments.expect("call arguments"));
    assert_eq!(call_arguments.nodes.len(), 2);
    assert_eq!(
        source.arena.node(call_arguments.nodes[1]).kind,
        SyntaxKind::SpreadElement
    );

    let optional_call_statement = source
        .arena
        .node(statements.nodes[1])
        .data
        .as_expression_statement()
        .expect("optional call statement");
    let optional_call = source
        .arena
        .node(optional_call_statement.expression.expect("optional call"))
        .data
        .as_call_expression()
        .expect("optional call expression");
    assert!(optional_call.question_dot_token.is_some());
    assert_eq!(
        source
            .arena
            .node(optional_call.expression.expect("optional callee"))
            .kind,
        SyntaxKind::ElementAccessExpression
    );

    let tagged_statement = source
        .arena
        .node(statements.nodes[2])
        .data
        .as_expression_statement()
        .expect("tagged template statement");
    let tagged = source
        .arena
        .node(tagged_statement.expression.expect("tagged template"))
        .data
        .as_tagged_template_expression()
        .expect("tagged template expression");
    assert!(tagged.type_arguments.is_some());
    assert_eq!(
        source.arena.node(tagged.template.expect("template")).kind,
        SyntaxKind::TemplateExpression
    );

    let new_statement = source
        .arena
        .node(statements.nodes[3])
        .data
        .as_expression_statement()
        .expect("new expression statement");
    let new_expression = source
        .arena
        .node(new_statement.expression.expect("new expression"))
        .data
        .as_new_expression()
        .expect("new expression");
    assert!(new_expression.type_arguments.is_some());
    assert_eq!(
        source
            .arena
            .node_array(new_expression.arguments.expect("new arguments"))
            .nodes
            .len(),
        1
    );

    let non_null_statement = source
        .arena
        .node(statements.nodes[4])
        .data
        .as_expression_statement()
        .expect("non-null statement");
    let property_access = source
        .arena
        .node(non_null_statement.expression.expect("property access"))
        .data
        .as_property_access_expression()
        .expect("property access expression");
    assert_eq!(
        source
            .arena
            .node(property_access.expression.expect("non-null base"))
            .kind,
        SyntaxKind::NonNullExpression
    );
}

#[test]
fn parse_unary_update_await_and_yield_shapes() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "++a; b--; delete obj.x; typeof y; void z; await q; const g = function*(){ yield; yield* q; }; const h = async function(){ await q; };".to_owned(),
        ParseOptions::default(),
        None,
    );

    assert!(source.parse_diagnostics.is_empty());
    let root = source
        .arena
        .node(source.root)
        .data
        .as_source_file()
        .expect("source file root");
    let statements = source
        .arena
        .node_array(root.statements.expect("statements"));
    assert_eq!(statements.nodes.len(), 8);

    let expression_statement_expression = |index: usize| -> NodeId {
        source
            .arena
            .node(statements.nodes[index])
            .data
            .as_expression_statement()
            .expect("expression statement")
            .expression
            .expect("expression")
    };

    assert_eq!(
        source.arena.node(expression_statement_expression(0)).kind,
        SyntaxKind::PrefixUnaryExpression
    );
    assert_eq!(
        source.arena.node(expression_statement_expression(1)).kind,
        SyntaxKind::PostfixUnaryExpression
    );
    assert_eq!(
        source.arena.node(expression_statement_expression(2)).kind,
        SyntaxKind::DeleteExpression
    );
    assert_eq!(
        source.arena.node(expression_statement_expression(3)).kind,
        SyntaxKind::TypeOfExpression
    );
    assert_eq!(
        source.arena.node(expression_statement_expression(4)).kind,
        SyntaxKind::VoidExpression
    );
    assert_eq!(
        source.arena.node(expression_statement_expression(5)).kind,
        SyntaxKind::AwaitExpression
    );

    let variable_initializer = |index: usize| -> NodeId {
        let variable_statement = source
            .arena
            .node(statements.nodes[index])
            .data
            .as_variable_statement()
            .expect("variable statement");
        let declaration_list = source
            .arena
            .node(
                variable_statement
                    .declaration_list
                    .expect("declaration list"),
            )
            .data
            .as_variable_declaration_list()
            .expect("declaration list data");
        let declarations = source
            .arena
            .node_array(declaration_list.declarations.expect("declarations"));
        source
            .arena
            .node(declarations.nodes[0])
            .data
            .as_variable_declaration()
            .expect("declaration")
            .initializer
            .expect("initializer")
    };

    let generator = source
        .arena
        .node(variable_initializer(6))
        .data
        .as_function_expression()
        .expect("generator function expression");
    let generator_body = source
        .arena
        .node(generator.body.expect("generator body"))
        .data
        .as_block()
        .expect("generator body block");
    let generator_statements = source
        .arena
        .node_array(generator_body.statements.expect("generator statements"));
    assert_eq!(generator_statements.nodes.len(), 2);
    for statement in &generator_statements.nodes {
        let expression = source
            .arena
            .node(*statement)
            .data
            .as_expression_statement()
            .expect("yield expression statement")
            .expression
            .expect("yield expression");
        assert_eq!(
            source.arena.node(expression).kind,
            SyntaxKind::YieldExpression
        );
    }

    let async_function = source
        .arena
        .node(variable_initializer(7))
        .data
        .as_function_expression()
        .expect("async function expression");
    let async_body = source
        .arena
        .node(async_function.body.expect("async body"))
        .data
        .as_block()
        .expect("async body block");
    let async_statements = source
        .arena
        .node_array(async_body.statements.expect("async statements"));
    let await_expression = source
        .arena
        .node(async_statements.nodes[0])
        .data
        .as_expression_statement()
        .expect("await expression statement")
        .expression
        .expect("await expression");
    assert_eq!(
        source.arena.node(await_expression).kind,
        SyntaxKind::AwaitExpression
    );
}

fn expression_statements(source: &SourceFile) -> Vec<NodeId> {
    let root = source
        .arena
        .node(source.root)
        .data
        .as_source_file()
        .expect("source file root");
    let statements = source
        .arena
        .node_array(root.statements.expect("statements"));
    statements
        .nodes
        .iter()
        .map(|&statement| {
            source
                .arena
                .node(statement)
                .data
                .as_expression_statement()
                .expect("expression statement")
                .expression
                .expect("expression")
        })
        .collect()
}

fn binary_parts(source: &SourceFile, id: NodeId) -> (NodeId, SyntaxKind, NodeId) {
    let binary = source
        .arena
        .node(id)
        .data
        .as_binary_expression()
        .expect("binary expression");
    (
        binary.left.expect("left"),
        source
            .arena
            .node(binary.operator_token.expect("operator token"))
            .kind,
        binary.right.expect("right"),
    )
}

#[test]
fn parse_binary_expression_precedence_shapes() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "1 + 2 * 3; 2 ** 3 ** 4; a >> b >>> c; x, y;".to_owned(),
        ParseOptions::default(),
        None,
    );

    assert!(source.parse_diagnostics.is_empty());
    let expressions = expression_statements(&source);
    assert_eq!(expressions.len(), 4);

    let (_, plus, multiply) = binary_parts(&source, expressions[0]);
    assert_eq!(plus, SyntaxKind::PlusToken);
    let (_, asterisk, _) = binary_parts(&source, multiply);
    assert_eq!(asterisk, SyntaxKind::AsteriskToken);

    let (base, outer_exponent, tower) = binary_parts(&source, expressions[1]);
    assert_eq!(outer_exponent, SyntaxKind::AsteriskAsteriskToken);
    assert_eq!(source.arena.node(base).kind, SyntaxKind::NumericLiteral);
    let (_, inner_exponent, _) = binary_parts(&source, tower);
    assert_eq!(inner_exponent, SyntaxKind::AsteriskAsteriskToken);

    let (shift, unsigned_shift, _) = binary_parts(&source, expressions[2]);
    assert_eq!(
        unsigned_shift,
        SyntaxKind::GreaterThanGreaterThanGreaterThanToken
    );
    let (_, signed_shift, _) = binary_parts(&source, shift);
    assert_eq!(signed_shift, SyntaxKind::GreaterThanGreaterThanToken);

    let (_, comma, _) = binary_parts(&source, expressions[3]);
    assert_eq!(comma, SyntaxKind::CommaToken);
}

#[test]
fn parse_relational_chain_not_type_arguments() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "a < b > c;".to_owned(),
        ParseOptions::default(),
        None,
    );

    assert!(source.parse_diagnostics.is_empty());
    let expressions = expression_statements(&source);
    let (less, greater, _) = binary_parts(&source, expressions[0]);
    assert_eq!(greater, SyntaxKind::GreaterThanToken);
    let (_, less_operator, _) = binary_parts(&source, less);
    assert_eq!(less_operator, SyntaxKind::LessThanToken);
}

#[test]
fn parse_as_satisfies_and_type_assertion() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "x as T; y satisfies U; <T>z;".to_owned(),
        ParseOptions::default(),
        None,
    );

    assert!(source.parse_diagnostics.is_empty());
    let expressions = expression_statements(&source);
    assert_eq!(
        source.arena.node(expressions[0]).kind,
        SyntaxKind::AsExpression
    );
    assert_eq!(
        source.arena.node(expressions[1]).kind,
        SyntaxKind::SatisfiesExpression
    );
    assert_eq!(
        source.arena.node(expressions[2]).kind,
        SyntaxKind::TypeAssertionExpression
    );
}

#[test]
fn as_on_new_line_breaks_binary_loop() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "x\nas;".to_owned(),
        ParseOptions::default(),
        None,
    );

    assert!(source.parse_diagnostics.is_empty());
    let expressions = expression_statements(&source);
    assert_eq!(expressions.len(), 2);
    assert_eq!(
        source.arena.node(expressions[0]).kind,
        SyntaxKind::Identifier
    );
    assert_eq!(
        source.arena.node(expressions[1]).kind,
        SyntaxKind::Identifier
    );
}

#[test]
fn unary_left_of_exponent_reports_17006_but_still_parses() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "-x ** 2;".to_owned(),
        ParseOptions::default(),
        None,
    );

    assert_eq!(source.parse_diagnostics.len(), 1);
    assert_eq!(source.parse_diagnostics[0].code(), 17006);
    assert_eq!(
        source.parse_diagnostics[0].message_text(),
        "An unary expression with the '-' operator is not allowed in the left-hand side of an exponentiation expression. Consider enclosing the expression in parentheses."
    );
    let expressions = expression_statements(&source);
    let (negated, exponent, _) = binary_parts(&source, expressions[0]);
    assert_eq!(exponent, SyntaxKind::AsteriskAsteriskToken);
    assert_eq!(
        source.arena.node(negated).kind,
        SyntaxKind::PrefixUnaryExpression
    );
}

#[test]
fn parse_assignment_right_associative_and_rescanned_operator() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "a = b = c; x >>= y;".to_owned(),
        ParseOptions::default(),
        None,
    );

    assert!(source.parse_diagnostics.is_empty());
    let expressions = expression_statements(&source);

    let (left, equals, chained) = binary_parts(&source, expressions[0]);
    assert_eq!(equals, SyntaxKind::EqualsToken);
    assert_eq!(source.arena.node(left).kind, SyntaxKind::Identifier);
    let (_, inner_equals, _) = binary_parts(&source, chained);
    assert_eq!(inner_equals, SyntaxKind::EqualsToken);

    let (_, shift_assign, _) = binary_parts(&source, expressions[1]);
    assert_eq!(shift_assign, SyntaxKind::GreaterThanGreaterThanEqualsToken);
}

#[test]
fn assignment_to_non_lhs_leaves_equals_for_outer_context() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "a + b = c;".to_owned(),
        ParseOptions::default(),
        None,
    );

    assert!(!source.parse_diagnostics.is_empty());
    let root = source
        .arena
        .node(source.root)
        .data
        .as_source_file()
        .expect("source file root");
    let statements = source
        .arena
        .node_array(root.statements.expect("statements"));
    let first = source
        .arena
        .node(statements.nodes[0])
        .data
        .as_expression_statement()
        .expect("expression statement")
        .expression
        .expect("expression");
    let (_, plus, _) = binary_parts(&source, first);
    assert_eq!(plus, SyntaxKind::PlusToken);
}

#[test]
fn parse_conditional_expression_shapes() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "a ? b : c ? d : e;".to_owned(),
        ParseOptions::default(),
        None,
    );

    assert!(source.parse_diagnostics.is_empty());
    let expressions = expression_statements(&source);
    let conditional = source
        .arena
        .node(expressions[0])
        .data
        .as_conditional_expression()
        .expect("conditional expression");
    let when_false = conditional.when_false.expect("when false");
    assert_eq!(
        source.arena.node(when_false).kind,
        SyntaxKind::ConditionalExpression
    );
}

#[test]
fn conditional_missing_colon_recovers_with_missing_when_false() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "a ? b;".to_owned(),
        ParseOptions::default(),
        None,
    );

    assert!(!source.parse_diagnostics.is_empty());
    let expressions = expression_statements(&source);
    let conditional = source
        .arena
        .node(expressions[0])
        .data
        .as_conditional_expression()
        .expect("conditional expression");
    let colon = conditional.colon_token.expect("colon token");
    let colon_node = source.arena.node(colon);
    assert_eq!(colon_node.pos, colon_node.end);
    let when_false = conditional.when_false.expect("when false");
    let when_false_node = source.arena.node(when_false);
    assert_eq!(when_false_node.pos, when_false_node.end);
}

#[test]
fn parse_arrow_function_shapes() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "x => x; (a, b) => a; () => 1; (...xs) => xs; (a) => { return a; };".to_owned(),
        ParseOptions::default(),
        None,
    );

    assert!(source.parse_diagnostics.is_empty());
    let expressions = expression_statements(&source);
    assert_eq!(expressions.len(), 5);

    for &expression in &expressions {
        assert_eq!(
            source.arena.node(expression).kind,
            SyntaxKind::ArrowFunction
        );
    }

    let simple = source
        .arena
        .node(expressions[0])
        .data
        .as_arrow_function()
        .expect("simple arrow");
    let simple_parameters = source
        .arena
        .node_array(simple.parameters.expect("parameters"));
    assert_eq!(simple_parameters.nodes.len(), 1);
    assert!(simple.equals_greater_than_token.is_some());

    let two_parameters = source
        .arena
        .node(expressions[1])
        .data
        .as_arrow_function()
        .expect("two-parameter arrow");
    assert_eq!(
        source
            .arena
            .node_array(two_parameters.parameters.expect("parameters"))
            .nodes
            .len(),
        2
    );

    let rest = source
        .arena
        .node(expressions[3])
        .data
        .as_arrow_function()
        .expect("rest arrow");
    let rest_parameters = source
        .arena
        .node_array(rest.parameters.expect("parameters"));
    let rest_parameter = source
        .arena
        .node(rest_parameters.nodes[0])
        .data
        .as_parameter()
        .expect("rest parameter");
    assert!(rest_parameter.dot_dot_dot_token.is_some());

    let block_body = source
        .arena
        .node(expressions[4])
        .data
        .as_arrow_function()
        .expect("block-body arrow");
    assert_eq!(
        source.arena.node(block_body.body.expect("body")).kind,
        SyntaxKind::Block
    );
}

#[test]
fn parse_async_arrow_and_line_break_asi() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "async x => x; async (a) => a; async\ny => y;".to_owned(),
        ParseOptions::default(),
        None,
    );

    assert!(source.parse_diagnostics.is_empty());
    let expressions = expression_statements(&source);
    assert_eq!(expressions.len(), 4);

    for &expression in &expressions[..2] {
        let arrow = source
            .arena
            .node(expression)
            .data
            .as_arrow_function()
            .expect("async arrow");
        assert!(arrow.modifiers.is_some());
    }

    assert_eq!(
        source.arena.node(expressions[2]).kind,
        SyntaxKind::Identifier
    );
    assert_eq!(
        source.arena.node(expressions[3]).kind,
        SyntaxKind::ArrowFunction
    );
}

#[test]
fn parenthesized_expression_not_mistaken_for_arrow() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "(a, b); (a);".to_owned(),
        ParseOptions::default(),
        None,
    );

    assert!(source.parse_diagnostics.is_empty());
    let expressions = expression_statements(&source);
    for &expression in &expressions {
        assert_eq!(
            source.arena.node(expression).kind,
            SyntaxKind::ParenthesizedExpression
        );
    }
}

#[test]
fn conditional_when_true_rejects_arrow_return_type() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "a ? (b): c => d;".to_owned(),
        ParseOptions::default(),
        None,
    );

    assert!(source.parse_diagnostics.is_empty());
    let expressions = expression_statements(&source);
    let conditional = source
        .arena
        .node(expressions[0])
        .data
        .as_conditional_expression()
        .expect("conditional expression");
    assert_eq!(
        source
            .arena
            .node(conditional.when_true.expect("when true"))
            .kind,
        SyntaxKind::ParenthesizedExpression
    );
    assert_eq!(
        source
            .arena
            .node(conditional.when_false.expect("when false"))
            .kind,
        SyntaxKind::ArrowFunction
    );
}

#[test]
fn function_expression_parses_real_parameters() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "(function (this: T, a, b = 1) { return a; });".to_owned(),
        ParseOptions::default(),
        None,
    );

    assert!(source.parse_diagnostics.is_empty());
    let expressions = expression_statements(&source);
    let parenthesized = source
        .arena
        .node(expressions[0])
        .data
        .as_parenthesized_expression()
        .expect("parenthesized expression");
    let function = source
        .arena
        .node(parenthesized.expression.expect("function expression"))
        .data
        .as_function_expression()
        .expect("function expression");
    let parameters = source
        .arena
        .node_array(function.parameters.expect("parameters"));
    assert_eq!(parameters.nodes.len(), 3);
    let default_parameter = source
        .arena
        .node(parameters.nodes[2])
        .data
        .as_parameter()
        .expect("defaulted parameter");
    assert!(default_parameter.initializer.is_some());
}

#[test]
fn same_start_dedup_and_finish_node_error_transfer() {
    let mut parser = Parser::new("a.ts".to_owned(), "", LanguageVariant::Standard, false);
    parser.next_token();

    parser.parse_error_at_position(0, 0, &gen::Identifier_expected, &[]);
    parser.parse_error_at_position(0, 0, &gen::Unexpected_token, &[]);
    let first = parser.create_missing_node(SyntaxKind::Identifier, false, None, &[]);
    let second = parser.create_missing_node(SyntaxKind::Identifier, false, None, &[]);

    assert_eq!(parser.parse_diagnostics.len(), 1);
    assert!(NodeFlags::from_bits(parser.arena.node(first).flags)
        .contains(NodeFlags::THIS_NODE_HAS_ERROR));
    assert!(!NodeFlags::from_bits(parser.arena.node(second).flags)
        .contains(NodeFlags::THIS_NODE_HAS_ERROR));
}

#[test]
fn parse_token_node_consumes_current_token() {
    let mut parser = Parser::new("a.ts".to_owned(), ";", LanguageVariant::Standard, false);
    parser.next_token();

    let token = parser.parse_token_node();

    assert_eq!(parser.arena.node(token).kind, SyntaxKind::SemicolonToken);
    assert_eq!(parser.token(), SyntaxKind::EndOfFileToken);
}

#[test]
fn expected_optional_context_and_speculation_restore_parser_state() {
    let mut parser = Parser::new("a.ts".to_owned(), ";x", LanguageVariant::Standard, false);
    parser.next_token();

    assert!(parser.parse_optional(SyntaxKind::SemicolonToken));
    assert_eq!(parser.node_pos(), 1);
    assert!(!parser.parse_expected(SyntaxKind::CommaToken, None));
    assert_eq!(parser.parse_diagnostics.len(), 1);

    let context_node = parser.do_in_context(NodeFlags::AWAIT_CONTEXT, NodeFlags::NONE, |parser| {
        parser.create_missing_node(SyntaxKind::Identifier, false, None, &[])
    });
    assert!(NodeFlags::from_bits(parser.arena.node(context_node).flags)
        .contains(NodeFlags::AWAIT_CONTEXT));

    let result: Option<NodeId> = parser.try_parse(|parser| {
        parser.parse_error_at_current_token(&gen::Unexpected_token, &[]);
        parser.next_token();
        None
    });
    assert!(result.is_none());
    assert_eq!(parser.token(), SyntaxKind::Identifier);
    assert_eq!(parser.parse_diagnostics.len(), 1);

    let lookahead = parser.look_ahead(|parser| {
        parser.next_token();
        parser.token()
    });
    assert_eq!(lookahead, SyntaxKind::EndOfFileToken);
    assert_eq!(parser.token(), SyntaxKind::Identifier);
}

#[test]
fn delimited_list_tracks_trailing_comma() {
    let mut parser = Parser::new("a.ts".to_owned(), "a,)", LanguageVariant::Standard, false);
    parser.next_token();

    let list = parser.parse_delimited_list(
        ParsingContext::ArgumentExpressions,
        |parser| Some(parser.parse_token_node()),
        false,
    );
    let list = parser.arena.node_array(list);

    assert_eq!(list.nodes.len(), 1);
    assert!(list.has_trailing_comma);
    assert_eq!(parser.token(), SyntaxKind::CloseParenToken);
}

#[test]
fn delimited_list_reports_missing_commas_and_keeps_progressing() {
    let mut parser = Parser::new("a.ts".to_owned(), "a b)", LanguageVariant::Standard, false);
    parser.next_token();

    let list = parser.parse_delimited_list(
        ParsingContext::ArgumentExpressions,
        |parser| Some(parser.parse_token_node()),
        false,
    );
    let list = parser.arena.node_array(list);

    assert_eq!(list.nodes.len(), 2);
    assert!(!list.has_trailing_comma);
    assert_eq!(parser.token(), SyntaxKind::CloseParenToken);
    assert_eq!(parser.parse_diagnostics.len(), 1);
    assert_eq!(parser.parse_diagnostics[0].code(), 1005);
    assert_eq!(parser.parse_diagnostics[0].message_text(), "',' expected.");
}

#[test]
fn list_recovery_aborts_when_outer_context_can_consume_token() {
    let mut parser = Parser::new("a.ts".to_owned(), "}", LanguageVariant::Standard, false);
    parser.next_token();
    parser.parsing_context |= ParsingContext::BlockStatements.bit();

    let list = parser.parse_delimited_list(
        ParsingContext::ArgumentExpressions,
        |parser| Some(parser.parse_token_node()),
        false,
    );

    assert!(parser.arena.node_array(list).nodes.is_empty());
    assert_eq!(parser.token(), SyntaxKind::CloseBraceToken);
    assert_eq!(parser.parse_diagnostics.len(), 1);
    assert_eq!(parser.parse_diagnostics[0].code(), 1135);
}

#[test]
fn parse_list_skips_unrecoverable_tokens() {
    let mut parser = Parser::new(
        "a.ts".to_owned(),
        "x case",
        LanguageVariant::Standard,
        false,
    );
    parser.next_token();

    let list = parser.parse_list(ParsingContext::SwitchClauses, |parser| {
        Some(parser.parse_token_node())
    });

    let list = parser.arena.node_array(list);
    assert_eq!(list.nodes.len(), 1);
    assert_eq!(
        parser.arena.node(list.nodes[0]).kind,
        SyntaxKind::CaseKeyword
    );
    assert_eq!(parser.token(), SyntaxKind::EndOfFileToken);
    assert_eq!(parser.parse_diagnostics.len(), 1);
    assert_eq!(parser.parse_diagnostics[0].code(), 1130);
}

fn variable_types(source: &SourceFile) -> Vec<NodeId> {
    let root = source
        .arena
        .node(source.root)
        .data
        .as_source_file()
        .expect("source file root");
    let statements = source
        .arena
        .node_array(root.statements.expect("statements"));
    statements
        .nodes
        .iter()
        .map(|&statement| {
            let NodeData::VariableStatement(data) = &source.arena.node(statement).data else {
                panic!("expected variable statement");
            };
            let list = source
                .arena
                .node(data.declaration_list.expect("declaration list"))
                .data
                .as_variable_declaration_list()
                .expect("variable declaration list");
            let declarations = source
                .arena
                .node_array(list.declarations.expect("declarations"));
            source
                .arena
                .node(declarations.nodes[0])
                .data
                .as_variable_declaration()
                .expect("variable declaration")
                .r#type
                .expect("type annotation")
        })
        .collect()
}

#[test]
fn parse_type_reference_and_postfix_shapes() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "let a: string; let b: Array<number>; let c: ns.Entity<string>[]; let d: A[\"k\"]; let e: string!; let f: ?string;".to_owned(),
        ParseOptions::default(),
        None,
    );

    assert!(source.parse_diagnostics.is_empty());
    let types = variable_types(&source);
    assert_eq!(source.arena.node(types[0]).kind, SyntaxKind::StringKeyword);

    let NodeData::TypeReference(array_ref) = &source.arena.node(types[1]).data else {
        panic!("expected type reference");
    };
    assert!(array_ref.type_arguments.is_some());

    let NodeData::ArrayType(array) = &source.arena.node(types[2]).data else {
        panic!("expected array type");
    };
    let NodeData::TypeReference(qualified) =
        &source.arena.node(array.element_type.expect("element")).data
    else {
        panic!("expected type reference element");
    };
    assert_eq!(
        source
            .arena
            .node(qualified.type_name.expect("type name"))
            .kind,
        SyntaxKind::QualifiedName
    );

    assert_eq!(
        source.arena.node(types[3]).kind,
        SyntaxKind::IndexedAccessType
    );
    assert_eq!(
        source.arena.node(types[4]).kind,
        SyntaxKind::JSDocNonNullableType
    );
    assert_eq!(
        source.arena.node(types[5]).kind,
        SyntaxKind::JSDocNullableType
    );
}

#[test]
fn parse_union_intersection_and_type_operators() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "let a: A | B & C; let b: keyof A; let c: readonly string[]; let d: unique symbol;"
            .to_owned(),
        ParseOptions::default(),
        None,
    );

    assert!(source.parse_diagnostics.is_empty());
    let types = variable_types(&source);
    let NodeData::UnionType(union) = &source.arena.node(types[0]).data else {
        panic!("expected union type");
    };
    let members = source.arena.node_array(union.types.expect("types"));
    assert_eq!(members.nodes.len(), 2);
    assert_eq!(
        source.arena.node(members.nodes[1]).kind,
        SyntaxKind::IntersectionType
    );
    assert_eq!(source.arena.node(types[1]).kind, SyntaxKind::TypeOperator);
    let NodeData::TypeOperator(readonly_array) = &source.arena.node(types[2]).data else {
        panic!("expected type operator");
    };
    assert_eq!(
        source
            .arena
            .node(readonly_array.r#type.expect("operand"))
            .kind,
        SyntaxKind::ArrayType
    );
    assert_eq!(source.arena.node(types[3]).kind, SyntaxKind::TypeOperator);
}

#[test]
fn parse_object_type_member_shapes() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "let o: { a: string; readonly b?: number, m<T>(x: T): T; (x: number): void; new (): any; [k: string]: any; get p(): number; set p(v); };".to_owned(),
        ParseOptions::default(),
        None,
    );

    assert!(source.parse_diagnostics.is_empty());
    let types = variable_types(&source);
    let NodeData::TypeLiteral(literal) = &source.arena.node(types[0]).data else {
        panic!("expected type literal");
    };
    let members = source.arena.node_array(literal.members.expect("members"));
    let kinds: Vec<SyntaxKind> = members
        .nodes
        .iter()
        .map(|&member| source.arena.node(member).kind)
        .collect();
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::PropertySignature,
            SyntaxKind::PropertySignature,
            SyntaxKind::MethodSignature,
            SyntaxKind::CallSignature,
            SyntaxKind::ConstructSignature,
            SyntaxKind::IndexSignature,
            SyntaxKind::GetAccessor,
            SyntaxKind::SetAccessor,
        ]
    );
    let NodeData::PropertySignature(readonly_property) = &source.arena.node(members.nodes[1]).data
    else {
        panic!("expected property signature");
    };
    assert!(readonly_property.modifiers.is_some());
    assert!(readonly_property.question_token.is_some());
}

#[test]
fn parse_tuple_function_and_constructor_types() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "let t: [string, number?, ...boolean[], name: string]; let f: (a: string) => void; let g: new () => any; let h: abstract new () => any;".to_owned(),
        ParseOptions::default(),
        None,
    );

    assert!(source.parse_diagnostics.is_empty());
    let types = variable_types(&source);
    let NodeData::TupleType(tuple) = &source.arena.node(types[0]).data else {
        panic!("expected tuple type");
    };
    let elements = source.arena.node_array(tuple.elements.expect("elements"));
    let kinds: Vec<SyntaxKind> = elements
        .nodes
        .iter()
        .map(|&element| source.arena.node(element).kind)
        .collect();
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::StringKeyword,
            SyntaxKind::OptionalType,
            SyntaxKind::RestType,
            SyntaxKind::NamedTupleMember,
        ]
    );
    assert_eq!(source.arena.node(types[1]).kind, SyntaxKind::FunctionType);
    assert_eq!(
        source.arena.node(types[2]).kind,
        SyntaxKind::ConstructorType
    );
    let NodeData::ConstructorType(abstract_ctor) = &source.arena.node(types[3]).data else {
        panic!("expected constructor type");
    };
    assert!(abstract_ctor.modifiers.is_some());
}

#[test]
fn parse_conditional_infer_typeof_and_import_types() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "let a: T extends U ? V : W; let b: T extends infer U extends X ? U : never; let c: typeof ns.entity; let d: import(\"m\").T<U>; let e: typeof import(\"m\");".to_owned(),
        ParseOptions::default(),
        None,
    );

    assert!(source.parse_diagnostics.is_empty());
    let types = variable_types(&source);
    assert_eq!(
        source.arena.node(types[0]).kind,
        SyntaxKind::ConditionalType
    );
    let NodeData::ConditionalType(conditional) = &source.arena.node(types[1]).data else {
        panic!("expected conditional type");
    };
    let NodeData::InferType(infer) = &source
        .arena
        .node(conditional.extends_type.expect("extends type"))
        .data
    else {
        panic!("expected infer type");
    };
    let NodeData::TypeParameter(infer_parameter) = &source
        .arena
        .node(infer.type_parameter.expect("type parameter"))
        .data
    else {
        panic!("expected type parameter");
    };
    assert!(infer_parameter.constraint.is_some());

    let NodeData::TypeQuery(query) = &source.arena.node(types[2]).data else {
        panic!("expected type query");
    };
    assert_eq!(
        source.arena.node(query.expr_name.expect("expr name")).kind,
        SyntaxKind::QualifiedName
    );

    let NodeData::ImportType(import_type) = &source.arena.node(types[3]).data else {
        panic!("expected import type");
    };
    assert!(import_type.qualifier.is_some());
    assert!(import_type.type_arguments.is_some());
    assert_eq!(source.arena.node(types[4]).kind, SyntaxKind::ImportType);
}

#[test]
fn parse_mapped_and_template_literal_types() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "let m: { readonly [K in keyof T as `get${K}`]?: T[K]; }; let t: `a${T}b`;".to_owned(),
        ParseOptions::default(),
        None,
    );

    assert!(source.parse_diagnostics.is_empty());
    let types = variable_types(&source);
    let NodeData::MappedType(mapped) = &source.arena.node(types[0]).data else {
        panic!("expected mapped type");
    };
    assert!(mapped.readonly_token.is_some());
    assert!(mapped.name_type.is_some());
    assert!(mapped.question_token.is_some());
    assert_eq!(
        source.arena.node(mapped.r#type.expect("type")).kind,
        SyntaxKind::IndexedAccessType
    );

    let NodeData::TemplateLiteralType(template) = &source.arena.node(types[1]).data else {
        panic!("expected template literal type");
    };
    let spans = source
        .arena
        .node_array(template.template_spans.expect("spans"));
    assert_eq!(spans.nodes.len(), 1);
    assert_eq!(
        source.arena.node(spans.nodes[0]).kind,
        SyntaxKind::TemplateLiteralTypeSpan
    );
}

#[test]
fn parse_type_predicates_in_return_types() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "const f = function (x): x is string { return true; }; const g = function (x): asserts x is string {}; let h: { isC(): this is C; };".to_owned(),
        ParseOptions::default(),
        None,
    );

    assert!(source.parse_diagnostics.is_empty());
    let root = source
        .arena
        .node(source.root)
        .data
        .as_source_file()
        .expect("source file root");
    let statements = source
        .arena
        .node_array(root.statements.expect("statements"));

    let function_return_type = |statement: NodeId| {
        let NodeData::VariableStatement(data) = &source.arena.node(statement).data else {
            panic!("expected variable statement");
        };
        let list = source
            .arena
            .node(data.declaration_list.expect("list"))
            .data
            .as_variable_declaration_list()
            .expect("declaration list");
        let declarations = source
            .arena
            .node_array(list.declarations.expect("declarations"));
        let declaration = source
            .arena
            .node(declarations.nodes[0])
            .data
            .as_variable_declaration()
            .expect("variable declaration");
        let NodeData::FunctionExpression(function) = &source
            .arena
            .node(declaration.initializer.expect("initializer"))
            .data
        else {
            panic!("expected function expression");
        };
        function.r#type.expect("return type")
    };

    let predicate = function_return_type(statements.nodes[0]);
    let NodeData::TypePredicate(data) = &source.arena.node(predicate).data else {
        panic!("expected type predicate");
    };
    assert!(data.asserts_modifier.is_none());

    let asserts_predicate = function_return_type(statements.nodes[1]);
    let NodeData::TypePredicate(asserts_data) = &source.arena.node(asserts_predicate).data else {
        panic!("expected asserts predicate");
    };
    assert!(asserts_data.asserts_modifier.is_some());

    let NodeData::VariableStatement(h_statement) = &source.arena.node(statements.nodes[2]).data
    else {
        panic!("expected variable statement");
    };
    let h_list = source
        .arena
        .node(h_statement.declaration_list.expect("list"))
        .data
        .as_variable_declaration_list()
        .expect("declaration list");
    let h_declarations = source
        .arena
        .node_array(h_list.declarations.expect("declarations"));
    let h_type = source
        .arena
        .node(h_declarations.nodes[0])
        .data
        .as_variable_declaration()
        .expect("variable declaration")
        .r#type
        .expect("type annotation");
    let NodeData::TypeLiteral(literal) = &source.arena.node(h_type).data else {
        panic!("expected type literal");
    };
    let members = source.arena.node_array(literal.members.expect("members"));
    let NodeData::MethodSignature(method) = &source.arena.node(members.nodes[0]).data else {
        panic!("expected method signature");
    };
    let NodeData::TypePredicate(this_predicate) =
        &source.arena.node(method.r#type.expect("return type")).data
    else {
        panic!("expected this predicate");
    };
    assert_eq!(
        source
            .arena
            .node(this_predicate.parameter_name.expect("parameter name"))
            .kind,
        SyntaxKind::ThisType
    );
}

#[test]
fn parse_generic_arrow_type_assertion_and_object_accessors() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "const f = <T>(x: T): T => x; const v = <Foo<string>>bar; const o = { get x() { return 1; }, set x(v) {}, async m<T>(a: T) { return a; } };".to_owned(),
        ParseOptions::default(),
        None,
    );

    assert!(source.parse_diagnostics.is_empty());
    let root = source
        .arena
        .node(source.root)
        .data
        .as_source_file()
        .expect("source file root");
    let statements = source
        .arena
        .node_array(root.statements.expect("statements"));

    let initializer = |statement: NodeId| {
        let NodeData::VariableStatement(data) = &source.arena.node(statement).data else {
            panic!("expected variable statement");
        };
        let list = source
            .arena
            .node(data.declaration_list.expect("list"))
            .data
            .as_variable_declaration_list()
            .expect("declaration list");
        let declarations = source
            .arena
            .node_array(list.declarations.expect("declarations"));
        source
            .arena
            .node(declarations.nodes[0])
            .data
            .as_variable_declaration()
            .expect("variable declaration")
            .initializer
            .expect("initializer")
    };

    let NodeData::ArrowFunction(arrow) = &source.arena.node(initializer(statements.nodes[0])).data
    else {
        panic!("expected arrow function");
    };
    assert!(arrow.type_parameters.is_some());
    assert!(arrow.r#type.is_some());

    let NodeData::TypeAssertionExpression(assertion) =
        &source.arena.node(initializer(statements.nodes[1])).data
    else {
        panic!("expected type assertion");
    };
    let NodeData::TypeReference(assertion_type) = &source
        .arena
        .node(assertion.r#type.expect("assertion type"))
        .data
    else {
        panic!("expected type reference");
    };
    assert!(assertion_type.type_arguments.is_some());

    let NodeData::ObjectLiteralExpression(object) =
        &source.arena.node(initializer(statements.nodes[2])).data
    else {
        panic!("expected object literal");
    };
    let properties = source
        .arena
        .node_array(object.properties.expect("properties"));
    let kinds: Vec<SyntaxKind> = properties
        .nodes
        .iter()
        .map(|&property| source.arena.node(property).kind)
        .collect();
    assert_eq!(
        kinds,
        vec![
            SyntaxKind::GetAccessor,
            SyntaxKind::SetAccessor,
            SyntaxKind::MethodDeclaration,
        ]
    );
    let NodeData::MethodDeclaration(method) = &source.arena.node(properties.nodes[2]).data else {
        panic!("expected method declaration");
    };
    assert!(method.modifiers.is_some());
    assert!(method.type_parameters.is_some());
}

#[test]
fn union_function_type_error_and_type_expected_recovery() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "let x: A | () => void; let y: ;".to_owned(),
        ParseOptions::default(),
        None,
    );

    let codes: Vec<u32> = source
        .parse_diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code())
        .collect();
    assert_eq!(
        codes,
        vec![
            gen::Function_type_notation_must_be_parenthesized_when_used_in_a_union_type.code,
            gen::Type_expected.code,
        ]
    );

    let types = variable_types(&source);
    let NodeData::UnionType(union) = &source.arena.node(types[0]).data else {
        panic!("expected union type");
    };
    let members = source.arena.node_array(union.types.expect("types"));
    assert_eq!(
        source.arena.node(members.nodes[1]).kind,
        SyntaxKind::FunctionType
    );
}

fn statement_kinds(source: &SourceFile) -> Vec<SyntaxKind> {
    let root = source
        .arena
        .node(source.root)
        .data
        .as_source_file()
        .expect("source file root");
    source
        .arena
        .node_array(root.statements.expect("statements"))
        .nodes
        .iter()
        .map(|&statement| source.arena.node(statement).kind)
        .collect()
}

fn statement_nodes(source: &SourceFile) -> Vec<NodeId> {
    let root = source
        .arena
        .node(source.root)
        .data
        .as_source_file()
        .expect("source file root");
    source
        .arena
        .node_array(root.statements.expect("statements"))
        .nodes
        .clone()
}

#[test]
fn parse_class_declaration_shapes() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "@dec export abstract class C<T> extends B<T> implements I, J {\n  constructor(private readonly x: number) { super(); }\n  static { C.count = 0; }\n  #secret = 1;\n  declare readonly f: string;\n  get p(): number { return 1; }\n  set p(v) {}\n  static async *m<U>(u: U): Promise<U> { return u; }\n  [k: string]: any;\n  ;\n}".to_owned(),
        ParseOptions::default(),
        None,
    );

    assert!(
        source.parse_diagnostics.is_empty(),
        "{:?}",
        source.parse_diagnostics
    );
    let statements = statement_nodes(&source);
    let NodeData::ClassDeclaration(class) = &source.arena.node(statements[0]).data else {
        panic!("expected class declaration");
    };
    assert!(class.modifiers.is_some());
    assert!(class.type_parameters.is_some());
    let heritage = source
        .arena
        .node_array(class.heritage_clauses.expect("heritage clauses"));
    assert_eq!(heritage.nodes.len(), 2);
    let modifier_kinds: Vec<SyntaxKind> = source
        .arena
        .node_array(class.modifiers.expect("modifiers"))
        .nodes
        .iter()
        .map(|&modifier| source.arena.node(modifier).kind)
        .collect();
    assert_eq!(
        modifier_kinds,
        vec![
            SyntaxKind::Decorator,
            SyntaxKind::ExportKeyword,
            SyntaxKind::AbstractKeyword,
        ]
    );
    let member_kinds: Vec<SyntaxKind> = source
        .arena
        .node_array(class.members.expect("members"))
        .nodes
        .iter()
        .map(|&member| source.arena.node(member).kind)
        .collect();
    assert_eq!(
        member_kinds,
        vec![
            SyntaxKind::Constructor,
            SyntaxKind::ClassStaticBlockDeclaration,
            SyntaxKind::PropertyDeclaration,
            SyntaxKind::PropertyDeclaration,
            SyntaxKind::GetAccessor,
            SyntaxKind::SetAccessor,
            SyntaxKind::MethodDeclaration,
            SyntaxKind::IndexSignature,
            SyntaxKind::SemicolonClassElement,
        ]
    );
}

#[test]
fn parse_interface_type_alias_and_enum() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "interface I<T> extends A, B<T> { a: string; }\ntype Alias<T> = T | null;\ntype Str = intrinsic;\nconst enum E { A, B = 2, \"c\" = 3 }".to_owned(),
        ParseOptions::default(),
        None,
    );

    assert!(
        source.parse_diagnostics.is_empty(),
        "{:?}",
        source.parse_diagnostics
    );
    assert_eq!(
        statement_kinds(&source),
        vec![
            SyntaxKind::InterfaceDeclaration,
            SyntaxKind::TypeAliasDeclaration,
            SyntaxKind::TypeAliasDeclaration,
            SyntaxKind::EnumDeclaration,
        ]
    );
    let statements = statement_nodes(&source);
    let NodeData::TypeAliasDeclaration(intrinsic_alias) = &source.arena.node(statements[2]).data
    else {
        panic!("expected type alias");
    };
    assert_eq!(
        source
            .arena
            .node(intrinsic_alias.r#type.expect("type"))
            .kind,
        SyntaxKind::IntrinsicKeyword
    );
    let NodeData::EnumDeclaration(enum_declaration) = &source.arena.node(statements[3]).data else {
        panic!("expected enum declaration");
    };
    assert!(enum_declaration.modifiers.is_some());
    assert_eq!(
        source
            .arena
            .node_array(enum_declaration.members.expect("members"))
            .nodes
            .len(),
        3
    );
}

#[test]
fn parse_namespace_and_ambient_modules() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "namespace a.b { export const x = 1; }\ndeclare module \"m\" { let y: number; }\ndeclare global { interface Window {} }\nmodule Simple { }".to_owned(),
        ParseOptions::default(),
        None,
    );

    assert!(
        source.parse_diagnostics.is_empty(),
        "{:?}",
        source.parse_diagnostics
    );
    let statements = statement_nodes(&source);
    assert_eq!(
        statement_kinds(&source),
        vec![
            SyntaxKind::ModuleDeclaration,
            SyntaxKind::ModuleDeclaration,
            SyntaxKind::ModuleDeclaration,
            SyntaxKind::ModuleDeclaration,
        ]
    );
    // namespace a.b desugars into a nested module declaration.
    let NodeData::ModuleDeclaration(outer) = &source.arena.node(statements[0]).data else {
        panic!("expected module declaration");
    };
    let body = outer.body.expect("body");
    assert_eq!(source.arena.node(body).kind, SyntaxKind::ModuleDeclaration);
    assert!(
        NodeFlags::from_bits(source.arena.node(body).flags).contains(NodeFlags::NESTED_NAMESPACE)
    );
    let NodeData::ModuleDeclaration(global) = &source.arena.node(statements[2]).data else {
        panic!("expected global augmentation");
    };
    assert!(NodeFlags::from_bits(source.arena.node(statements[2]).flags)
        .contains(NodeFlags::GLOBAL_AUGMENTATION));
    assert!(global.body.is_some());
}

#[test]
fn parse_import_and_export_forms() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "import d, { e as f, type g } from \"m\";\nimport * as ns from \"m\";\nimport type { A } from \"m\";\nimport eq = require(\"m\");\nexport * as everything from \"m\";\nexport { a as b };\nexport default 42;\nexport = eq;\nexport as namespace NS;\nimport \"side-effect\";".to_owned(),
        ParseOptions::default(),
        None,
    );

    assert!(
        source.parse_diagnostics.is_empty(),
        "{:?}",
        source.parse_diagnostics
    );
    assert_eq!(
        statement_kinds(&source),
        vec![
            SyntaxKind::ImportDeclaration,
            SyntaxKind::ImportDeclaration,
            SyntaxKind::ImportDeclaration,
            SyntaxKind::ImportEqualsDeclaration,
            SyntaxKind::ExportDeclaration,
            SyntaxKind::ExportDeclaration,
            SyntaxKind::ExportAssignment,
            SyntaxKind::ExportAssignment,
            SyntaxKind::NamespaceExportDeclaration,
            SyntaxKind::ImportDeclaration,
        ]
    );
    let statements = statement_nodes(&source);
    let NodeData::ImportDeclaration(first) = &source.arena.node(statements[0]).data else {
        panic!("expected import declaration");
    };
    let NodeData::ImportClause(clause) = &source
        .arena
        .node(first.import_clause.expect("import clause"))
        .data
    else {
        panic!("expected import clause");
    };
    assert!(clause.name.is_some());
    let NodeData::NamedImports(named) = &source
        .arena
        .node(clause.named_bindings.expect("named bindings"))
        .data
    else {
        panic!("expected named imports");
    };
    assert_eq!(
        source
            .arena
            .node_array(named.elements.expect("elements"))
            .nodes
            .len(),
        2
    );
    let NodeData::ImportEqualsDeclaration(equals) = &source.arena.node(statements[3]).data else {
        panic!("expected import equals");
    };
    assert_eq!(
        source
            .arena
            .node(equals.module_reference.expect("module reference"))
            .kind,
        SyntaxKind::ExternalModuleReference
    );
    let NodeData::ImportDeclaration(side_effect) = &source.arena.node(statements[9]).data else {
        panic!("expected side-effect import");
    };
    assert!(side_effect.import_clause.is_none());
}

#[test]
fn exported_internal_import_equals_is_an_external_module_indicator() {
    let exported = parse_source_file(
        "exported.ts".to_owned(),
        "export import value = ns.value;".to_owned(),
        ParseOptions::default(),
        None,
    );
    let exported_statement = statement_nodes(&exported)[0];
    assert_eq!(exported.external_module_indicator, Some(exported_statement));
    assert_eq!(
        exported.arena.node(exported_statement).kind,
        SyntaxKind::ImportEqualsDeclaration
    );

    let internal = parse_source_file(
        "internal.ts".to_owned(),
        "import value = ns.value;".to_owned(),
        ParseOptions::default(),
        None,
    );
    assert_eq!(internal.external_module_indicator, None);
}

#[test]
fn legacy_module_call_import_equals_recovers_as_internal_reference() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "import rect = module(\"rect\"); var bar = new rect.Rect();".to_owned(),
        ParseOptions::default(),
        None,
    );
    let statements = statement_nodes(&source);
    assert_eq!(
        statements
            .iter()
            .map(|&node| source.arena.node(node).kind)
            .collect::<Vec<_>>(),
        [
            SyntaxKind::ImportEqualsDeclaration,
            SyntaxKind::ExpressionStatement,
            SyntaxKind::VariableStatement,
        ]
    );
    let NodeData::ImportEqualsDeclaration(import) = &source.arena.node(statements[0]).data else {
        unreachable!()
    };
    let reference = import.module_reference.expect("module reference");
    let NodeData::Identifier(identifier) = &source.arena.node(reference).data else {
        panic!("expected recovered identifier module reference")
    };
    assert_eq!(identifier.escaped_text, "module");
    assert_eq!(source.parse_diagnostics.len(), 1);
}

#[test]
fn triple_slash_resolution_mode_diagnostic_uses_the_types_span() {
    let source = parse_source_file(
        "/index.ts".to_owned(),
        "/// <reference types=\"pkg\" resolution-mode=\"esm\"/>\nexport {};".to_owned(),
        ParseOptions::default(),
        None,
    );
    assert_eq!(source.parse_diagnostics.len(), 1);
    let diagnostic = &source.parse_diagnostics[0];
    assert_eq!(
        diagnostic.code(),
        gen::resolution_mode_should_be_either_require_or_import.code
    );
    assert_eq!(diagnostic.start, Some(22));
    assert_eq!(diagnostic.length, Some(3));
    assert_eq!(source.type_reference_directives.len(), 1);
    let reference = &source.type_reference_directives[0];
    assert_eq!(reference.file_name, "pkg");
    assert_eq!(reference.pos, 22);
    assert_eq!(reference.end, 25);
    assert_eq!(reference.resolution_mode, None);
}

#[test]
fn triple_slash_reference_spans_are_utf16_offsets() {
    let source = parse_source_file(
        "/index.ts".to_owned(),
        "/// <reference types=\"😀pkg\" resolution-mode=\"esm\"/>\nexport {};".to_owned(),
        ParseOptions::default(),
        None,
    );

    assert_eq!(source.parse_diagnostics.len(), 1);
    assert_eq!(source.parse_diagnostics[0].start, Some(22));
    assert_eq!(source.parse_diagnostics[0].length, Some(5));
    let reference = &source.type_reference_directives[0];
    assert_eq!(reference.file_name, "😀pkg");
    assert_eq!(reference.pos, 22);
    assert_eq!(reference.end, 27);
}

#[test]
fn triple_slash_type_references_retain_exact_spelling_span_mode_and_order() {
    let source = parse_source_file(
        "/index.ts".to_owned(),
        concat!(
            "/// <reference types=\"JqUeRy\" />\n",
            "/// <reference types='@scope/pkg' resolution-mode='import'/>\n",
            "/// <reference types=\"required\" resolution-mode=\"require\"/>\n",
            "export {};",
        )
        .to_owned(),
        ParseOptions::default(),
        None,
    );
    assert!(source.parse_diagnostics.is_empty());
    assert_eq!(source.type_reference_directives.len(), 3);
    assert_eq!(source.type_reference_directives[0].file_name, "JqUeRy");
    assert_eq!(source.type_reference_directives[0].pos, 22);
    assert_eq!(source.type_reference_directives[0].end, 28);
    assert_eq!(source.type_reference_directives[0].resolution_mode, None);
    assert_eq!(
        source.type_reference_directives[1].resolution_mode,
        Some(TypeReferenceDirectiveResolutionMode::Import)
    );
    assert_eq!(
        source.type_reference_directives[2].resolution_mode,
        Some(TypeReferenceDirectiveResolutionMode::Require)
    );
}

#[test]
fn triple_slash_path_type_and_lib_references_share_upstream_precedence() {
    let text = concat!(
        "/// <reference path=\"./first.ts\" preserve=\"true\" />\n",
        "/// <reference lib='es2023' />\n",
        "/// <reference path='ignored.ts' lib='dom' />\n",
        "/// <reference path='ignored-again.ts' lib='ignored' types='pkg' />\n",
        "/// <reference no-default-lib='true' path='also-ignored.ts' />\n",
        "export {};",
    );
    let source = parse_source_file(
        "/index.ts".to_owned(),
        text.to_owned(),
        ParseOptions::default(),
        None,
    );

    assert!(source.parse_diagnostics.is_empty());
    assert_eq!(source.referenced_files.len(), 1);
    assert_eq!(source.referenced_files[0].file_name, "./first.ts");
    assert_eq!(source.referenced_files[0].pos, 21);
    assert_eq!(source.referenced_files[0].end, 31);
    assert!(source.referenced_files[0].preserve);

    assert_eq!(source.lib_reference_directives.len(), 2);
    assert_eq!(source.lib_reference_directives[0].file_name, "es2023");
    assert_eq!(source.lib_reference_directives[1].file_name, "dom");
    assert!(!source.lib_reference_directives[0].preserve);

    assert_eq!(source.type_reference_directives.len(), 1);
    assert_eq!(source.type_reference_directives[0].file_name, "pkg");
    assert!(!source.type_reference_directives[0].preserve);
}

#[test]
fn malformed_triple_slash_reference_reports_the_complete_comment_span() {
    let comment = "/// <reference resolution-mode=\"import\" />";
    let source = parse_source_file(
        "/index.ts".to_owned(),
        format!("{comment}\nexport {{}};"),
        ParseOptions::default(),
        None,
    );

    assert_eq!(source.parse_diagnostics.len(), 1);
    let diagnostic = &source.parse_diagnostics[0];
    assert_eq!(
        diagnostic.code(),
        gen::Invalid_reference_directive_syntax.code
    );
    assert_eq!(diagnostic.start, Some(0));
    assert_eq!(diagnostic.length, Some(comment.len() as u32));
    assert!(source.referenced_files.is_empty());
    assert!(source.type_reference_directives.is_empty());
    assert!(source.lib_reference_directives.is_empty());
}

#[test]
fn triple_slash_attributes_use_javascript_whitespace_boundaries() {
    let source = parse_source_file(
        "/index.ts".to_owned(),
        concat!(
            "/// <reference\u{FEFF}path\u{FEFF}=\u{FEFF}\"./dependency.ts\" />\n",
            "/// <reference\u{0085}path=\"ignored.ts\" />\n",
            "export {};",
        )
        .to_owned(),
        ParseOptions::default(),
        None,
    );

    assert!(source.parse_diagnostics.is_empty());
    assert_eq!(source.referenced_files.len(), 1);
    assert_eq!(source.referenced_files[0].file_name, "./dependency.ts");
}

#[test]
fn malformed_pragma_attribute_does_not_hide_a_later_valid_duplicate() {
    let source = parse_source_file(
        "/index.ts".to_owned(),
        "/// <reference types=\"broken types='good' />\nexport {};".to_owned(),
        ParseOptions::default(),
        None,
    );

    assert!(source.parse_diagnostics.is_empty());
    assert_eq!(source.type_reference_directives.len(), 1);
    assert_eq!(source.type_reference_directives[0].file_name, "good");
}

#[test]
fn jsx_runtime_pragmas_are_limited_to_recognized_leading_multiline_comments() {
    let source = parse_source_file(
        "/index.tsx".to_owned(),
        concat!(
            "/** @jsxImportSource preact */\n",
            "/*\n * @jsxRuntime automatic\n */\n",
            "const value = 1;",
        )
        .to_owned(),
        ParseOptions::default(),
        None,
    );
    assert!(source.has_jsx_import_source_pragma);
    assert_eq!(source.jsx_import_source_pragma.as_deref(), Some("preact"));
    assert!(source.has_jsx_runtime_pragma);
    assert_eq!(source.jsx_runtime_pragma.as_deref(), Some("automatic"));

    let final_pragma = parse_source_file(
        "/final.tsx".to_owned(),
        concat!(
            "/** @jsxRuntime classic */\n",
            "/** @jsxImportSource @emotion/react */\n",
            "/** @jsxRuntime automatic */\n",
            "/** @jsxRuntime */\n",
            "const value = 1;",
        )
        .to_owned(),
        ParseOptions::default(),
        None,
    );
    assert_eq!(
        final_pragma.jsx_import_source_pragma.as_deref(),
        Some("@emotion/react")
    );
    assert_eq!(
        final_pragma.jsx_runtime_pragma.as_deref(),
        Some("automatic")
    );

    for text in [
        "// @jsxRuntime automatic\nconst value = 1;",
        "/* @unknown value @jsxRuntime automatic */\nconst value = 1;",
        "const text = '@jsxRuntime automatic';\n/** @jsxImportSource preact */",
    ] {
        let control = parse_source_file(
            "/control.tsx".to_owned(),
            text.to_owned(),
            ParseOptions::default(),
            None,
        );
        assert!(!control.has_jsx_import_source_pragma, "{text:?}");
        assert!(!control.has_jsx_runtime_pragma, "{text:?}");
    }

    let unknown_consumes_next_line = parse_source_file(
        "/unknown.tsx".to_owned(),
        "/**\n * @unknown\n * @jsxRuntime automatic\n */".to_owned(),
        ParseOptions::default(),
        None,
    );
    assert!(!unknown_consumes_next_line.has_jsx_import_source_pragma);
    assert!(!unknown_consumes_next_line.has_jsx_runtime_pragma);

    let runtime_consumes_next_line = parse_source_file(
        "/runtime.tsx".to_owned(),
        "/**\n * @jsxRuntime\n * @jsxImportSource preact\n */".to_owned(),
        ParseOptions::default(),
        None,
    );
    assert!(runtime_consumes_next_line.has_jsx_runtime_pragma);
    assert!(!runtime_consumes_next_line.has_jsx_import_source_pragma);

    let trailing_whitespace = parse_source_file(
        "/trailing.tsx".to_owned(),
        "/**\n * @jsxRuntime   \n */".to_owned(),
        ParseOptions::default(),
        None,
    );
    assert!(trailing_whitespace.has_jsx_runtime_pragma);

    let unterminated = parse_source_file(
        "/unterminated.tsx".to_owned(),
        "/* @jsxRuntime automatic".to_owned(),
        ParseOptions::default(),
        None,
    );
    assert!(unterminated.has_jsx_runtime_pragma);
    assert!(unterminated
        .parse_diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code() == 1010));
}

#[test]
fn triple_slash_resolution_mode_honors_pragma_precedence_and_leading_scope() {
    for text in [
        "/// <reference types=\"pkg\" resolution-mode=\"import\"/>\nexport {};",
        "/// <reference types=\"pkg\" resolution-mode=\"require\"/>\nexport {};",
        "/// <reference types=\"pkg\" resolution-mode=\"\"/>\nexport {};",
        "/// <reference path=\"pkg\" resolution-mode=\"esm\"/>\nexport {};",
        "/// <reference no-default-lib=\"true\" types=\"pkg\" resolution-mode=\"esm\"/>\nexport {};",
        "export {};\n/// <reference types=\"pkg\" resolution-mode=\"esm\"/>",
        "///\u{0085}<reference types=\"pkg\" resolution-mode=\"esm\"/>\nexport {};",
    ] {
        let source = parse_source_file(
            "/index.ts".to_owned(),
            text.to_owned(),
            ParseOptions::default(),
            None,
        );
        assert!(
            source.parse_diagnostics.is_empty(),
            "{text:?}: {:?}",
            source.parse_diagnostics
        );
    }

    let source = parse_source_file(
        "/index.ts".to_owned(),
        "/* leading */\n/// <REFERENCE TYPES='pkg' RESOLUTION-MODE='esm'/>\nexport {};".to_owned(),
        ParseOptions::default(),
        None,
    );
    assert_eq!(source.parse_diagnostics.len(), 1);
    assert_eq!(source.parse_diagnostics[0].code(), 1453);

    for text in [
        "///\u{FEFF}<reference types=\"pkg\" resolution-mode=\"esm\"/>\nexport {};",
        "\u{200B}/// <reference types=\"pkg\" resolution-mode=\"esm\"/>\nexport {};",
    ] {
        let source = parse_source_file(
            "/index.ts".to_owned(),
            text.to_owned(),
            ParseOptions::default(),
            None,
        );
        assert_eq!(
            source.parse_diagnostics.len(),
            1,
            "{text:?}: {:?}",
            source.parse_diagnostics
        );
        assert_eq!(source.parse_diagnostics[0].code(), 1453);
    }
}

#[test]
fn matched_bracket_error_points_back_to_the_open_token() {
    let text = "if (true { }";
    let source = parse_source_file(
        "a.ts".to_owned(),
        text.to_owned(),
        ParseOptions::default(),
        None,
    );
    let diagnostic = source
        .parse_diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic.code() == 1005 && diagnostic.message_text() == "')' expected."
        })
        .expect("the missing close parenthesis is reported");
    assert_eq!(diagnostic.related.len(), 1);
    let related = &diagnostic.related[0];
    assert_eq!(related.message.code, 1007);
    assert_eq!(related.start, Some(text.find('(').unwrap() as u32));
    assert_eq!(related.length, Some(1));
    assert_eq!(
        related.message.text,
        "The parser expected to find a ')' to match the '(' token here."
    );
}

#[test]
fn import_attribute_brace_errors_retain_their_exact_open_tokens() {
    for text in [
        "type T = import(\"x\", { with: { type: \"json\" } );",
        "type T = import(\"x\", { \"resolution-mode\": \"require\" });",
        "import value from \"x\" with { type: \"json\";",
    ] {
        let source = parse_source_file(
            "a.ts".to_owned(),
            text.to_owned(),
            ParseOptions::default(),
            None,
        );
        let diagnostic = source
            .parse_diagnostics
            .iter()
            .find(|diagnostic| {
                diagnostic.code() == 1005
                    && matches!(
                        diagnostic.message_text(),
                        "'}' expected." | "'with' expected."
                    )
            })
            .expect("the malformed attribute object is reported");
        assert_eq!(diagnostic.related.len(), 1, "{text:?}");
        let related = &diagnostic.related[0];
        assert_eq!(related.message.code, 1007);
        assert_eq!(
            related.start,
            Some(text.find('{').unwrap() as u32),
            "{text:?}"
        );
        assert_eq!(related.length, Some(1));
        assert_eq!(
            related.message.text,
            "The parser expected to find a '}' to match the '{' token here."
        );
    }
}

#[test]
fn missing_semicolon_reports_spelling_suggestions() {
    let source = parse_source_file(
        "a.ts".to_owned(),
        "interfaz Foo {}\nvar x = 1;\nnamespacefoo Bar {}".to_owned(),
        ParseOptions::default(),
        None,
    );

    let codes: Vec<u32> = source
        .parse_diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code())
        .collect();
    // `interfaz` levenshteins to `interface`; `namespacefoo` splits into
    // `namespace foo` via the space suggestion.
    assert!(
        codes
            .iter()
            .filter(|&&code| code == gen::Unknown_keyword_or_identifier_Did_you_mean_0.code)
            .count()
            >= 2,
        "{:?}",
        source.parse_diagnostics
    );
}

#[test]
fn jsdoc_typedef_properties_and_satisfies_are_arena_nodes() {
    let text = "/**\r\n\
                * 😀 description\r\n\
                * @typedef {Object} Required\r\n\
                * @property {number} required\r\n\
                */\r\n\
                const value = /** @satisfies {Required} */ ({});\r\n";
    let source = parse_source_file(
        "a.js".to_owned(),
        text.to_owned(),
        ParseOptions {
            javascript_file: true,
            ..ParseOptions::default()
        },
        None,
    );
    let root = source
        .arena
        .node(source.root)
        .data
        .as_source_file()
        .expect("source file");
    let statement = source
        .arena
        .node_array(root.statements.expect("statements"))
        .nodes[0];
    let doc_array = source
        .arena
        .node(statement)
        .js_doc
        .expect("statement JSDoc");
    let doc = source.arena.node_array(doc_array).nodes[0];
    assert_eq!(source.arena.node(doc).parent, Some(statement));
    assert!(NodeFlags::from_bits(source.arena.node(doc).flags).contains(NodeFlags::JS_DOC));
    let NodeData::JSDoc(data) = &source.arena.node(doc).data else {
        panic!("JSDoc root");
    };
    let tags = &source
        .arena
        .node_array(data.tags.expect("JSDoc tags"))
        .nodes;
    assert_eq!(tags.len(), 1);
    let typedef = tags[0];
    let NodeData::JSDocTypedefTag(data) = &source.arena.node(typedef).data else {
        panic!("typedef tag");
    };
    assert_eq!(source.arena.node(typedef).parent, Some(doc));
    let type_literal = data.type_expression.expect("typedef type literal");
    let NodeData::JSDocTypeLiteral(data) = &source.arena.node(type_literal).data else {
        panic!("JSDoc type literal");
    };
    let properties = &source
        .arena
        .node_array(data.js_doc_property_tags.expect("properties"))
        .nodes;
    assert_eq!(properties.len(), 1);
    let property = properties[0];
    let property_start = text.find("@property").expect("property");
    let comment_close = text.find("*/").expect("comment close");
    assert_eq!(
        (
            source.arena.node(property).pos as usize,
            source.arena.node(property).end as usize,
            source.arena.node(property).parent,
        ),
        (property_start, comment_close, Some(type_literal))
    );

    let paren = source
        .arena
        .node_ids()
        .find(|&node| {
            source.arena.node(node).kind == SyntaxKind::ParenthesizedExpression
                && source.arena.node(node).js_doc.is_some()
        })
        .expect("inline JSDoc host");
    let inline_doc = source
        .arena
        .node_array(source.arena.node(paren).js_doc.expect("inline docs"))
        .nodes[0];
    let NodeData::JSDoc(inline) = &source.arena.node(inline_doc).data else {
        panic!("inline JSDoc");
    };
    let satisfies = source
        .arena
        .node_array(inline.tags.expect("inline tag"))
        .nodes[0];
    let NodeData::JSDocSatisfiesTag(satisfies_data) = &source.arena.node(satisfies).data else {
        panic!("satisfies tag");
    };
    let type_expression = satisfies_data
        .type_expression
        .expect("satisfies type expression");
    let tag_name = satisfies_data.tag_name.expect("tag name");
    let NodeData::JSDocTypeExpression(type_expression_data) =
        &source.arena.node(type_expression).data
    else {
        panic!("JSDoc type expression");
    };
    let target = type_expression_data.r#type.expect("satisfies target");
    assert!(NodeFlags::from_bits(source.arena.node(target).flags).contains(NodeFlags::JS_DOC));
    assert_eq!(
        &text[source.arena.node(tag_name).pos as usize..source.arena.node(tag_name).end as usize],
        "satisfies"
    );
    assert_eq!(source.arena.node(satisfies).parent, Some(inline_doc));
    assert_eq!(source.arena.node(inline_doc).parent, Some(paren));
}

#[test]
fn no_jsdoc_source_allocates_no_jsdoc_nodes_or_attachments() {
    let mut text = String::new();
    for index in 0..512 {
        use std::fmt::Write;
        writeln!(text, "const value_{index} = {index};").expect("write source");
    }
    let source = parse_source_file(
        "large.js".to_owned(),
        text,
        ParseOptions {
            javascript_file: true,
            ..ParseOptions::default()
        },
        None,
    );

    assert!(source.arena.nodes().iter().all(|node| {
        node.js_doc.is_none() && !NodeFlags::from_bits(node.flags).contains(NodeFlags::JS_DOC)
    }));
}

#[test]
fn jsdoc_parsing_modes_match_tsc_script_kind_rules() {
    fn parse(
        text: &str,
        javascript_file: bool,
        js_doc_parsing_mode: JSDocParsingMode,
    ) -> SourceFile {
        parse_source_file(
            if javascript_file {
                "a.js".to_owned()
            } else {
                "a.ts".to_owned()
            },
            text.to_owned(),
            ParseOptions {
                javascript_file,
                js_doc_parsing_mode,
                ..ParseOptions::default()
            },
            None,
        )
    }

    fn function_has_jsdoc(source: &SourceFile) -> bool {
        source
            .arena
            .nodes()
            .iter()
            .find(|node| node.kind == SyntaxKind::FunctionDeclaration)
            .is_some_and(|node| node.js_doc.is_some())
    }

    let ordinary = "/** @param {number} x */\nfunction f(x) {}";
    let deprecated = "/** @deprecated use g instead */\nfunction f() {}";
    let see = "/** @SeE f */\nfunction f() {}";
    let link = "/** {@LiNk f} */\nfunction f() {}";

    let all = parse(ordinary, false, JSDocParsingMode::ParseAll);
    assert!(function_has_jsdoc(&all));
    assert_eq!(all.js_doc_parsing_mode, JSDocParsingMode::ParseAll);

    assert!(!function_has_jsdoc(&parse(
        ordinary,
        false,
        JSDocParsingMode::ParseNone,
    )));
    assert!(!function_has_jsdoc(&parse(
        ordinary,
        false,
        JSDocParsingMode::ParseForTypeErrors,
    )));
    assert!(!function_has_jsdoc(&parse(
        ordinary,
        false,
        JSDocParsingMode::ParseForTypeInfo,
    )));
    assert!(function_has_jsdoc(&parse(
        see,
        false,
        JSDocParsingMode::ParseForTypeErrors,
    )));
    assert!(function_has_jsdoc(&parse(
        link,
        false,
        JSDocParsingMode::ParseForTypeErrors,
    )));

    // Both reduced modes still parse all JSDoc in JS/JSX.
    assert!(function_has_jsdoc(&parse(
        ordinary,
        true,
        JSDocParsingMode::ParseForTypeErrors,
    )));
    assert!(function_has_jsdoc(&parse(
        ordinary,
        true,
        JSDocParsingMode::ParseForTypeInfo,
    )));

    let deprecated = parse(deprecated, false, JSDocParsingMode::ParseAll);
    let function = deprecated
        .arena
        .nodes()
        .iter()
        .find(|node| node.kind == SyntaxKind::FunctionDeclaration)
        .expect("function declaration");
    assert!(NodeFlags::from_bits(function.flags).contains(NodeFlags::DEPRECATED));
}
