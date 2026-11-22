use tsc_syntax::NodeLookup;
use tsc_types::{CompilerOptions, ObjectFlags, SymbolFlags, TypeFlags, UnionReduction};

use crate::state::test_support::with_program_state;
use crate::state::CheckerState;

#[test]
fn child_scratch_preserves_nested_visits_early_returns_and_the_callers_prefix() {
    with_program_state(
        &[("a.ts", "let x = 1; function f() { x = 2; } x = 3;")],
        &CompilerOptions::default(),
        |state| {
            let root = state.binder.source(0).root;
            let mut expected = Vec::new();
            tsc_syntax::for_each_child(
                &tsc_syntax::Published,
                tsc_syntax::Published.node(root),
                |child| {
                    expected.push(child);
                    false
                },
            );
            state.child_scratch.push(root);
            let mut observed = Vec::new();
            assert!(!state
                .for_each_child_of(root, |state, child| {
                    observed.push(child);
                    let first = state.child_scratch.len();
                    state.for_each_child_of(child, |_, _| Ok(true))?;
                    assert_eq!(state.child_scratch.len(), first);
                    Ok(false)
                })
                .unwrap());
            assert_eq!(observed, expected);
            assert_eq!(state.child_scratch, [root]);
            assert!(state.for_each_child_of(root, |_, _| Ok(true)).unwrap());
            assert_eq!(state.child_scratch, [root]);
        },
    );
}

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
            let mapper = state.create_type_mapper(&[t], Some(&[string]));
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
            let unrelated = state.create_type_mapper(&[u], Some(&[string]));
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
            let mapper = state.create_type_mapper(&[t], Some(&[x]));
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
            let mapper = state.create_type_mapper(&[t], Some(&[number]));
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
fn reference_worker_preserves_eager_and_self_target_instantiation_counts() {
    with_program_state(
        &[(
            "a.ts",
            "interface Box<P> { p: P } function f<T>() { var tuple: [T]; var boxed: Box<T>; }",
        )],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let number = state.tables.intrinsics.number;
            for name in ["tuple", "boxed"] {
                let node = annotation_of_var(state, name);
                let annotation = state.get_type_from_type_node(node).unwrap();
                let target = state.tables.reference_target(annotation);
                assert_eq!(state.tables.reference_target(target), target);
                let parameter = state.tables.type_arguments(target)[0];
                let eager = state.tables.create_type_reference(target, &[t]);
                let expected = state.tables.create_type_reference(target, &[number]);
                for (source, from, result) in [
                    (eager, parameter, eager),
                    (eager, t, expected),
                    (target, parameter, expected),
                ] {
                    let mapper = state.make_unary_type_mapper(from, number);
                    let before = state.total_instantiation_count;
                    assert_eq!(
                        state.instantiate_type(source, Some(mapper)).unwrap(),
                        result
                    );
                    assert_eq!(state.total_instantiation_count - before, 2);
                }
            }
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
            let mapper = state.create_type_mapper(&[t], Some(&[string]));
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
            let mapper2 = state.create_type_mapper(&[t], Some(&[string]));
            let again = state
                .instantiate_type(anonymous, Some(mapper2))
                .expect("instantiation in slice");
            assert_eq!(again, shell);
            // A Block sits between the type literal and the type-
            // parameter container, so isTypeParameterPossiblyReferenced
            // answers true for U as well (63527-63529): a U-only
            // mapper still mints a (distinct) instantiation, exactly
            // like tsc.
            let unrelated = state.create_type_mapper(&[u], Some(&[string]));
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
            let unrelated = state.create_type_mapper(&[u], Some(&[string]));
            let unchanged = state
                .instantiate_type(anonymous, Some(unrelated))
                .expect("instantiation in slice");
            assert_eq!(unchanged, anonymous);
            // T stays: a T mapper mints the shell.
            let mapper = state.create_type_mapper(&[t], Some(&[string]));
            let shell = state
                .instantiate_type(anonymous, Some(mapper))
                .expect("instantiation in slice");
            assert_ne!(shell, anonymous);
        },
    );
}

