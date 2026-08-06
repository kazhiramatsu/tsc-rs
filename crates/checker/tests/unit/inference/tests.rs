use tsc_types::{
    CompilerOptions, ContextFlags, ElementFlags, IndexFlags, InferenceFlags, InferencePriority,
    ObjectFlags, PseudoBigInt, SymbolFlags, TypeData, TypeFlags, TypeId, UnionReduction,
};

use super::CompareTypesFn;
use crate::instantiate::{DeferredMapperTargets, TypeMapper};
use crate::state::test_support::with_program_state;
use crate::state::CheckerState;

fn declared_type_parameter(state: &mut CheckerState, name: &str) -> TypeId {
    let source = state.binder.source(0);
    let inside = source
        .arena
        .node_ids()
        .find(|&id| source.arena.node(id).kind == tsc_syntax::SyntaxKind::VariableDeclaration)
        .expect("var declaration");
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

fn node_of_kind(state: &CheckerState, kind: tsc_syntax::SyntaxKind) -> tsc_syntax::NodeId {
    let source = state.binder.source(0);
    source
        .arena
        .node_ids()
        .find(|&id| source.arena.node(id).kind == kind)
        .expect("node of kind")
}

/// M6 7.5d: the RelationFrame slot invariant — a RelationFrame
/// context whose clamp fires OUTSIDE any parked loan is a
/// programmer error (the B8 arm parks the loan around iSICO;
/// nothing else may mint such a context).
#[test]
#[should_panic(expected = "RelationFrame compare_types consumed without a parked frame loan")]
fn relation_frame_clamp_without_parked_loan_panics() {
    with_program_state(
        &[("a.ts", "declare function f<T extends string>(x: T): T;\n")],
        &CompilerOptions::default(),
        |state| {
            let symbol = state
                .resolve_file_scope_name("f", SymbolFlags::FUNCTION)
                .expect("f resolves");
            let ty = state.get_type_of_symbol(symbol).expect("f types");
            let signature = state
                .get_signatures_of_type(ty, crate::state::SignatureKind::Call)
                .expect("f has call signatures")[0];
            let type_parameters = state
                .signature_of(signature)
                .type_parameters
                .clone()
                .expect("f is generic");
            let context = state.create_inference_context(
                &type_parameters,
                Some(signature),
                InferenceFlags::NONE,
                Some(CompareTypesFn::RelationFrame),
            );
            let info = state.inference_context(context).inferences[0];
            let number = state.tables.intrinsics.number;
            state.inference_info_mut(info).candidates = Some(vec![number]);
            // T's constraint clamp (number vs string) must reach
            // the RelationFrame dispatch and hit the invariant.
            let _ = state.get_inferred_type(context, 0);
        },
    );
}

fn annotation_of_var(state: &CheckerState, name: &str) -> tsc_syntax::NodeId {
    crate::relpin::find_probe_annotation(state.binder.source(0), name).expect("var with annotation")
}

/// The info behind LIVE slot `index` of `ctx` (tsc
/// `context.inferences[index]`).
fn slot<'x>(
    state: &'x CheckerState,
    ctx: super::InferenceContextId,
    index: usize,
) -> &'x super::InferenceInfo {
    state.inference_info(state.inference_context(ctx).inferences[index])
}

fn slot_mut<'x>(
    state: &'x mut CheckerState<'_>,
    ctx: super::InferenceContextId,
    index: usize,
) -> &'x mut super::InferenceInfo {
    let id = state.inference_context(ctx).inferences[index];
    state.inference_info_mut(id)
}

const GENERIC_SRC: &str = "function f<T, U>() { var v: T; }\n";

#[test]
fn create_inference_context_initial_shape() {
    with_program_state(
        &[("a.ts", GENERIC_SRC)],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let u = declared_type_parameter(state, "U");
            let ctx =
                state.create_inference_context(&[t, u], None, InferenceFlags::NO_DEFAULT, None);
            let context = state.inference_context(ctx);
            assert_eq!(context.inferences.len(), 2);
            // 68254-68255: the mapper pair's creation capture
            // mirrors the slots and their type parameters.
            assert_eq!(context.mapper_infos, context.inferences);
            assert_eq!(context.mapper_sources, vec![t, u]);
            assert_eq!(context.flags.bits(), InferenceFlags::NO_DEFAULT.bits());
            assert_eq!(context.compare_types, CompareTypesFn::Assignable);
            assert!(context.signature.is_none());
            assert!(context.return_mapper.is_none());
            assert!(context.inferred_type_parameters.is_none());
            assert!(context.intra_expression_inference_sites.is_none());
            assert!(context.outer_return_mapper.is_none());
            let mapper = context.mapper;
            let non_fixing = context.non_fixing_mapper;
            for (index, tp) in [t, u].into_iter().enumerate() {
                let info = slot(state, ctx, index);
                assert_eq!(info.type_parameter, tp);
                assert!(info.candidates.is_none());
                assert!(info.contra_candidates.is_none());
                assert!(info.inferred_type.is_none());
                assert!(info.priority.is_none());
                assert!(info.top_level, "createInferenceInfo topLevel: true (68307)");
                assert!(!info.is_fixed, "createInferenceInfo isFixed: false (68308)");
                assert!(info.implied_arity.is_none());
            }
            // 68254-68255: the pair is Deferred over THIS context,
            // fixing first.
            match state.mapper(mapper) {
                TypeMapper::Deferred(DeferredMapperTargets::InferenceFixing(id)) => {
                    assert_eq!(*id, ctx)
                }
                other => panic!("fixing mapper shape: {other:?}"),
            }
            match state.mapper(non_fixing) {
                TypeMapper::Deferred(DeferredMapperTargets::InferenceNonFixing(id)) => {
                    assert_eq!(*id, ctx)
                }
                other => panic!("non-fixing mapper shape: {other:?}"),
            }
        },
    );
}

#[test]
fn clone_inference_context_deep_copies_infos_only() {
    with_program_state(
        &[("a.ts", GENERIC_SRC)],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let u = declared_type_parameter(state, "U");
            let string = state.tables.intrinsics.string;
            let number = state.tables.intrinsics.number;
            let ctx =
                state.create_inference_context(&[t, u], None, InferenceFlags::NO_DEFAULT, None);
            let var_decl = node_of_kind(state, tsc_syntax::SyntaxKind::VariableDeclaration);
            slot_mut(state, ctx, 0).candidates = Some(vec![string]);
            state.inference_context_mut(ctx).return_mapper =
                Some(state.make_unary_type_mapper(t, string));
            state.add_intra_expression_inference_site(ctx, var_decl, string);
            // None passes through (68242 `context && ...`).
            assert!(state
                .clone_inference_context(None, InferenceFlags::NONE)
                .is_none());
            let clone = state
                .clone_inference_context(Some(ctx), InferenceFlags::SKIPPED_GENERIC_FUNCTION)
                .expect("Some in, Some out");
            let cloned = state.inference_context(clone);
            // extraFlags OR onto the original's flags (68242).
            assert_eq!(
                cloned.flags.bits(),
                (InferenceFlags::NO_DEFAULT | InferenceFlags::SKIPPED_GENERIC_FUNCTION).bits()
            );
            // cloneInferenceContext clones the INFOS (fresh
            // objects: distinct ids from the original's slots);
            // lazily-attached context fields do not survive.
            assert_ne!(
                cloned.inferences[0],
                state.inference_context(ctx).inferences[0]
            );
            assert!(cloned.return_mapper.is_none());
            assert!(cloned.intra_expression_inference_sites.is_none());
            assert!(cloned.outer_return_mapper.is_none());
            // Fresh mapper pair over the CLONE.
            let clone_mapper = cloned.mapper;
            assert_eq!(slot(state, clone, 0).candidates, Some(vec![string]));
            match state.mapper(clone_mapper) {
                TypeMapper::Deferred(DeferredMapperTargets::InferenceFixing(id)) => {
                    assert_eq!(*id, clone)
                }
                other => panic!("clone mapper shape: {other:?}"),
            }
            // cloneInferenceInfo slices the candidate arrays: a
            // later push into the original is invisible to the
            // clone (68315 `.slice()`).
            slot_mut(state, ctx, 0)
                .candidates
                .as_mut()
                .expect("candidates present")
                .push(number);
            assert_eq!(
                slot(state, clone, 0)
                    .candidates
                    .as_ref()
                    .expect("cloned candidates")
                    .len(),
                1
            );
        },
    );
}

#[test]
fn clone_inferred_part_filters_to_candidate_rows() {
    with_program_state(
        &[("a.ts", GENERIC_SRC)],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let u = declared_type_parameter(state, "U");
            let string = state.tables.intrinsics.string;
            let ctx = state.create_inference_context(&[t, u], None, InferenceFlags::NONE, None);
            // No candidates anywhere → undefined (68326).
            assert!(state.clone_inferred_part_of_context(ctx).is_none());
            slot_mut(state, ctx, 1).contra_candidates = Some(vec![string]);
            let part = state
                .clone_inferred_part_of_context(ctx)
                .expect("one candidate row");
            assert_eq!(state.inference_context(part).inferences.len(), 1);
            assert_eq!(slot(state, part, 0).type_parameter, u);
            assert_eq!(slot(state, part, 0).contra_candidates, Some(vec![string]));
        },
    );
}

