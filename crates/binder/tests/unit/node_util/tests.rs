use super::*;
use tsc_syntax::{parse_source_file, ParseOptions};

#[test]
fn assigned_expression_names_include_static_property_and_element_accesses() {
    let source = parse_source_file(
        "a.js",
        "ns.member = function() {};\nns['key'] = class {};\n",
        ParseOptions {
            script_kind: tsc_syntax::ScriptKind::Js,
            ..ParseOptions::default()
        },
        None,
    );
    let function = (0..source.arena.len() as u32)
        .map(NodeId::new)
        .find(|&node| kind_of(&source, node) == SyntaxKind::FunctionExpression)
        .expect("function expression");
    let class = (0..source.arena.len() as u32)
        .map(NodeId::new)
        .find(|&node| kind_of(&source, node) == SyntaxKind::ClassExpression)
        .expect("class expression");

    let function_name = get_name_of_declaration(&source, function).expect("property name");
    let class_name = get_name_of_declaration(&source, class).expect("element name");
    assert!(matches!(
        &source.arena.node(function_name).data(),
        NodeData::Identifier(data) if data.escaped_text == "member"
    ));
    assert!(matches!(
        &source.arena.node(class_name).data(),
        NodeData::StringLiteral(data) if source.arena.text(data.text) == b"key"
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

#[test]
fn payload_predicates_reject_unrelated_declarations_with_tails() {
    let source = parse_source_file(
        "predicates.ts",
        r#"
declare module "ambient" { export const value: number; }
namespace Local { export const value = 1; }
/** A declaration with parameters and a body. */
function f<T>(value: T): T { return value; }
interface Shape { readonly field: string; method(value: string): void; }
class Container { field = 1; method(value: number) { return value; } }
"#,
        ParseOptions::default(),
        None,
    );
    assert!(source.parse_diagnostics.is_empty());
    let mut modules = Vec::new();
    let mut declarations = 0;
    for index in 0..source.arena.len() as u32 {
        let id = NodeId::new(index);
        match kind_of(&source, id) {
            SyntaxKind::ModuleDeclaration => modules.push(id),
            SyntaxKind::FunctionDeclaration
            | SyntaxKind::InterfaceDeclaration
            | SyntaxKind::ClassDeclaration
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::MethodSignature
            | SyntaxKind::Parameter => {
                declarations += 1;
                assert!(!is_ambient_module(&source, id));
                assert!(!is_dynamic_name(&source, id));
                assert!(!is_narrowable_reference(&source, id));
                assert_eq!(get_escaped_text_of_identifier_or_literal(&source, id), None);
                assert_eq!(
                    crate::assignment::get_assignment_declaration_kind(&source, id),
                    crate::assignment::AssignmentDeclarationKind::None,
                );
            }
            _ => {}
        }
    }
    assert!(declarations >= 8);
    assert_eq!(modules.len(), 2);
    assert!(is_ambient_module(&source, modules[0]));
    assert!(!is_ambient_module(&source, modules[1]));
}

#[test]
fn narrowing_predicates_keep_nested_access_and_parenthesis_semantics() {
    for (text, expected) in [
        ("((value.member));", true),
        ("value!.member;", true),
        ("value[\"member\"];", true),
        ("value[index];", true),
        ("make().member;", false),
        ("value[true];", false),
    ] {
        let source = parse_source_file("access.ts", text, ParseOptions::default(), None);
        assert!(source.parse_diagnostics.is_empty(), "{text}");
        let statement = source
            .arena
            .nodes()
            .iter()
            .find_map(|node| node.as_expression_statement())
            .expect("expression statement");
        let expression = statement.expression.expect("expression");
        assert_eq!(
            is_narrowable_reference(&source, expression),
            expected,
            "{text}"
        );
        let inner = skip_parentheses_pub(&source, expression);
        assert_ne!(kind_of(&source, inner), SyntaxKind::ParenthesizedExpression);
        assert_eq!(is_narrowable_reference(&source, inner), expected, "{text}");
    }
}
