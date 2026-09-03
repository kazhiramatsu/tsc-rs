//! h2-7a-m-3 P1 boundary contracts: the seven NodeBuilder-backed resolver
//! members fail closed by default, and the three factory seams (arena
//! factory view, resolver→transform projection, ThisType token) behave as
//! the packet §4 specifies. The 33 pre-existing members keep their own
//! contract suites untouched.

use super::*;
use crate::factory::TransformArena;
use crate::transform::TransformFlags;
use tsc_program::SourceFileId;
use tsc_syntax::{NodeId, SyntaxKind};

struct DefaultResolver;
impl EmitResolver for DefaultResolver {}

struct NoopTracker;
impl EmitSymbolTracker for NoopTracker {}

fn arena_with_source() -> (TransformArena, crate::factory::TransformSourceId, u32) {
    let parsed = tsc_syntax::parse_source_file(
        "seams.ts".to_owned(),
        "export const answer = 42;\n".to_owned(),
        Default::default(),
        None,
    );
    let node_end = parsed.arena.node_end();
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(7)));
    (arena, source, node_end)
}

#[test]
fn serialization_members_fail_closed_by_default() {
    let resolver = DefaultResolver;
    let mut tracker = NoopTracker;
    let (mut arena, source, _) = arena_with_source();
    let node = EmitResolverNode::from_raw_source(7, NodeId(1));
    let enclosing = EmitResolverNode::from_raw_source(7, NodeId(0));

    let unavailable = |method: EmitResolverMethod, error: EmitResolverError| {
        assert_eq!(error, EmitResolverError::Unavailable { method, node });
    };

    unavailable(
        EmitResolverMethod::CreateTypeOfDeclaration,
        resolver
            .create_type_of_declaration(
                &mut arena,
                source,
                node,
                enclosing,
                EmitNodeBuilderFlags::DECLARATION_EMIT,
                EmitInternalNodeBuilderFlags::DECLARATION_EMIT,
                &mut tracker,
            )
            .unwrap_err(),
    );
    unavailable(
        EmitResolverMethod::CreateTypeOfDeclarationInExpandoScope,
        resolver
            .create_type_of_declaration_in_expando_scope(
                &mut arena,
                source,
                node,
                node,
                enclosing,
                EmitNodeBuilderFlags::DECLARATION_EMIT,
                EmitInternalNodeBuilderFlags::DECLARATION_EMIT,
                &mut tracker,
            )
            .unwrap_err(),
    );
    unavailable(
        EmitResolverMethod::CreateReturnTypeOfSignatureDeclaration,
        resolver
            .create_return_type_of_signature_declaration(
                &mut arena,
                source,
                node,
                enclosing,
                EmitNodeBuilderFlags::DECLARATION_EMIT,
                EmitInternalNodeBuilderFlags::DECLARATION_EMIT,
                &mut tracker,
            )
            .unwrap_err(),
    );
    unavailable(
        EmitResolverMethod::CreateTypeOfExpression,
        resolver
            .create_type_of_expression(
                &mut arena,
                source,
                node,
                enclosing,
                EmitNodeBuilderFlags::DECLARATION_EMIT,
                EmitInternalNodeBuilderFlags::DECLARATION_EMIT,
                &mut tracker,
            )
            .unwrap_err(),
    );
    unavailable(
        EmitResolverMethod::CreateLiteralConstValue,
        resolver
            .create_literal_const_value(&mut arena, source, node, &mut tracker)
            .unwrap_err(),
    );
    unavailable(
        EmitResolverMethod::GetDeclarationStatementsForSourceFile,
        resolver
            .get_declaration_statements_for_source_file(
                &mut arena,
                source,
                node,
                EmitNodeBuilderFlags::DECLARATION_EMIT,
                EmitInternalNodeBuilderFlags::DECLARATION_EMIT,
                &mut tracker,
            )
            .unwrap_err(),
    );
    unavailable(
        EmitResolverMethod::CreateLateBoundIndexSignatures,
        resolver
            .create_late_bound_index_signatures(
                &mut arena,
                source,
                node,
                enclosing,
                EmitNodeBuilderFlags::DECLARATION_EMIT,
                EmitInternalNodeBuilderFlags::DECLARATION_EMIT,
                &mut tracker,
            )
            .unwrap_err(),
    );
    unavailable(
        EmitResolverMethod::IsLastBodilessOverloadOfSymbol,
        resolver
            .is_last_bodiless_overload_of_symbol(node)
            .unwrap_err(),
    );
    unavailable(
        EmitResolverMethod::IsFirstDeclarationOfSymbol,
        resolver.is_first_declaration_of_symbol(node).unwrap_err(),
    );
    assert_eq!(
        EmitResolverMethod::CreateTypeOfDeclarationInExpandoScope.name(),
        "createTypeOfDeclarationInExpandoScope"
    );
    assert_eq!(
        EmitResolverMethod::IsLastBodilessOverloadOfSymbol.name(),
        "isLastBodilessOverloadOfSymbol"
    );
    assert_eq!(
        EmitResolverMethod::IsFirstDeclarationOfSymbol.name(),
        "isFirstDeclarationOfSymbol"
    );

    let symbol = EmitResolverSymbol {
        session_token: 11,
        symbol_index: 3,
    };
    assert_eq!(
        resolver
            .symbol_to_declarations(
                &mut arena,
                source,
                symbol,
                EmitSymbolMeaning::TYPE,
                EmitNodeBuilderFlags::NONE,
                None,
                None,
                None,
            )
            .unwrap_err(),
        EmitResolverError::UnavailableForSymbol {
            method: EmitResolverMethod::SymbolToDeclarations,
            symbol,
        }
    );
}

