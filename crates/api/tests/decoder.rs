//! tsgo `api/encoder/decoder_test.go`: tsc-rs's encodings decoded back
//! into tsc-rs's syntax tree, and the shapes tsgo's tree has where tsc's
//! differs.

mod common;

use tsc_api::decoder::{decode_nodes, DecodedTree};
use tsc_api::encoder::{encode_node, ScriptKind};
use tsc_api::parse_source_file;
use tsc_syntax::{LanguageVariant, Node, NodeData, NodeId, SyntaxKind};
use tsc_types::NodeFlags;

fn decode(name: &str, text: &str) -> DecodedTree {
    decode_nodes(&common::encode(name, text)).unwrap()
}

fn node(tree: &DecodedTree, id: NodeId) -> &Node {
    tree.source_file.arena.node(id)
}

fn list(tree: &DecodedTree, id: Option<tsc_syntax::NodeArrayId>) -> Vec<NodeId> {
    tree.source_file
        .arena
        .node_array(id.expect("a list"))
        .nodes
        .to_vec()
}

fn statements(tree: &DecodedTree) -> Vec<NodeId> {
    let NodeData::SourceFile(file) = &node(tree, tree.root).data else {
        panic!("a source file root");
    };
    list(tree, file.statements)
}

fn statement(tree: &DecodedTree, index: usize) -> &Node {
    node(tree, statements(tree)[index])
}

/// The first declaration of the `index`th statement, a variable statement.
fn declaration(tree: &DecodedTree, index: usize) -> &Node {
    let NodeData::VariableStatement(statement) = &statement(tree, index).data else {
        panic!("a variable statement");
    };
    let NodeData::VariableDeclarationList(declarations) =
        &node(tree, statement.declaration_list.unwrap()).data
    else {
        panic!("a declaration list");
    };
    node(tree, list(tree, declarations.declarations)[0])
}

fn initializer(tree: &DecodedTree, index: usize) -> &Node {
    let NodeData::VariableDeclaration(declaration) = &declaration(tree, index).data else {
        panic!("a variable declaration");
    };
    node(tree, declaration.initializer.unwrap())
}

fn expression_statement(tree: &DecodedTree, index: usize) -> &Node {
    let NodeData::ExpressionStatement(statement) = &statement(tree, index).data else {
        panic!("an expression statement");
    };
    node(tree, statement.expression.unwrap())
}

fn identifier_text(tree: &DecodedTree, id: Option<NodeId>) -> String {
    let NodeData::Identifier(identifier) = &node(tree, id.expect("a name")).data else {
        panic!("an identifier");
    };
    identifier.text().to_owned()
}

#[test]
fn basic() {
    let tree = decode("/test.ts", "let x = 1;");
    assert!(tree.is_source_file);
    assert_eq!(node(&tree, tree.root).kind, SyntaxKind::SourceFile);
    assert_eq!(tree.source_file.file_name.as_str(), Some("/test.ts"));
    assert_eq!(tree.source_file.text(), "let x = 1;");
    let NodeData::SourceFile(file) = &node(&tree, tree.root).data else {
        panic!("a source file");
    };
    assert!(file.statements.is_some());
    assert!(file.end_of_file_token.is_some());
}