#[test]
fn outer_parameter_filter_visits_later_method_declarations() {
    let source = "declare function f<X, T, U>(): { m(x: T): void; m(x: U): void };";
    for has_alias_arguments in [false, true] {
        with_program_state(&[("a.ts", source)], &CompilerOptions::default(), |state| {
            let file = state.binder.source(0);
            let first = file
                .arena
                .node_ids()
                .find(|&id| file.arena.node(id).kind == tsc_syntax::SyntaxKind::MethodSignature)
                .unwrap();
            let method = state.get_symbol_of_declaration(first).unwrap();
            let declaration = state.binder.symbol(method).declarations()[0];
            assert_eq!(state.binder.symbol(method).declarations().len(), 2);
            let ty = state.get_type_of_symbol(method).unwrap();
            let x = declared_type_parameter_at(state, declaration, "X");
            let t = declared_type_parameter_at(state, declaration, "T");
            let u = declared_type_parameter_at(state, declaration, "U");
            if has_alias_arguments {
                state
                    .tables
                    .set_alias(ty, Some(method), Some(state.tables.empty_type_list()));
            }
            let identity = state.make_unary_type_mapper(t, t);
            assert_eq!(state.instantiate_type(ty, Some(identity)).unwrap(), ty);
            let kept = state.links.node_outer_type_parameters(declaration).unwrap();
            let parameters = [x, t, u];
            let expected = if has_alias_arguments {
                &parameters[..]
            } else {
                &parameters[1..]
            };
            assert_eq!(kept, state.tables.intern_type_list(expected));
            let mapper = state.make_unary_type_mapper(x, state.tables.intrinsics.string);
            let result = state.instantiate_type(ty, Some(mapper)).unwrap();
            assert_eq!(result != ty, has_alias_arguments);
        });
    }
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
            generic.set_type_parameters(Some(Box::new([t])));
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

fn literal_mapping_symbol(state: &mut CheckerState, name: &'static str) -> tsc_binder::SymbolId {
    state.binder.create_symbol(
        SymbolFlags::TYPE_ALIAS,
        tsc_types::EscapedName::internal(name),
    )
}

#[test]
fn literal_mappings_keep_their_domain_when_names_or_generic_entries_change() {
    with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
        let symbol = literal_mapping_symbol(state, "Uppercase");
        let input = state.tables.get_string_literal_type("mIxED");
        let generic = state
            .tables
            .get_string_mapping_type_for_generic_type(symbol, input);
        let upper = state.get_string_mapping_type(symbol, input).unwrap();
        assert_eq!(upper, state.tables.get_string_literal_type("MIXED"));
        assert!(state
            .tables
            .flags_of(upper)
            .intersects(TypeFlags::STRING_LITERAL));
        assert!(state
            .tables
            .flags_of(generic)
            .intersects(TypeFlags::STRING_MAPPING));
        assert_ne!(upper, generic);

        state.binder.symbol_mut(symbol).escaped_name =
            tsc_types::EscapedName::internal("Lowercase");
        let lower = state.get_string_mapping_type(symbol, input).unwrap();
        assert_eq!(lower, state.tables.get_string_literal_type("mixed"));
        state.binder.symbol_mut(symbol).escaped_name =
            tsc_types::EscapedName::internal("Uppercase");
        assert_eq!(state.get_string_mapping_type(symbol, input).unwrap(), upper);
        assert_eq!(
            state
                .tables
                .get_string_mapping_type_for_generic_type(symbol, input),
            generic
        );

        let other = state.tables.get_string_literal_type("other");
        let literal_first = state.get_string_mapping_type(symbol, other).unwrap();
        let generic_second = state
            .tables
            .get_string_mapping_type_for_generic_type(symbol, other);
        assert_ne!(literal_first, generic_second);
        assert!(state
            .tables
            .flags_of(generic_second)
            .intersects(TypeFlags::STRING_MAPPING));
        let count = state.tables.len();
        for _ in 0..8 {
            assert_eq!(state.get_string_mapping_type(symbol, input).unwrap(), upper);
            assert_eq!(
                state.get_string_mapping_type(symbol, other).unwrap(),
                literal_first
            );
            assert_eq!(state.tables.len(), count);
        }
    });
}

#[test]
fn literal_mappings_keep_enum_and_fresh_inputs_plain() {
    with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
        let capitalize = literal_mapping_symbol(state, "Capitalize");
        let regular = state.tables.get_string_literal_type("ßoo");
        let fresh = state.tables.get_fresh_type_of_literal_type(regular);
        let enumeration = state.binder.create_symbol(
            SymbolFlags::REGULAR_ENUM,
            tsc_types::EscapedName::internal("E"),
        );
        let member = state.binder.create_symbol(
            SymbolFlags::ENUM_MEMBER,
            tsc_types::EscapedName::internal("V"),
        );
        let enum_type = state.tables.get_enum_literal_type(
            tsc_types::LiteralValue::String("ßoo".into()),
            enumeration,
            member,
        );
        let fresh_enum = state.tables.get_fresh_type_of_literal_type(enum_type);
        let mapped = state.get_string_mapping_type(capitalize, regular).unwrap();
        assert_eq!(mapped, state.tables.get_string_literal_type("SSoo"));
        assert!(!state
            .tables
            .flags_of(mapped)
            .intersects(TypeFlags::ENUM_LITERAL));
        let count = state.tables.len();
        for input in [regular, fresh, enum_type, fresh_enum] {
            for _ in 0..3 {
                assert_eq!(
                    state.get_string_mapping_type(capitalize, input).unwrap(),
                    mapped
                );
                assert_eq!(state.tables.len(), count);
            }
        }
        assert!(state
            .tables
            .flags_of(enum_type)
            .intersects(TypeFlags::ENUM_LITERAL));
        assert_eq!(state.tables.type_of(fresh_enum).symbol, Some(member));
    });
}

#[test]
fn literal_mapping_values_survive_rejected_trials_and_utf16_inputs() {
    let units = |text: &str| text.encode_utf16().collect::<Vec<_>>();
    let cases = [
        ("Lowercase", units("ΟΣ"), units("ος")),
        (
            "Uppercase",
            vec![0xD800, 0x00DF],
            vec![0xD800, b'S' as u16, b'S' as u16],
        ),
        (
            "Lowercase",
            vec![0xD800, b'A' as u16],
            vec![0xD800, b'a' as u16],
        ),
        ("Capitalize", units("ßeta"), units("SSeta")),
        (
            "Capitalize",
            vec![0xD801, 0xDC28, b'x' as u16],
            vec![0xD801, 0xDC28, b'x' as u16],
        ),
        (
            "Uncapitalize",
            vec![0xDC00, b'X' as u16],
            vec![0xDC00, b'X' as u16],
        ),
        ("Uppercase", Vec::new(), Vec::new()),
    ];
    with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
        for (name, input, expected) in &cases {
            let symbol = literal_mapping_symbol(state, name);
            let input = state.tables.get_string_literal_type_from_utf16(input);
            let mapped = state
                .speculate(|state| {
                    let mapped = state.get_string_mapping_type(symbol, input)?;
                    Ok(crate::speculate::SpeculationOutcome::Reject(mapped))
                })
                .unwrap();
            let expected = state.tables.get_string_literal_type_from_utf16(expected);
            assert_eq!(mapped, expected);
            let count = state.tables.len();
            for _ in 0..4 {
                assert_eq!(
                    state.get_string_mapping_type(symbol, input).unwrap(),
                    expected
                );
                assert_eq!(state.tables.len(), count);
            }
        }
    });
}

