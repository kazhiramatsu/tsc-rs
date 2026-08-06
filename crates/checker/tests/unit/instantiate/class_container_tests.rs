use tsc_types::{CompilerOptions, ObjectFlags, SymbolFlags, TypeData};

use crate::state::test_support::with_program_state;

#[test]
fn type_literals_inside_class_bodies_instantiate_with_this_type_filtered() {
    with_program_state(
        &[("a.ts", "class C<T> { p: { a: T } }\n")],
        &CompilerOptions::default(),
        |state| {
            let source = state.binder.source(0);
            let literal_node = source
                .arena
                .node_ids()
                .find(|&id| source.arena.node(id).kind == tsc_syntax::SyntaxKind::TypeLiteral)
                .expect("type literal");
            let anonymous = state
                .get_type_from_type_node(literal_node)
                .expect("type literal type");
            let t = {
                let symbol = state
                    .resolve_name(
                        Some(literal_node),
                        "T",
                        SymbolFlags::TYPE_PARAMETER,
                        None,
                        false,
                        false,
                    )
                    .expect("resolve_name")
                    .expect("T resolves");
                state.get_declared_type_of_type_parameter(symbol)
            };
            let string = state.tables.intrinsics.string;
            let mapper = state.create_type_mapper(vec![t], Some(vec![string]));
            // The walk crosses the ClassDeclaration container: its
            // thisType joins the outer parameters and containsReference
            // filters it back out (no `this` in the literal).
            let shell = state
                .instantiate_type(anonymous, Some(mapper))
                .expect("instantiation crosses class containers since the GenericType port");
            assert_ne!(shell, anonymous);
            assert!(state
                .tables
                .object_flags_of(shell)
                .intersects(ObjectFlags::INSTANTIATED));
            // The class declared type itself instantiates through the
            // reference arm.
            let c = state
                .resolve_file_scope_name("C", SymbolFlags::CLASS)
                .expect("C resolves");
            let declared = state
                .get_declared_type_of_class_or_interface(c)
                .expect("C declared");
            let mapper = state.create_type_mapper(vec![t], Some(vec![string]));
            let instantiated = state
                .instantiate_type(declared, Some(mapper))
                .expect("declared-type instantiation");
            assert_ne!(instantiated, declared);
            assert!(matches!(
                state.tables.type_of(instantiated).data,
                TypeData::Reference { target, .. } if target == declared
            ));
        },
    );
}
