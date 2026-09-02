use super::*;

fn parsed(name: &str, text: &str) -> SourceFile {
    tsc_syntax::parse_source_file(name.to_owned(), text.to_owned(), Default::default(), None)
}

#[test]
fn reuse_clone_accepts_cross_source_original_within_one_arena_only() {
    let first = parsed("first.ts", "export const first = 1;\n");
    let second = parsed("second.ts", "export const second = 2;\n");
    let mut arena = TransformArena::new();
    let first_source = arena.add_source(&first, Some(SourceFileId::from_raw(11)));
    let second_source = arena.add_source(&second, Some(SourceFileId::from_raw(22)));
    let first_root = arena.root(first_source).expect("first root");
    let second_root = arena.root(second_source).expect("second root");

    let reused = arena.factory().clone_node(first_root).expect("reuse clone");
    arena
        .set_original_node(reused, Some(second_root))
        .expect("same-arena cross-source original");
    assert_eq!(
        arena
            .parse_tree_resolver_node(reused)
            .expect("original projection"),
        Some(EmitResolverNode::new(
            SourceFileId::from_raw(22),
            second_root.node(),
        )),
    );

    let rehomed = arena
        .factory()
        .clone_node_to_source(second_root, first_source)
        .expect("same-arena cross-source reuse clone");
    assert_eq!(rehomed.source(), first_source);
    assert_eq!(
        arena
            .parse_tree_resolver_node(rehomed)
            .expect("rehome projection"),
        Some(EmitResolverNode::new(
            SourceFileId::from_raw(22),
            second_root.node(),
        )),
    );

    let mut foreign_arena = TransformArena::new();
    foreign_arena.add_source(&first, None);
    foreign_arena.add_source(&second, None);
    let foreign_source = foreign_arena.add_source(&first, None);
    let foreign = foreign_arena.root(foreign_source).expect("foreign root");
    assert_eq!(
        arena.set_original_node(reused, Some(foreign)),
        Err(TransformError::UnknownSource(foreign_source)),
    );
    assert_eq!(
        arena.factory().clone_node_to_source(foreign, first_source),
        Err(TransformError::UnknownSource(foreign_source)),
    );
}

fn synthetic_arena() -> (TransformArena, TransformSourceId) {
    let source_file = parsed("factory.ts", "");
    let mut arena = TransformArena::new();
    let source = arena.add_source(&source_file, None);
    (arena, source)
}