#[test]
fn string_mapping_applies_to_literals_unions_and_generics() {
    with_program_state(
        &[("a.ts", "function f<T extends string>() { var v: T; }\n")],
        &CompilerOptions::default(),
        |state| {
            let uppercase = state.binder.create_symbol(
                SymbolFlags::TYPE_ALIAS,
                tsc_types::EscapedName::from_escaped_value(("Uppercase".to_owned()).into()),
            );
            let capitalize = state.binder.create_symbol(
                SymbolFlags::TYPE_ALIAS,
                tsc_types::EscapedName::from_escaped_value(("Capitalize".to_owned()).into()),
            );
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
            let mapper = state.create_type_mapper(&[t], Some(&[foo]));
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
            let uppercase = state.binder.create_symbol(
                SymbolFlags::TYPE_ALIAS,
                tsc_types::EscapedName::from_escaped_value(("Uppercase".to_owned()).into()),
            );
            let lowercase = state.binder.create_symbol(
                SymbolFlags::TYPE_ALIAS,
                tsc_types::EscapedName::from_escaped_value(("Lowercase".to_owned()).into()),
            );
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
            generic.set_type_parameters(Some(Box::new([t])));
            let generic = state.alloc_signature(generic);
            // A non-erasing instantiation clones the parameters.
            let identity = state.create_type_mapper(&[t], Some(&[t]));
            let cloned_signature = state
                .instantiate_signature(generic, identity, /*erase*/ false)
                .expect("instantiation in slice");
            let fresh = state
                .signature_of(cloned_signature)
                .cloned_type_parameters()
                .expect("fresh type parameters")[0];
            assert_ne!(fresh, t);
            // The clone carries its target and the instantiation's mapper
            // in its own record (tsc TypeParameter.target / .mapper).
            assert_eq!(state.tables.type_parameter_target(fresh), Some(t));
            assert!(state.tables.type_parameter_mapper(fresh).is_some());
            assert_eq!(state.tables.type_parameter_target(t), None);
            assert_eq!(state.tables.type_parameter_mapper(t), None);
            let constraint = state
                .get_constraint_from_type_parameter(fresh)
                .expect("constraint in slice");
            assert_eq!(constraint, Some(state.tables.intrinsics.string));
        },
    );
}