#[test]
fn deferred_dispatch_identity_and_live_resolution() {
    with_program_state(
        &[("a.ts", GENERIC_SRC)],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let string = state.tables.intrinsics.string;
            let unknown = state.tables.intrinsics.unknown;
            let ctx = state.create_inference_context(&[t], None, InferenceFlags::NONE, None);
            let non_fixing = state.inference_context(ctx).non_fixing_mapper;
            // 63348: non-member types map to themselves.
            let mapped = state
                .get_mapped_type(string, non_fixing)
                .expect("identity on non-member");
            assert_eq!(mapped, string);
            // A member dispatches into 7.3 resolution: no
            // signature, no candidates → getTypeFromInference is
            // undefined and the 69296 fold lands unknownType.
            let resolved = state
                .get_mapped_type(t, non_fixing)
                .expect("live resolution");
            assert_eq!(resolved, unknown);
            assert_eq!(slot(state, ctx, 0).inferred_type, Some(unknown));
            // The non-fixing thunk never fixes (68274-68275).
            assert!(!slot(state, ctx, 0).is_fixed);
        },
    );
}

#[test]
fn fixing_dispatch_clears_caches_and_fixes_before_resolution() {
    with_program_state(
        &[("a.ts", GENERIC_SRC)],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let u = declared_type_parameter(state, "U");
            let string = state.tables.intrinsics.string;
            let unknown = state.tables.intrinsics.unknown;
            let ctx = state.create_inference_context(&[t, u], None, InferenceFlags::NONE, None);
            slot_mut(state, ctx, 0).inferred_type = Some(string);
            slot_mut(state, ctx, 1).inferred_type = Some(string);
            let fixing = state.inference_context(ctx).mapper;
            let resolved = state.get_mapped_type(t, fixing).expect("live resolution");
            // 68263-68265 order: clearCachedInferences runs while
            // the row is still unfixed (its own STALE Some(string)
            // cache drops — resolution then re-memoizes unknown),
            // THEN isFixed is set, THEN resolution.
            assert_eq!(resolved, unknown);
            assert!(slot(state, ctx, 0).is_fixed);
            assert_eq!(slot(state, ctx, 0).inferred_type, Some(unknown));
            // Other unfixed rows lose their cache too — and stay
            // unresolved (only slot 0 was dispatched).
            assert!(!slot(state, ctx, 1).is_fixed);
            assert!(slot(state, ctx, 1).inferred_type.is_none());
            // A second dispatch on the SAME (now fixed) row skips
            // the drain/clear preamble entirely (68262 guard) and
            // memo-hits.
            slot_mut(state, ctx, 1).inferred_type = Some(string);
            let resolved_again = state.get_mapped_type(t, fixing).expect("memo hit");
            assert_eq!(resolved_again, unknown);
            assert_eq!(
                slot(state, ctx, 1).inferred_type,
                Some(string),
                "fixed-row dispatch must not re-clear other caches"
            );
        },
    );
}

#[test]
fn fixing_dispatch_consults_creation_capture_after_slot_replacement() {
    // The mergeInferences shape (80836 `target[i] = source[i]`),
    // simulated ahead of its 7.4 port: replace a fixed-but-
    // candidateless LIVE slot with a fresh info. tsc's thunk
    // closes over the CREATION-TIME object (68261-68267), so the
    // second fixing dispatch skips the preamble — the fresh live
    // row must stay unfixed (the 68710 candidate gate reopens)
    // and keep its cache (no clearCachedInferences run).
    with_program_state(
        &[("a.ts", GENERIC_SRC)],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let string = state.tables.intrinsics.string;
            let unknown = state.tables.intrinsics.unknown;
            let ctx = state.create_inference_context(&[t], None, InferenceFlags::NONE, None);
            let fixing = state.inference_context(ctx).mapper;
            let first = state.get_mapped_type(t, fixing).expect("live resolution");
            assert_eq!(first, unknown);
            assert!(slot(state, ctx, 0).is_fixed);
            // 80786: a fresh candidate-bearing info replaces the
            // slot (isFixed starts false).
            let mut fresh = super::create_inference_info(t);
            fresh.candidates = Some(vec![string]);
            fresh.inferred_type = Some(string);
            let fresh_id = state.alloc_inference_info(fresh);
            state.inference_context_mut(ctx).inferences[0] = fresh_id;
            // The thunk-captured info stays fixed → preamble
            // skipped; resolution reads the LIVE slot and
            // memo-hits its Some(string).
            let second = state.get_mapped_type(t, fixing).expect("live-slot memo");
            assert_eq!(second, string);
            assert!(
                !slot(state, ctx, 0).is_fixed,
                "live merged row stays unfixed — tsc's detached capture absorbs the fix"
            );
            assert_eq!(
                slot(state, ctx, 0).inferred_type,
                Some(string),
                "preamble skip must not clear the merged row's cache"
            );
        },
    );
}

#[test]
fn fixing_dispatch_drains_sites_with_no_contextual_type() {
    with_program_state(
        &[("a.ts", "function f<T>() { var w = 1; }\n")],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let string = state.tables.intrinsics.string;
            let literal = node_of_kind(state, tsc_syntax::SyntaxKind::NumericLiteral);
            let ctx = state.create_inference_context(&[t], None, InferenceFlags::NONE, None);
            // Lazy array creation (68287 `??=`).
            state.add_intra_expression_inference_site(ctx, literal, string);
            state.add_intra_expression_inference_site(ctx, literal, string);
            assert_eq!(
                state
                    .inference_context(ctx)
                    .intra_expression_inference_sites
                    .as_ref()
                    .expect("lazily created")
                    .len(),
                2
            );
            let fixing = state.inference_context(ctx).mapper;
            // `var w = 1` has no contextual type at the
            // initializer, so the drain loop completes without
            // touching inferTypes and clears the list (68297),
            // then resolution runs live (no candidates →
            // unknown).
            let resolved = state.get_mapped_type(t, fixing).expect("live resolution");
            assert_eq!(resolved, state.tables.intrinsics.unknown);
            assert!(state
                .inference_context(ctx)
                .intra_expression_inference_sites
                .is_none());
            assert!(slot(state, ctx, 0).is_fixed);
        },
    );
}

#[test]
fn fixing_dispatch_drains_conditional_context_and_fixes() {
    with_program_state(
        &[(
            "a.ts",
            "function f<T>() { var v: T extends string ? string : number = 1; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let string = state.tables.intrinsics.string;
            let literal = node_of_kind(state, tsc_syntax::SyntaxKind::NumericLiteral);
            let ctx = state.create_inference_context(&[t], None, InferenceFlags::NONE, None);
            state.add_intra_expression_inference_site(ctx, literal, string);
            let fixing = state.inference_context(ctx).mapper;
            // 9.6d: the conditional target is a live inference
            // shape, so the drain completes before fixing T.
            let resolved = state
                .get_mapped_type(t, fixing)
                .expect("conditional inference drains");
            assert_eq!(resolved, state.tables.intrinsics.unknown);
            assert!(state
                .inference_context(ctx)
                .intra_expression_inference_sites
                .is_none());
            assert!(slot(state, ctx, 0).is_fixed);
        },
    );
}

#[test]
fn check_expression_with_contextual_type_clears_undrained_sites() {
    with_program_state(
        &[("a.ts", "function f<T>() { var w = 1; }\n")],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let string = state.tables.intrinsics.string;
            let number = state.tables.intrinsics.number;
            let literal = node_of_kind(state, tsc_syntax::SyntaxKind::NumericLiteral);
            let ctx = state.create_inference_context(&[t], None, InferenceFlags::NONE, None);
            state.add_intra_expression_inference_site(ctx, literal, string);
            // 80566-80569: the sites are DISCARDED (not drained)
            // once the full expression has been checked.
            state
                .check_expression_with_contextual_type(
                    literal,
                    number,
                    Some(ctx),
                    tsc_types::CheckMode::NORMAL,
                )
                .expect("literal checks");
            assert!(state
                .inference_context(ctx)
                .intra_expression_inference_sites
                .is_none());
            assert!(
                !slot(state, ctx, 0).is_fixed,
                "clear is not a drain — nothing fixed"
            );
        },
    );
}

#[test]
fn inferential_annotated_arity_arm_infers_live() {
    with_program_state(
        &[(
            "a.ts",
            "function f<T>() { var target: (a: T, b: string) => void; var g = (x: number) => 1; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let annotation = annotation_of_var(state, "target");
            let contextual = state.get_type_from_type_node(annotation).expect("fn type");
            let arrow = node_of_kind(state, tsc_syntax::SyntaxKind::ArrowFunction);
            let ctx = state.create_inference_context(&[t], None, InferenceFlags::NONE, None);
            // 79184-79187, LIVE since 7.4b (the 7.1-era pin
            // asserted the named escape): non-context-sensitive,
            // no own type parameters, contextual arity 2 > own
            // arity 1 — the Inferential bit now feeds the
            // ANNOTATED parameter types into the inference context
            // (inferFromAnnotatedParametersAndReturn): x's
            // `number` annotation lands as a candidate for T from
            // the contextual `(a: T, ...)`.
            //
            // The sibling context-sensitive arm is unreachable for
            // a fully annotated arrow — pin the arm selection.
            assert!(
                !state.is_context_sensitive(arrow),
                "fully annotated arrow must take the 79184 arity arm"
            );
            state
                .check_expression_with_contextual_type(
                    arrow,
                    contextual,
                    Some(ctx),
                    tsc_types::CheckMode::NORMAL,
                )
                .expect("live inference completes");
            let number = state.tables.intrinsics.number;
            let slot = state.inference_context(ctx).inferences[0];
            assert_eq!(
                state.inference_info(slot).candidates.as_deref(),
                Some(&[number][..]),
                "annotated parameter inference records number for T"
            );
        },
    );
}

#[test]
fn outer_return_mapper_merges_and_caches() {
    with_program_state(
        &[("a.ts", GENERIC_SRC)],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let string = state.tables.intrinsics.string;
            let ctx = state.create_inference_context(&[t], None, InferenceFlags::NONE, None);
            // returnMapper None: mergeTypeMappers(undefined, m2)
            // = m2 — the clone's fixing mapper alone.
            let outer = state.create_outer_return_mapper(ctx);
            match state.mapper(outer) {
                TypeMapper::Deferred(DeferredMapperTargets::InferenceFixing(clone_id)) => {
                    assert_ne!(*clone_id, ctx, "mapper belongs to the CLONE")
                }
                other => panic!("outer mapper shape: {other:?}"),
            }
            // 63386 `??=`: the second call is a cache hit — same
            // mapper, no new context cloned.
            let arena_len = state.inference_context_arena.len();
            let again = state.create_outer_return_mapper(ctx);
            assert_eq!(again, outer);
            assert_eq!(state.inference_context_arena.len(), arena_len);
            // With a returnMapper present the pair merges.
            let ctx2 = state.create_inference_context(&[t], None, InferenceFlags::NONE, None);
            let ret = state.make_unary_type_mapper(t, string);
            state.inference_context_mut(ctx2).return_mapper = Some(ret);
            let merged = state.create_outer_return_mapper(ctx2);
            match state.mapper(merged) {
                TypeMapper::Merged { mapper1, mapper2 } => {
                    assert_eq!(*mapper1, ret);
                    match state.mapper(*mapper2) {
                        TypeMapper::Deferred(DeferredMapperTargets::InferenceFixing(id)) => {
                            assert_ne!(*id, ctx2)
                        }
                        other => panic!("merged rhs shape: {other:?}"),
                    }
                }
                other => panic!("merged mapper shape: {other:?}"),
            }
        },
    );
}

