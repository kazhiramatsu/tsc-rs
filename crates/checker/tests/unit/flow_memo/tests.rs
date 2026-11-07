use super::*;
use crate::state::test_support::with_program_state;
use crate::state::ResolutionTarget;
use tsc_types::{CompilerOptions, TypeSystemPropertyName};

fn query<'a>(state: &mut CheckerState<'a>, reference: NodeId) -> FlowQuery<'a> {
    FlowQuery::for_memo_test(state, reference)
}

fn pure() -> FrameTaint {
    FrameTaint {
        transient: UNTAINTED,
        reference: UNTAINTED,
        flags: 0,
    }
}

fn identifiers(state: &CheckerState<'_>, name: &str) -> Vec<NodeId> {
    state
        .binder
        .source(0)
        .arena
        .node_ids()
        .filter(|&node| {
            state.kind_of(node) == SyntaxKind::Identifier
                && state.escaped_text_of(Some(node)) == Some(name)
        })
        .collect()
}

fn counters() -> FlowCounters {
    FlowCounters {
        instantiation_count: 7,
        ..FlowCounters::default()
    }
}

#[test]
fn frames_balance_and_reuse_the_stack() {
    let mut memo = FlowMemo::new();
    memo.begin(counters());
    memo.begin(counters());
    assert_eq!(memo.frame_depth(), 2);
    assert_eq!(memo.end(counters()).1, 0);
    assert_eq!(memo.end(counters()).1, 1);
    assert_eq!(memo.frame_depth(), 0);
    let capacity = memo.frames.capacity();
    memo.begin(counters());
    assert_eq!(memo.end(counters()).1, 0);
    assert_eq!(memo.frames.capacity(), capacity);
}

#[test]
fn only_sources_older_than_a_frame_taint_its_answer() {
    let mut memo = FlowMemo::new();
    memo.begin(counters());
    let loop_source = memo.next_serial();
    memo.begin(counters());
    memo.taint(loop_source);
    let (inner, _) = memo.end(counters());
    assert_eq!(inner.transient, loop_source);
    let (outer, _) = memo.end(counters());
    assert_eq!(outer.transient, UNTAINTED);

    memo.begin(counters());
    memo.taint(memo.source_now());
    assert_eq!(memo.end(counters()).0.transient, UNTAINTED);
}

#[test]
fn oldest_taints_and_flags_propagate_to_enclosing_frames() {
    let mut memo = FlowMemo::new();
    let oldest = memo.next_serial();
    let newer = memo.next_serial();
    memo.begin(counters());
    memo.taint(newer);
    memo.begin(counters());
    memo.taint(oldest);
    memo.taint(newer);
    memo.taint_reference(newer);
    memo.add_flags(FLAG_TYPE_CACHE);
    let (inner, _) = memo.end(counters());
    let (outer, _) = memo.end(counters());
    assert_eq!(inner, outer);
    assert_eq!(outer.transient, oldest);
    assert_eq!(outer.reference, newer);
    assert_eq!(outer.flags, FLAG_TYPE_CACHE);
}

#[test]
fn a_nested_walk_has_its_own_reference_floor_and_height() {
    let mut memo = FlowMemo::new();
    memo.begin(counters());
    memo.raise_height(4);
    let floor = memo.next_serial();
    let saved = memo.save_height();
    memo.begin(counters());
    memo.taint_reference(floor);
    memo.raise_height(20);
    assert_eq!(memo.end(counters()).0.reference, floor);
    memo.restore_height(saved);
    let (outer, height) = memo.end(counters());
    assert_eq!(outer.reference, UNTAINTED);
    assert_eq!(height, 4);
}

#[test]
fn transparent_frames_and_raise_height_do_not_add_a_flow_level() {
    let mut memo = FlowMemo::new();
    memo.begin(counters());
    memo.begin(counters());
    memo.raise_height(8);
    memo.raise_height(3);
    memo.add_flags(FLAG_TYPE_CACHE);
    let (_, height, flags) = memo.end_transparent();
    assert_eq!((height, flags), (8, FLAG_TYPE_CACHE));
    assert_eq!(memo.end(counters()).1, 8);

    memo.begin(counters());
    memo.raise_height(u16::MAX);
    memo.end(counters());
    assert_eq!(memo.regs.height, u16::MAX);

    memo.begin(FlowCounters::default());
    assert_eq!(memo.end_transparent().2, 0);
}

#[test]
fn checker_frames_notice_each_kind_of_new_identity() {
    with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
        state.flow_frame_begin();
        state.tables.get_number_literal_type(12345.0);
        assert_ne!(state.flow_frame_end().0.flags & FLAG_EFFECTS, 0);
        state.flow_frame_begin();
        state.binder.create_symbol(
            tsc_types::SymbolFlags::PROPERTY,
            tsc_types::EscapedName::from_identifier_escaped_text("flowMemoTest"),
        );
        assert_ne!(state.flow_frame_end().0.flags & FLAG_EFFECTS, 0);
        state.flow_frame_begin();
        state.clone_signature(state.unknown_signature);
        assert_ne!(state.flow_frame_end().0.flags & FLAG_EFFECTS, 0);
        state.flow_frame_begin();
        // An interned type has no new identity on its second request.
        state.tables.get_number_literal_type(12345.0);
        assert_eq!(state.flow_frame_end().0.flags & FLAG_EFFECTS, 0);
    });
}