#[test]
fn a_this_only_mapper_keeps_thisless_symbols() {
    // tsgo instantiateSymbol: under a mapper of the `this` type only, a
    // symbol free of `this` references is its own instantiation. In the
    // implements clause's `I<this>`, `f` is instantiated (its second
    // overload returns `this is I`) but its signatures keep the declared
    // `i: any` parameter, though a fresh checker has not resolved its type.
    with_program_state(
        &[(
            "a.ts",
            "interface I {\n    f(i: any): i is I;\n    f(): this is I;\n}\nclass C implements I {}\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let source = state.binder.source(0);
            let find = |kind| {
                source
                    .arena
                    .node_ids()
                    .find(|&id| source.arena.node(id).kind == kind)
                    .expect("a node of the kind")
            };
            let heritage = find(tsc_syntax::SyntaxKind::ExpressionWithTypeArguments);
            let parameter = find(tsc_syntax::SyntaxKind::Parameter);
            let declared = state
                .get_symbol_of_declaration(parameter)
                .expect("the parameter's symbol");
            let implemented = state.get_type_at_location(heritage).expect("I<this>");
            let properties = state
                .get_properties_of_type(implemented)
                .expect("I<this>'s properties");
            let [f] = properties[..] else {
                panic!("one property: {properties:?}");
            };
            assert!(state
                .get_check_flags(f)
                .intersects(tsc_types::CheckFlags::INSTANTIATED));
            let ty = state.get_type_of_symbol(f).expect("f's type");
            let signatures = state
                .get_signatures_of_type(ty, crate::state::SignatureKind::Call)
                .expect("f's signatures");
            assert_eq!(signatures.len(), 2);
            assert_eq!(&state.signature_of(signatures[0]).parameters[..], &[declared]);
        },
    );
}

/// tsgo isNonGenericTopLevelType tests `len(t.alias.typeArguments) == 0`.
/// An imported alias whose type parameters all have defaults, written
/// without arguments, gets the alias with an empty argument list
/// (getTypeArgumentsFromNode). It is a non-generic top-level type, so
/// couldContainTypeVariables is false and instantiating the optional
/// property's `Concrete | undefined` stops at the union. tsc-rs matched
/// only an absent list and instantiated `Concrete` and its members as well.
/// Pinned to tsgo aa814927 over the same three files
/// (`tsc --singleThreaded --extendedDiagnostics`, `strict`, `noLib`,
/// `types: []`): the one TS2322 below and `Instantiations: 9` (11 before).
#[test]
fn imported_all_defaults_alias_without_arguments_is_non_generic() {
    let globals = concat!(
        "interface IArguments { length: number; callee: Function; }\n",
        "interface Array<T> { length: number; [index: number]: T; }\n",
        "interface Object {}\n",
        "interface Function {}\n",
        "interface CallableFunction extends Function {}\n",
        "interface NewableFunction extends Function {}\n",
        "interface String {}\n",
        "interface Number {}\n",
        "interface Boolean {}\n",
        "interface RegExp {}\n",
    );
    let alias = "export type Concrete<P = {}> = { setup(): void } | { props: P };\n";
    let main = concat!(
        "import type { Concrete } from \"./a\";\n",
        "interface Options<T> { value: T; resolved?: Concrete }\n",
        "declare function make<T>(value: T): Options<T>;\n",
        "const options = make(1);\n",
        "export const resolved = options.resolved;\n",
        "export const check: string = options.value;\n",
    );
    let result = crate::check_program(
        &[
            crate::InputFile::new("/globals.d.ts".to_owned(), globals.to_owned()),
            crate::InputFile::new("/a.ts".to_owned(), alias.to_owned()),
            crate::InputFile::new("/b.ts".to_owned(), main.to_owned()),
        ],
        &CompilerOptions {
            strict: Some(true),
            ..CompilerOptions::default()
        },
    );
    let rows: Vec<_> = result
        .diagnostics
        .iter()
        .map(|diagnostic| {
            (
                diagnostic
                    .file_name
                    .as_ref()
                    .map(|name| name.as_js().as_str().expect("scalar name").to_owned()),
                diagnostic.start,
                diagnostic.length,
                diagnostic.code(),
                diagnostic
                    .message_text()
                    .as_str()
                    .expect("scalar diagnostic observation")
                    .to_owned(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [(
            Some("/b.ts".to_owned()),
            Some(220),
            Some(5),
            2322,
            "Type 'number' is not assignable to type 'string'.".to_owned(),
        )]
    );
    assert_eq!(result.work_counters.instantiations(), 9);
}

#[test]
fn template_builders_keep_tsgo_case_mapping_and_surrogate_diagnostics() {
    // Pinned tsgo aa8149273, fixtures/A1.ts, strict/esnext/noEmit.
    let text = r#"export {};
type Uppercase<S extends string> = intrinsic;
type Lowercase<S extends string> = intrinsic;
type Capitalize<S extends string> = intrinsic;
type Uncapitalize<S extends string> = intrinsic;
type Nested = `pre-${`inner-${"x" | "y"}`}-post`;
const badNested: Nested = "wrong";
type Upper = Uppercase<"straße" | "\uD800ß" | "a\uDC00">;
const badUpper: Upper = "wrong";
type Lower = Lowercase<"ΟΣ" | "İ" | "\uD800A">;
const badLower: Lower = "wrong";
type Capped = Capitalize<`a${string}z`> | Uncapitalize<`B${number}Z`>;
const badCapped: Capped = "wrong";
type SurrogateSeam = `${"\uD800"}${"\uDC00"}`;
const badSeam: SurrogateSeam = "wrong";
type MappedTemplate = Uppercase<`pre${string}tail`> | Lowercase<`PRE${number}TAIL`>;
const badMappedTemplate: MappedTemplate = "wrong";
type EmptyPrefix = Capitalize<`${string}tail`> | Uncapitalize<`${string}TAIL`>;
const badEmptyPrefix: EmptyPrefix = "wrong";
type LiteralCase = Capitalize<"ßeta" | "\uD800alpha"> | Uncapitalize<"İOTA" | "\uDC00X">;
const badLiteralCase: LiteralCase = "wrong";
"#;
    let options = tsc_types::CompilerOptions {
        strict: Some(true),
        target: Some(tsc_types::ScriptTarget::ES_NEXT.bits()),
        ..Default::default()
    };
    crate::state::test_support::with_program_state(&[("a.ts", text)], &options, |state| {
        state.check_source_file(0);
        let messages = state
            .diagnostics
            .iter()
            .filter(|d| d.code() == 2322)
            .map(|d| {
                tsc_diagnostics::render::flattened_message(&d.message, "\n", None)
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            messages,
            [
                r#"Type '"wrong"' is not assignable to type '"pre-inner-x-post" | "pre-inner-y-post"'."#,
                r#"Type '"wrong"' is not assignable to type '"A\uDC00" | "STRASSE" | "\uD800SS"'."#,
                r#"Type '"wrong"' is not assignable to type '"i̇" | "ος" | "\uD800a"'."#,
                r#"Type '"wrong"' is not assignable to type 'Capped'."#,
                r#"Type '"wrong"' is not assignable to type '"𐀀"'."#,
                r#"Type '"wrong"' is not assignable to type 'MappedTemplate'."#,
                r#"Type '"wrong"' is not assignable to type 'EmptyPrefix'."#,
                r#"Type '"wrong"' is not assignable to type 'LiteralCase'."#,
            ]
        );
    });
}

#[test]
fn union_instantiation_keeps_its_source_members_while_nested_lists_grow() {
    let members = |element: &str| {
        (0..40)
            .map(|index| format!("[{element}, {index}]"))
            .collect::<Vec<_>>()
            .join(" | ")
    };
    let text = format!(
        "function f<T>() {{ var v: {}; var w: {}; }}",
        members("T"),
        members("string"),
    );
    with_program_state(
        &[("a.ts", text.as_str())],
        &CompilerOptions::default(),
        |state| {
            let annotation = annotation_of_var(state, "v");
            let ty = state.get_type_from_type_node(annotation).unwrap();
            let tsc_types::TypeData::Union { types, .. } = state.tables.type_of(ty).data else {
                panic!("tuple union");
            };
            let original = state.tables.list(types).to_vec();
            assert_eq!(original.len(), 40);
            let parameter = declared_type_parameter(state, "T");
            let string = state.tables.intrinsics.string;
            let mapper = state.create_type_mapper(&[parameter], Some(&[string]));
            let mapped = state.instantiate_type(ty, Some(mapper)).unwrap();
            assert_eq!(state.tables.list(types), original);
            let annotation = annotation_of_var(state, "w");
            let expected = state.get_type_from_type_node(annotation).unwrap();
            assert_eq!(mapped, expected);
        },
    );
}

#[test]
fn instantiation_scratch_keeps_nested_aliases_templates_and_conditionals_separate() {
    with_program_state(
        &[(
            "a.ts",
            "type Pair<A, B> = [A, B];\n\
             type Select<A> = A extends string ? `x${A}` : A;\n\
             function f<T extends string, U>() {\n\
               var source: Pair<T | U, [T, Select<T>, { value: T }]>;\n\
               var first: Pair<\"a\" | 1, [\"a\", \"xa\", { value: \"a\" }]>;\n\
               var second: Pair<\"b\" | 2, [\"b\", \"xb\", { value: \"b\" }]>;\n\
             }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let source_node = annotation_of_var(state, "source");
            let source = state.get_type_from_type_node(source_node).unwrap();
            let t = declared_type_parameter(state, "T");
            let u = declared_type_parameter(state, "U");
            for (name, text, value) in [("first", "a", 1.0), ("second", "b", 2.0)] {
                let literal = state.tables.get_string_literal_type(text);
                let number = state.tables.get_number_literal_type(value);
                let mapper = state.create_type_mapper(&[t, u], Some(&[literal, number]));
                let actual = state.with_instantiation_type_list(|state, enclosing| {
                    enclosing.push(source);
                    let actual = state.instantiate_type(source, Some(mapper)).unwrap();
                    assert_eq!(enclosing, &[source]);
                    actual
                });
                let expected_node = annotation_of_var(state, name);
                let expected = state.get_type_from_type_node(expected_node).unwrap();
                assert_eq!(
                    state.type_to_string(actual).unwrap(),
                    state.type_to_string(expected).unwrap()
                );
                let again = state.instantiate_type(source, Some(mapper)).unwrap();
                assert_eq!(again, actual, "cached results survive reuse of the buffers");
                assert!(state.instantiation_type_lists.iter().all(Vec::is_empty));
                assert!(state.instantiation_text_lists.iter().all(Vec::is_empty));
            }
        },
    );
}

#[test]
fn instantiation_scratch_returns_buffers_after_an_early_error() {
    with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
        let string = state.tables.intrinsics.string;
        let result: Result<(), &str> = state.with_instantiation_type_list(|state, outer| {
            outer.push(string);
            let inner: Result<(), &str> = state.with_instantiation_type_list(|_, inner| {
                inner.resize(128, string);
                Err("stop")
            });
            assert_eq!(outer, &[string]);
            inner?;
            Ok(())
        });
        assert_eq!(result, Err("stop"));
        assert!(state.instantiation_type_lists.iter().all(Vec::is_empty));
        let capacity: usize = state
            .instantiation_type_lists
            .iter()
            .map(Vec::capacity)
            .sum();
        state.with_instantiation_type_list(|state, outer| {
            outer.push(string);
            state.with_instantiation_type_list(|_, inner| inner.resize(128, string));
        });
        assert_eq!(
            state
                .instantiation_type_lists
                .iter()
                .map(Vec::capacity)
                .sum::<usize>(),
            capacity
        );
    });
}

#[test]
fn instantiation_expression_copies_keep_the_originating_type_query() {
    with_program_state(
        &[(
            "a.ts",
            "declare function id<A>(x: A): A; function f<T>() { var x: typeof id<T>; }",
        )],
        &CompilerOptions::default(),
        |state| {
            let query = annotation_of_var(state, "x");
            let source = state.get_type_from_type_node(query).unwrap();
            let node = state.type_deferred_node(source).unwrap();
            assert_eq!(node, query);
            let t = declared_type_parameter(state, "T");
            let mapper = state.make_unary_type_mapper(t, state.tables.intrinsics.string);
            let copy = state.instantiate_type(source, Some(mapper)).unwrap();
            assert_ne!(copy, source);
            assert_eq!(state.type_deferred_node(copy), Some(node));
            assert!(state
                .tables
                .object_flags_of(copy)
                .contains(ObjectFlags::INSTANTIATION_EXPRESSION_TYPE | ObjectFlags::INSTANTIATED));
        },
    );
}

#[test]
fn leaf_mappers_count_once_and_keep_the_first_matching_source() {
    with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
        let string = state.tables.intrinsics.string;
        let number = state.tables.intrinsics.number;
        let boolean = state.tables.intrinsics.boolean;
        let simple = state.make_unary_type_mapper(string, number);
        let array = state.create_type_mapper(&[string, string], Some(&[number, boolean]));
        let eraser = state.create_type_eraser(&[number, boolean]);
        let before = state.profile_ops[crate::line_profile::OP_MAPPED];
        for mapper in [simple, array] {
            assert_eq!(state.map_type_with_mapper(string, mapper).unwrap(), number);
            assert_eq!(
                state.map_type_with_mapper(boolean, mapper).unwrap(),
                boolean
            );
        }
        assert_eq!(
            state.map_type_with_mapper(number, eraser).unwrap(),
            state.tables.intrinsics.any
        );
        assert_eq!(state.map_type_with_mapper(string, eraser).unwrap(), string);
        assert_eq!(
            state.profile_ops[crate::line_profile::OP_MAPPED] - before,
            6
        );
        let parameter = state.tables.create_synthesized_type_parameter(None);
        let first = state.create_type_mapper(&[string, string], Some(&[parameter, boolean]));
        let second = state.make_unary_type_mapper(parameter, number);
        let merged = state.merge_type_mappers(Some(first), second);
        let composite = state.combine_type_mappers(Some(first), second);
        for (mapper, instantiations) in [(merged, 0), (composite, 1)] {
            let count = state.total_instantiation_count;
            let ops = state.profile_ops[crate::line_profile::OP_MAPPED];
            assert_eq!(state.map_type_with_mapper(string, mapper).unwrap(), number);
            assert_eq!(state.total_instantiation_count - count, instantiations);
            assert_eq!(state.profile_ops[crate::line_profile::OP_MAPPED] - ops, 3);
        }
    });
}

#[test]
fn mapper_cache_depths_keep_storage_and_clear_only_discarded_answers() {
    with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
        let ty = state.tables.intrinsics.string;
        let key = crate::state::MapperCacheKey {
            ty,
            alias_symbol: None,
            alias_arguments: None,
        };
        state.push_active_mapper(state.permissive_mapper);
        state.active_type_mappers_caches[0].insert(key, ty);
        let checkpoint = state.begin_speculation();
        state.push_active_mapper(state.restrictive_mapper);
        state.active_type_mappers_caches[1].insert(key, ty);
        let empty = state.tables.empty_type_list();
        let number = state.tables.intrinsics.number;
        for (alias, args, answer) in [(1, None, number), (1, Some(empty), ty), (2, None, ty)] {
            let aliased = crate::state::MapperCacheKey {
                alias_symbol: Some(tsc_types::SymbolId::new(alias)),
                alias_arguments: args,
                ..key
            };
            state.active_type_mappers_caches[1].insert(aliased, answer);
            assert_eq!(
                state.active_type_mappers_caches[1].get(&aliased),
                Some(&answer)
            );
        }
        assert_eq!(state.active_type_mappers_caches[1].len(), 4);
        let capacity = state.active_type_mappers_caches[1].capacities();
        state.rollback_speculation(checkpoint);
        assert_eq!(state.active_type_mappers_caches[0].get(&key), Some(&ty));
        state.push_active_mapper(state.unique_literal_mapper);
        assert_eq!(state.active_type_mappers_caches.len(), 2);
        assert_eq!(state.active_type_mappers_caches[1].len(), 0);
        assert_eq!(state.active_type_mappers_caches[1].capacities(), capacity);
        state.clear_active_mapper_caches();
        assert_eq!(state.active_type_mappers_caches[0].len(), 0);
        state.active_type_mappers_caches[1].insert(key, ty);
        state.pop_active_mapper();
        assert_eq!(state.active_type_mappers_caches[1].len(), 0);
        state.pop_active_mapper();
    });
}