#[test]
fn get_mapper_from_context_reads_fixing_mapper() {
    with_program_state(
        &[("a.ts", GENERIC_SRC)],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            assert!(state.get_mapper_from_context(None).is_none());
            let ctx = state.create_inference_context(&[t], None, InferenceFlags::NONE, None);
            assert_eq!(
                state.get_mapper_from_context(Some(ctx)),
                Some(state.inference_context(ctx).mapper)
            );
        },
    );
}

#[test]
fn instantiate_contextual_type_return_mapper_branch() {
    with_program_state(
        &[("a.ts", GENERIC_SRC)],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let string = state.tables.intrinsics.string;
            let any = state.tables.intrinsics.any;
            let false_regular = state.tables.intrinsics.false_regular;
            let true_regular = state.tables.intrinsics.true_regular;
            let var_decl = node_of_kind(state, tsc_syntax::SyntaxKind::VariableDeclaration);
            // returnMapper maps T to a union carrying BOTH regular
            // boolean literals — 73453-73454 filters them out.
            let union = state
                .get_union_type_ex(&[false_regular, true_regular, string], UnionReduction::None)
                .expect("union");
            let ctx = state.create_inference_context(&[t], None, InferenceFlags::NONE, None);
            let ret = state.make_unary_type_mapper(t, union);
            state.inference_context_mut(ctx).return_mapper = Some(ret);
            state.push_inference_context(var_decl, Some(ctx));
            let out = state
                .instantiate_contextual_type(Some(t), var_decl, ContextFlags::NONE)
                .expect("instantiates");
            assert_eq!(out, Some(string));
            // An AnyOrUnknown instantiation falls through to the
            // identity read (73447 guard).
            let ctx_any = state.create_inference_context(&[t], None, InferenceFlags::NONE, None);
            let ret_any = state.make_unary_type_mapper(t, any);
            state.inference_context_mut(ctx_any).return_mapper = Some(ret_any);
            state.pop_inference_context();
            state.push_inference_context(var_decl, Some(ctx_any));
            let out = state
                .instantiate_contextual_type(Some(t), var_decl, ContextFlags::NONE)
                .expect("falls through");
            assert_eq!(out, Some(t));
            state.pop_inference_context();
        },
    );
}

#[test]
fn instantiate_contextual_type_signature_branch_consults_non_fixing_mapper() {
    with_program_state(
        &[("a.ts", GENERIC_SRC)],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let string = state.tables.intrinsics.string;
            let var_decl = node_of_kind(state, tsc_syntax::SyntaxKind::VariableDeclaration);
            let ctx = state.create_inference_context(&[t], None, InferenceFlags::NONE, None);
            slot_mut(state, ctx, 0).candidates = Some(vec![string]);
            state.push_inference_context(var_decl, Some(ctx));
            // 73444-73445: Signature flags + a candidate-bearing
            // row instantiate through the NON-fixing mapper —
            // resolution unions the candidates (getTypeFromInference,
            // no context signature) without fixing the row.
            let out = state
                .instantiate_contextual_type(Some(t), var_decl, ContextFlags::SIGNATURE)
                .expect("resolves through the non-fixing mapper");
            assert_eq!(out, Some(string));
            assert!(
                !slot(state, ctx, 0).is_fixed,
                "the Signature branch must NOT fix"
            );
            state.pop_inference_context();
        },
    );
}

#[test]
fn context_arena_survives_rollback_while_stack_truncates() {
    with_program_state(
        &[("a.ts", GENERIC_SRC)],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let var_decl = node_of_kind(state, tsc_syntax::SyntaxKind::VariableDeclaration);
            let checkpoint = state.begin_speculation();
            let ctx = state.create_inference_context(&[t], None, InferenceFlags::NONE, None);
            state.push_inference_context(var_decl, Some(ctx));
            state.rollback_speculation(checkpoint);
            // The node stack is A-class (truncated to the mark);
            // the arena is E-class — the context object survives
            // exactly like tsc's GC object would (chooseOverload
            // 76842 depends on trial-surviving context state).
            assert!(state.inference_contexts.is_empty());
            assert_eq!(state.inference_context_arena.len(), 1);
            assert_eq!(state.inference_context(ctx).inferences.len(), 1);
        },
    );
}

// ---- 7.3: getInferredType resolution + constraint clamp ----

/// The declared function's call signature — the realistic
/// `context.signature` for resolution tests (return-type position
/// and constraints come from the declaration).
fn call_signature_of(state: &mut CheckerState, name: &str) -> crate::state::SignatureId {
    let inside = node_of_kind(state, tsc_syntax::SyntaxKind::VariableDeclaration);
    let symbol = state
        .resolve_name(Some(inside), name, SymbolFlags::VALUE, None, false, false)
        .expect("resolve_name")
        .expect("function resolves");
    let ty = state.get_type_of_symbol(symbol).expect("function type");
    state
        .get_signatures_of_type(ty, crate::state::SignatureKind::Call)
        .expect("signatures")[0]
}

fn fresh_string_literal(state: &mut CheckerState, value: &str) -> TypeId {
    let regular = state.tables.get_string_literal_type(value);
    state.tables.get_fresh_type_of_literal_type(regular)
}

#[test]
fn resolution_keeps_literal_when_parameter_tops_return_type() {
    with_program_state(
        &[("a.ts", "function fr<T>(x: T): T { var v = 1; return x; }\n")],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let signature = call_signature_of(state, "fr");
            let fresh_a = fresh_string_literal(state, "a");
            let ctx =
                state.create_inference_context(&[t], Some(signature), InferenceFlags::NONE, None);
            slot_mut(state, ctx, 0).candidates = Some(vec![fresh_a]);
            // 69265: topLevel + unfixed + T at top level of the
            // return type → widenLiteralTypes stays false; the
            // fresh literal survives (getWidenedType widens object
            // literals, not fresh primitives).
            let resolved = state.get_inferred_type(ctx, 0).expect("resolves");
            assert_eq!(resolved, fresh_a);
        },
    );
}

#[test]
fn resolution_widens_literal_off_return_position() {
    with_program_state(
        &[("a.ts", "function fv<T>(x: T): void { var v = 1; }\n")],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let signature = call_signature_of(state, "fv");
            let fresh_a = fresh_string_literal(state, "a");
            let string = state.tables.intrinsics.string;
            let ctx =
                state.create_inference_context(&[t], Some(signature), InferenceFlags::NONE, None);
            slot_mut(state, ctx, 0).candidates = Some(vec![fresh_a]);
            let resolved = state.get_inferred_type(ctx, 0).expect("resolves");
            assert_eq!(resolved, string);
        },
    );
}

#[test]
fn resolution_fixed_row_widens_even_in_return_position() {
    with_program_state(
        &[("a.ts", "function fr<T>(x: T): T { var v = 1; return x; }\n")],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let signature = call_signature_of(state, "fr");
            let fresh_a = fresh_string_literal(state, "a");
            let string = state.tables.intrinsics.string;
            let ctx =
                state.create_inference_context(&[t], Some(signature), InferenceFlags::NONE, None);
            slot_mut(state, ctx, 0).candidates = Some(vec![fresh_a]);
            slot_mut(state, ctx, 0).is_fixed = true;
            // 69265: isFixed short-circuits the return-type probe.
            let resolved = state.get_inferred_type(ctx, 0).expect("resolves");
            assert_eq!(resolved, string);
        },
    );
}