fn assert_parenthesized(factory: &NodeFactory<'_>, result: TransformNode, original: TransformNode) {
    assert_ne!(result, original, "rule must allocate a fresh wrapper");
    let NodeData::ParenthesizedType(data) = &factory.arena.node(result).unwrap().data else {
        panic!("expected ParenthesizedType")
    };
    assert_eq!(data.r#type, Some(original.node()));
}

#[test]
fn type_parenthesizer_rule_tables_cover_direct_and_delegated_kinds() {
    let (mut arena, source) = synthetic_arena();
    let mut factory = arena.factory();
    let any = factory
        .create_keyword_type_node(source, SyntaxKind::AnyKeyword)
        .unwrap();
    let empty = factory.create_node_array(source, Vec::new()).unwrap();
    let function = factory
        .create_function_type_node(source, None, empty, any)
        .unwrap();
    let empty = factory.create_node_array(source, Vec::new()).unwrap();
    let constructor = factory
        .create_constructor_type_node(source, None, None, empty, any)
        .unwrap();
    let conditional = factory
        .create_conditional_type_node(source, any, any, any, any)
        .unwrap();
    let members = factory.create_node_array(source, vec![any]).unwrap();
    let union = factory.create_union_type_node(source, members).unwrap();
    let members = factory.create_node_array(source, vec![any]).unwrap();
    let intersection = factory
        .create_intersection_type_node(source, members)
        .unwrap();
    let parameter_name = factory.create_identifier(source, "T").unwrap();
    let parameter = factory
        .create_type_parameter_declaration(source, None, parameter_name, None, None)
        .unwrap();
    let infer = factory.create_infer_type_node(source, parameter).unwrap();
    let operator = factory
        .create_type_operator_node(source, SyntaxKind::KeyOfKeyword, any)
        .unwrap();
    let query_name = factory.create_identifier(source, "value").unwrap();
    let query = factory
        .create_type_query_node(source, query_name, None)
        .unwrap();
    let nullable = factory
        .create_node(
            source,
            NodeData::JSDocNullableType(JSDocNullableTypeData {
                r#type: Some(any.node()),
                postfix: true,
            }),
            TransformFlags::CONTAINS_TYPE_SCRIPT,
        )
        .unwrap();

    for node in [function, constructor, conditional] {
        let result =
            TypeParenthesizer::parenthesize_check_type_of_conditional_type(&mut factory, node)
                .unwrap();
        assert_parenthesized(&factory, result, node);
    }
    assert_eq!(
        TypeParenthesizer::parenthesize_check_type_of_conditional_type(&mut factory, any).unwrap(),
        any,
    );
    let result =
        TypeParenthesizer::parenthesize_extends_type_of_conditional_type(&mut factory, conditional)
            .unwrap();
    assert_parenthesized(&factory, result, conditional);
    assert_eq!(
        TypeParenthesizer::parenthesize_extends_type_of_conditional_type(&mut factory, any)
            .unwrap(),
        any,
    );

    for node in [union, intersection, function] {
        let result =
            TypeParenthesizer::parenthesize_constituent_type_of_union_type(&mut factory, node)
                .unwrap();
        assert_parenthesized(&factory, result, node);
    }
    assert_eq!(
        TypeParenthesizer::parenthesize_constituent_type_of_union_type(&mut factory, any).unwrap(),
        any,
    );
    for node in [union, intersection, constructor] {
        let result = TypeParenthesizer::parenthesize_constituent_type_of_intersection_type(
            &mut factory,
            node,
        )
        .unwrap();
        assert_parenthesized(&factory, result, node);
    }
    assert_eq!(
        TypeParenthesizer::parenthesize_constituent_type_of_intersection_type(&mut factory, any)
            .unwrap(),
        any,
    );
    let result =
        TypeParenthesizer::parenthesize_operand_of_type_operator(&mut factory, intersection)
            .unwrap();
    assert_parenthesized(&factory, result, intersection);
    assert_eq!(
        TypeParenthesizer::parenthesize_operand_of_type_operator(&mut factory, any).unwrap(),
        any,
    );
    let result =
        TypeParenthesizer::parenthesize_operand_of_readonly_type_operator(&mut factory, operator)
            .unwrap();
    assert_parenthesized(&factory, result, operator);
    assert_eq!(
        TypeParenthesizer::parenthesize_operand_of_readonly_type_operator(&mut factory, any)
            .unwrap(),
        any,
    );
    for node in [infer, operator, query, union] {
        let result =
            TypeParenthesizer::parenthesize_non_array_type_of_postfix_type(&mut factory, node)
                .unwrap();
        assert_parenthesized(&factory, result, node);
    }
    assert_eq!(
        TypeParenthesizer::parenthesize_non_array_type_of_postfix_type(&mut factory, any).unwrap(),
        any,
    );
    let result =
        TypeParenthesizer::parenthesize_element_type_of_tuple_type(&mut factory, nullable).unwrap();
    assert_parenthesized(&factory, result, nullable);
    assert_eq!(
        TypeParenthesizer::parenthesize_element_type_of_tuple_type(&mut factory, any).unwrap(),
        any,
    );
    let result =
        TypeParenthesizer::parenthesize_type_of_optional_type(&mut factory, query).unwrap();
    assert_parenthesized(&factory, result, query);
    let result =
        TypeParenthesizer::parenthesize_type_of_optional_type(&mut factory, nullable).unwrap();
    assert_parenthesized(&factory, result, nullable);
    assert_eq!(
        TypeParenthesizer::parenthesize_type_of_optional_type(&mut factory, any).unwrap(),
        any,
    );

    let type_parameters = factory.create_node_array(source, vec![parameter]).unwrap();
    let parameters = factory.create_node_array(source, Vec::new()).unwrap();
    let generic_function = factory
        .create_function_type_node(source, Some(type_parameters), parameters, any)
        .unwrap();
    let result =
        TypeParenthesizer::parenthesize_leading_type_argument(&mut factory, generic_function)
            .unwrap();
    assert_parenthesized(&factory, result, generic_function);
    let type_parameters = factory.create_node_array(source, vec![parameter]).unwrap();
    let parameters = factory.create_node_array(source, Vec::new()).unwrap();
    let generic_constructor = factory
        .create_constructor_type_node(source, None, Some(type_parameters), parameters, any)
        .unwrap();
    let result =
        TypeParenthesizer::parenthesize_leading_type_argument(&mut factory, generic_constructor)
            .unwrap();
    assert_parenthesized(&factory, result, generic_constructor);
    assert_eq!(
        TypeParenthesizer::parenthesize_leading_type_argument(&mut factory, any).unwrap(),
        any,
    );
}

#[test]
fn typed_faces_smoke_families_and_preserve_exact_type_flags() {
    let (mut arena, source) = synthetic_arena();
    let mut factory = arena.factory();
    let any = factory
        .create_keyword_type_node(source, SyntaxKind::AnyKeyword)
        .unwrap();
    let await_name = factory.create_identifier(source, "await").unwrap();
    let reference = factory
        .create_type_reference_node(source, await_name, None)
        .unwrap();
    assert_eq!(
        factory.arena.transform_flags(reference),
        TransformFlags::CONTAINS_TYPE_SCRIPT,
    );

    let union_members = factory
        .create_node_array(source, vec![any, reference])
        .unwrap();
    let union = factory
        .create_union_type_node(source, union_members)
        .unwrap();
    assert_eq!(
        factory.arena.transform_flags(union),
        TransformFlags::CONTAINS_TYPE_SCRIPT,
    );
    let array = factory.create_array_type_node(source, union).unwrap();
    assert_eq!(
        factory.arena.transform_flags(array),
        TransformFlags::CONTAINS_TYPE_SCRIPT,
    );
    let NodeData::ArrayType(array_data) = &factory.arena.node(array).unwrap().data else {
        panic!("ArrayType expected")
    };
    let element = TransformNode::new(source, array_data.element_type.unwrap());
    assert_parenthesized(&factory, element, union);

    let tuple_elements = factory.create_node_array(source, vec![any]).unwrap();
    let tuple = factory
        .create_tuple_type_node(source, tuple_elements)
        .unwrap();
    assert_eq!(
        factory.arena.node(tuple).unwrap().kind,
        SyntaxKind::TupleType
    );
    assert_eq!(
        factory.arena.transform_flags(tuple),
        TransformFlags::CONTAINS_TYPE_SCRIPT,
    );
    let conditional = factory
        .create_conditional_type_node(source, any, any, tuple, reference)
        .unwrap();
    assert_eq!(
        factory.arena.node(conditional).unwrap().kind,
        SyntaxKind::ConditionalType,
    );
    assert_eq!(
        factory.arena.transform_flags(conditional),
        TransformFlags::CONTAINS_TYPE_SCRIPT,
    );
    let parameter_name = factory.create_identifier(source, "K").unwrap();
    let parameter = factory
        .create_type_parameter_declaration(source, None, parameter_name, None, None)
        .unwrap();
    let mapped = factory
        .create_mapped_type_node(source, None, parameter, None, None, Some(any), None)
        .unwrap();
    assert_eq!(
        factory.arena.node(mapped).unwrap().kind,
        SyntaxKind::MappedType
    );
    assert_eq!(
        factory.arena.transform_flags(mapped),
        TransformFlags::CONTAINS_TYPE_SCRIPT,
    );
    let module = factory.create_string_literal(source, "pkg", false).unwrap();
    let argument = factory.create_literal_type_node(source, module).unwrap();
    let import_type = factory
        .create_import_type_node(source, argument, None, None, None, false)
        .unwrap();
    assert_eq!(
        factory.arena.node(import_type).unwrap().kind,
        SyntaxKind::ImportType,
    );
    assert_eq!(
        factory.arena.transform_flags(import_type),
        TransformFlags::CONTAINS_TYPE_SCRIPT,
    );

    let parameters = factory.create_node_array(source, Vec::new()).unwrap();
    let call = factory
        .create_call_signature(source, None, parameters, Some(any))
        .unwrap();
    assert_eq!(
        factory.arena.node(call).unwrap().kind,
        SyntaxKind::CallSignature
    );
    assert_eq!(
        factory.arena.transform_flags(call),
        TransformFlags::CONTAINS_TYPE_SCRIPT,
    );
    let alias_name = factory.create_identifier(source, "Alias").unwrap();
    let alias = factory
        .create_type_alias_declaration(source, None, alias_name, None, any)
        .unwrap();
    assert_eq!(
        factory.arena.node(alias).unwrap().kind,
        SyntaxKind::TypeAliasDeclaration,
    );
    assert_eq!(
        factory.arena.transform_flags(alias),
        TransformFlags::CONTAINS_TYPE_SCRIPT,
    );
    let class_name = factory.create_identifier(source, "Class").unwrap();
    let class_members = factory.create_node_array(source, Vec::new()).unwrap();
    let class = factory
        .create_class_declaration(source, None, Some(class_name), None, None, class_members)
        .unwrap();
    assert_eq!(
        factory.arena.transform_flags(class),
        TransformFlags::CONTAINS_ES_2015,
    );
    let interface_name = factory.create_identifier(source, "Interface").unwrap();
    let interface_members = factory.create_node_array(source, Vec::new()).unwrap();
    let interface = factory
        .create_interface_declaration(source, None, interface_name, None, None, interface_members)
        .unwrap();
    assert_eq!(
        factory.arena.transform_flags(interface),
        TransformFlags::CONTAINS_TYPE_SCRIPT,
    );
    let expression = factory.create_identifier(source, "value").unwrap();
    let statement = factory
        .create_expression_statement(source, expression)
        .unwrap();
    assert_eq!(
        factory.arena.node(statement).unwrap().kind,
        SyntaxKind::ExpressionStatement,
    );
    assert_eq!(
        factory.arena.transform_flags(statement),
        TransformFlags::NONE,
    );
    let variable_name = factory.create_identifier(source, "declared").unwrap();
    let declaration = factory
        .create_variable_declaration(source, variable_name, None, None, None)
        .unwrap();
    let declarations = factory
        .create_node_array(source, vec![declaration])
        .unwrap();
    let declaration_list = factory
        .create_variable_declaration_list(source, declarations, NodeFlags::CONST)
        .unwrap();
    let variable = factory
        .create_variable_statement(source, None, declaration_list)
        .unwrap();
    let declare = factory
        .create_modifiers_from_modifier_flags(source, ModifierFlags::AMBIENT)
        .unwrap();
    let ambient_variable = factory.replace_modifiers(variable, declare).unwrap();
    assert_eq!(
        factory.arena.transform_flags(ambient_variable),
        TransformFlags::CONTAINS_TYPE_SCRIPT,
    );
    let import_name = factory.create_identifier(source, "value").unwrap();
    let specifier = factory
        .create_import_specifier(source, false, None, import_name)
        .unwrap();
    let specifiers = factory.create_node_array(source, vec![specifier]).unwrap();
    let named = factory.create_named_imports(source, specifiers).unwrap();
    let clause = factory
        .create_import_clause(source, None, None, Some(named))
        .unwrap();
    let module = factory.create_string_literal(source, "pkg", false).unwrap();
    let import = factory
        .create_import_declaration(source, None, Some(clause), module, None)
        .unwrap();
    assert_eq!(
        factory.arena.node(import).unwrap().kind,
        SyntaxKind::ImportDeclaration,
    );
    assert_eq!(factory.arena.transform_flags(import), TransformFlags::NONE,);
    let export_name = factory.create_identifier(source, "value").unwrap();
    let export_specifier = factory
        .create_export_specifier(source, false, None, export_name)
        .unwrap();
    let export_specifiers = factory
        .create_node_array(source, vec![export_specifier])
        .unwrap();
    let named_exports = factory
        .create_named_exports(source, export_specifiers)
        .unwrap();
    let export = factory
        .create_export_declaration(source, None, false, Some(named_exports), None, None)
        .unwrap();
    assert_eq!(
        factory.arena.node(export).unwrap().kind,
        SyntaxKind::ExportDeclaration,
    );
    assert_eq!(factory.arena.transform_flags(export), TransformFlags::NONE,);
}

#[test]
fn typed_updates_reuse_identity_or_preserve_original_provenance() {
    let (mut arena, source) = synthetic_arena();
    let mut factory = arena.factory();
    let first_name = factory.create_identifier(source, "First").unwrap();
    let reference = factory
        .create_type_reference_node(source, first_name, None)
        .unwrap();
    assert_eq!(
        factory
            .update_type_reference_node(reference, first_name, None)
            .unwrap(),
        reference,
    );

    let second_name = factory.create_identifier(source, "Second").unwrap();
    let updated = factory
        .update_type_reference_node(reference, second_name, None)
        .unwrap();
    assert_ne!(updated, reference);
    assert_eq!(factory.arena.get_original_node(updated), reference);
}

#[test]
fn unique_names_allocate_arena_owned_generated_binding_identities() {
    let (mut arena, source) = synthetic_arena();
    let mut factory = arena.factory();
    let first = factory
        .create_unique_name(source, "y", GeneratedIdentifierFlags::NONE)
        .unwrap();
    let second = factory
        .create_unique_name(source, "y", GeneratedIdentifierFlags::NONE)
        .unwrap();
    let optimistic = factory
        .create_unique_name(source, "z", GeneratedIdentifierFlags::OPTIMISTIC)
        .unwrap();
    let first_metadata = factory.arena.metadata(first).unwrap();
    let second_metadata = factory.arena.metadata(second).unwrap();
    let optimistic_metadata = factory.arena.metadata(optimistic).unwrap();
    assert_eq!(first_metadata.generated_binding_base(), Some("y"));
    assert_eq!(second_metadata.generated_binding_base(), Some("y"));
    assert_ne!(
        first_metadata.generated_binding_id(),
        second_metadata.generated_binding_id(),
    );
    assert_eq!(optimistic_metadata.generated_binding_base(), None);
    assert_eq!(
        optimistic_metadata.generated_binding_preferred_base(),
        Some("z"),
    );
}

#[test]
fn not_emitted_type_element_and_utf16_literal_metadata_are_exact() {
    let (mut arena, source) = synthetic_arena();
    let mut factory = arena.factory();
    let erased = factory.create_not_emitted_type_element(source).unwrap();
    assert_eq!(
        factory.arena.node(erased).unwrap().kind,
        SyntaxKind::NotEmittedTypeElement,
    );

    for (units, single_quote) in [
        (vec![0xd800], false),
        (vec![0xd800], true),
        (vec![0xd83d, 0xde00], false),
        (vec![0xd83d, 0xde00], true),
    ] {
        let literal = factory
            .create_string_literal_from_code_units(source, &units, single_quote)
            .unwrap();
        let metadata = factory.arena.metadata(literal).unwrap();
        assert_eq!(
            metadata.javascript_string_value().unwrap().code_units(),
            units.as_slice(),
        );
        assert_eq!(metadata.string_literal_single_quote(), Some(single_quote));
    }
}

struct Utf16LiteralTransformer {
    units: Vec<u16>,
    single_quote: bool,
}

impl crate::Transformer for Utf16LiteralTransformer {
    fn name(&self) -> &'static str {
        "utf16-literal-factory-test"
    }

    fn transform_root(
        &mut self,
        context: &mut crate::TransformationContext,
        root: crate::TransformRoot,
    ) -> Result<crate::TransformRoot, TransformError> {
        let source = match root {
            crate::TransformRoot::SourceFile(source) => source,
            other => return Ok(other),
        };
        let root = context.arena().root(source)?;
        let end_of_file_token = match &context.arena().node(root)?.data {
            NodeData::SourceFile(data) => data.end_of_file_token,
            _ => unreachable!("transform root is a SourceFile"),
        };
        let name = context.factory()?.create_identifier(source, "value")?;
        let literal = context.factory()?.create_string_literal_from_code_units(
            source,
            &self.units,
            self.single_quote,
        )?;
        let declaration = context.factory()?.create_variable_declaration(
            source,
            name,
            None,
            None,
            Some(literal),
        )?;
        let declarations = context
            .factory()?
            .create_node_array(source, vec![declaration])?;
        let list = context.factory()?.create_variable_declaration_list(
            source,
            declarations,
            NodeFlags::CONST,
        )?;
        let statement = context
            .factory()?
            .create_variable_statement(source, None, list)?;
        let statements = context
            .factory()?
            .create_node_array(source, vec![statement])?;
        let root_flags = context.arena().transform_flags(root);
        let updated = context.factory()?.update_node(
            root,
            NodeData::SourceFile(SourceFileData {
                statements: Some(statements.array()),
                end_of_file_token,
            }),
            root_flags,
        )?;
        context.arena_mut()?.replace_root(source, updated)?;
        Ok(crate::TransformRoot::SourceFile(source))
    }
}