#[test]
fn sparse_mapper_maps_rebuild_the_same_answers_after_pop() {
    with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
        let parameter = state.tables.create_synthesized_type_parameter(None);
        let number = state.tables.intrinsics.number;
        let outer = crate::state::MapperCacheKey {
            ty: parameter,
            alias_symbol: None,
            alias_arguments: None,
        };
        let mapper = state.make_unary_type_mapper(parameter, number);
        state.push_active_mapper(state.restrictive_mapper);
        state.active_type_mappers_caches[0].insert(outer, parameter);

        for alias_symbol in [None, Some(state.unknown_symbol)] {
            let key = crate::state::MapperCacheKey {
                alias_symbol,
                ..outer
            };
            let other = crate::state::MapperCacheKey {
                alias_symbol: alias_symbol.is_none().then_some(state.unknown_symbol),
                ..outer
            };
            let capacities = |cache: &crate::state::ActiveMapperCache| {
                let (plain, aliased) = cache.capacities();
                if alias_symbol.is_some() {
                    (aliased, plain)
                } else {
                    (plain, aliased)
                }
            };
            state.push_active_mapper(mapper);
            let cache = &mut state.active_type_mappers_caches[1];
            for index in 0..1024 {
                cache.insert(
                    crate::state::MapperCacheKey {
                        ty: tsc_types::TypeId::new(index),
                        ..key
                    },
                    number,
                );
            }
            cache.insert(other, number);
            let (large, small) = capacities(cache);
            assert!(large >= 1024);
            state.pop_active_mapper();
            assert_eq!(
                capacities(&state.active_type_mappers_caches[1]),
                (large, small)
            );

            let check_answer = |state: &mut CheckerState| {
                let count = state.total_instantiation_count;
                for _ in 0..2 {
                    assert_eq!(
                        state
                            .instantiate_type_with_alias(
                                parameter,
                                mapper,
                                alias_symbol,
                                None,
                                None
                            )
                            .unwrap(),
                        number
                    );
                    assert_eq!(state.total_instantiation_count - count, 1);
                }
            };
            let checkpoint = state.begin_speculation();
            state.push_active_mapper(mapper);
            check_answer(state);
            state.rollback_speculation(checkpoint);
            assert_eq!(capacities(&state.active_type_mappers_caches[1]), (0, small));
            assert_eq!(
                state.active_type_mappers_caches[0].get(&outer),
                Some(&parameter)
            );
            state.push_active_mapper(mapper);
            check_answer(state);
            state.pop_active_mapper();
        }
        state.pop_active_mapper();
    });
}