#[test]
fn resolution_primitive_constraint_keeps_regular_literal() {
    with_program_state(
        &[(
            "a.ts",
            "function fc<T extends string>(x: T): T { var v = 1; return x; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let signature = call_signature_of(state, "fc");
            let regular_a = state.tables.get_string_literal_type("a");
            let fresh_a = state.tables.get_fresh_type_of_literal_type(regular_a);
            let ctx =
                state.create_inference_context(&[t], Some(signature), InferenceFlags::NONE, None);
            slot_mut(state, ctx, 0).candidates = Some(vec![fresh_a]);
            // 69266: a primitive constraint maps candidates to
            // their REGULAR form instead of widening — the literal
            // survives at its regular identity even though T also
            // tops the return type (primitiveConstraint wins the
            // split before widenLiteralTypes is consulted).
            let resolved = state.get_inferred_type(ctx, 0).expect("resolves");
            assert_eq!(resolved, regular_a);
            assert_ne!(resolved, fresh_a);
        },
    );
}

#[test]
fn resolution_no_default_flag_yields_silent_never() {
    with_program_state(
        &[("a.ts", "function fr<T>(x: T): T { var v = 1; return x; }\n")],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let signature = call_signature_of(state, "fr");
            let ctx = state.create_inference_context(
                &[t],
                Some(signature),
                InferenceFlags::NO_DEFAULT,
                None,
            );
            // 69285: no candidates + NoDefault → silentNeverType
            // (NonInferrableType-flagged so it can never be
            // recorded as a candidate later).
            let resolved = state.get_inferred_type(ctx, 0).expect("resolves");
            assert_eq!(resolved, state.tables.intrinsics.silent_never);
        },
    );
}

#[test]
fn resolution_default_pulls_earlier_slot_through_non_fixing_mapper() {
    with_program_state(
        &[(
            "a.ts",
            "function fd<T, U = T>(x: T, y?: U): void { var v = 1; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let u = declared_type_parameter(state, "U");
            let signature = call_signature_of(state, "fd");
            let fresh_a = fresh_string_literal(state, "a");
            let string = state.tables.intrinsics.string;
            let ctx = state.create_inference_context(
                &[t, u],
                Some(signature),
                InferenceFlags::NONE,
                None,
            );
            slot_mut(state, ctx, 0).candidates = Some(vec![fresh_a]);
            // 69289: U's default (T) instantiates under
            // backreference+nonFixing — T is BEFORE the resolving
            // index, so it routes through the non-fixing mapper
            // and resolves for real (widened off return position).
            let resolved_u = state.get_inferred_type(ctx, 1).expect("resolves");
            assert_eq!(resolved_u, string);
            assert_eq!(slot(state, ctx, 0).inferred_type, Some(string));
            assert!(!slot(state, ctx, 0).is_fixed, "non-fixing route");
        },
    );
}

#[test]
fn resolution_forward_default_collapses_to_unknown_via_backreference() {
    with_program_state(
        &[(
            "a.ts",
            "function fe<T = U, U> (x: U): void { var v = 1; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let u = declared_type_parameter(state, "U");
            let signature = call_signature_of(state, "fe");
            let unknown = state.tables.intrinsics.unknown;
            let ctx = state.create_inference_context(
                &[t, u],
                Some(signature),
                InferenceFlags::NONE,
                None,
            );
            // 63381: the backreference mapper covers every slot AT
            // or AFTER the resolving index — the forward reference
            // U inside T's default collapses to unknown and the
            // non-fixing mapper is never consulted for U.
            let resolved_t = state.get_inferred_type(ctx, 0).expect("resolves");
            assert_eq!(resolved_t, unknown);
            assert!(
                slot(state, ctx, 1).inferred_type.is_none(),
                "backreference must shadow the non-fixing mapper for forward slots"
            );
        },
    );
}

#[test]
fn resolution_clamp_filters_return_type_priority_to_compatible_part() {
    with_program_state(
        &[(
            "a.ts",
            "function ff<T extends \"a\" | \"b\">(x: T): T { var v = 1; return x; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let signature = call_signature_of(state, "ff");
            let regular_a = state.tables.get_string_literal_type("a");
            let number = state.tables.intrinsics.number;
            let violating = state
                .get_union_type_ex(&[regular_a, number], UnionReduction::Literal)
                .expect("union");
            let ctx =
                state.create_inference_context(&[t], Some(signature), InferenceFlags::NONE, None);
            slot_mut(state, ctx, 0).candidates = Some(vec![violating]);
            slot_mut(state, ctx, 0).priority = Some(InferencePriority::RETURN_TYPE);
            // 69302: ReturnType-priority (EQUALITY, not mask)
            // violations FILTER the inference to the part
            // compatible with the instantiated constraint.
            let resolved = state.get_inferred_type(ctx, 0).expect("resolves");
            assert_eq!(resolved, regular_a);
        },
    );
}

#[test]
fn resolution_clamp_non_return_priority_replaces_with_constraint() {
    with_program_state(
        &[(
            "a.ts",
            "function ff<T extends \"a\" | \"b\">(x: T): T { var v = 1; return x; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let signature = call_signature_of(state, "ff");
            let regular_a = state.tables.get_string_literal_type("a");
            let number = state.tables.intrinsics.number;
            let violating = state
                .get_union_type_ex(&[regular_a, number], UnionReduction::Literal)
                .expect("union");
            let constraint = state
                .get_constraint_of_type_parameter(t)
                .expect("constraint lookup")
                .expect("declared constraint");
            let ctx =
                state.create_inference_context(&[t], Some(signature), InferenceFlags::NONE, None);
            slot_mut(state, ctx, 0).candidates = Some(vec![violating]);
            // priority None: the filter arm is neverType, no
            // fallback exists → the instantiated constraint wins
            // (69303-69307).
            let resolved = state.get_inferred_type(ctx, 0).expect("resolves");
            assert_eq!(resolved, constraint);
        },
    );
}

#[test]
fn resolution_clamp_falls_back_to_contravariant_inference() {
    with_program_state(
        &[(
            "a.ts",
            "function fs<T extends string>(x: T): void { var v = 1; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let signature = call_signature_of(state, "fs");
            let string = state.tables.intrinsics.string;
            let number = state.tables.intrinsics.number;
            let string_or_number = state
                .get_union_type_ex(&[string, number], UnionReduction::Literal)
                .expect("union");
            let ctx =
                state.create_inference_context(&[t], Some(signature), InferenceFlags::NONE, None);
            // Covariant string|number is preferred (assignable to
            // the string|number contra candidate; sibling clause
            // vacuous) but violates the string constraint; the
            // never-filtered result falls back to the
            // CONTRAVARIANT inference — commonSubtype([string,
            // string|number]) = string — which satisfies it
            // (69306).
            slot_mut(state, ctx, 0).candidates = Some(vec![string_or_number]);
            slot_mut(state, ctx, 0).contra_candidates = Some(vec![string, string_or_number]);
            let resolved = state.get_inferred_type(ctx, 0).expect("resolves");
            assert_eq!(resolved, string);
        },
    );
}

#[test]
fn resolution_prefer_covariant_vetoed_by_constrained_sibling_candidates() {
    with_program_state(
        &[(
            "a.ts",
            "function fg<T, U extends T>(x: T, y: U): void { var v = 1; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let u = declared_type_parameter(state, "U");
            let string = state.tables.intrinsics.string;
            let number = state.tables.intrinsics.number;
            let unknown = state.tables.intrinsics.unknown;
            let signature = call_signature_of(state, "fg");
            // Veto: U is constrained to T and carries a candidate
            // NOT assignable to T's covariant inference — the
            // 69281 every-clause rejects covariant preference and
            // the contravariant inference wins.
            let ctx = state.create_inference_context(
                &[t, u],
                Some(signature),
                InferenceFlags::NONE,
                None,
            );
            slot_mut(state, ctx, 0).candidates = Some(vec![string]);
            slot_mut(state, ctx, 0).contra_candidates = Some(vec![unknown]);
            slot_mut(state, ctx, 1).candidates = Some(vec![number]);
            let vetoed = state.get_inferred_type(ctx, 0).expect("resolves");
            assert_eq!(vetoed, unknown);
            // Control: same shape, no sibling candidates → the
            // every-clause is vacuous and covariant wins.
            let ctx2 = state.create_inference_context(
                &[t, u],
                Some(signature),
                InferenceFlags::NONE,
                None,
            );
            slot_mut(state, ctx2, 0).candidates = Some(vec![string]);
            slot_mut(state, ctx2, 0).contra_candidates = Some(vec![unknown]);
            let preferred = state.get_inferred_type(ctx2, 0).expect("resolves");
            assert_eq!(preferred, string);
        },
    );
}

#[test]
fn resolution_without_signature_intersects_contra_candidates() {
    with_program_state(
        &[("a.ts", GENERIC_SRC)],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let string = state.tables.intrinsics.string;
            let ctx = state.create_inference_context(&[t], None, InferenceFlags::NONE, None);
            slot_mut(state, ctx, 0).contra_candidates = Some(vec![string]);
            // 68507: no context signature → getTypeFromInference's
            // contra arm intersects.
            let resolved = state.get_inferred_type(ctx, 0).expect("resolves");
            assert_eq!(resolved, string);
        },
    );
}

#[test]
fn get_inferred_types_resolves_slots_in_order() {
    with_program_state(
        &[(
            "a.ts",
            "function fd<T, U = T>(x: T, y?: U): void { var v = 1; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let u = declared_type_parameter(state, "U");
            let signature = call_signature_of(state, "fd");
            let fresh_a = fresh_string_literal(state, "a");
            let string = state.tables.intrinsics.string;
            let ctx = state.create_inference_context(
                &[t, u],
                Some(signature),
                InferenceFlags::NONE,
                None,
            );
            slot_mut(state, ctx, 0).candidates = Some(vec![fresh_a]);
            let resolved = state.get_inferred_types(ctx).expect("resolves");
            assert_eq!(resolved, vec![string, string]);
        },
    );
}

#[test]
fn get_common_subtype_reduces_left_keeping_deepest() {
    with_program_state(
        &[("a.ts", GENERIC_SRC)],
        &CompilerOptions::default(),
        |state| {
            let string = state.tables.intrinsics.string;
            let number = state.tables.intrinsics.number;
            let string_or_number = state
                .get_union_type_ex(&[string, number], UnionReduction::Literal)
                .expect("union");
            let common = state
                .get_common_subtype(&[string_or_number, string])
                .expect("common subtype");
            assert_eq!(common, string);
            // Ties keep the EARLIER element (strict `?:` — the
            // later one wins only when it IS a subtype).
            let common_rev = state
                .get_common_subtype(&[string, string_or_number])
                .expect("common subtype");
            assert_eq!(common_rev, string);
        },
    );
}

#[test]
fn resolution_clears_active_mapper_caches_on_every_miss() {
    with_program_state(
        &[("a.ts", GENERIC_SRC)],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let any = state.tables.intrinsics.any;
            let string = state.tables.intrinsics.string;
            let mapper = state.make_unary_type_mapper(t, any);
            // Simulate an in-flight instantiation frame (73607):
            // one active mapper with a warm cache row.
            state.active_type_mappers.push(mapper);
            state
                .active_type_mappers_caches
                .push(std::collections::HashMap::new());
            state.active_type_mappers_caches[0].insert("probe".to_string(), string);
            let ctx = state.create_inference_context(&[t], None, InferenceFlags::NONE, None);
            let _ = state.get_inferred_type(ctx, 0).expect("resolves");
            // 69310: a fresh resolution invalidates every level of
            // the active-mapper cache stack (depth preserved).
            assert_eq!(state.active_type_mappers.len(), 1);
            assert!(state.active_type_mappers_caches[0].is_empty());
            // A memo HIT does not re-clear (the 69272 early
            // return).
            state.active_type_mappers_caches[0].insert("probe".to_string(), string);
            let _ = state.get_inferred_type(ctx, 0).expect("memo");
            assert_eq!(state.active_type_mappers_caches[0].len(), 1);
            state.active_type_mappers.pop();
            state.active_type_mappers_caches.pop();
        },
    );
}

// ---- 7.2a: inferTypes / inferFromTypes spine ----

/// A detached single-info array — the inferReverseMappedTypeWorker
/// 68438 `inferTypes([inference], ...)` seam shape.
fn detached_info(state: &mut CheckerState, tp: TypeId) -> super::InferenceInfoId {
    let info = super::create_inference_info(tp);
    state.alloc_inference_info(info)
}

#[test]
fn infer_types_records_covariant_candidate() {
    with_program_state(
        &[("a.ts", GENERIC_SRC)],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let string = state.tables.intrinsics.string;
            let info = detached_info(state, t);
            state
                .infer_types(&[info], string, t, InferencePriority::NONE, false)
                .expect("live spine");
            let info = state.inference_info(info);
            assert_eq!(info.candidates.as_deref(), Some(&[string][..]));
            assert!(info.contra_candidates.is_none());
            assert_eq!(info.priority, Some(InferencePriority::NONE));
            assert!(info.top_level, "T at top level of T (68732)");
            assert!(info.inferred_type.is_none());
        },
    );
}

#[test]
fn infer_types_contravariant_entry_records_contra_candidate() {
    with_program_state(
        &[("a.ts", GENERIC_SRC)],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let string = state.tables.intrinsics.string;
            let info = detached_info(state, t);
            // 68714: contravariant && !bivariant → contra side.
            state
                .infer_types(&[info], string, t, InferencePriority::NONE, true)
                .expect("live spine");
            let info = state.inference_info(info);
            assert_eq!(info.contra_candidates.as_deref(), Some(&[string][..]));
            assert!(info.candidates.is_none());
        },
    );
}

#[test]
fn equal_priority_candidates_append_unique_in_insertion_order() {
    with_program_state(
        &[("a.ts", GENERIC_SRC)],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let string = state.tables.intrinsics.string;
            let number = state.tables.intrinsics.number;
            let info = detached_info(state, t);
            for source in [string, string, number] {
                state
                    .infer_types(&[info], source, t, InferencePriority::NONE, false)
                    .expect("live spine");
            }
            // 68727 `!contains(...)` + append: unique, in order.
            assert_eq!(
                state.inference_info(info).candidates.as_deref(),
                Some(&[string, number][..])
            );
        },
    );
}

#[test]
fn lower_priority_resets_and_higher_priority_is_ignored() {
    with_program_state(
        &[("a.ts", GENERIC_SRC)],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let string = state.tables.intrinsics.string;
            let number = state.tables.intrinsics.number;
            let boolean = state.tables.intrinsics.boolean;
            let info = detached_info(state, t);
            state
                .infer_types(&[info], string, t, InferencePriority::RETURN_TYPE, false)
                .expect("live spine");
            assert_eq!(
                state.inference_info(info).priority,
                Some(InferencePriority::RETURN_TYPE)
            );
            // 68715: numerically-lower priority wipes the record.
            state
                .infer_types(&[info], number, t, InferencePriority::NONE, false)
                .expect("live spine");
            assert_eq!(
                state.inference_info(info).candidates.as_deref(),
                Some(&[number][..])
            );
            assert_eq!(
                state.inference_info(info).priority,
                Some(InferencePriority::NONE)
            );
            // 68721: a higher priority neither resets nor appends.
            state
                .infer_types(&[info], boolean, t, InferencePriority::RETURN_TYPE, false)
                .expect("live spine");
            assert_eq!(
                state.inference_info(info).candidates.as_deref(),
                Some(&[number][..])
            );
            assert_eq!(
                state.inference_info(info).priority,
                Some(InferencePriority::NONE)
            );
        },
    );
}

#[test]
fn fixed_info_skips_recording() {
    with_program_state(
        &[("a.ts", GENERIC_SRC)],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let string = state.tables.intrinsics.string;
            let info = detached_info(state, t);
            state.inference_info_mut(info).is_fixed = true;
            state
                .infer_types(&[info], string, t, InferencePriority::NONE, false)
                .expect("live spine");
            let info = state.inference_info(info);
            assert!(info.candidates.is_none(), "68710 isFixed gate");
            assert!(info.priority.is_none());
        },
    );
}

