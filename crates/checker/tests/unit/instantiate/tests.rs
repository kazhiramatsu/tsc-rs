use tsc_types::{CompilerOptions, ObjectFlags, SymbolFlags, TypeFlags, UnionReduction};

use crate::state::test_support::with_program_state;
use crate::state::CheckerState;

fn annotation_of_var(state: &CheckerState, name: &str) -> tsc_syntax::NodeId {
    crate::relpin::find_probe_annotation(state.binder.source(0), name).expect("var with annotation")
}

fn declared_type_parameter_at(
    state: &mut CheckerState,
    inside: tsc_syntax::NodeId,
    name: &str,
) -> tsc_types::TypeId {
    let symbol = state
        .resolve_name(
            Some(inside),
            name,
            SymbolFlags::TYPE_PARAMETER,
            None,
            false,
            false,
        )
        .expect("resolve_name")
        .expect("type parameter resolves");
    state.get_declared_type_of_type_parameter(symbol)
}

fn declared_type_parameter(state: &mut CheckerState, name: &str) -> tsc_types::TypeId {
    let source = state.binder.source(0);
    let inside = source
        .arena
        .node_ids()
        .find(|&id| source.arena.node(id).kind == tsc_syntax::SyntaxKind::VariableDeclaration)
        .expect("var declaration");
    declared_type_parameter_at(state, inside, name)
}

