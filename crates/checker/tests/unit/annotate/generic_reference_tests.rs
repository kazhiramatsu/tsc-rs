use tsc_types::{CompilerOptions, TypeData, TypeFlags};

use crate::relpin::find_probe_annotation;
use crate::state::test_support::with_program_state;
use crate::state::CheckerState;

fn annotation_of(state: &CheckerState, name: &str) -> tsc_syntax::NodeId {
    find_probe_annotation(state.binder.source(0), name).expect("var with annotation")
}

#[test]
fn generic_reference_instantiates_and_interns() {
    with_program_state(
        &[(
            "a.ts",
            "interface I<T> { a: T }\ndeclare var v: I<string>;\ndeclare var w: I<string>;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let v = annotation_of(state, "v");
            let reference = state.get_type_from_type_node(v).expect("I<string>");
            assert!(matches!(
                state.tables.type_of(reference).data,
                TypeData::Reference { .. }
            ));
            assert_eq!(
                state.tables.type_arguments(reference),
                &[state.tables.intrinsics.string]
            );
            let w = annotation_of(state, "w");
            let again = state.get_type_from_type_node(w).expect("I<string>");
            assert_eq!(again, reference, "reference interning by target+list");
            assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
        },
    );
}

#[test]
fn bare_generic_reference_reports_2314_with_local_parameter_display() {
    with_program_state(
        &[(
            "a.ts",
            "function f<T>() { interface I<U> { a: [T, U] } var v: I; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let v = annotation_of(state, "v");
            let resolved = state.get_type_from_type_node(v).expect("errorType flows");
            assert!(state.tables.is_error_type(resolved));
            let rendered: Vec<(u32, String)> = state
                .diagnostics
                .iter()
                .map(|d| (d.code(), d.message_text().to_owned()))
                .collect();
            assert_eq!(
                rendered,
                [(
                    2314,
                    "Generic type 'I<U>' requires 1 type argument(s).".to_owned()
                )],
                "oracle-pinned local-parameters-only display"
            );
        },
    );
}

#[test]
fn arity_range_reports_2707() {
    with_program_state(
        &[(
            "a.ts",
            "interface K<T, U = string> { }\ndeclare var v: K;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let v = annotation_of(state, "v");
            let resolved = state.get_type_from_type_node(v).expect("errorType flows");
            assert!(state.tables.is_error_type(resolved));
            let rendered: Vec<(u32, String)> = state
                .diagnostics
                .iter()
                .map(|d| (d.code(), d.message_text().to_owned()))
                .collect();
            assert_eq!(
                rendered,
                [(
                    2707,
                    "Generic type 'K<T, U>' requires between 1 and 2 type arguments.".to_owned()
                )]
            );
        },
    );
}

#[test]
fn type_parameter_defaults_fill_missing_arguments() {
    with_program_state(
        &[(
            "a.ts",
            "interface K<T, U = string> { }\ninterface L<T, U = T> { }\n\
             declare var v: K<number>;\ndeclare var w: L<number>;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let v = annotation_of(state, "v");
            let reference = state.get_type_from_type_node(v).expect("K<number>");
            assert_eq!(
                state.tables.type_arguments(reference),
                &[
                    state.tables.intrinsics.number,
                    state.tables.intrinsics.string
                ]
            );
            // U = T instantiates the default through the partially
            // filled argument list.
            let w = annotation_of(state, "w");
            let reference = state.get_type_from_type_node(w).expect("L<number>");
            assert_eq!(
                state.tables.type_arguments(reference),
                &[
                    state.tables.intrinsics.number,
                    state.tables.intrinsics.number
                ]
            );
            assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
        },
    );
}

#[test]
fn mutually_circular_defaults_resolve_silently_via_the_sentinel() {
    with_program_state(
        &[(
            "a.ts",
            "interface P<T = Q> { }\ninterface Q<U = P> { }\ndeclare var v: P;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let v = annotation_of(state, "v");
            let reference = state.get_type_from_type_node(v).expect("P resolves");
            // P<Q<P<unknown>>>: the re-entrant default stamps the
            // circular sentinel, which reads as "no default" and
            // falls back to unknownType (2716 is a 5.8 declaration
            // check, not a reference-site diagnostic).
            assert!(matches!(
                state.tables.type_of(reference).data,
                TypeData::Reference { .. }
            ));
            assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
            // The re-entrant stamp survives (tsc keeps the circular
            // sentinel over the successfully computed default), so
            // T's default reads as none -> unknownType.
            let args = state.tables.type_arguments(reference).to_vec();
            assert_eq!(args, [state.tables.intrinsics.unknown]);
        },
    );
}

#[test]
fn stray_type_arguments_report_2315() {
    with_program_state(
        &[(
            "a.ts",
            "type A = string;\ndeclare var v: A<number>;\nfunction f<T>() { var w: T<string>; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let v = annotation_of(state, "v");
            let resolved = state.get_type_from_type_node(v).expect("errorType flows");
            assert!(state.tables.is_error_type(resolved));
            let w = annotation_of(state, "w");
            let resolved = state.get_type_from_type_node(w).expect("errorType flows");
            assert!(state.tables.is_error_type(resolved));
            let rendered: Vec<(u32, String)> = state
                .diagnostics
                .iter()
                .map(|d| (d.code(), d.message_text().to_owned()))
                .collect();
            assert_eq!(
                rendered,
                [
                    (2315, "Type 'A' is not generic.".to_owned()),
                    (2315, "Type 'T' is not generic.".to_owned()),
                ]
            );
        },
    );
}

#[test]
fn alias_hosted_generic_references_defer_and_resolve_lazily() {
    with_program_state(
        &[(
            "a.ts",
            "interface I<T> { a: T }\ntype X = I<number>;\ndeclare var v: X;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let v = annotation_of(state, "v");
            let deferred = state.get_type_from_type_node(v).expect("alias RHS defers");
            // The deferred shell: Reference object flags, a node,
            // the alias stamp, and NO resolved arguments yet.
            assert!(matches!(
                state.tables.type_of(deferred).data,
                TypeData::Reference {
                    resolved_type_arguments: None,
                    ..
                }
            ));
            assert!(state.links.ty(deferred).deferred_node.is_some());
            assert!(state.tables.type_of(deferred).alias_symbol.is_some());
            // Forcing reads the node lazily.
            let arguments = state.get_type_arguments(deferred).expect("forcible");
            assert_eq!(arguments, [state.tables.intrinsics.number]);
            assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
        },
    );
}

#[test]
fn self_referential_deferred_aliases_resolve_without_circularity() {
    with_program_state(
        &[("a.ts", "type A = [A];\ndeclare var v: A;\n")],
        &CompilerOptions::default(),
        |state| {
            let v = annotation_of(state, "v");
            let deferred = state.get_type_from_type_node(v).expect("tuple RHS defers");
            // `type A = [A]` is LEGAL through deferral (the eager
            // path would 2456): the argument list is the deferred
            // reference itself.
            let arguments = state.get_type_arguments(deferred).expect("forcible");
            assert_eq!(arguments, [deferred]);
            assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
        },
    );
}

#[test]
fn alias_hosted_array_nodes_defer_over_the_global_array_target() {
    with_program_state(
        &[(
            "a.ts",
            "interface Array<T> { length: number }\ntype A = string[];\ndeclare var v: A;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let v = annotation_of(state, "v");
            let deferred = state.get_type_from_type_node(v).expect("array RHS defers");
            assert!(state.links.ty(deferred).deferred_node.is_some());
            let target = state.tables.reference_target(deferred);
            assert!(matches!(
                state.tables.type_of(target).data,
                TypeData::GenericType { .. }
            ));
            let arguments = state.get_type_arguments(deferred).expect("forcible");
            assert_eq!(arguments, [state.tables.intrinsics.string]);
            assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
        },
    );
}

#[test]
fn plain_array_annotations_resolve_eagerly_against_the_array_global() {
    with_program_state(
        &[(
            "a.ts",
            "interface Array<T> { length: number }\ndeclare var v: number[];\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let v = annotation_of(state, "v");
            let reference = state.get_type_from_type_node(v).expect("arrays construct");
            // No alias host, no alias-resolvable elements: the
            // eager arm builds a plain resolved reference.
            assert!(state.links.ty(reference).deferred_node.is_none());
            assert_eq!(
                state.tables.type_arguments(reference),
                [state.tables.intrinsics.number]
            );
            assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
        },
    );
}

#[test]
fn missing_array_global_reports_2318_and_empty_object_type() {
    with_program_state(
        &[("a.ts", "declare var v: number[];\n")],
        &CompilerOptions::default(),
        |state| {
            let v = annotation_of(state, "v");
            let resolved = state.get_type_from_type_node(v).expect("fallback resolves");
            // getArrayOrTupleTargetType finds emptyGenericType (the
            // memoized getGlobalType failure) -> emptyObjectType
            // (61122-61123), with the one-shot 2318.
            assert_eq!(resolved, state.empty_object_type);
            assert_eq!(state.diagnostics.len(), 1, "{:?}", state.diagnostics);
            assert_eq!(state.diagnostics[0].code(), 2318);
        },
    );
}

#[test]
fn empty_tuple_aliases_resolve_to_the_tuple_target() {
    with_program_state(
        &[("a.ts", "type E = [];\ndeclare var v: E;\n")],
        &CompilerOptions::default(),
        |state| {
            let v = annotation_of(state, "v");
            let resolved = state.get_type_from_type_node(v).expect("empty tuple");
            // 61124: zero-element deferrable tuples return the
            // TARGET itself, not a deferred reference.
            assert!(matches!(
                state.tables.type_of(resolved).data,
                TypeData::TupleTarget(_)
            ));
            assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
        },
    );
}

#[test]
fn heritage_with_type_arguments_instantiates_inherited_members() {
    with_program_state(
        &[(
            "a.ts",
            "interface A<T> { a: T }\ninterface B extends A<string> { b: number }\n\
             declare var v: B;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let v = annotation_of(state, "v");
            let b = state.get_type_from_type_node(v).expect("B resolves");
            let a_property = state
                .get_property_of_type_full(b, "a")
                .expect("members resolve")
                .expect("inherited property");
            let a_type = state
                .get_type_of_symbol(a_property)
                .expect("inherited property type");
            assert_eq!(a_type, state.tables.intrinsics.string);
            assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
        },
    );
}

#[test]
fn generic_heritage_chains_map_members_through_the_reference() {
    with_program_state(
        &[(
            "a.ts",
            "interface A<T> { a: T }\ninterface B<U> extends A<U> { b: U }\n\
             declare var v: B<number>;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let v = annotation_of(state, "v");
            let b = state.get_type_from_type_node(v).expect("B<number>");
            for name in ["a", "b"] {
                let property = state
                    .get_property_of_type_full(b, name)
                    .expect("members resolve")
                    .expect("property present");
                let property_type = state.get_type_of_symbol(property).expect("property type");
                assert_eq!(
                    property_type, state.tables.intrinsics.number,
                    "{name} instantiates through the heritage mapper"
                );
            }
            assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
        },
    );
}

#[test]
fn deferred_reference_members_force_arguments_lazily() {
    with_program_state(
        &[(
            "a.ts",
            "interface Box<T> { value: T }\ntype A = Box<number>;\ndeclare var v: A;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let v = annotation_of(state, "v");
            let deferred = state.get_type_from_type_node(v).expect("alias RHS defers");
            assert!(state.links.ty(deferred).deferred_node.is_some());
            let value = state
                .get_property_of_type_full(deferred, "value")
                .expect("deferred members resolve")
                .expect("value property");
            let value_type = state.get_type_of_symbol(value).expect("property type");
            assert_eq!(value_type, state.tables.intrinsics.number);
            assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
        },
    );
}

#[test]
fn circular_heritage_reports_one_2310_per_interface() {
    with_program_state(
        &[(
            "a.ts",
            "interface A extends B { }\ninterface B extends A { }\ndeclare var v: A;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let v = annotation_of(state, "v");
            let a = state.get_type_from_type_node(v).expect("A resolves");
            let members = state
                .resolve_structured_type_members(a)
                .expect("cycle-cut members resolve");
            assert!(state.members_of(members).properties.is_empty());
            // Oracle-pinned (with-lib CLI): exactly one 2310 per
            // interface — the duplicate report on A collapses in
            // tsc's diagnostics.add equality dedupe.
            let codes: Vec<u32> = state
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code())
                .collect();
            assert_eq!(codes, [2310, 2310], "{:?}", state.diagnostics);
            assert_ne!(
                state.diagnostics[0].start, state.diagnostics[1].start,
                "one per declaration"
            );
        },
    );
}

#[test]
fn thisful_interface_members_substitute_the_reference_for_this() {
    with_program_state(
        &[(
            "a.ts",
            "interface C { self: this; tag: string }\ndeclare var v: C;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let v = annotation_of(state, "v");
            let c = state.get_type_from_type_node(v).expect("C resolves");
            // this-ful interfaces are GenericType targets; the
            // annotation resolves to the declared type itself.
            let self_property = state
                .get_property_of_type_full(c, "self")
                .expect("members resolve")
                .expect("self property");
            let self_type = state.get_type_of_symbol(self_property).expect("self type");
            assert_eq!(
                self_type, c,
                "this maps to the reference through the this-argument mapper"
            );
            // Thisless members skip instantiation entirely
            // (mappingThisOnly): `tag` keeps the ORIGINAL symbol.
            let tag_property = state
                .get_property_of_type_full(c, "tag")
                .expect("members resolve")
                .expect("tag property");
            assert!(
                !state
                    .links
                    .symbol(tag_property)
                    .check_flags
                    .intersects(tsc_types::CheckFlags::INSTANTIATED),
                "thisless member symbols pass through uninstantiated"
            );
            assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
        },
    );
}

#[test]
fn primitive_apparent_types_read_the_wrapper_globals() {
    with_program_state(
        &[(
            "a.ts",
            "interface String { length: number }\ndeclare var v: \"abc\";\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let v = annotation_of(state, "v");
            let literal = state.get_type_from_type_node(v).expect("literal type");
            let length = state
                .get_property_of_type_full(literal, "length")
                .expect("apparent members resolve")
                .expect("length property via globalStringType");
            let length_type = state.get_type_of_symbol(length).expect("length type");
            assert_eq!(length_type, state.tables.intrinsics.number);
            assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
        },
    );
}

#[test]
fn intersection_apparent_substitutes_this_across_constituents() {
    with_program_state(
        &[(
            "a.ts",
            "interface C { self: this }\ntype X = C & { x: number };\ndeclare var v: X;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let v = annotation_of(state, "v");
            let x = state.get_type_from_type_node(v).expect("X resolves");
            // getApparentTypeOfIntersectionType: this maps to the
            // WHOLE intersection before the property lookup.
            let self_property = state
                .get_property_of_type_full(x, "self")
                .expect("intersection apparent resolves")
                .expect("self property");
            let self_type = state.get_type_of_symbol(self_property).expect("self type");
            assert_eq!(self_type, x, "this-argument = the intersection");
            assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
        },
    );
}

#[test]
fn empty_subinterfaces_normalize_to_their_single_base() {
    with_program_state(
        &[(
            "a.ts",
            "interface A { self: this; a: number }\ninterface J extends A { }\n\
             declare var v: J;\ndeclare var w: A;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            // A is this-ful, so the empty J is this-ful too — both
            // are GenericType references, the shape getSingleBase
            // requires.
            let v = annotation_of(state, "v");
            let j = state.get_type_from_type_node(v).expect("J resolves");
            let w = annotation_of(state, "w");
            let a = state.get_type_from_type_node(w).expect("A resolves");
            let normalized = state
                .get_normalized_type(j, /*writing*/ false)
                .expect("single-base collapse");
            assert_eq!(normalized, a, "empty J collapses to its single base A");
            assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
        },
    );
}

#[test]
fn generic_subinterfaces_do_not_collapse_their_single_base() {
    with_program_state(
        &[(
            "a.ts",
            "interface I<T> { a: T }\ninterface J<T> extends I<T> { }\n\
             declare var v: J<number>;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            // The type parameter T lives in J's symbol MEMBERS
            // (binder parity with tsc), so the non-augmenting
            // collapse's `getMembersOfSymbol(symbol).size` gate
            // rejects generic subinterfaces.
            let v = annotation_of(state, "v");
            let j = state.get_type_from_type_node(v).expect("J<number>");
            let single = state
                .get_single_base_for_non_augmenting_subtype(j)
                .expect("computes");
            assert_eq!(single, None);
            let normalized = state
                .get_normalized_type(j, /*writing*/ false)
                .expect("normalizes");
            assert_eq!(normalized, j);
            assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
        },
    );
}

#[test]
fn circular_tuple_type_arguments_report_4110() {
    with_program_state(
        &[(
            "a.ts",
            "interface Array<T> { length: number }\ntype A = [A[0]];\ndeclare var v: A;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let v = annotation_of(state, "v");
            let deferred = state.get_type_from_type_node(v).expect("tuple RHS defers");
            // Forcing the arguments resolves A[0], whose property
            // lookup re-enters getTypeArguments on the same
            // reference — the pop-failure arm fills errorType and
            // reports 4110 at the tuple node (oracle-pinned).
            let arguments = state.get_type_arguments(deferred).expect("forcible");
            assert_eq!(arguments, [state.tables.intrinsics.error]);
            let codes: Vec<u32> = state
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code())
                .collect();
            assert_eq!(codes, [4110], "{:?}", state.diagnostics);
        },
    );
}

#[test]
fn circular_interface_type_arguments_report_4109() {
    with_program_state(
        &[(
            "a.ts",
            "interface I<T> { a: T }\ntype B = I<B[\"a\"]>;\ndeclare var w: B;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let w = annotation_of(state, "w");
            let deferred = state.get_type_from_type_node(w).expect("alias RHS defers");
            let arguments = state.get_type_arguments(deferred).expect("forcible");
            assert_eq!(arguments, [state.tables.intrinsics.error]);
            let codes: Vec<u32> = state
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code())
                .collect();
            assert_eq!(codes, [4109], "{:?}", state.diagnostics);
        },
    );
}

#[test]
fn labeled_tuples_synthesize_named_index_members() {
    with_program_state(
        &[(
            "a.ts",
            "interface Array<T> { length: number }\n\
             type P = [x: number, y?: string];\ndeclare var v: P[0];\n",
        )],
        &CompilerOptions::default(),
        |state| {
            // Labeled tuple targets intern with the node-id key
            // segment and carry tupleLabelDeclaration on the
            // synthesized properties.
            let v = annotation_of(state, "v");
            let resolved = state.get_type_from_type_node(v).expect("P[0]");
            assert_eq!(resolved, state.tables.intrinsics.number);
            assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
        },
    );
}

#[test]
fn rest_parameter_arity_reads_tuple_rest_types() {
    with_program_state(
        &[(
            "a.ts",
            "interface Array<T> { length: number }\n\
             declare var f: (...args: [number, string?]) => void;\n\
             declare var g: (a: number, b?: string) => void;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            // Tuple rest parameters expand for arity: f accepts
            // (number, string?) exactly like g — assignable both
            // ways through the signature arity machinery.
            let f_node = annotation_of(state, "f");
            let f = state.get_type_from_type_node(f_node).expect("f resolves");
            let g_node = annotation_of(state, "g");
            let g = state.get_type_from_type_node(g_node).expect("g resolves");
            assert_eq!(state.is_type_assignable_to(f, g), Ok(true));
            assert_eq!(state.is_type_assignable_to(g, f), Ok(true));
            assert!(
                state.diagnostics.iter().all(|d| d.file_name.is_none()),
                "{:?}",
                state.diagnostics
            );
        },
    );
}

#[test]
fn union_members_synthesize_combined_call_signatures() {
    with_program_state(
        &[(
            "a.ts",
            // The Array interface stands in for the lib global:
            // the 5.3b array-target relation arm probes
            // global(Readonly)ArrayType on object-object pairs,
            // and the no-lib one-shot 2318 would dirty the
            // asserted-empty diagnostics.
            "interface Array<T> { length: number }\n\
             interface ReadonlyArray<T> { length: number }\n\
             type F = (() => number) | (() => string);\ndeclare var v: F;\n\
             declare var w: () => number | string;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let v = annotation_of(state, "v");
            let f = state.get_type_from_type_node(v).expect("F resolves");
            let signatures = state
                .get_signatures_of_type(f, crate::structural::SignatureKind::Call)
                .expect("union call signatures synthesize");
            assert_eq!(signatures.len(), 1, "matching arities combine to one");
            // The composite return is the Subtype-reduced union.
            let w = annotation_of(state, "w");
            let expected = state.get_type_from_type_node(w).expect("w resolves");
            assert_eq!(state.is_type_assignable_to(f, expected), Ok(true));
            assert!(
                state.diagnostics.iter().all(|d| d.file_name.is_none()),
                "{:?}",
                state.diagnostics
            );
        },
    );
}

#[test]
fn union_index_infos_intersect_across_constituents() {
    with_program_state(
        &[(
            "a.ts",
            "interface Array<T> { length: number }\n\
             interface ReadonlyArray<T> { length: number }\n\
             type U = { [k: string]: number } | { [k: string]: string };\n\
             declare var v: U;\ndeclare var w: { [k: string]: number | string };\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let v = annotation_of(state, "v");
            let u = state.get_type_from_type_node(v).expect("U resolves");
            let infos = state
                .get_index_infos_of_type(u)
                .expect("union index infos synthesize");
            assert_eq!(infos.len(), 1);
            let w = annotation_of(state, "w");
            let expected = state.get_type_from_type_node(w).expect("w resolves");
            assert_eq!(state.is_type_assignable_to(u, expected), Ok(true));
            assert!(
                state.diagnostics.iter().all(|d| d.file_name.is_none()),
                "{:?}",
                state.diagnostics
            );
        },
    );
}

#[test]
fn class_instance_members_resolve_with_heritage() {
    with_program_state(
        &[(
            "a.ts",
            "declare class B { b: string }\ndeclare class C extends B { c: number }\n\
             declare var v: C;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let v = annotation_of(state, "v");
            let c = state.get_type_from_type_node(v).expect("C resolves");
            for (name, expected) in [
                ("c", state.tables.intrinsics.number),
                ("b", state.tables.intrinsics.string),
            ] {
                let property = state
                    .get_property_of_type_full(c, name)
                    .expect("class members resolve")
                    .expect("property present");
                let property_type = state.get_type_of_symbol(property).expect("property type");
                assert_eq!(property_type, expected, "{name}");
            }
            assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
        },
    );
}

#[test]
fn generic_class_references_instantiate_members() {
    with_program_state(
        &[(
            "a.ts",
            "declare class Box<T> { value: T }\ndeclare var v: Box<string>;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let v = annotation_of(state, "v");
            let boxed = state.get_type_from_type_node(v).expect("Box<string>");
            let value = state
                .get_property_of_type_full(boxed, "value")
                .expect("members resolve")
                .expect("value property");
            let value_type = state.get_type_of_symbol(value).expect("value type");
            assert_eq!(value_type, state.tables.intrinsics.string);
            assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
        },
    );
}

#[test]
fn accessor_properties_read_getter_and_setter_annotations() {
    with_program_state(
        &[(
            "a.ts",
            "declare class A { get x(): number; set x(value: number); }\n\
             declare var v: A;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let v = annotation_of(state, "v");
            let a = state.get_type_from_type_node(v).expect("A resolves");
            let x = state
                .get_property_of_type_full(a, "x")
                .expect("members resolve")
                .expect("x property");
            let x_type = state.get_type_of_symbol(x).expect("accessor type");
            assert_eq!(x_type, state.tables.intrinsics.number);
            let write_type = state
                .get_write_type_of_accessors(x)
                .expect("setter write type");
            assert_eq!(write_type, state.tables.intrinsics.number);
            assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
        },
    );
}

#[test]
fn own_base_expression_circularity_reports_2506() {
    with_program_state(
        &[("a.ts", "declare class C extends C { }\ndeclare var v: C;\n")],
        &CompilerOptions::default(),
        |state| {
            let v = annotation_of(state, "v");
            let c = state.get_type_from_type_node(v).expect("C resolves");
            let members = state
                .resolve_structured_type_members(c)
                .expect("cycle-cut members resolve");
            assert!(state.members_of(members).properties.is_empty());
            let codes: Vec<u32> = state
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code())
                .collect();
            assert_eq!(codes, [2506], "{:?}", state.diagnostics);
        },
    );
}

#[test]
fn deferred_references_instantiate_through_the_canonical_node_cache() {
    with_program_state(
        &[(
            "a.ts",
            "interface I<T> { a: T }\ntype A<U> = I<U>;\n\
             declare var v: A<string>;\ndeclare var w: A<string>;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let v = annotation_of(state, "v");
            let instance = state
                .get_type_from_type_node(v)
                .expect("alias instantiation over a deferred RHS");
            // getObjectTypeInstantiation minted a fresh deferred
            // reference carrying the U->string mapper.
            assert!(state.links.ty(instance).deferred_node.is_some());
            assert!(state.links.ty(instance).deferred_mapper.is_some());
            let arguments = state.get_type_arguments(instance).expect("forcible");
            assert_eq!(arguments, [state.tables.intrinsics.string]);
            // The canonical node reference hosts the instantiations
            // map: the same argument list reuses the instance.
            let w = annotation_of(state, "w");
            let again = state.get_type_from_type_node(w).expect("cached");
            assert_eq!(again, instance);
            assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
        },
    );
}

#[test]
fn variadic_expansion_pre_forces_deferred_tuple_elements() {
    with_program_state(
        &[(
            "a.ts",
            "type B = [number];\ntype A = [...B, string];\ndeclare var v: A;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let v = annotation_of(state, "v");
            // A has a variadic element, so it resolves EAGERLY; the
            // spread forces B's (deferred) arguments through the
            // pre-force wrapper.
            let resolved = state.get_type_from_type_node(v).expect("variadic expands");
            assert!(state.links.ty(resolved).deferred_node.is_none());
            assert_eq!(
                state.tables.type_arguments(resolved),
                [
                    state.tables.intrinsics.number,
                    state.tables.intrinsics.string
                ]
            );
            assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
        },
    );
}

#[test]
fn generic_reference_relations_flow_through_instantiated_arguments() {
    with_program_state(
        &[(
            "a.ts",
            "interface I<T> { a: T }\ndeclare var v: I<\"x\">;\ndeclare var w: I<string>;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let v = annotation_of(state, "v");
            let narrow = state.get_type_from_type_node(v).expect("I<\"x\">");
            let w = annotation_of(state, "w");
            let wide = state.get_type_from_type_node(w).expect("I<string>");
            assert_ne!(narrow, wide);
            // Reference MEMBERS resolve since 5.3a: the relation
            // flows through the instantiated `a` property.
            assert_eq!(state.is_type_assignable_to(narrow, wide), Ok(true));
            assert_eq!(state.is_type_assignable_to(wide, narrow), Ok(false));
            assert!(state.tables.flags_of(narrow).intersects(TypeFlags::OBJECT));
        },
    );
}