#[test]
fn a_shared_answer_from_a_finished_loop_taints_every_enclosing_frame() {
    with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
        state.flow_frame_begin();
        let source = state.flow_memo.next_serial();
        state.flow_loop_stack.push(crate::flow::FlowLoopEntry {
            file: 0,
            flow: tsc_binder::flow::FlowId::new(0),
            key: tsc_types::JsString::from("loop"),
            types: vec![state.tables.intrinsics.string],
            serial: source,
        });
        state.flow_frame_begin();
        state.flow_memo_consume(source, UNTAINTED);
        assert_eq!(state.flow_frame_end().0.transient, source);
        state.flow_loop_stack.pop();
        state.flow_frame_begin();
        state.flow_memo_consume(source, UNTAINTED);
        assert_eq!(state.flow_frame_end().0.transient, 0);
        assert_eq!(state.flow_frame_end().0.transient, 0);
    });
}

#[test]
fn frame_flags_compare_instantiations_creations_and_count_resets() {
    for changed in [
        FlowCounters {
            total_instantiation_count: 1,
            ..counters()
        },
        FlowCounters {
            created: 1,
            ..counters()
        },
    ] {
        let mut memo = FlowMemo::new();
        memo.begin(counters());
        assert_eq!(memo.end(changed).0.flags, FLAG_EFFECTS);
    }
    let mut memo = FlowMemo::new();
    memo.begin(FlowCounters {
        in_instantiation: true,
        ..counters()
    });
    assert_eq!(memo.end(counters()).0.flags, FLAG_EFFECTS);
    memo.begin(counters());
    assert_eq!(memo.end(FlowCounters::default()).0.flags, FLAG_COUNT_RESET);
    memo.begin(FlowCounters::default());
    assert_eq!(memo.end(FlowCounters::default()).0.flags, FLAG_COUNT_RESET);
}

#[test]
fn serial_exhaustion_never_wraps_or_leaves_a_pure_frame() {
    let mut memo = FlowMemo::new();
    memo.begin(counters());
    memo.serial = UNTAINTED - 2;
    assert_eq!(memo.next_serial(), UNTAINTED - 2);
    memo.begin(counters());
    assert_eq!(memo.next_serial(), UNTAINTED - 1);
    assert_eq!(memo.source_now(), UNTAINTED - 2);
    assert_eq!(memo.end(counters()).0.transient, 0);
    assert_eq!(memo.end(counters()).0.transient, 0);
    memo.begin(counters());
    assert_eq!(memo.end(counters()).0.transient, 0);
}

#[test]
fn resolution_cycles_and_shared_answers_keep_their_source_lifetime() {
    with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
        state.flow_frame_begin();
        let target = ResolutionTarget::Type(state.tables.intrinsics.string);
        let property = TypeSystemPropertyName::IMMEDIATE_BASE_CONSTRAINT;
        assert!(state.push_type_resolution(target, property));
        let source = state.resolution_serials[0];
        state.flow_frame_begin();
        assert!(!state.push_type_resolution(target, property));
        assert_eq!(state.flow_frame_end().0.transient, source);
        state.flow_frame_begin();
        state.flow_memo_consume(source, UNTAINTED);
        assert_eq!(state.flow_frame_end().0.transient, source);
        assert!(!state.pop_type_resolution());
        state.flow_frame_begin();
        state.flow_memo_consume(source, UNTAINTED);
        assert_eq!(state.flow_frame_end().0.transient, 0);
        assert_eq!(state.flow_frame_end().0.transient, 0);
        assert_eq!(state.flow_memo.frame_depth(), 0);
    });
}

#[test]
fn serials_follow_the_live_resolution_prefix_without_a_transaction_hook() {
    with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
        let target = ResolutionTarget::Type(state.tables.intrinsics.string);
        let property = TypeSystemPropertyName::IMMEDIATE_BASE_CONSTRAINT;
        assert!(state.push_type_resolution(target, property));
        let old = state.resolution_serials[0];
        // The existing speculation boundary restores these stacks alone.
        state.resolution_targets.clear();
        state.resolution_property_names.clear();
        state.resolution_results.clear();
        state.flow_memo.next_serial();
        assert!(state.push_type_resolution(target, property));
        assert_eq!(state.resolution_serials.len(), 1);
        assert!(state.resolution_serials[0] > old);
        assert!(state.pop_type_resolution());
        assert!(state.resolution_serials.is_empty());
    });
}