#[test]
fn non_inferrable_any_source_skips_recording() {
    with_program_state(
        &[("a.ts", GENERIC_SRC)],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let source = state.tables.intrinsics.non_inferrable_any;
            let info = detached_info(state, t);
            state
                .infer_types(&[info], source, t, InferencePriority::NONE, false)
                .expect("live spine");
            assert!(state.inference_info(info).candidates.is_none());
        },
    );
}

#[test]
fn wildcard_propagation_records_the_marker_source() {
    with_program_state(
        &[("a.ts", GENERIC_SRC)],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let wildcard = state.tables.intrinsics.wildcard;
            let info = detached_info(state, t);
            // 68650-68655: target-to-target under the propagation
            // type — T receives the wildcard marker itself.
            state
                .infer_types(&[info], wildcard, t, InferencePriority::NONE, false)
                .expect("live spine");
            assert_eq!(
                state.inference_info(info).candidates.as_deref(),
                Some(&[wildcard][..])
            );
        },
    );
}

#[test]
fn blocked_string_candidate_is_skipped_before_the_priority_reset() {
    with_program_state(
        &[("a.ts", GENERIC_SRC)],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let blocked = state.tables.intrinsics.blocked_string;
            let info = detached_info(state, t);
            state
                .infer_types(&[info], blocked, t, InferencePriority::NONE, false)
                .expect("live spine");
            let info = state.inference_info(info);
            // 68712-68714: the return fires BEFORE 68715's reset,
            // so priority stays unrecorded too.
            assert!(info.candidates.is_none());
            assert!(info.priority.is_none());
        },
    );
}

#[test]
fn union_target_unmatched_remainder_infers_into_the_naked_variable() {
    with_program_state(
        &[("a.ts", GENERIC_SRC)],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let string = state.tables.intrinsics.string;
            let number = state.tables.intrinsics.number;
            let info = detached_info(state, t);
            let target = state
                .get_union_type_ex(&[t, number], UnionReduction::Literal)
                .expect("union");
            // 68948-68953: one naked variable, nothing matched →
            // the unmatched remainder infers PLAIN (no priority
            // elevation).
            state
                .infer_types(&[info], string, target, InferencePriority::NONE, false)
                .expect("live spine");
            let info = state.inference_info(info);
            assert_eq!(info.candidates.as_deref(), Some(&[string][..]));
            assert_eq!(info.priority, Some(InferencePriority::NONE));
        },
    );
}

#[test]
fn union_target_fully_matched_source_records_naked_priority() {
    with_program_state(
        &[("a.ts", GENERIC_SRC)],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let string = state.tables.intrinsics.string;
            let info = detached_info(state, t);
            let target = state
                .get_union_type_ex(&[t, string], UnionReduction::Literal)
                .expect("union");
            // 68674-68682: the identical member strips, sources
            // empty → NakedTypeVariable inference of the ORIGINAL
            // source into the remainder.
            state
                .infer_types(&[info], string, target, InferencePriority::NONE, false)
                .expect("live spine");
            let info = state.inference_info(info);
            assert_eq!(info.candidates.as_deref(), Some(&[string][..]));
            assert_eq!(info.priority, Some(InferencePriority::NAKED_TYPE_VARIABLE));
        },
    );
}

#[test]
fn intersection_target_fully_matched_source_aborts() {
    with_program_state(
        &[("a.ts", "function f<T>() { var x: T & string; }\n")],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let string = state.tables.intrinsics.string;
            let info = detached_info(state, t);
            let annotation = annotation_of_var(state, "x");
            let target = state
                .get_type_from_type_node(annotation)
                .expect("intersection annotation");
            // 68688-68689: the identical member consumes the whole
            // source → sources empty → abort with no record (the
            // asymmetric twin of the union NakedTypeVariable path).
            state
                .infer_types(&[info], string, target, InferencePriority::NONE, false)
                .expect("live spine");
            let info = state.inference_info(info);
            assert!(info.candidates.is_none());
            assert!(info.priority.is_none());
        },
    );
}

