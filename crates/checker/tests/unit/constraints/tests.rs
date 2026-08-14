use tsc_types::{CompilerOptions, SymbolFlags, TypeFlags};

use crate::state::test_support::with_program_state;
use crate::state::CheckerState;

fn annotation_of_var(state: &CheckerState, name: &str) -> tsc_syntax::NodeId {
    crate::relpin::find_probe_annotation(state.binder.source(0), name).expect("var with annotation")
}

fn declared_type_parameter(state: &mut CheckerState, name: &str) -> tsc_types::TypeId {
    let symbol = state
        .resolve_name(
            Some(state.binder.source(0).root),
            name,
            SymbolFlags::TYPE_PARAMETER,
            None,
            false,
            false,
        )
        .expect("resolve_name")
        .or_else(|| {
            // Type parameters live in their container's scope; walk
            // from the first identifier inside the function body.
            let source = state.binder.source(0);
            let inside = source
                .arena
                .node_ids()
                .find(|&id| {
                    source.arena.node(id).kind == tsc_syntax::SyntaxKind::VariableDeclaration
                })
                .expect("var declaration");
            state
                .resolve_name(
                    Some(inside),
                    name,
                    SymbolFlags::TYPE_PARAMETER,
                    None,
                    false,
                    false,
                )
                .expect("resolve_name")
        })
        .expect("type parameter resolves");
    state.get_declared_type_of_type_parameter(symbol)
}

#[test]
fn intersection_with_covering_constraint_collapses_to_type_parameter() {
    with_program_state(
        &[(
            "a.ts",
            "function f<T extends string>() { var v: T & string; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let annotation = annotation_of_var(state, "v");
            let resolved = state
                .get_type_from_type_node(annotation)
                .expect("intersection resolves");
            let t = declared_type_parameter(state, "T");
            assert_eq!(resolved, t, "T & string collapses to T (step 6)");
        },
    );
}

#[test]
fn declared_type_parameter_identity_survives_candidate_rollback() {
    with_program_state(
        &[("a.ts", "function f<T>() { var v: T; }\n")],
        &CompilerOptions::default(),
        |state| {
            let checkpoint = state.begin_speculation();
            let inside_candidate = declared_type_parameter(state, "T");
            state.rollback_speculation(checkpoint);

            let after_candidate = declared_type_parameter(state, "T");
            assert_eq!(
                after_candidate, inside_candidate,
                "semantic types created with the declared parameter retain its TypeId"
            );
        },
    );
}

#[test]
fn class_interface_identity_survives_candidate_rollback() {
    with_program_state(
        &[(
            "a.ts",
            "interface Box<T> { value: T; }\nvar v: Box<string>;\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let annotation = annotation_of_var(state, "v");
            let checkpoint = state.begin_speculation();
            let inside_candidate = state
                .get_type_from_type_node(annotation)
                .expect("candidate reference resolves");
            state.rollback_speculation(checkpoint);

            let after_candidate = state
                .get_type_from_type_node(annotation)
                .expect("reference resolves after rollback");
            assert_eq!(
                after_candidate, inside_candidate,
                "declaration-owned reference identity must outlive candidate rollback"
            );
            assert_eq!(
                state.tables.reference_target(after_candidate),
                state.tables.reference_target(inside_candidate),
                "the class/interface target cannot be reminted after rollback"
            );
        },
    );
}

#[test]
fn intersection_with_disjoint_primitive_collapses_to_never() {
    with_program_state(
        &[(
            "a.ts",
            "function f<T extends string>() { var v: T & number; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let annotation = annotation_of_var(state, "v");
            let resolved = state
                .get_type_from_type_node(annotation)
                .expect("intersection resolves");
            assert_eq!(resolved, state.tables.intrinsics.never);
        },
    );
}

#[test]
fn union_of_constrained_intersections_collapses_to_type_parameter() {
    with_program_state(
        &[(
            "a.ts",
            "function f<T extends \"a\" | \"b\">() { var v: (T & \"a\") | (T & \"b\"); }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let annotation = annotation_of_var(state, "v");
            let resolved = state
                .get_type_from_type_node(annotation)
                .expect("union resolves");
            let t = declared_type_parameter(state, "T");
            assert_eq!(
                resolved, t,
                "removeConstrainedTypeVariables collapses the union to T"
            );
        },
    );
}

#[test]
fn circular_constraint_reports_2313_and_disables_collapse() {
    with_program_state(
        &[("a.ts", "function f<T extends T>() { var v: T & string; }\n")],
        &CompilerOptions::default(),
        |state| {
            let annotation = annotation_of_var(state, "v");
            let resolved = state
                .get_type_from_type_node(annotation)
                .expect("intersection resolves without collapse");
            // No collapse: the intersection interns as-is.
            assert!(state
                .tables
                .flags_of(resolved)
                .intersects(TypeFlags::INTERSECTION));
            let codes: Vec<u32> = state.diagnostics.iter().map(|d| d.code()).collect();
            assert_eq!(codes, [2313]);
            assert!(
                state.diagnostics[0].related.is_empty(),
                "a direct constraint query has no independent currentNode origin"
            );
            let t = declared_type_parameter(state, "T");
            let constraint = state
                .get_constraint_of_type_parameter(t)
                .expect("constraint query in slice");
            assert_eq!(constraint, None, "circular constraint yields none");
        },
    );
}

#[test]
fn circular_constraint_reports_the_independent_driver_origin() {
    let text = "function f<T extends T>() { var v: T & string; }\nconst origin = 0;\n";
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        let annotation = annotation_of_var(state, "v");
        let source = state.binder.source(0);
        let origin = source
            .arena
            .node_ids()
            .find(|&node| {
                matches!(
                    &source.arena.node(node).data,
                    tsc_syntax::NodeData::Identifier(data) if data.text == "origin"
                )
            })
            .expect("origin identifier");
        state.current_node = Some(origin);
        state
            .get_type_from_type_node(annotation)
            .expect("circular constraint resolves to its sentinel");
        state.current_node = None;
        let diagnostic = state
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code() == 2313)
            .expect("circular mapped constraint");
        assert_eq!(diagnostic.related.len(), 1);
        let related = &diagnostic.related[0];
        assert_eq!(related.message.code, 2751);
        assert_eq!(
            related.message.text,
            "Circularity originates in type at this location."
        );
        assert_eq!(
            (related.start, related.length),
            (
                Some(text.find("origin").expect("origin span") as u32),
                Some("origin".len() as u32)
            )
        );
        assert!(related.message.next.is_empty());
    });
}

#[test]
fn unconstrained_type_parameter_intersections_intern_plainly() {
    with_program_state(
        &[("a.ts", "function f<T>() { var v: T & string; }\n")],
        &CompilerOptions::default(),
        |state| {
            let annotation = annotation_of_var(state, "v");
            let resolved = state
                .get_type_from_type_node(annotation)
                .expect("intersection resolves");
            assert!(state
                .tables
                .flags_of(resolved)
                .intersects(TypeFlags::INTERSECTION));
            assert!(state.diagnostics.is_empty(), "{:?}", state.diagnostics);
        },
    );
}