fn print_synthetic_utf16(units: &[u16], single_quote: bool) -> String {
    let parsed = parsed("synthetic.js", "");
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    let mut transformation = crate::transform_nodes(
        arena,
        vec![crate::TransformRoot::SourceFile(source)],
        vec![Box::new(Utf16LiteralTransformer {
            units: units.to_vec(),
            single_quote,
        })],
        false,
    )
    .unwrap();
    crate::create_printer(crate::PrinterOptions::new(crate::NewLineKind::LineFeed))
        .print(
            &mut transformation,
            crate::PrintRequest::SourceFile(source),
            None,
        )
        .unwrap()
        .text()
        .to_owned()
}

fn print_parsed_literal(source_text: &str) -> String {
    let parsed = parsed("parsed.js", source_text);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    let mut transformation = crate::transform_nodes(
        arena,
        vec![crate::TransformRoot::SourceFile(source)],
        Vec::new(),
        false,
    )
    .unwrap();
    crate::create_printer(
        crate::PrinterOptions::new(crate::NewLineKind::LineFeed)
            .with_source_file_text_mode(crate::SourceFileTextMode::Canonical),
    )
    .print(
        &mut transformation,
        crate::PrintRequest::SourceFile(source),
        None,
    )
    .unwrap()
    .text()
    .to_owned()
}

#[test]
fn utf16_factory_literals_print_like_equivalent_parsed_literals_for_both_quotes() {
    for (units, double_source, single_source) in [
        (
            &[0xd800][..],
            r#"const value = "\uD800";"#,
            r#"const value = '\uD800';"#,
        ),
        (
            &[0xd83d, 0xde00][..],
            r#"const value = "\uD83D\uDE00";"#,
            r#"const value = '\uD83D\uDE00';"#,
        ),
    ] {
        assert_eq!(
            print_synthetic_utf16(units, false),
            print_parsed_literal(double_source),
        );
        assert_eq!(
            print_synthetic_utf16(units, true),
            print_parsed_literal(single_source),
        );
    }
}