#[test]
fn cache_epochs_are_scoped_with_the_saved_cache() {
    with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
        let node = state.binder.source(0).root;
        state.flow_type_cache = Some(FxHashMap::default());
        state
            .flow_type_cache
            .as_mut()
            .unwrap()
            .insert(node, state.tables.intrinsics.string);
        let original = state.flow_memo.type_cache_epoch;
        let saved = state.take_flow_type_cache();
        let outer = state.flow_memo.type_cache_epoch;
        assert_ne!(outer, original);
        assert!(state.flow_type_cache.is_none());
        let nested = state.take_flow_type_cache();
        assert_ne!(state.flow_memo.type_cache_epoch, outer);
        state.restore_flow_type_cache(nested);
        assert_eq!(state.flow_memo.type_cache_epoch, outer);
        state.restore_flow_type_cache(saved);
        assert_eq!(state.flow_memo.type_cache_epoch, original);
        assert_eq!(
            state.flow_type_cache.as_ref().unwrap()[&node],
            state.tables.intrinsics.string
        );
    });
}

#[test]
fn checker_run_leaves_no_open_frame() {
    let text = r#"
        function f(x: string | number, b: boolean) {
            const isString = typeof x === "string";
            while (b) {
                if (isString) { x = "s"; } else { x = 1; }
                try { if (b) continue; return x; }
                finally { b = false; }
            }
            return x;
        }
        function generic<T extends string | undefined>(x: T) {
            if (x) { const s: string = x; return s; }
        }
    "#;
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        assert!(state.flow_invocation_count > 0);
        assert!(state.flow_memo.source_now() > 0);
        assert_eq!(state.flow_memo.frame_depth(), 0);
        assert!(state.shared_flow.is_empty());
        assert!(state.flow_loop_stack.is_empty());
        assert!(state.reduce_label_overrides.is_empty());
        assert!(state.resolution_targets.is_empty());
        assert!(state.flow_memo.checkpoints.is_empty());
        assert!(state.flow_memo.shadow_hits.is_empty());
    });
}

#[test]
fn packed_keys_reserve_a_distinct_domain_without_restricting_initial_type_ids() {
    with_program_state(&[("a.ts", "this;")], &CompilerOptions::default(), |state| {
        let reference = state
            .binder
            .source(0)
            .arena
            .node_ids()
            .find(|&n| state.kind_of(n) == SyntaxKind::ThisKeyword)
            .unwrap();
        let mut q = query(state, reference);
        q.initial_type = TypeId::new(1 << 31);
        let packed = state.flow_memo_key(&q).unwrap();
        assert_eq!(packed.hash >> 96, 0, "zero root denotes this");
        assert_eq!(packed.hash & (1 << 95), 0);
        assert_eq!((packed.hash >> 32) as u32, q.initial_type.index());
        q.declared_type = TypeId::new(1 << 31);
        let hashed = state.flow_memo_key(&q).unwrap();
        assert_ne!(hashed.hash & (1 << 95), 0);
        assert_ne!(packed.hash, hashed.hash);
    });
}

#[test]
fn structural_keys_use_cached_symbols_types_and_container_without_resolving() {
    with_program_state(
        &[("a.ts", "let x = 1; x; x; { let x = 2; x; }")],
        &CompilerOptions::default(),
        |state| {
            let refs = identifiers(state, "x");
            let mut q = query(state, refs[1]);
            let created = state.flow_frame_counters().created;
            assert!(state.flow_memo_key(&q).is_none());
            assert_eq!(state.flow_frame_counters().created, created);
            assert!(state
                .links
                .node(refs[1])
                .resolved_symbol
                .resolved()
                .is_none());
            for &node in &refs {
                state.get_resolved_symbol(node).unwrap();
            }
            let key = state.flow_memo_key(&q).unwrap();
            q.reference = refs[2];
            assert_eq!(key.full, state.flow_memo_key(&q).unwrap().full);
            assert_eq!(key.hash, state.flow_memo_key(&q).unwrap().hash);
            q.reference = *refs.last().unwrap();
            assert_ne!(key.full, state.flow_memo_key(&q).unwrap().full);
            q.reference = refs[1];
            q.declared_type = state.tables.intrinsics.number;
            assert_ne!(key.hash, state.flow_memo_key(&q).unwrap().hash);
            q.declared_type = state.tables.intrinsics.string;
            q.initial_type = state.tables.intrinsics.number;
            assert_ne!(key.hash, state.flow_memo_key(&q).unwrap().hash);
            q.initial_type = state.tables.intrinsics.string;
            q.flow_container = Some(state.binder.source(0).root);
            assert_ne!(key.hash, state.flow_memo_key(&q).unwrap().hash);
            q.synthetic_props = Some(Vec::new());
            assert!(state.flow_memo_key(&q).is_none());
            q.synthetic_props = None;
            q.synthetic_this_root = true;
            assert!(state.flow_memo_key(&q).is_none());
        },
    );
}