#[test]
fn metadata() {
    for (name, text) in [
        ("/test.json", r#"{"x": 1}"#),
        ("/test.jsx", "const x = <div />;"),
        ("/test.d.ts", "declare const x: number;"),
    ] {
        let parsed = parse_source_file(name, text, ScriptKind::from_file_name(name));
        let tree = decode(name, text);
        assert_eq!(tree.script_kind, ScriptKind::from_file_name(name), "{name}");
        assert_eq!(
            tree.source_file.language_variant, parsed.language_variant,
            "{name}"
        );
        assert_eq!(
            tree.source_file.is_declaration_file, parsed.is_declaration_file,
            "{name}"
        );
    }
    assert_eq!(
        decode("/test.jsx", "const x = <div />;")
            .source_file
            .language_variant,
        LanguageVariant::Jsx
    );
    assert!(
        decode("/test.d.ts", "declare const x: number;")
            .source_file
            .is_declaration_file
    );
}

#[test]
fn statements_of_a_file() {
    let tree = decode("/test.ts", "let a = 1;\nlet b = 2;\nlet c = 3;");
    let kinds = statements(&tree)
        .into_iter()
        .map(|id| node(&tree, id).kind)
        .collect::<Vec<_>>();
    assert_eq!(kinds, [SyntaxKind::VariableStatement; 3]);
}

#[test]
fn variable_declaration() {
    let tree = decode("/test.ts", "let x = 1;");
    let NodeData::VariableDeclaration(declaration) = &declaration(&tree, 0).data else {
        panic!("a variable declaration");
    };
    assert_eq!(identifier_text(&tree, declaration.name), "x");
    let NodeData::NumericLiteral(literal) = &initializer(&tree, 0).data else {
        panic!("a numeric literal");
    };
    assert_eq!(literal.text, "1");
}

#[test]
fn variable_declaration_list_flags() {
    for (text, expected) in [
        ("const x = 1;", NodeFlags::CONST),
        ("let x = 1;", NodeFlags::LET),
        ("var x = 1;", NodeFlags::NONE),
    ] {
        let tree = decode("/test.ts", text);
        let NodeData::VariableStatement(statement) = &statement(&tree, 0).data else {
            panic!("a variable statement");
        };
        let flags = node(&tree, statement.declaration_list.unwrap()).flags;
        assert_eq!(
            NodeFlags::from_bits(flags & (NodeFlags::LET | NodeFlags::CONST).bits()),
            expected,
            "{text}"
        );
    }
}

#[test]
fn function_declaration() {
    let tree = decode(
        "/test.ts",
        "function add(a: number, b: number): number { return a + b; }",
    );
    let NodeData::FunctionDeclaration(function) = &statement(&tree, 0).data else {
        panic!("a function declaration");
    };
    assert_eq!(identifier_text(&tree, function.name), "add");
    let parameters = list(&tree, function.parameters);
    assert_eq!(parameters.len(), 2);
    assert!(function.r#type.is_some());
    assert!(function.body.is_some());
    let NodeData::Parameter(parameter) = &node(&tree, parameters[0]).data else {
        panic!("a parameter");
    };
    assert_eq!(identifier_text(&tree, parameter.name), "a");
    assert!(parameter.r#type.is_some());
}

#[test]
fn import_declaration() {
    let tree = decode("/test.ts", r#"import { bar } from "bar";"#);
    let NodeData::ImportDeclaration(import) = &statement(&tree, 0).data else {
        panic!("an import declaration");
    };
    let NodeData::StringLiteral(specifier) = &node(&tree, import.module_specifier.unwrap()).data
    else {
        panic!("a string literal");
    };
    assert_eq!(specifier.text.as_str(), Some("bar"));
    let NodeData::ImportClause(clause) = &node(&tree, import.import_clause.unwrap()).data else {
        panic!("an import clause");
    };
    let NodeData::NamedImports(named) = &node(&tree, clause.named_bindings.unwrap()).data else {
        panic!("named imports");
    };
    let elements = list(&tree, named.elements);
    assert_eq!(elements.len(), 1);
    let NodeData::ImportSpecifier(specifier) = &node(&tree, elements[0]).data else {
        panic!("an import specifier");
    };
    assert_eq!(identifier_text(&tree, specifier.name), "bar");
}

#[test]
fn if_statement() {
    let tree = decode("/test.ts", "if (true) { } else { }");
    let NodeData::IfStatement(statement) = &statement(&tree, 0).data else {
        panic!("an if statement");
    };
    assert!(statement.expression.is_some());
    assert_eq!(
        node(&tree, statement.then_statement.unwrap()).kind,
        SyntaxKind::Block
    );
    assert_eq!(
        node(&tree, statement.else_statement.unwrap()).kind,
        SyntaxKind::Block
    );
}

#[test]
fn template_expression() {
    let tree = decode("/test.ts", "let x = `hello ${name} world`;");
    let NodeData::TemplateExpression(template) = &initializer(&tree, 0).data else {
        panic!("a template expression");
    };
    let NodeData::TemplateHead(head) = &node(&tree, template.head.unwrap()).data else {
        panic!("a template head");
    };
    assert_eq!(head.text.as_str(), Some("hello "));
    let spans = list(&tree, template.template_spans);
    assert_eq!(spans.len(), 1);
    let NodeData::TemplateSpan(span) = &node(&tree, spans[0]).data else {
        panic!("a template span");
    };
    assert_eq!(
        node(&tree, span.expression.unwrap()).kind,
        SyntaxKind::Identifier
    );
    let NodeData::TemplateTail(tail) = &node(&tree, span.literal.unwrap()).data else {
        panic!("a template tail");
    };
    assert_eq!(tail.text.as_str(), Some(" world"));
}

#[test]
fn export_modifier() {
    let tree = decode("/test.ts", "export function foo() {}");
    let NodeData::FunctionDeclaration(function) = &statement(&tree, 0).data else {
        panic!("a function declaration");
    };
    let modifiers = list(&tree, function.modifiers);
    assert_eq!(modifiers.len(), 1);
    assert_eq!(node(&tree, modifiers[0]).kind, SyntaxKind::ExportKeyword);
}

#[test]
fn positions() {
    let text = "let x = 1;";
    let tree = decode("/test.ts", text);
    let root = node(&tree, tree.root);
    assert_eq!((root.pos, root.end), (0, text.len() as u32));
}

#[test]
fn class_declaration() {
    let tree = decode("/test.ts", "class Foo { bar(): void {} }");
    let NodeData::ClassDeclaration(class) = &statement(&tree, 0).data else {
        panic!("a class declaration");
    };
    assert_eq!(identifier_text(&tree, class.name), "Foo");
    let members = list(&tree, class.members);
    assert_eq!(members.len(), 1);
    assert_eq!(node(&tree, members[0]).kind, SyntaxKind::MethodDeclaration);
}

#[test]
fn subtree_round_trip() {
    let file = parse_source_file(
        "/test.ts",
        "function greet(name: string) { return `Hello, ${name}!`; }",
        ScriptKind::Ts,
    );
    let NodeData::SourceFile(root) = &file.arena.node(file.root).data else {
        panic!("a source file");
    };
    let function = file.arena.node_array(root.statements.unwrap()).nodes[0];
    let tree = decode_nodes(&encode_node(&file, function, &|_| false).0).unwrap();
    assert!(!tree.is_source_file);
    let NodeData::FunctionDeclaration(function) = &node(&tree, tree.root).data else {
        panic!("a function declaration");
    };
    assert_eq!(identifier_text(&tree, function.name), "greet");
    assert_eq!(list(&tree, function.parameters).len(), 1);
    assert!(function.body.is_some());
    // A node of no source file keeps its positions, which index no text.
    let root = node(&tree, tree.root);
    assert_eq!((root.pos, root.end), (0, 58));
    assert!(tree.source_file.positions().is_detached());
}

#[test]
fn binary_expression() {
    let tree = decode("/test.ts", "let x = 1 + 2;");
    let NodeData::BinaryExpression(binary) = &initializer(&tree, 0).data else {
        panic!("a binary expression");
    };
    assert_eq!(
        node(&tree, binary.left.unwrap()).kind,
        SyntaxKind::NumericLiteral
    );
    assert_eq!(
        node(&tree, binary.operator_token.unwrap()).kind,
        SyntaxKind::PlusToken
    );
    assert_eq!(
        node(&tree, binary.right.unwrap()).kind,
        SyntaxKind::NumericLiteral
    );
}

#[test]
fn keyword_expressions() {
    // tsgo's KeywordExpression is tsc's token.
    let tree = decode("/test.ts", "const x = this;");
    let this = initializer(&tree, 0);
    assert_eq!(this.kind, SyntaxKind::ThisKeyword);
    assert!(matches!(this.data, NodeData::Token));
}

#[test]
fn empty_module_block() {
    let tree = decode("/test.ts", "namespace N { }");
    let NodeData::ModuleDeclaration(module) = &statement(&tree, 0).data else {
        panic!("a module declaration");
    };
    let NodeData::ModuleBlock(block) = &node(&tree, module.body.unwrap()).data else {
        panic!("a module block");
    };
    assert!(list(&tree, block.statements).is_empty());
}

#[test]
fn empty_block_and_parameters() {
    let tree = decode("/test.ts", "function foo() {}");
    let NodeData::FunctionDeclaration(function) = &statement(&tree, 0).data else {
        panic!("a function declaration");
    };
    assert!(list(&tree, function.parameters).is_empty());
    let NodeData::Block(block) = &node(&tree, function.body.unwrap()).data else {
        panic!("a block");
    };
    assert!(list(&tree, block.statements).is_empty());
}

#[test]
fn arrow_function_empty_parameters() {
    let tree = decode("/test.ts", "const f = () => {};");
    let NodeData::ArrowFunction(arrow) = &initializer(&tree, 0).data else {
        panic!("an arrow function");
    };
    assert!(list(&tree, arrow.parameters).is_empty());
    let NodeData::Block(block) = &node(&tree, arrow.body.unwrap()).data else {
        panic!("a block");
    };
    assert!(list(&tree, block.statements).is_empty());
}

#[test]
fn function_expression_empty_parameters() {
    let tree = decode("/test.ts", "const f = function() {};");
    let NodeData::FunctionExpression(function) = &initializer(&tree, 0).data else {
        panic!("a function expression");
    };
    assert!(list(&tree, function.parameters).is_empty());
}

#[test]
fn postfix_unary_operator() {
    let tree = decode("/test.ts", "let i = 0; i++;");
    let NodeData::PostfixUnaryExpression(postfix) = &expression_statement(&tree, 1).data else {
        panic!("a postfix unary expression");
    };
    assert_eq!(postfix.operator, SyntaxKind::PlusPlusToken);
    assert_eq!(
        node(&tree, postfix.operand.unwrap()).kind,
        SyntaxKind::Identifier
    );
}

#[test]
fn prefix_unary_operator() {
    let tree = decode("/test.ts", "let x = true; !x;");
    let NodeData::PrefixUnaryExpression(prefix) = &expression_statement(&tree, 1).data else {
        panic!("a prefix unary expression");
    };
    assert_eq!(prefix.operator, SyntaxKind::ExclamationToken);
    assert_eq!(
        node(&tree, prefix.operand.unwrap()).kind,
        SyntaxKind::Identifier
    );
}

#[test]
fn postfix_decrement() {
    let tree = decode("/test.ts", "let n = 5; n--;");
    let NodeData::PostfixUnaryExpression(postfix) = &expression_statement(&tree, 1).data else {
        panic!("a postfix unary expression");
    };
    assert_eq!(postfix.operator, SyntaxKind::MinusMinusToken);
}

#[test]
fn a_type_heritage_reference_is_an_expression_with_type_arguments() {
    // tsgo writes `extends A.B<T>` of an interface as a TypeReference of a
    // QualifiedName.
    let tree = decode("/test.ts", "interface I extends A.B<T> {}");
    let NodeData::InterfaceDeclaration(interface) = &statement(&tree, 0).data else {
        panic!("an interface declaration");
    };
    let clauses = list(&tree, interface.heritage_clauses);
    let NodeData::HeritageClause(clause) = &node(&tree, clauses[0]).data else {
        panic!("a heritage clause");
    };
    assert_eq!(clause.token, SyntaxKind::ExtendsKeyword);
    let element = node(&tree, list(&tree, clause.types)[0]);
    let NodeData::ExpressionWithTypeArguments(element) = &element.data else {
        panic!("an expression with type arguments, not {:?}", element.kind);
    };
    assert_eq!(list(&tree, element.type_arguments).len(), 1);
    let NodeData::PropertyAccessExpression(access) = &node(&tree, element.expression.unwrap()).data
    else {
        panic!("a property access");
    };
    assert_eq!(identifier_text(&tree, access.expression), "A");
    assert_eq!(identifier_text(&tree, access.name), "B");
}

#[test]
fn a_nested_namespace_is_flagged_and_has_no_modifiers() {
    // tsgo gives `B` of `namespace A.B` an implicit `export`.
    let tree = decode("/test.ts", "namespace A.B { }");
    let NodeData::ModuleDeclaration(outer) = &statement(&tree, 0).data else {
        panic!("a module declaration");
    };
    let inner = node(&tree, outer.body.unwrap());
    let NodeData::ModuleDeclaration(inner_data) = &inner.data else {
        panic!("a nested module declaration");
    };
    assert!(inner_data.modifiers.is_none());
    assert!(NodeFlags::from_bits(inner.flags).contains(NodeFlags::NESTED_NAMESPACE));
    assert!(NodeFlags::from_bits(statement(&tree, 0).flags).contains(NodeFlags::NAMESPACE));
}

#[test]
fn an_array_binding_hole_is_an_omitted_expression() {
    let tree = decode("/test.ts", "const [, b] = c;");
    let NodeData::VariableDeclaration(declaration) = &declaration(&tree, 0).data else {
        panic!("a variable declaration");
    };
    let NodeData::ArrayBindingPattern(pattern) = &node(&tree, declaration.name.unwrap()).data
    else {
        panic!("an array binding pattern");
    };
    let elements = list(&tree, pattern.elements);
    assert_eq!(node(&tree, elements[0]).kind, SyntaxKind::OmittedExpression);
    assert_eq!(node(&tree, elements[1]).kind, SyntaxKind::BindingElement);
}

#[test]
fn literals_keep_the_facts_tsgo_prints_from() {
    let tree = decode("/test.ts", "let s = 'x';\nlet r = /a\nlet t = `a${b}`;");
    assert_eq!(tree.single_quoted, [statements_literal(&tree)]);
    let regex = initializer(&tree, 1);
    assert_eq!(regex.kind, SyntaxKind::RegularExpressionLiteral);
    assert_eq!(regex.is_unterminated(), Some(true));
    let NodeData::TemplateExpression(template) = &initializer(&tree, 2).data else {
        panic!("a template expression");
    };
    let NodeData::TemplateHead(head) = &node(&tree, template.head.unwrap()).data else {
        panic!("a template head");
    };
    assert_eq!(head.raw_text.as_deref(), Some("a"));
}

/// The string literal initializer of the first statement.
fn statements_literal(tree: &DecodedTree) -> NodeId {
    let NodeData::VariableDeclaration(declaration) = &declaration(tree, 0).data else {
        panic!("a variable declaration");
    };
    declaration.initializer.unwrap()
}

#[test]
fn errors_are_tsgos() {
    let data = common::encode("/test.ts", "let x = 1;");
    let error = |data: &[u8]| decode_nodes(data).err().unwrap();
    assert_eq!(error(&data[..10]), "data too short for header: 10 bytes");
    let mut version = data.clone();
    version[3] = 8;
    assert_eq!(
        error(&version),
        "unsupported protocol version 8 (expected 9)"
    );
    let mut offsets = data.clone();
    offsets[24..28].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        error(&offsets),
        format!(
            "invalid AST header offsets: offsets exceed data length ({})",
            data.len()
        )
    );
    let nodes = u32::from_le_bytes(data[40..44].try_into().unwrap()) as usize;
    assert_eq!(error(&data[..nodes + 28]), "no nodes to decode");
}