#[test]
fn union_source_intersection_target_records_single_naked_variable() {
    with_program_state(
        &[("a.ts", "function f<T>() { var x: T & string; }\n")],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let number = state.tables.intrinsics.number;
            let boolean = state.tables.intrinsics.boolean;
            let info = detached_info(state, t);
            let annotation = annotation_of_var(state, "x");
            let target = state
                .get_type_from_type_node(annotation)
                .expect("intersection annotation");
            let source = state
                .get_union_type_ex(&[number, boolean], UnionReduction::Literal)
                .expect("union");
            // A union source skips the 68685 reduction; the
            // intersection branch of inferToMultipleTypes counts
            // exactly one type variable (68964) and lands a naked
            // inference on it.
            state
                .infer_types(&[info], source, target, InferencePriority::NONE, false)
                .expect("live spine");
            let info = state.inference_info(info);
            assert_eq!(info.candidates.as_deref(), Some(&[source][..]));
            assert_eq!(info.priority, Some(InferencePriority::NAKED_TYPE_VARIABLE));
        },
    );
}

#[test]
fn identical_union_source_and_target_infer_members_into_themselves() {
    with_program_state(
        &[("a.ts", GENERIC_SRC)],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let string = state.tables.intrinsics.string;
            let info = detached_info(state, t);
            let union = state
                .get_union_type_ex(&[t, string], UnionReduction::Literal)
                .expect("union");
            // 68667-68671: source === target → per-member (t, t),
            // so T records ITSELF as its candidate.
            state
                .infer_types(&[info], union, union, InferencePriority::NONE, false)
                .expect("live spine");
            assert_eq!(
                state.inference_info(info).candidates.as_deref(),
                Some(&[t][..])
            );
        },
    );
}

#[test]
fn same_alias_reference_infers_between_argument_lists() {
    with_program_state(
        &[(
            "a.ts",
            "type Box<B> = { v: B };\nfunction f<T>() { var a: Box<T>; var b: Box<string>; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let string = state.tables.intrinsics.string;
            let info = detached_info(state, t);
            let target_annotation = annotation_of_var(state, "a");
            let target = state
                .get_type_from_type_node(target_annotation)
                .expect("Box<T>");
            let source_annotation = annotation_of_var(state, "b");
            let source = state
                .get_type_from_type_node(source_annotation)
                .expect("Box<string>");
            // 68657-68663: same alias symbol → pairwise argument
            // inference under the alias' measured variances.
            state
                .infer_types(&[info], source, target, InferencePriority::NONE, false)
                .expect("live spine");
            assert_eq!(
                state.inference_info(info).candidates.as_deref(),
                Some(&[string][..])
            );
        },
    );
}

#[test]
fn substitution_source_uses_its_intersection_at_substitute_priority() {
    with_program_state(
        &[(
            "a.ts",
            "function f<T>() {\n\
             var base: { v: string };\n\
             var constraint: { w: number };\n\
             var target: { w: T };\n\
             }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let base_node = annotation_of_var(state, "base");
            let constraint_node = annotation_of_var(state, "constraint");
            let target_node = annotation_of_var(state, "target");
            let base = state.get_type_from_type_node(base_node).expect("base");
            let constraint = state
                .get_type_from_type_node(constraint_node)
                .expect("constraint");
            let target = state.get_type_from_type_node(target_node).expect("target");
            let source = state
                .tables
                .get_or_create_substitution_type(base, constraint);
            let info = detached_info(state, t);

            state
                .infer_types(&[info], source, target, InferencePriority::NONE, false)
                .expect("substitution inference");
            let info = state.inference_info(info);
            assert_eq!(
                info.candidates.as_deref(),
                Some(&[state.tables.intrinsics.number][..])
            );
            assert_eq!(
                info.priority,
                Some(InferencePriority::SUBSTITUTE_SOURCE),
                "only the substitution intersection contains target property `w`"
            );
        },
    );
}

#[test]
fn conditional_pair_infers_from_both_branches() {
    with_program_state(
        &[(
            "a.ts",
            "function f<T, U>() {\n\
             var source: U extends string ? { value: string } : { value: number };\n\
             var target: U extends string ? { value: T } : { value: T };\n\
             }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let source_node = annotation_of_var(state, "source");
            let target_node = annotation_of_var(state, "target");
            let source = state.get_type_from_type_node(source_node).expect("source");
            let target = state.get_type_from_type_node(target_node).expect("target");
            let info = detached_info(state, t);

            state
                .infer_types(&[info], source, target, InferencePriority::NONE, false)
                .expect("conditional-pair inference");
            let info = state.inference_info(info);
            assert_eq!(
                info.candidates.as_deref(),
                Some(
                    &[
                        state.tables.intrinsics.string,
                        state.tables.intrinsics.number
                    ][..]
                )
            );
            assert_eq!(info.priority, Some(InferencePriority::NONE));
        },
    );
}

#[test]
fn contravariant_nonconditional_source_uses_conditional_priority() {
    with_program_state(
        &[(
            "a.ts",
            "function f<T, U>() {\n\
             var target: U extends string ? T : T;\n\
             }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let target_node = annotation_of_var(state, "target");
            let target = state.get_type_from_type_node(target_node).expect("target");
            let info = detached_info(state, t);
            let string = state.tables.intrinsics.string;

            state
                .infer_types(&[info], string, target, InferencePriority::NONE, true)
                .expect("contravariant conditional inference");
            let info = state.inference_info(info);
            assert_eq!(info.contra_candidates.as_deref(), Some(&[string][..]));
            assert_eq!(
                info.priority,
                Some(
                    InferencePriority::CONTRAVARIANT_CONDITIONAL
                        | InferencePriority::NAKED_TYPE_VARIABLE
                )
            );
        },
    );
}

// ---- 7.2b: the literal-keyof arm ----

/// The empty-object reverse shape recorded for `"a"` vs `keyof T`
/// (68774-68776): a contra candidate at LiteralKeyof priority
/// whose members table holds an any-typed `a`.
#[test]
fn string_literal_against_keyof_records_reverse_empty_object() {
    with_program_state(
        &[("a.ts", GENERIC_SRC)],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let info = detached_info(state, t);
            let keyof_t = state.get_index_type(t, IndexFlags::NONE).expect("keyof T");
            let lit = state.tables.get_string_literal_type("a");
            state
                .infer_types(&[info], lit, keyof_t, InferencePriority::NONE, false)
                .expect("live arm");
            let (contra, priority, top_level) = {
                let info = state.inference_info(info);
                (
                    info.contra_candidates
                        .clone()
                        .expect("contravariant record"),
                    info.priority,
                    info.top_level,
                )
            };
            assert_eq!(priority, Some(InferencePriority::LITERAL_KEYOF));
            assert!(
                state.inference_info(info).candidates.is_none(),
                "the toggled entry lands on the contra side (68722)"
            );
            assert!(
                !top_level,
                "T is not at top level of `keyof T` — record-time demotion (68732)"
            );
            let [empty] = contra[..] else {
                panic!("exactly one contra candidate");
            };
            assert!(state.tables.flags_of(empty).intersects(TypeFlags::OBJECT));
            assert!(state
                .tables
                .object_flags_of(empty)
                .intersects(ObjectFlags::ANONYMOUS));
            let members = state
                .links
                .ty(empty)
                .resolved_members
                .resolved()
                .expect("created resolved");
            let resolved = state.members_of(members);
            assert_eq!(resolved.properties.len(), 1);
            assert!(resolved.index_infos.is_empty());
            let prop = *resolved.members.get("a").expect("member `a`");
            assert_eq!(
                state.links.symbol(prop).type_of_symbol.resolved(),
                Some(state.tables.intrinsics.any),
                "literalProp.links.type = anyType (68364)"
            );
        },
    );
}

/// A plain-string source contributes only the string→emptyObject
/// index signature (68371-68376).
#[test]
fn plain_string_against_keyof_builds_index_signature_shape() {
    with_program_state(
        &[("a.ts", GENERIC_SRC)],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let info = detached_info(state, t);
            let keyof_t = state.get_index_type(t, IndexFlags::NONE).expect("keyof T");
            let string = state.tables.intrinsics.string;
            state
                .infer_types(&[info], string, keyof_t, InferencePriority::NONE, false)
                .expect("live arm");
            let contra = state
                .inference_info(info)
                .contra_candidates
                .clone()
                .expect("contravariant record");
            let [empty] = contra[..] else {
                panic!("exactly one contra candidate");
            };
            let members = state
                .links
                .ty(empty)
                .resolved_members
                .resolved()
                .expect("created resolved");
            let resolved = state.members_of(members);
            assert!(resolved.properties.is_empty());
            let [ref info] = resolved.index_infos[..] else {
                panic!("exactly one index info");
            };
            assert_eq!(info.key_type, state.tables.intrinsics.string);
            assert_eq!(info.value_type, state.empty_object_type);
            assert!(!info.is_readonly);
        },
    );
}

/// forEachType distribution + the StringLiteral filter + leading-
/// underscore escaping: `"a" | "__x" | 1` keeps the string members
/// (escaped) and drops the number literal (68359-68361).
#[test]
fn literal_union_against_keyof_filters_and_escapes_members() {
    with_program_state(
        &[("a.ts", GENERIC_SRC)],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let info = detached_info(state, t);
            let keyof_t = state.get_index_type(t, IndexFlags::NONE).expect("keyof T");
            let lit_a = state.tables.get_string_literal_type("a");
            let lit_dunder = state.tables.get_string_literal_type("__x");
            let lit_one = state.tables.get_number_literal_type(1.0);
            let union = state
                .get_union_type_ex(&[lit_a, lit_dunder, lit_one], UnionReduction::Literal)
                .expect("literal union");
            state
                .infer_types(&[info], union, keyof_t, InferencePriority::NONE, false)
                .expect("live arm");
            let contra = state
                .inference_info(info)
                .contra_candidates
                .clone()
                .expect("contravariant record");
            let [empty] = contra[..] else {
                panic!("exactly one contra candidate");
            };
            let members = state
                .links
                .ty(empty)
                .resolved_members
                .resolved()
                .expect("created resolved");
            let resolved = state.members_of(members);
            assert_eq!(
                resolved.members.keys().cloned().collect::<Vec<_>>(),
                vec!["a".to_owned(), "___x".to_owned()],
                "union order kept, number literal dropped, __ escaped"
            );
            assert!(
                resolved.index_infos.is_empty(),
                "no String member in the union"
            );
        },
    );
}

// ---- 7.2c: inferToTemplateLiteralType ----

/// One `infer_types` run of `source_text` against `` `v${T}` ``
/// under the given constraint fixture; returns the recorded
/// candidates.
fn template_candidates(
    fixture: &str,
    source_text: &str,
    f: impl FnOnce(&mut CheckerState, Vec<TypeId>),
) {
    with_program_state(&[("a.ts", fixture)], &CompilerOptions::default(), |state| {
        let t = declared_type_parameter(state, "T");
        let info = detached_info(state, t);
        let target = state
            .tables
            .get_template_literal_type(&["v".to_owned(), String::new()], &[t]);
        let source = state.tables.get_string_literal_type(source_text);
        state
            .infer_types(&[info], source, target, InferencePriority::NONE, false)
            .expect("live arm");
        let candidates = state
            .inference_info(info)
            .candidates
            .clone()
            .expect("covariant record");
        f(state, candidates);
    });
}

/// tsc probe (scratchpad probe-template.mjs, 2026-07-20): a number
/// constraint coerces the "123" match to the 123 literal (69051
/// Number arm).
#[test]
fn template_number_constraint_coerces_string_match() {
    template_candidates(
        "function f<T extends number>() { var v: T; }\n",
        "v123",
        |state, candidates| {
            assert_eq!(
                candidates,
                vec![state.tables.get_number_literal_type(123.0)]
            );
        },
    );
}

/// Probe: bigint constraint → 123n (parseBigIntLiteralType arm).
#[test]
fn template_bigint_constraint_coerces_string_match() {
    template_candidates(
        "function f<T extends bigint>() { var v: T; }\n",
        "v123",
        |state, candidates| {
            let expected = state.tables.get_bigint_literal_type(PseudoBigInt {
                negative: false,
                base10_value: "123".to_owned(),
            });
            assert_eq!(candidates, vec![expected]);
        },
    );
}

/// Probe: a boolean constraint expands to its regular literal
/// members and "true" matches the BooleanLiteral arm — the MEMBER
/// (regular), not the fresh intrinsic.
#[test]
fn template_boolean_constraint_matches_regular_literal_member() {
    template_candidates(
        "function f<T extends boolean>() { var v: T; }\n",
        "vtrue",
        |state, candidates| {
            assert_eq!(candidates, vec![state.tables.intrinsics.true_regular]);
        },
    );
}

/// Probe: "0x10" does not round-trip as a number (js `+` yields
/// "16"), so NumberLike drops out and the RAW string literal is
/// the candidate (constraint clamping is 7.3's business).
#[test]
fn template_non_round_trip_number_string_stays_string() {
    template_candidates(
        "function f<T extends number>() { var v: T; }\n",
        "v0x10",
        |state, candidates| {
            assert_eq!(
                candidates,
                vec![state.tables.get_string_literal_type("0x10")]
            );
        },
    );
}

/// Probe: a String-flagged constraint member disables the whole
/// coercion block (69037 `!(allTypeFlags & String)`).
#[test]
fn template_string_in_constraint_union_disables_coercion() {
    template_candidates(
        "function f<T extends number | string>() { var v: T; }\n",
        "v123",
        |state, candidates| {
            assert_eq!(
                candidates,
                vec![state.tables.get_string_literal_type("123")]
            );
        },
    );
}

/// Probe: a number-literal union constraint matches the MEMBER
/// itself (NumberLiteral arm value compare).
#[test]
fn template_number_literal_union_matches_member() {
    template_candidates(
        "function f<T extends 1 | 2>() { var v: T; }\n",
        "v1",
        |state, candidates| {
            assert_eq!(candidates, vec![state.tables.get_number_literal_type(1.0)]);
        },
    );
}

/// Probe (`rbase → number`): the equal-texts path compares BASE
/// CONSTRAINTS on both sides (68577) — `` `a${number}` `` against
/// `` `a${T extends number}` `` keeps `number` as the candidate.
/// The pre-7.2c shortcut (assignability on the raw pair) wrapped
/// it into `` `${number}` `` — the stale-M3-justification pin.
#[test]
fn template_source_placeholder_keeps_type_under_matching_base_constraint() {
    with_program_state(
        &[("a.ts", "function f<T extends number>() { var v: T; }\n")],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let info = detached_info(state, t);
            let number = state.tables.intrinsics.number;
            let target = state
                .tables
                .get_template_literal_type(&["a".to_owned(), String::new()], &[t]);
            let source = state
                .tables
                .get_template_literal_type(&["a".to_owned(), String::new()], &[number]);
            state
                .infer_types(&[info], source, target, InferencePriority::NONE, false)
                .expect("live arm");
            assert_eq!(
                state.inference_info(info).candidates.as_deref(),
                Some(&[number][..])
            );
        },
    );
}