#[test]
fn property_this_and_optional_chain_key_shapes() {
    let text =
        "declare let o: { p: { q: number } }; o.p.q; o.p.q; o?.p; o['p']; (o).p; this; this;";
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        for node in identifiers(state, "o") {
            state.get_resolved_symbol(node).unwrap();
        }
        let nodes: Vec<_> = state.binder.source(0).arena.node_ids().collect();
        let properties: Vec<_> = nodes
            .iter()
            .copied()
            .filter(|&node| {
                matches!(state.data_of(node), NodeData::PropertyAccessExpression(data)
                if state.escaped_text_of(data.name) == Some("q"))
            })
            .collect();
        let q = query(state, properties[0]);
        let key = state.flow_memo_key(&q).unwrap();
        let q2 = query(state, properties[1]);
        assert_eq!(key.full, state.flow_memo_key(&q2).unwrap().full);
        assert!(key.full.windows(6).any(|bytes| bytes == b"P\x01\0\0\0p"));
        for node in nodes.iter().copied().filter(|&node| {
            state.kind_of(node) == SyntaxKind::ElementAccessExpression
                || state.binder.flags_of(node).intersects(NodeFlags::OPTIONAL_CHAIN)
                || matches!(state.data_of(node), NodeData::PropertyAccessExpression(data)
                    if data.expression.is_some_and(|e| state.kind_of(e) == SyntaxKind::ParenthesizedExpression))
        }).collect::<Vec<_>>() {
            let q = query(state, node);
            assert!(state.flow_memo_key(&q).is_none());
        }
        let this: Vec<_> = nodes
            .into_iter()
            .filter(|&node| state.kind_of(node) == SyntaxKind::ThisKeyword)
            .collect();
        let q = query(state, this[0]);
        let q2 = query(state, this[1]);
        assert_eq!(
            state.flow_memo_key(&q).unwrap().full,
            state.flow_memo_key(&q2).unwrap().full
        );
        assert_ne!(key.hash, state.flow_memo_key(&q).unwrap().hash);
    });
}

#[test]
fn every_store_guard_and_count_reset_epoch_metadata() {
    let ty = FlowType::Type(TypeId::new(7));
    assert!(can_store(pure(), ty, 1, 1, false));
    for taint in [
        FrameTaint {
            transient: 0,
            ..pure()
        },
        FrameTaint {
            transient: 42,
            ..pure()
        },
        FrameTaint {
            reference: 0,
            ..pure()
        },
        FrameTaint {
            reference: 42,
            ..pure()
        },
        FrameTaint {
            flags: FLAG_EFFECTS,
            ..pure()
        },
    ] {
        assert!(!can_store(taint, ty, 1, 4, false), "{taint:?}");
    }
    // tsrs conditions 3/5 retain these flags and guard consultation, rather
    // than discarding all count-reset or expression-cache dependent results.
    for flags in [
        FLAG_COUNT_RESET,
        FLAG_TYPE_CACHE,
        FLAG_COUNT_RESET | FLAG_TYPE_CACHE,
    ] {
        assert!(can_store(FrameTaint { flags, ..pure() }, ty, 1, 4, false));
    }
    assert!(!can_store(
        pure(),
        FlowType::Incomplete(TypeId::new(7)),
        1,
        4,
        false
    ));
    assert!(!can_store(pure(), ty, 1, 4, true));
    assert!(!can_store(pure(), ty, 2000, 4, false));
    assert!(!can_store(pure(), ty, 0, 3, false));
    assert!(can_store(pure(), ty, 0, 4, false));
}

#[test]
fn existing_shared_effects_block_replacement_without_creating_new_identities() {
    with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
        let mut q = query(state, state.binder.source(0).root);
        q.flow_depth = 1;
        let hit = MemoHit {
            ty: state.tables.intrinsics.string,
            height: 4,
            epoch: 0,
            flags: FLAG_COUNT_RESET | FLAG_TYPE_CACHE,
        };
        assert!(state.flow_memo_consult_ok(&q, hit));
        let created = state.flow_frame_counters().created;
        state.record_shared_flow(
            &mut q,
            SharedFlow {
                flow: FlowId::new(1),
                flow_type: FlowType::Type(hit.ty),
                transient: UNTAINTED,
                reference: UNTAINTED,
                height: 2,
                flags: FLAG_EFFECTS | FLAG_COUNT_RESET,
            },
        );
        assert_eq!(state.flow_frame_counters().created, created);
        assert!(!state.flow_memo_consult_ok(&q, hit));
        state.flow_frame_begin();
        let cached = state.shared_flow[0];
        state.flow_memo_consume(cached.transient, cached.reference);
        state.flow_memo.add_flags(cached.flags);
        let (taint, _) = state.flow_frame_end();
        assert_ne!(taint.flags & FLAG_EFFECTS, 0);
    });
}

#[test]
fn shared_reference_dependencies_and_incomplete_results_block_replacement() {
    with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
        let mut q = query(state, state.binder.source(0).root);
        q.flow_depth = 1;
        let hit = MemoHit {
            ty: state.tables.intrinsics.string,
            height: 1,
            epoch: 0,
            flags: 0,
        };
        for (reference, flow_type, allowed) in [
            (UNTAINTED, FlowType::Type(hit.ty), true),
            (1, FlowType::Type(hit.ty), false),
            (UNTAINTED, FlowType::Incomplete(hit.ty), false),
        ] {
            q.impure_shared = 0;
            state.shared_flow.clear();
            state.record_shared_flow(
                &mut q,
                SharedFlow {
                    flow: FlowId::new(1),
                    flow_type,
                    transient: UNTAINTED,
                    reference,
                    height: 1,
                    flags: 0,
                },
            );
            assert_eq!(state.flow_memo_consult_ok(&q, hit), allowed);
        }
    });
}

