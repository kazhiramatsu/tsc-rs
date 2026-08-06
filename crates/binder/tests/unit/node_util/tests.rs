use super::*;
use tsc_syntax::{parse_source_file, ParseOptions};

#[test]
fn assigned_expression_names_include_static_property_and_element_accesses() {
    let source = parse_source_file(
        "a.js",
        "ns.member = function() {};\nns['key'] = class {};\n",
        ParseOptions {
            javascript_file: true,
            ..ParseOptions::default()
        },
        None,
    );
    let function = (0..source.arena.len() as u32)
        .map(NodeId)
        .find(|&node| kind_of(&source, node) == SyntaxKind::FunctionExpression)
        .expect("function expression");
    let class = (0..source.arena.len() as u32)
        .map(NodeId)
        .find(|&node| kind_of(&source, node) == SyntaxKind::ClassExpression)
        .expect("class expression");

    let function_name = get_name_of_declaration(&source, function).expect("property name");
    let class_name = get_name_of_declaration(&source, class).expect("element name");
    assert!(matches!(
        &source.arena.node(function_name).data,
        NodeData::Identifier(data) if data.escaped_text == "member"
    ));
    assert!(matches!(
        &source.arena.node(class_name).data,
        NodeData::StringLiteral(data) if data.text == "key"
    ));
}

#[test]
fn token_spans_convert_scanner_utf16_offsets_back_to_exact_bytes() {
    let source = parse_source_file(
        "unicode.ts",
        "\u{feff}/* 😀 */return;",
        ParseOptions::default(),
        None,
    );
    let (start, end) = get_span_of_token_at_position(&source, 0);

    assert!(source.text().is_char_boundary(start));
    assert!(source.text().is_char_boundary(end));
    assert_eq!(&source.text()[start..end], "return");
}