#[test]
fn changed_instantiation_list_materializes_only_after_a_difference() {
    with_program_state(
        &[("a.ts", "function f<T>() { var x: T; }")],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let number = state.tables.intrinsics.number;
            let string = state.tables.intrinsics.string;
            let mapper = state.make_unary_type_mapper(t, string);
            for (items, expected) in [
                (vec![], None),
                (vec![number, number], None),
                (vec![t, number, t], Some(vec![string, number, string])),
                (vec![number, t, number], Some(vec![number, string, number])),
                (vec![number, number, t], Some(vec![number, number, string])),
            ] {
                let list = state.tables.intern_type_list(&items);
                let depth = state.instantiation_type_lists.len();
                let count = state.total_instantiation_count;
                state
                    .with_changed_instantiation_list(list, mapper, |_, changed| {
                        assert_eq!(changed, expected.as_deref());
                        Ok(())
                    })
                    .unwrap();
                assert_eq!(
                    state.total_instantiation_count - count,
                    items.iter().filter(|&&item| item == t).count() as u64
                );
                if expected.is_none() {
                    assert_eq!(state.instantiation_type_lists.len(), depth);
                }
                assert!(state.instantiation_type_lists.iter().all(Vec::is_empty));
            }
        },
    );
}

#[test]
fn conditional_key_mappers_match_composition_on_distributed_parameters() {
    with_program_state(
        &[("a.ts", "function f<T,U>() { var x: T; }")],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let u = declared_type_parameter(state, "U");
            let distributed = state.get_distributed_type_from_type_parameter(t);
            let string = state.tables.intrinsics.string;
            for (source, target) in [(u, string), (t, u), (t, distributed)] {
                let first = state.make_unary_type_mapper(source, target);
                let second = state.create_type_mapper(&[t, u], Some(&[string, string]));
                let composite = state.combine_type_mappers(Some(first), second);
                let before = state.total_instantiation_count;
                let visits = state.profile_ops[crate::line_profile::OP_MAPPED];
                let expected = state.get_mapped_type(distributed, composite).unwrap();
                let after = state.total_instantiation_count;
                let middle = state.profile_ops[crate::line_profile::OP_MAPPED];
                let mapper_count = state.mappers.len();
                assert_eq!(
                    state
                        .map_conditional_parameter(distributed, Some(first), second)
                        .unwrap(),
                    expected
                );
                assert_eq!(state.mappers.len(), mapper_count);
                assert_eq!(state.total_instantiation_count - after, after - before);
                assert_eq!(
                    state.profile_ops[crate::line_profile::OP_MAPPED] - middle,
                    middle - visits
                );
            }
        },
    );
}