#[test]
fn consultation_guards_live_context_and_depth() {
    with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
        let mut q = query(state, state.binder.source(0).root);
        q.flow_depth = 1;
        let mut hit = MemoHit {
            ty: state.tables.intrinsics.string,
            height: 4,
            epoch: 0,
            flags: 0,
        };
        assert!(state.flow_memo_consult_ok(&q, hit));
        q.memo_faithful = true;
        assert!(!state.flow_memo_consult_ok(&q, hit));
        q.memo_faithful = false;
        q.impure_shared = 1;
        assert!(!state.flow_memo_consult_ok(&q, hit));
        q.impure_shared = 0;
        q.flow_depth = 1997;
        assert!(!state.flow_memo_consult_ok(&q, hit));
        q.flow_depth = 1996;
        assert!(state.flow_memo_consult_ok(&q, hit));
        q.flow_depth = 1;
        hit.epoch = state.flow_memo.next_serial();
        assert!(!state.flow_memo_consult_ok(&q, hit));
        state.flow_memo.type_cache_epoch = hit.epoch;
        assert!(state.flow_memo_consult_ok(&q, hit));
        hit.flags = FLAG_COUNT_RESET;
        state.instantiation_count = 1;
        assert!(!state.flow_memo_consult_ok(&q, hit));
        state.instantiation_count = 0;
        assert!(state.flow_memo_consult_ok(&q, hit));
        state.flow_loop_stack.push(crate::flow::FlowLoopEntry {
            file: 0,
            flow: FlowId::new(0),
            key: "x".into(),
            types: Vec::new(),
            serial: 1,
        });
        assert!(!state.flow_memo_consult_ok(&q, hit));
        state.flow_loop_stack.clear();
        state
            .active_type_mappers
            .push(crate::instantiate::MapperId::new(0));
        assert!(!state.flow_memo_consult_ok(&q, hit));
        state.active_type_mappers.clear();
        state.flow_analysis_disabled = true;
        assert!(!state.flow_memo_consult_ok(&q, hit));
        state.flow_analysis_disabled = false;
        state
            .reduce_label_overrides
            .insert((0, FlowId::new(0)), Vec::new());
        assert!(!state.flow_memo_consult_ok(&q, hit));
        state.reduce_label_overrides.clear();
        state.flow_memo.serial = UNTAINTED - 1;
        assert!(!state.flow_memo_consult_ok(&q, hit));
    });
}

#[test]
fn table_is_bounded_and_flow_ids_are_file_local() {
    with_program_state(&[("a.ts", "this;")], &CompilerOptions::default(), |state| {
        let reference = state
            .binder
            .source(0)
            .arena
            .node_ids()
            .find(|&n| state.kind_of(n) == SyntaxKind::ThisKeyword)
            .unwrap();
        let q = query(state, reference);
        let key = state.flow_memo_key(&q).unwrap();
        let hit = MemoHit {
            ty: state.tables.intrinsics.string,
            height: 4,
            epoch: 0,
            flags: 0,
        };
        assert!(state.flow_memo.lookup(0, FlowId::new(7), &key).is_none());
        state.flow_memo.store(&q, FlowId::new(7), &key, hit);
        assert_eq!(
            state.flow_memo.lookup(0, FlowId::new(7), &key).unwrap().ty,
            hit.ty
        );
        assert!(state.flow_memo.lookup(1, FlowId::new(7), &key).is_none());
        for index in 0..TABLE_SLOTS as u32 * 4 {
            state.flow_memo.store(&q, FlowId::new(index), &key, hit);
        }
        assert_eq!(state.flow_memo.slots.len(), TABLE_SLOTS);
        assert!(state.flow_memo.stats.occupied <= TABLE_SLOTS);
        assert_eq!(std::mem::size_of::<Slot>() * TABLE_SLOTS, 128 * 1024);
    });
}