#[test]
fn union_instantiation_maps_parameters_and_keeps_identity() {
    with_program_state(
        &[(
            "a.ts",
            "function f<T, U>() { var v: T | number; var w: string | number; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let annotation = annotation_of_var(state, "v");
            let union = state.get_type_from_type_node(annotation).expect("union");
            let t = declared_type_parameter(state, "T");
            let u = declared_type_parameter(state, "U");
            assert!(state.could_contain_type_variables(union));
            let string = state.tables.intrinsics.string;
            let mapper = state.create_type_mapper(vec![t], Some(vec![string]));
            let mapped = state
                .instantiate_type(union, Some(mapper))
                .expect("instantiation in slice");
            let expected_annotation = annotation_of_var(state, "w");
            let expected = state
                .get_type_from_type_node(expected_annotation)
                .expect("string | number");
            assert_eq!(mapped, expected, "T|number [T:=string] is string|number");
            // A mapper over an unreferenced parameter maps nothing:
            // tsc returns the SAME type object.
            let unrelated = state.create_type_mapper(vec![u], Some(vec![string]));
            let unchanged = state
                .instantiate_type(union, Some(unrelated))
                .expect("instantiation in slice");
            assert_eq!(unchanged, union);
        },
    );
}

#[test]
fn template_literal_over_type_parameter_interns_and_instantiates() {
    with_program_state(
        &[(
            "a.ts",
            "function f<T extends string>() { var v: `a${T}`; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let annotation = annotation_of_var(state, "v");
            let template = state.get_type_from_type_node(annotation).expect("template");
            // Regression: the tables isGenericIndexType stub used to
            // collapse `a${T}` to string.
            assert!(
                state
                    .tables
                    .flags_of(template)
                    .intersects(TypeFlags::TEMPLATE_LITERAL),
                "generic span keeps the template literal shape"
            );
            let t = declared_type_parameter(state, "T");
            let x = state.tables.get_string_literal_type("x");
            let mapper = state.create_type_mapper(vec![t], Some(vec![x]));
            let mapped = state
                .instantiate_type(template, Some(mapper))
                .expect("instantiation in slice");
            let expected = state.tables.get_string_literal_type("ax");
            assert_eq!(mapped, expected);
        },
    );
}

#[test]
fn tuple_reference_instantiation_reuses_the_interned_reference() {
    with_program_state(
        &[(
            "a.ts",
            "function f<T>() { var v: [T, string]; var w: [number, string]; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let annotation = annotation_of_var(state, "v");
            let tuple = state.get_type_from_type_node(annotation).expect("tuple");
            let t = declared_type_parameter(state, "T");
            let number = state.tables.intrinsics.number;
            let mapper = state.create_type_mapper(vec![t], Some(vec![number]));
            let mapped = state
                .instantiate_type(tuple, Some(mapper))
                .expect("instantiation in slice");
            let expected_annotation = annotation_of_var(state, "w");
            let expected = state
                .get_type_from_type_node(expected_annotation)
                .expect("[number, string]");
            assert_eq!(mapped, expected);
        },
    );
}

#[test]
fn anonymous_type_instantiation_creates_a_cached_shell() {
    with_program_state(
        &[("a.ts", "function f<T, U>() { var v: { a: T }; }\n")],
        &CompilerOptions::default(),
        |state| {
            let annotation = annotation_of_var(state, "v");
            let anonymous = state
                .get_type_from_type_node(annotation)
                .expect("type literal");
            let t = declared_type_parameter(state, "T");
            let u = declared_type_parameter(state, "U");
            let string = state.tables.intrinsics.string;
            let mapper = state.create_type_mapper(vec![t], Some(vec![string]));
            let shell = state
                .instantiate_type(anonymous, Some(mapper))
                .expect("instantiation in slice");
            assert_ne!(shell, anonymous);
            assert!(state
                .tables
                .object_flags_of(shell)
                .intersects(ObjectFlags::INSTANTIATED));
            assert_eq!(state.links.ty(shell).instantiated_target, Some(anonymous));
            // Same type arguments -> the interned instantiation.
            let mapper2 = state.create_type_mapper(vec![t], Some(vec![string]));
            let again = state
                .instantiate_type(anonymous, Some(mapper2))
                .expect("instantiation in slice");
            assert_eq!(again, shell);
            // A Block sits between the type literal and the type-
            // parameter container, so isTypeParameterPossiblyReferenced
            // answers true for U as well (63527-63529): a U-only
            // mapper still mints a (distinct) instantiation, exactly
            // like tsc.
            let unrelated = state.create_type_mapper(vec![u], Some(vec![string]));
            let u_shell = state
                .instantiate_type(anonymous, Some(unrelated))
                .expect("instantiation in slice");
            assert_ne!(u_shell, anonymous);
            assert_ne!(u_shell, shell);
            // Member resolution of the shell reads the target's
            // properties through the mapper (5.3a): `a: T` lands as
            // `a: string`.
            let members = state
                .resolve_structured_type_members(shell)
                .expect("instantiated members resolve");
            let properties = state.members_of(members).properties.clone();
            assert_eq!(properties.len(), 1);
            let property_type = state
                .get_type_of_symbol(properties[0])
                .expect("instantiated property type");
            assert_eq!(property_type, string);
        },
    );
}

#[test]
fn unreferenced_outer_parameters_are_filtered_without_a_block() {
    with_program_state(
        &[("a.ts", "declare function f<T, U>(): { a: T };\n")],
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
            let t = declared_type_parameter_at(state, literal_node, "T");
            let u = declared_type_parameter_at(state, literal_node, "U");
            let string = state.tables.intrinsics.string;
            // No Block intervenes: containsReference filters U out,
            // so a U-only mapper hits the seeded self entry.
            let unrelated = state.create_type_mapper(vec![u], Some(vec![string]));
            let unchanged = state
                .instantiate_type(anonymous, Some(unrelated))
                .expect("instantiation in slice");
            assert_eq!(unchanged, anonymous);
            // T stays: a T mapper mints the shell.
            let mapper = state.create_type_mapper(vec![t], Some(vec![string]));
            let shell = state
                .instantiate_type(anonymous, Some(mapper))
                .expect("instantiation in slice");
            assert_ne!(shell, anonymous);
        },
    );
}

#[test]
fn erased_and_argument_instantiated_signatures_map_lazily() {
    with_program_state(
        &[("a.ts", "function f<T>() { var v: (x: T) => T; }\n")],
        &CompilerOptions::default(),
        |state| {
            let annotation = annotation_of_var(state, "v");
            let base = state
                .get_signature_from_declaration(annotation)
                .expect("function-type signature");
            let t = declared_type_parameter(state, "T");
            // Promote to a generic signature by hand — generic
            // getSignatureFromDeclaration lands with the follow-up.
            let mut generic = state.signature_of(base).clone();
            generic.type_parameters = Some(vec![t]);
            let generic = state.alloc_signature(generic);

            let erased = state.get_erased_signature(generic).expect("erased");
            assert_ne!(erased, generic);
            let erased_return = state
                .get_return_type_of_signature(erased)
                .expect("erased return");
            assert_eq!(erased_return, state.tables.intrinsics.any);
            let erased_parameter = state.signature_of(erased).parameters[0];
            let erased_parameter_type = state
                .get_type_of_symbol(erased_parameter)
                .expect("instantiated parameter type");
            assert_eq!(erased_parameter_type, state.tables.intrinsics.any);
            // The erased signature is cached.
            assert_eq!(state.get_erased_signature(generic).expect("cached"), erased);

            let string = state.tables.intrinsics.string;
            let instantiated = state
                .get_signature_instantiation(generic, Some(&[string]), false, None)
                .expect("signature instantiation");
            let instantiated_return = state
                .get_return_type_of_signature(instantiated)
                .expect("instantiated return");
            assert_eq!(instantiated_return, string);
            let parameter = state.signature_of(instantiated).parameters[0];
            let parameter_type = state
                .get_type_of_symbol(parameter)
                .expect("instantiated parameter type");
            assert_eq!(parameter_type, string);
            // Interned per type-argument list.
            let again = state
                .get_signature_instantiation(generic, Some(&[string]), false, None)
                .expect("signature instantiation");
            assert_eq!(again, instantiated);
        },
    );
}

#[test]
fn string_mapping_applies_to_literals_unions_and_generics() {
    with_program_state(
        &[("a.ts", "function f<T extends string>() { var v: T; }\n")],
        &CompilerOptions::default(),
        |state| {
            let uppercase = state
                .binder
                .create_symbol(SymbolFlags::TYPE_ALIAS, "Uppercase".to_owned());
            let capitalize = state
                .binder
                .create_symbol(SymbolFlags::TYPE_ALIAS, "Capitalize".to_owned());
            let abc = state.tables.get_string_literal_type("abc");
            let mapped = state
                .get_string_mapping_type(uppercase, abc)
                .expect("literal mapping");
            assert_eq!(mapped, state.tables.get_string_literal_type("ABC"));
            // charAt(0)-faithful Capitalize: ß expands to SS.
            let eszett = state.tables.get_string_literal_type("ßoo");
            let capitalized = state
                .get_string_mapping_type(capitalize, eszett)
                .expect("literal mapping");
            assert_eq!(capitalized, state.tables.get_string_literal_type("SSoo"));
            // Unions map member-wise.
            let a = state.tables.get_string_literal_type("a");
            let b = state.tables.get_string_literal_type("b");
            let union = state
                .get_union_type_ex(&[a, b], UnionReduction::Literal)
                .expect("union");
            let mapped_union = state
                .get_string_mapping_type(uppercase, union)
                .expect("union mapping");
            let upper_a = state.tables.get_string_literal_type("A");
            let upper_b = state.tables.get_string_literal_type("B");
            let expected = state
                .get_union_type_ex(&[upper_a, upper_b], UnionReduction::Literal)
                .expect("union");
            assert_eq!(mapped_union, expected);
            // Generic operands intern a StringMapping type;
            // instantiation maps through it.
            let t = declared_type_parameter(state, "T");
            let generic = state
                .get_string_mapping_type(uppercase, t)
                .expect("generic mapping");
            assert!(state
                .tables
                .flags_of(generic)
                .intersects(TypeFlags::STRING_MAPPING));
            let again = state
                .get_string_mapping_type(uppercase, t)
                .expect("generic mapping");
            assert_eq!(again, generic, "stringMappingTypes interning");
            let foo = state.tables.get_string_literal_type("foo");
            let mapper = state.create_type_mapper(vec![t], Some(vec![foo]));
            let instantiated = state
                .instantiate_type(generic, Some(mapper))
                .expect("instantiation in slice");
            assert_eq!(instantiated, state.tables.get_string_literal_type("FOO"));
            // Mapping<Mapping<T>> === Mapping<T>.
            let doubled = state
                .get_string_mapping_type(uppercase, generic)
                .expect("idempotent mapping");
            assert_eq!(doubled, generic);
        },
    );
}

#[test]
fn string_mapping_relations_and_constraints() {
    with_program_state(
        &[("a.ts", "function f<T extends string>() { var v: T; }\n")],
        &CompilerOptions::default(),
        |state| {
            let uppercase = state
                .binder
                .create_symbol(SymbolFlags::TYPE_ALIAS, "Uppercase".to_owned());
            let lowercase = state
                .binder
                .create_symbol(SymbolFlags::TYPE_ALIAS, "Lowercase".to_owned());
            let string = state.tables.intrinsics.string;
            let upper_string = state
                .get_string_mapping_type(uppercase, string)
                .expect("Uppercase<string>");
            let lower_string = state
                .get_string_mapping_type(lowercase, string)
                .expect("Lowercase<string>");
            let foo_upper = state.tables.get_string_literal_type("FOO");
            let foo_lower = state.tables.get_string_literal_type("foo");
            assert_eq!(
                state.is_type_assignable_to(foo_upper, upper_string),
                Ok(true),
                "\"FOO\" is a member of Uppercase<string>"
            );
            assert_eq!(
                state.is_member_of_string_mapping(foo_lower, upper_string),
                Ok(false),
                "\"foo\" is not a member of Uppercase<string>"
            );
            assert_eq!(
                state.is_type_assignable_to(upper_string, string),
                Ok(true),
                "Uppercase<string> relates through its base constraint"
            );
            assert_eq!(
                state.is_type_assignable_to(upper_string, lower_string),
                Ok(false),
                "different intrinsics are unrelated"
            );
            // computeBaseConstraint: Uppercase<T> -> Uppercase<string>.
            let t = declared_type_parameter(state, "T");
            let upper_t = state
                .get_string_mapping_type(uppercase, t)
                .expect("Uppercase<T>");
            let constraint = state
                .get_base_constraint_of_type(upper_t)
                .expect("constraint in slice");
            assert_eq!(constraint, Some(upper_string));
            // Union reduction drops literals matched by mappings.
            let union = state
                .get_union_type_ex(&[foo_upper, upper_string], UnionReduction::Literal)
                .expect("union");
            assert_eq!(union, upper_string, "\"FOO\" | Uppercase<string> reduces");
        },
    );
}

#[test]
fn permissive_and_restrictive_instantiations() {
    with_program_state(
        &[("a.ts", "function f<T extends string, U>() { var v: T; }\n")],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let u = declared_type_parameter(state, "U");
            let permissive = state
                .get_permissive_instantiation(t)
                .expect("permissive in slice");
            assert_eq!(permissive, state.tables.intrinsics.wildcard);
            let restrictive = state
                .get_restrictive_instantiation(t)
                .expect("restrictive in slice");
            assert_ne!(restrictive, t, "constrained parameters get a fresh twin");
            assert_eq!(
                state.get_constraint_from_type_parameter(restrictive),
                Ok(None),
                "the twin's constraint is the noConstraint sentinel"
            );
            let restrictive_again = state
                .get_restrictive_instantiation(t)
                .expect("restrictive cached");
            assert_eq!(restrictive_again, restrictive);
            // Unconstrained parameters ARE their restrictive form.
            let u_restrictive = state
                .get_restrictive_instantiation(u)
                .expect("restrictive in slice");
            assert_eq!(u_restrictive, u);
        },
    );
}

#[test]
fn cloned_type_parameters_instantiate_their_target_constraint() {
    with_program_state(
        &[(
            "a.ts",
            "function f<T extends string>() { var v: (x: T) => T; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let annotation = annotation_of_var(state, "v");
            let base = state
                .get_signature_from_declaration(annotation)
                .expect("function-type signature");
            let t = declared_type_parameter(state, "T");
            let mut generic = state.signature_of(base).clone();
            generic.type_parameters = Some(vec![t]);
            let generic = state.alloc_signature(generic);
            // A non-erasing instantiation clones the parameters.
            let identity = state.create_type_mapper(vec![t], Some(vec![t]));
            let cloned_signature = state
                .instantiate_signature(generic, identity, /*erase*/ false)
                .expect("instantiation in slice");
            let fresh = state
                .signature_of(cloned_signature)
                .type_parameters
                .clone()
                .expect("fresh type parameters")[0];
            assert_ne!(fresh, t);
            assert_eq!(state.links.ty(fresh).type_parameter_target, Some(t));
            let constraint = state
                .get_constraint_from_type_parameter(fresh)
                .expect("constraint in slice");
            assert_eq!(constraint, Some(state.tables.intrinsics.string));
        },
    );
}