#[test]
fn declaration_emit_flag_words_match_the_vendored_constants() {
    // _tsc.js:114263-114264.
    assert_eq!(EmitNodeBuilderFlags::DECLARATION_EMIT.0, 531_469);
    assert_eq!(EmitInternalNodeBuilderFlags::DECLARATION_EMIT.0, 8);
    assert!(EmitNodeBuilderFlags::DECLARATION_EMIT.contains(EmitNodeBuilderFlags::NO_TRUNCATION));
    assert_eq!(
        EmitNodeBuilderFlags::DECLARATION_EMIT
            .union(EmitNodeBuilderFlags::MULTILINE_OBJECT_LITERALS),
        EmitNodeBuilderFlags::DECLARATION_EMIT,
    );
}

#[test]
fn parse_tree_transform_node_round_trips_and_rejects_foreign_ids() {
    let (arena, source, node_end) = arena_with_source();
    let parsed = EmitResolverNode::from_raw_source(7, NodeId(1));
    let projected = arena
        .parse_tree_transform_node(parsed)
        .expect("projection")
        .expect("mounted source");
    assert_eq!(projected.source(), source);
    // Round trip back through the parse-tree resolver projection.
    let back = arena
        .parse_tree_resolver_node(projected)
        .expect("reverse projection")
        .expect("parse identity");
    assert_eq!(back, parsed);
    // Unknown Program file: absent, not an error.
    assert_eq!(
        arena
            .parse_tree_transform_node(EmitResolverNode::from_raw_source(99, NodeId(1)))
            .expect("projection"),
        None,
    );
    // A node id outside the mounted parse lease is a hard error.
    assert!(arena
        .parse_tree_transform_node(EmitResolverNode::from_raw_source(7, NodeId(node_end + 10)))
        .is_err());
}

#[test]
fn factory_view_constructs_this_type_token_and_rejects_other_type_kinds() {
    let (mut arena, source, _) = arena_with_source();
    let this_type = {
        let mut factory = arena.factory();
        factory
            .create_token(source, SyntaxKind::ThisType, TransformFlags::NONE)
            .expect("ThisType token")
    };
    // Kind preserved on the synthesized node.
    assert!(arena
        .parse_tree_resolver_node(this_type)
        .expect("projection")
        .is_none());
    let mut factory = arena.factory();
    assert!(factory
        .create_token(source, SyntaxKind::TypeReference, TransformFlags::NONE)
        .is_err());
    assert!(factory
        .create_token(source, SyntaxKind::UnionType, TransformFlags::NONE)
        .is_err());
}