#[test]
fn checkpoints_are_node_based_and_nested_frames_do_not_steal_them() {
    assert_eq!(memo_shared_node(false, Some(FlowId::new(1))), None);
    assert_eq!(
        memo_shared_node(true, Some(FlowId::new(1))),
        Some(FlowId::new(1))
    );
    let chosen: Vec<_> = (0..1024)
        .filter(|&n| is_checkpoint(0, FlowId::new(n)))
        .collect();
    assert!(
        (192..=320).contains(&chosen.len()),
        "{} checkpoints",
        chosen.len()
    );
    assert!(chosen.iter().any(|&n| !is_checkpoint(1, FlowId::new(n))));
    with_program_state(&[("a.ts", "this;")], &CompilerOptions::default(), |state| {
        let reference = state
            .binder
            .source(0)
            .arena
            .node_ids()
            .find(|&n| state.kind_of(n) == SyntaxKind::ThisKeyword)
            .unwrap();
        let q = query(state, reference);
        let key = state.flow_memo_key(&q).unwrap();
        state.flow_memo.checkpoints.push(FlowId::new(1));
        let memo = FrameMemo {
            key: key.clone(),
            checkpoints: 1,
            shadow: 0,
            shared_start: 0,
        };
        state.flow_memo.checkpoints.push(FlowId::new(2));
        let nested = FrameMemo {
            key: key.clone(),
            checkpoints: 2,
            shadow: 0,
            shared_start: 0,
        };
        state.flow_memo.checkpoints.push(FlowId::new(3));
        let result = Ok(FlowType::Type(state.tables.intrinsics.string));
        state.flow_memo_finish(
            &q,
            nested,
            FlowId::new(4),
            FlowId::new(5),
            &result,
            pure(),
            2,
            5,
            None,
            false,
        );
        assert_eq!(
            state.flow_memo.checkpoints,
            [FlowId::new(1), FlowId::new(2)]
        );
        for flow in [3, 4, 5] {
            assert!(state.flow_memo.lookup(0, FlowId::new(flow), &key).is_some());
        }
        // Even a cold, cache-independent frame acquires an epoch guard:
        // its later real walk may consume an expression-cache hit.
        let cold = state.flow_memo.lookup(0, FlowId::new(4), &key).unwrap();
        assert_eq!(cold.epoch, state.flow_memo.type_cache_epoch);
        assert_ne!(cold.flags & FLAG_TYPE_CACHE, 0);
        state.flow_memo_finish(
            &q,
            memo,
            FlowId::new(6),
            FlowId::new(7),
            &result,
            pure(),
            3,
            6,
            None,
            false,
        );
        assert_eq!(state.flow_memo.checkpoints, [FlowId::new(1)]);
        assert!(state.flow_memo.lookup(0, FlowId::new(2), &key).is_some());
        assert_eq!(
            state
                .flow_memo
                .lookup(0, FlowId::new(3), &key)
                .unwrap()
                .height,
            2
        );
        let memo = FrameMemo {
            key: key.clone(),
            checkpoints: 1,
            shadow: 0,
            shared_start: 0,
        };
        state.flow_memo.checkpoints.push(FlowId::new(8));
        state.flow_memo_finish(
            &q,
            memo,
            FlowId::new(9),
            FlowId::new(10),
            &result,
            FrameTaint {
                transient: 0,
                ..pure()
            },
            3,
            6,
            None,
            false,
        );
        assert_eq!(state.flow_memo.checkpoints.len(), 1);
        assert!(state.flow_memo.lookup(0, FlowId::new(8), &key).is_none());
        state.flow_memo.checkpoints.clear();
    });
}

#[test]
fn shared_cache_returns_settle_shadow_hits_without_filling_the_memo() {
    with_program_state(&[("a.ts", "this;")], &CompilerOptions::default(), |state| {
        let reference = state
            .binder
            .source(0)
            .arena
            .node_ids()
            .find(|&n| state.kind_of(n) == SyntaxKind::ThisKeyword)
            .unwrap();
        let q = query(state, reference);
        let key = state.flow_memo_key(&q).unwrap();
        let hit = MemoHit {
            ty: state.tables.intrinsics.string,
            height: 4,
            epoch: 0,
            flags: 0,
        };
        state.flow_memo.store(&q, FlowId::new(9), &key, hit);
        let stores = state.flow_memo.stats.stores;
        let memo = FrameMemo {
            key: key.clone(),
            checkpoints: 0,
            shadow: 0,
            shared_start: 0,
        };
        state.flow_memo.checkpoints.push(FlowId::new(2));
        state.flow_memo.shadow_hits.push(ShadowHit {
            flow: FlowId::new(1),
            hit,
            origin: ShadowOrigin {
                key: key.full.clone(),
                file: 0,
                flow: FlowId::new(1),
                reference,
            },
            before: pure(),
            height_before: 0,
            created_before: (0, 0, 0),
            shared_start: 0,
            shared_hash: shared_hash(&[]),
            ends_iteration: false,
            shared_node: None,
        });
        state.flow_memo_finish(
            &q,
            memo,
            FlowId::new(3),
            FlowId::new(4),
            &Ok(FlowType::Type(hit.ty)),
            pure(),
            3,
            10,
            None,
            true,
        );
        assert_eq!(state.flow_memo.stats.stores, stores);
        assert_eq!(state.flow_memo.stats.disagreements, 0);
        assert!(state.flow_memo.checkpoints.is_empty());
        assert!(state.flow_memo.shadow_hits.is_empty());
        assert!(state.flow_memo.lookup(0, FlowId::new(2), &key).is_none());
    });
}