#[test]
fn short_signature_keeps_this_and_parameter_order_with_a_fresh_generic() {
    with_program_state(
        &[(
            "a.ts",
            "function f<T>() { var sig: <U>(this: T, first: U, second: T) => U; }",
        )],
        &CompilerOptions::default(),
        |state| {
            let annotation = annotation_of_var(state, "sig");
            let signature = state.get_signature_from_declaration(annotation).unwrap();
            let original = state.signature_of(signature).type_parameters().unwrap()[0];
            let t = declared_type_parameter(state, "T");
            let string = state.tables.intrinsics.string;
            let mapper = state.make_unary_type_mapper(t, string);
            let mapped = state
                .instantiate_signature(signature, mapper, false)
                .unwrap();
            let result = state.signature_of(mapped);
            let fresh = result.type_parameters().unwrap()[0];
            assert_ne!(fresh, original);
            let this = result.this_parameter.unwrap();
            let [first, second] = *result.parameters else {
                panic!("expected two parameters")
            };
            assert_eq!(state.get_type_of_symbol(this).unwrap(), string);
            assert_eq!(state.get_type_of_symbol(first).unwrap(), fresh);
            assert_eq!(state.get_type_of_symbol(second).unwrap(), string);
        },
    );
}

#[test]
fn known_alias_arguments_preserve_absent_and_empty_cache_entries() {
    with_program_state(
        &[("a.ts", "function f<T>() { var v: T; }")],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let alias = state.tables.type_of(t).symbol;
            assert!(alias.is_some());
            let string = state.tables.intrinsics.string;
            let mapper = state.make_unary_type_mapper(t, string);
            let empty = state.tables.empty_type_list();
            state.push_active_mapper(mapper);
            let before = state.total_instantiation_count;
            for (args, known, delta) in [
                (None, None, 1),
                (Some([].as_slice()), Some(empty), 2),
                (Some([].as_slice()), None, 2),
                (None, None, 2),
            ] {
                assert_eq!(
                    state
                        .instantiate_type_with_alias(t, mapper, alias, args, known)
                        .unwrap(),
                    string
                );
                assert_eq!(state.total_instantiation_count - before, delta);
            }
            state.pop_active_mapper();
        },
    );
}

#[test]
fn depth_limit_precedes_an_active_mapper_hit() {
    with_program_state(
        &[("a.ts", "function f<T>() { var v: T; }")],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let string = state.tables.intrinsics.string;
            let mapper = state.make_unary_type_mapper(t, string);
            state.push_active_mapper(mapper);
            assert_eq!(state.instantiate_type(t, Some(mapper)).unwrap(), string);
            let before = state.total_instantiation_count;
            state.instantiation_stack.resize(100, string);
            assert_eq!(
                state.instantiate_type(t, Some(mapper)).unwrap(),
                state.tables.intrinsics.error
            );
            assert_eq!(state.total_instantiation_count, before);
            assert!(state.diagnostics.iter().any(|d| d.code() == 2589));
            state.instantiation_stack.clear();
            state.pop_active_mapper();
        },
    );
}

