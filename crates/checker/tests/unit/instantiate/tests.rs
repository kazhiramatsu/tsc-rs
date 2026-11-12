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
            generic.type_parameters = Some(Box::new([t]));
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
            generic.type_parameters = Some(Box::new([t]));
            let generic = state.alloc_signature(generic);
            // A non-erasing instantiation clones the parameters.
            let identity = state.create_type_mapper(&[t], Some(&[t]));
            let cloned_signature = state
                .instantiate_signature(generic, identity, /*erase*/ false)
                .expect("instantiation in slice");
            let fresh = state
                .signature_of(cloned_signature)
                .type_parameters
                .clone()
                .expect("fresh type parameters")[0];
            assert_ne!(fresh, t);
            assert_eq!(
                *state.links.type_cold().type_parameter_target.get(fresh),
                Some(t)
            );
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