#[test]
fn shared_effects_and_inline_reduce_contexts_block_stores() {
    let shared = SharedFlow {
        flow: FlowId::new(1),
        flow_type: FlowType::Type(TypeId::new(7)),
        transient: UNTAINTED,
        reference: UNTAINTED,
        height: 2,
        flags: 0,
    };
    assert!(shared_replay_safe(&[shared]));
    for entry in [
        SharedFlow {
            transient: 1,
            ..shared
        },
        SharedFlow {
            reference: 1,
            ..shared
        },
        SharedFlow {
            flags: FLAG_EFFECTS,
            ..shared
        },
        SharedFlow {
            flow_type: FlowType::Incomplete(TypeId::new(7)),
            ..shared
        },
    ] {
        assert!(!shared_replay_safe(&[entry]));
    }
    with_program_state(&[("a.ts", "this;")], &CompilerOptions::default(), |state| {
        let reference = state
            .binder
            .source(0)
            .arena
            .node_ids()
            .find(|&n| state.kind_of(n) == SyntaxKind::ThisKeyword)
            .unwrap();
        let mut q = query(state, reference);
        q.memo_key = state.flow_memo_key(&q);
        assert!(state.flow_frame_memo(&q).is_some());
        state.inline_level = 1;
        assert!(state.flow_frame_memo(&q).is_none());
        state.inline_level = 0;
        q.reduce_depth = 1;
        assert!(state.flow_frame_memo(&q).is_none());
        q.reduce_depth = 0;
        state
            .reduce_label_overrides
            .insert((0, FlowId::new(0)), Vec::new());
        assert!(state.flow_frame_memo(&q).is_none());
        state.reduce_label_overrides.clear();
        state.flow_analysis_disabled = true;
        assert!(state.flow_frame_memo(&q).is_none());
        state.flow_analysis_disabled = false;
        let key = q.memo_key.clone().unwrap();
        let memo = FrameMemo {
            key: key.clone(),
            checkpoints: 0,
            shadow: 0,
            shared_start: 0,
        };
        state.shared_flow.push(SharedFlow {
            transient: 1,
            ..shared
        });
        state.flow_memo_finish(
            &q,
            memo,
            FlowId::new(2),
            FlowId::new(3),
            &Ok(shared.flow_type),
            pure(),
            2,
            5,
            None,
            false,
        );
        assert!(state.flow_memo.slots.is_empty());
    });
}

#[test]
fn shadow_comparison_detects_types_keys_registers_and_shared_effects() {
    with_program_state(&[("a.ts", "this;")], &CompilerOptions::default(), |state| {
        let reference = state
            .binder
            .source(0)
            .arena
            .node_ids()
            .find(|&n| state.kind_of(n) == SyntaxKind::ThisKeyword)
            .unwrap();
        let q = query(state, reference);
        let key = state.flow_memo_key(&q).unwrap();
        let ty = state.tables.intrinsics.string;
        let make_hit = || ShadowHit {
            flow: FlowId::new(1),
            hit: MemoHit {
                ty,
                height: 2,
                epoch: 0,
                flags: 0,
            },
            origin: ShadowOrigin {
                key: key.full.clone(),
                file: 0,
                flow: FlowId::new(1),
                reference,
            },
            before: pure(),
            height_before: 0,
            created_before: (0, 0, 0),
            shared_start: 0,
            shared_hash: shared_hash(&[]),
            ends_iteration: true,
            shared_node: None,
        };
        state.flow_memo_shadow_check(
            &q,
            &key,
            &Ok(FlowType::Type(ty)),
            pure(),
            2,
            make_hit(),
            None,
        );
        assert_eq!(state.flow_memo.stats.disagreements, 0);
        state.flow_memo_shadow_check(
            &q,
            &key,
            &Ok(FlowType::Incomplete(ty)),
            pure(),
            2,
            make_hit(),
            None,
        );
        state.flow_memo_shadow_check(
            &q,
            &key,
            &Ok(FlowType::Type(state.tables.intrinsics.number)),
            pure(),
            2,
            make_hit(),
            None,
        );
        let mut hit = make_hit();
        hit.origin.key = Arc::from(&b"wrong key"[..]);
        state.flow_memo_shadow_check(&q, &key, &Ok(FlowType::Type(ty)), pure(), 2, hit, None);
        for taint in [
            FrameTaint {
                transient: 0,
                ..pure()
            },
            FrameTaint {
                reference: 0,
                ..pure()
            },
            FrameTaint {
                flags: FLAG_EFFECTS,
                ..pure()
            },
        ] {
            state.flow_memo_shadow_check(
                &q,
                &key,
                &Ok(FlowType::Type(ty)),
                taint,
                2,
                make_hit(),
                None,
            );
        }
        state.flow_memo_shadow_check(
            &q,
            &key,
            &Ok(FlowType::Type(ty)),
            pure(),
            3,
            make_hit(),
            None,
        );
        state.shared_flow.push(SharedFlow {
            flow: FlowId::new(2),
            flow_type: FlowType::Type(ty),
            transient: 1,
            reference: UNTAINTED,
            height: 0,
            flags: 0,
        });
        state.flow_memo_shadow_check(
            &q,
            &key,
            &Ok(FlowType::Type(ty)),
            pure(),
            2,
            make_hit(),
            None,
        );
        assert_eq!(state.flow_memo.stats.disagreements, 8);
        state.shared_flow.clear();
        state.flow_memo_shadow_check(
            &q,
            &key,
            &Ok(FlowType::Type(ty)),
            pure(),
            2,
            make_hit(),
            Some(FlowId::new(2)),
        );
        assert_eq!(state.flow_memo.stats.disagreements, 9);
    });
}