#[test]
fn instantiated_transient_keeps_original_flags_name_and_declarations_after_rollback() {
    use tsc_types::CheckFlags;
    with_program_state(
        &[("a.ts", "declare var prop: string;")],
        &CompilerOptions::default(),
        |state| {
            let annotation = annotation_of_var(state, "prop");
            let node = state.parent_of(annotation).unwrap();
            let persistent = state.get_symbol_of_declaration(node).unwrap();
            let mapper = state.permissive_mapper;
            let before = state.binder.transient_symbol_count();
            let copy = state.new_instantiated_symbol(persistent, mapper);
            assert_eq!(state.binder.transient_symbol_count(), before + 1);
            let links = state.links.symbol(copy);
            assert_eq!(links.target, Some(persistent));
            assert_eq!(links.mapper, Some(mapper));
            assert_eq!(links.check_flags, CheckFlags::INSTANTIATED);
            assert_eq!(links.name_type, None);
            assert_eq!(state.binder.symbol(copy).declarations(), &[node]);
            let name = state.binder.symbol(persistent).escaped_name;
            let original = state.binder.create_symbol(SymbolFlags::PROPERTY, name);
            assert_eq!(original.index(), copy.index() + 1);
            state.binder.set_declarations_from(original, &[node]);
            state.binder.symbol_mut(original).value_declaration = Some(node);
            let flags = CheckFlags::READONLY | CheckFlags::OPTIONAL_PARAMETER;
            let name_type = state.tables.intrinsics.string;
            state.links.set_symbol_check_flags(0, original, flags);
            state
                .links
                .set_symbol_name_type(0, original, Some(name_type));
            let first = state.new_instantiated_symbol(original, state.permissive_mapper);
            assert_eq!(state.binder.transient_symbol_count(), before + 3);
            assert_eq!(first.index(), original.index() + 1);
            let links = state.links.symbol(first);
            assert_eq!(links.target, Some(original));
            assert_eq!(links.mapper, Some(mapper));
            assert_eq!(links.check_flags, flags | CheckFlags::INSTANTIATED);
            assert_eq!(links.name_type, Some(name_type));
            state
                .links
                .set_symbol_name_type(0, first, Some(state.tables.intrinsics.number));
            let mut previous = first;
            for trial in [true, false] {
                let mark = trial.then(|| state.begin_speculation());
                let before = state.binder.transient_symbol_count();
                let result = state.new_instantiated_symbol(first, state.restrictive_mapper);
                assert_eq!(state.binder.transient_symbol_count(), before + 1);
                if let Some(mark) = mark {
                    state.rollback_speculation(mark);
                }
                let links = state.links.symbol(result);
                assert_eq!(links.target, Some(original));
                assert_eq!(links.check_flags, flags | CheckFlags::INSTANTIATED);
                assert_eq!(links.name_type, Some(name_type));
                assert!(
                    matches!(state.mapper(links.mapper.unwrap()), super::TypeMapper::Composite { mapper1, mapper2 } if *mapper1 == state.permissive_mapper && *mapper2 == state.restrictive_mapper)
                );
                assert_eq!(state.binder.symbol(result).declarations(), &[node]);
                assert_eq!(state.binder.symbol(result).value_declaration, Some(node));
                assert_eq!(result.index(), previous.index() + 1);
                previous = result;
            }
        },
    );
}

#[test]
fn anonymous_shells_keep_alias_arguments_creation_order_and_origin() {
    let source = "declare function id<A>(x: A): A;\n\
                  function f<T>() { var x: typeof id<T>; var m: { [K in keyof T]: T[K] }; }";
    with_program_state(&[("a.ts", source)], &CompilerOptions::default(), |state| {
        let t = declared_type_parameter(state, "T");
        let string = state.tables.intrinsics.string;
        let explicit_arg = state
            .tables
            .create_type(TypeFlags::OBJECT, tsc_types::TypeData::PLAIN_OBJECT);
        state.tables.type_mut(explicit_arg).object_flags =
            ObjectFlags::ANONYMOUS | ObjectFlags::CONTAINS_OBJECT_OR_ARRAY_LITERAL;
        let mapper = state.make_unary_type_mapper(t, string);
        let original = state.tables.type_of(t).symbol.unwrap();
        let other = state.binder.create_symbol(
            SymbolFlags::TYPE_ALIAS,
            tsc_types::EscapedName::internal("outer"),
        );
        let args = state.tables.intern_type_list(&[t]);
        for (var, mapped) in [("x", false), ("m", true)] {
            let node = annotation_of_var(state, var);
            let source = state.get_type_from_type_node(node).unwrap();
            state.tables.set_alias(source, Some(original), Some(args));
            for explicit in [false, true] {
                let before = state.tables.len() as u32;
                let result = state
                    .instantiate_anonymous_type(
                        source,
                        mapper,
                        explicit.then_some(other),
                        explicit.then_some([explicit_arg].as_slice()),
                    )
                    .unwrap();
                assert_eq!(result.index(), before + u32::from(mapped));
                let inherited = state.tables.object_flags_of(source).bits()
                    & !(ObjectFlags::COULD_CONTAIN_TYPE_VARIABLES_COMPUTED
                        | ObjectFlags::COULD_CONTAIN_TYPE_VARIABLES)
                        .bits();
                let propagated = if explicit {
                    ObjectFlags::CONTAINS_OBJECT_OR_ARRAY_LITERAL
                } else {
                    ObjectFlags::NONE
                };
                assert_eq!(
                    state.tables.object_flags_of(result),
                    ObjectFlags::from_bits(inherited) | ObjectFlags::INSTANTIATED | propagated
                );
                if mapped {
                    let fresh = state.get_type_parameter_from_mapped_type(result).unwrap();
                    assert_eq!(fresh.index(), before);
                    assert_eq!(state.mapped_type_declaration(result), node);
                } else {
                    assert_eq!(state.type_deferred_node(result), Some(node));
                }
                let expected = if explicit { explicit_arg } else { string };
                assert_eq!(
                    state.tables.alias_symbol(result),
                    Some(if explicit { other } else { original })
                );
                assert_eq!(
                    state.tables.alias_type_arguments(result),
                    Some([expected].as_slice())
                );
            }
        }
    });
}
