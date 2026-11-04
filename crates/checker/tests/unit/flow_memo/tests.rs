use super::*;
use crate::state::test_support::with_program_state;
use crate::state::ResolutionTarget;
use tsc_types::{CompilerOptions, TypeSystemPropertyName};

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
    });
}