#[test]
fn memo_depth_abort_unwinds_without_publishing_partial_answers() {
    let text = "function f(x: string | number, b: boolean) { while (b) { x; } }";
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        let reference = *identifiers(state, "x").last().unwrap();
        state.get_resolved_symbol(reference).unwrap();
        let condition = state.flow_node_of(reference).unwrap();
        assert!(state
            .flow_flags_of(0, condition)
            .intersects(FlowFlags::TRUE_CONDITION));
        let loop_node = state.flow_antecedent(0, condition);
        assert!(state
            .flow_flags_of(0, loop_node)
            .intersects(FlowFlags::LOOP_LABEL));
        state.flow_type_cache = Some(
            [(reference, state.tables.intrinsics.string)]
                .into_iter()
                .collect(),
        );
        let original_cache = state.flow_type_cache.clone();
        let epoch = state.flow_memo.type_cache_epoch;
        let diagnostics = state.diagnostics.len();
        for (flow, depth) in [
            (condition, FLOW_DEPTH_LIMIT - 1),
            (loop_node, FLOW_DEPTH_LIMIT - 2),
        ] {
            let mut q = query(state, reference);
            // A wider declared type forces the loop to inspect its back
            // edge instead of stopping at the first (initial-type) answer.
            q.declared_type = state.tables.intrinsics.unknown;
            q.memo_key = state.flow_memo_key(&q);
            q.memo_used = true;
            q.flow_depth = depth;
            let answer = state.get_type_at_flow_node(&mut q, flow).unwrap();
            assert_eq!(answer, FlowType::Type(state.tables.intrinsics.error));
            assert!(q.memo_aborted);
            assert!(!state.flow_analysis_disabled);
            assert_eq!(q.flow_depth, depth);
            assert_eq!(state.flow_memo.frame_depth(), 0);
            assert!(state.flow_loop_stack.is_empty());
            assert!(state.flow_loop_caches.is_empty());
            assert_eq!(state.flow_type_cache, original_cache);
            assert_eq!(state.flow_memo.type_cache_epoch, epoch);
            assert_eq!(state.diagnostics.len(), diagnostics);
            assert!(state.shared_flow.iter().all(|s| s.flow_type != answer));
            state.shared_flow.truncate(q.shared_flow_start);
            assert!(state.flow_memo.checkpoints.is_empty());
            assert!(state.flow_memo.shadow_hits.is_empty());
            let serial = state.flow_memo.serial;
            q.flow_depth = 0;
            assert_eq!(state.get_type_at_flow_node(&mut q, flow).unwrap(), answer);
            assert_eq!(state.flow_memo.serial, serial);
        }
    });
}

#[test]
#[cfg(feature = "flow-memo-shadow")]
fn cold_answers_replayed_after_expression_cache_warming_keep_cache_dependencies() {
    // Reduced from hono's buffer equality helper. The first flow walk can
    // compute an answer without a cache read; a later walk of the same graph
    // consumes the expression cache populated in the meantime.
    let lib = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../vendor/typescript-native/7.1.0-dev-aa814927/upstream/tsc/internal/bundled/libs/lib.es5.d.ts"
    ))
    .expect("vendored lib.es5.d.ts");
    let text = r#"
        export const equal = (a: ArrayBuffer, b: ArrayBuffer): boolean => {
            if (a === b) return true;
            if (a.byteLength !== b.byteLength) return false;
            const va = new DataView(a);
            const vb = new DataView(b);
            let i = va.byteLength;
            while (i--) {
                if (va.getUint8(i) !== vb.getUint8(i)) return false;
            }
            return true;
        };
    "#;
    with_program_state(
        &[("a.ts", text), ("lib.es5.d.ts", &lib)],
        &CompilerOptions::default(),
        |state| {
            state.check_source_file(0);
            assert!(state.flow_memo.stats.hits > 0);
            assert_eq!(state.flow_memo.stats.disagreements, 0);
        },
    );
}

#[test]
fn memo_walks_loops_reduce_labels_and_property_references_without_changing_answers() {
    let text = r#"
        function f(x: string | number, b: boolean, o: { p: string | number }) {
            while (b) {
                if (typeof x === "string") { const a: string = x; const c: string = x; x; x; }
                else { const a: number = x; const c: number = x; x; x; }
                try { if (b) { x = 1; continue; } break; }
                finally { b = false; }
            }
            if (typeof o.p === "string") { const s: string = o.p; const t: string = o.p; }
            try { if (typeof x === "number") { x; x; } }
            finally { if (b) x = "s"; }
            if (typeof x === "string") { const a: string = x; const c: string = x; x; x; }
            return x;
        }
    "#;
    with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
        state.check_source_file(0);
        let rows: Vec<_> = state
            .diagnostics
            .iter()
            .filter(|d| {
                d.file_name.is_some() && d.category() == tsc_diagnostics::DiagnosticCategory::Error
            })
            .map(|d| d.code())
            .collect();
        assert!(rows.is_empty(), "{rows:?}");
        assert!(state.flow_memo.stats.stores > 0);
        assert!(state.flow_memo.stats.hits > 0);
        assert_eq!(state.flow_memo.stats.disagreements, 0);
        assert!(state.flow_memo.checkpoints.is_empty());
        assert!(state.flow_memo.shadow_hits.is_empty());
        assert_eq!(state.flow_memo.frame_depth(), 0);
        assert!(state.shared_flow.is_empty());
        assert!(state.flow_loop_stack.is_empty());
        assert!(state.reduce_label_overrides.is_empty());
    });
}