// ---- 7.2d: the object tail ----

/// Two annotation types from one fixture (the alias-test pattern).
fn annotated_pair(
    state: &mut CheckerState,
    source_var: &str,
    target_var: &str,
) -> (TypeId, TypeId) {
    let source_annotation = annotation_of_var(state, source_var);
    let source = state
        .get_type_from_type_node(source_annotation)
        .expect("source annotation");
    let target_annotation = annotation_of_var(state, target_var);
    let target = state
        .get_type_from_type_node(target_annotation)
        .expect("target annotation");
    (source, target)
}

#[test]
fn homomorphic_mapped_inference_builds_lazy_reverse_members() {
    with_program_state(
        &[(
            "a.ts",
            "function f<T>() {\n\
               var s: { a: string; readonly b?: number };\n\
               var t: { [K in keyof T]: T[K] };\n\
             }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let info = detached_info(state, t);
            let (source, target) = annotated_pair(state, "s", "t");
            state
                .infer_types(&[info], source, target, InferencePriority::NONE, false)
                .expect("homomorphic mapped inference resolves");
            let reverse = state.inference_info(info).candidates.as_ref().unwrap()[0];
            assert!(state
                .tables
                .object_flags_of(reverse)
                .intersects(ObjectFlags::REVERSE_MAPPED));
            let TypeData::ReverseMapped(data) = state.tables.type_of(reverse).data.clone() else {
                panic!("object reverse inference retains its semantic inputs");
            };
            assert_eq!(data.source, source);
            assert_eq!(data.mapped_type, target);

            let a = state
                .get_property_of_type_full(reverse, "a")
                .expect("reverse members resolve")
                .expect("a is reconstructed");
            let b = state
                .get_property_of_type_full(reverse, "b")
                .expect("reverse members remain cached")
                .expect("b is reconstructed");
            assert_eq!(
                state.get_type_of_symbol(a).expect("a infers"),
                state.tables.intrinsics.string
            );
            let b_type = state.get_type_of_symbol(b).expect("b infers");
            assert_eq!(
                state
                    .remove_missing_or_undefined_type(b_type)
                    .expect("optional flavor removes"),
                state.tables.intrinsics.number
            );
            assert!(state.symbol_flags(b).intersects(SymbolFlags::OPTIONAL));
            assert!(state.is_readonly_symbol(b));
        },
    );
}

#[test]
fn homomorphic_mapped_inference_preserves_tuple_shape() {
    with_program_state(
        &[(
            "a.ts",
            "function f<T>() {\n\
               var s: readonly [string, number?];\n\
               var t: { [K in keyof T]: T[K] };\n\
             }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let info = detached_info(state, t);
            let (source, target) = annotated_pair(state, "s", "t");
            state
                .infer_types(&[info], source, target, InferencePriority::NONE, false)
                .expect("tuple reverse inference resolves");
            let reversed = state.inference_info(info).candidates.as_ref().unwrap()[0];
            assert!(state.tables.is_tuple_type(reversed));
            let target = state.tables.reference_target(reversed);
            let TypeData::TupleTarget(data) = state.tables.type_of(target).data.clone() else {
                panic!("reverse tuple has a tuple target");
            };
            assert!(data.readonly);
            assert_eq!(
                data.element_flags.as_ref(),
                &[ElementFlags::REQUIRED, ElementFlags::OPTIONAL]
            );
            let arguments = state.get_type_arguments(reversed).expect("tuple arguments");
            assert_eq!(arguments[0], state.tables.intrinsics.string);
            assert_eq!(
                state
                    .remove_missing_or_undefined_type(arguments[1])
                    .expect("optional tuple flavor removes"),
                state.tables.intrinsics.number
            );
        },
    );
}

/// Probe r4: `[string, string, string]` against `[string, ...T]`
/// slices the middle into the variadic (69140-69144).
#[test]
fn tuple_single_variadic_middle_collects_slice() {
    with_program_state(
        &[(
            "a.ts",
            "function f<T extends any[]>() { var s: [string, string, string]; var t: [string, ...T]; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let info = detached_info(state, t);
            let (source, target) = annotated_pair(state, "s", "t");
            state
                .infer_types(&[info], source, target, InferencePriority::NONE, false)
                .expect("live tail");
            let string = state.tables.intrinsics.string;
            let expected = state
                .create_tuple_type_forced(&[string, string], None, false, None)
                .expect("tuple");
            assert_eq!(
                state.inference_info(info).candidates.as_deref(),
                Some(&[expected][..])
            );
            assert_eq!(
                state.inference_info(info).priority,
                Some(InferencePriority::NONE)
            );
        },
    );
}

/// Probe r5: an optional-ended target records the slice at
/// SpeculativeTuple priority (69141).
#[test]
fn tuple_variadic_middle_before_optional_uses_speculative_priority() {
    with_program_state(
        &[(
            "a.ts",
            "function f<T extends any[]>() { var s: [string, string]; var t: [string, ...T, string?]; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let info = detached_info(state, t);
            let (source, target) = annotated_pair(state, "s", "t");
            state
                .infer_types(&[info], source, target, InferencePriority::NONE, false)
                .expect("live tail");
            let expected = state
                .create_tuple_type_forced(&[], None, false, None)
                .expect("empty tuple");
            assert_eq!(
                state.inference_info(info).candidates.as_deref(),
                Some(&[expected][..])
            );
            assert_eq!(
                state.inference_info(info).priority,
                Some(InferencePriority::SPECULATIVE_TUPLE)
            );
        },
    );
}

/// Probe r1: a source SHORTER than the target's fixed parts drives
/// the middle slice bounds negative — JS slice clamps to the empty
/// tuple (the slice_tuple_type clamp pin; the pre-fix port
/// panicked on the inverted range).
#[test]
fn tuple_short_source_clamps_middle_slice_to_empty() {
    with_program_state(
        &[(
            "a.ts",
            "function f<T extends any[]>() { var s: [string, string]; var t: [string, string, ...T, string]; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let info = detached_info(state, t);
            let (source, target) = annotated_pair(state, "s", "t");
            state
                .infer_types(&[info], source, target, InferencePriority::NONE, false)
                .expect("live tail");
            let expected = state
                .create_tuple_type_forced(&[], None, false, None)
                .expect("empty tuple");
            assert_eq!(
                state.inference_info(info).candidates.as_deref(),
                Some(&[expected][..])
            );
        },
    );
}

/// Probe f2: variadic+rest middle with a fixed-tuple constraint —
/// the saturated first slice takes the whole short source; the
/// variable-free rest target makes the undefined second slice
/// harmless (the couldContainTypeVariables early return).
#[test]
fn tuple_variadic_rest_middle_saturates_short_source() {
    with_program_state(
        &[(
            "a.ts",
            "function f<T extends [any, any]>() { var s: [string]; var t: [...T, ...string[]]; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let info = detached_info(state, t);
            let (source, target) = annotated_pair(state, "s", "t");
            state
                .infer_types(&[info], source, target, InferencePriority::NONE, false)
                .expect("harmless undefined slice (probe f2)");
            let string = state.tables.intrinsics.string;
            let expected = state
                .create_tuple_type_forced(&[string], None, false, None)
                .expect("tuple");
            assert_eq!(
                state.inference_info(info).candidates.as_deref(),
                Some(&[expected][..])
            );
        },
    );
}

/// Probe f6 (the recorded tsc-crash deviation, m8-readiness row
/// 4): the same shape with a TYPE-VARIABLE rest target stops at
/// the contained crash boundary. Candidates recorded before that
/// boundary survive; the missing slice records no U candidate.
#[test]
fn tuple_middle_slice_crash_shape_stops_after_prior_inference() {
    with_program_state(
        &[(
            "a.ts",
            "function f<T extends [any, any], U>() { var s: [string]; var t: [...T, ...U[]]; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let u = declared_type_parameter(state, "U");
            let info_t = detached_info(state, t);
            let info_u = detached_info(state, u);
            let (source, target) = annotated_pair(state, "s", "t");
            state
                .infer_types(
                    &[info_t, info_u],
                    source,
                    target,
                    InferencePriority::NONE,
                    false,
                )
                .expect("contained tsc crash boundary (probe f6)");
            let string = state.tables.intrinsics.string;
            let expected = state
                .create_tuple_type_forced(&[string], None, false, None)
                .expect("tuple");
            assert_eq!(
                state.inference_info(info_t).candidates.as_deref(),
                Some(&[expected][..])
            );
            let info_u = state.inference_info(info_u);
            assert!(info_u.candidates.is_none());
            assert!(info_u.contra_candidates.is_none());
            assert!(info_u.priority.is_none());
        },
    );
}

/// Structure-matched tuples pair element-wise (69094-69098).
#[test]
fn tuple_structure_match_infers_elementwise() {
    with_program_state(
        &[(
            "a.ts",
            "function f<T, U>() { var s: [string, number]; var t: [T, U]; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let u = declared_type_parameter(state, "U");
            let info_t = detached_info(state, t);
            let info_u = detached_info(state, u);
            let (source, target) = annotated_pair(state, "s", "t");
            state
                .infer_types(
                    &[info_t, info_u],
                    source,
                    target,
                    InferencePriority::NONE,
                    false,
                )
                .expect("live tail");
            assert_eq!(
                state.inference_info(info_t).candidates.as_deref(),
                Some(&[state.tables.intrinsics.string][..])
            );
            assert_eq!(
                state.inference_info(info_u).candidates.as_deref(),
                Some(&[state.tables.intrinsics.number][..])
            );
        },
    );
}

/// inferFromProperties (69170): matching members meet through
/// removeMissingType.
#[test]
fn object_properties_infer_into_target_members() {
    with_program_state(
        &[(
            "a.ts",
            "function f<T>() { var s: { a: string }; var t: { a: T }; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let info = detached_info(state, t);
            let (source, target) = annotated_pair(state, "s", "t");
            state
                .infer_types(&[info], source, target, InferencePriority::NONE, false)
                .expect("live tail");
            assert_eq!(
                state.inference_info(info).candidates.as_deref(),
                Some(&[state.tables.intrinsics.string][..])
            );
        },
    );
}

/// inferFromSignatures (69182): strict-default parameters infer
/// CONTRAvariantly, returns covariantly — both sides of the same
/// info.
#[test]
fn signature_params_contravariant_returns_covariant() {
    with_program_state(
        &[(
            "a.ts",
            "function f<T>() { var s: (x: string) => number; var t: (x: T) => T; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let info = detached_info(state, t);
            let (source, target) = annotated_pair(state, "s", "t");
            state
                .infer_types(&[info], source, target, InferencePriority::NONE, false)
                .expect("live tail");
            let info = state.inference_info(info);
            assert_eq!(
                info.contra_candidates.as_deref(),
                Some(&[state.tables.intrinsics.string][..]),
                "strictFunctionTypes default → parameter goes contra (68892)"
            );
            assert_eq!(
                info.candidates.as_deref(),
                Some(&[state.tables.intrinsics.number][..]),
                "return type covariant (69202)"
            );
        },
    );
}

/// getBaseSignature (59946): a generic source signature erases to
/// its base constraints before parameter/return application.
#[test]
fn generic_source_signature_infers_through_base() {
    with_program_state(
        &[(
            "a.ts",
            "function f<T>() { var s: <V extends string>(x: V) => V; var t: (x: T) => T; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let info = detached_info(state, t);
            let (source, target) = annotated_pair(state, "s", "t");
            state
                .infer_types(&[info], source, target, InferencePriority::NONE, false)
                .expect("live tail");
            let info = state.inference_info(info);
            assert_eq!(
                info.contra_candidates.as_deref(),
                Some(&[state.tables.intrinsics.string][..]),
                "V erases to its string constraint"
            );
            assert_eq!(
                info.candidates.as_deref(),
                Some(&[state.tables.intrinsics.string][..])
            );
        },
    );
}

/// inferFromIndexTypes (69204): an inferable-index source funnels
/// the applicable property union into the target's index info.
#[test]
fn index_signature_collects_property_union() {
    with_program_state(
        &[(
            "a.ts",
            "function f<T>() { var s: { a: string; b: number }; var t: { [k: string]: T }; }\n",
        )],
        &CompilerOptions::default(),
        |state| {
            let t = declared_type_parameter(state, "T");
            let info = detached_info(state, t);
            let (source, target) = annotated_pair(state, "s", "t");
            state
                .infer_types(&[info], source, target, InferencePriority::NONE, false)
                .expect("live tail");
            let string = state.tables.intrinsics.string;
            let number = state.tables.intrinsics.number;
            let expected = state
                .get_union_type_ex(&[string, number], UnionReduction::Literal)
                .expect("union");
            assert_eq!(
                state.inference_info(info).candidates.as_deref(),
                Some(&[expected][..])
            );
        },
    );
}

/// isValidBigIntString round-trip gates: separators, whitespace,
/// and non-canonical forms are invalid; canonical decimals and the
/// negative form are valid (18973-18989 via the placeholder
/// consumer's bigint arm, round_trip_only=false there but =true in
/// the 69049 clearing gate — both exercised through the state fns).
#[test]
fn valid_big_int_string_gates() {
    with_program_state(
        &[("a.ts", "var v = 1;\n")],
        &CompilerOptions::default(),
        |state| {
            for (s, round_trip, expected) in [
                ("123", true, true),
                ("-5", true, true),
                ("0x1f", false, true),
                ("0x1f", true, false),
                ("1_0", false, false),
                (" 1", false, false),
                ("1 ", false, false),
                ("", false, false),
                ("-0", true, false),
                ("007", true, false),
                ("1.5", false, false),
            ] {
                assert_eq!(
                    state.is_valid_big_int_string(s, round_trip),
                    expected,
                    "isValidBigIntString({s:?}, {round_trip})"
                );
            }
        },
    );
}
