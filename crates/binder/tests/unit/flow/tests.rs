use super::*;

#[test]
fn label_lifecycle_collapse_and_share() {
    let mut arena = FlowArena::default();
    let unreachable = arena.create_flow_node(FlowFlags::UNREACHABLE, FlowPayload::None, None);
    let start = arena.create_flow_node(FlowFlags::START, FlowPayload::None, None);

    // No antecedents ⇒ unreachable.
    let empty = arena.create_branch_label();
    assert_eq!(arena.finish_flow_label(empty, unreachable), unreachable);

    // One antecedent ⇒ collapses to it.
    let single = arena.create_branch_label();
    arena.add_antecedent(single, start);
    assert_eq!(arena.finish_flow_label(single, unreachable), start);
    assert!(arena.flow(start).flags.intersects(FlowFlags::REFERENCED));
    assert!(!arena.flow(start).flags.intersects(FlowFlags::SHARED));

    // Unreachable antecedents and duplicates are dropped.
    let multi = arena.create_branch_label();
    arena.add_antecedent(multi, unreachable);
    assert!(arena.label_is_empty(multi));
    arena.add_antecedent(multi, start);
    arena.add_antecedent(multi, start);
    assert_eq!(arena.label_items(multi).len(), 1);
    // Second REFERENCE marks Shared.
    assert!(arena.flow(start).flags.intersects(FlowFlags::SHARED));

    let other = arena.create_flow_node(FlowFlags::START, FlowPayload::None, None);
    arena.add_antecedent(multi, other);
    assert_eq!(arena.finish_flow_label(multi, unreachable), multi);
}

#[test]
fn a_frozen_arena_reads_every_kind_as_it_was_built() {
    let mut arena = FlowArena::default();
    let unreachable = arena.create_flow_node(FlowFlags::UNREACHABLE, FlowPayload::None, None);
    let start = arena.create_flow_node(FlowFlags::START, FlowPayload::None, None);
    let node = NodeId::new(7);
    let assignment =
        arena.create_flow_node(FlowFlags::ASSIGNMENT, FlowPayload::Node(node), Some(start));
    let switch_statement = NodeId::new(11);
    let clause = arena.create_flow_node(
        FlowFlags::SWITCH_CLAUSE,
        FlowPayload::SwitchClause {
            switch_statement,
            clause_start: 2,
            clause_end: 5,
        },
        Some(assignment),
    );
    let many = arena.create_branch_label();
    arena.add_antecedent(many, start);
    arena.add_antecedent(many, assignment);
    arena.add_antecedent(many, clause);
    let one = arena.create_loop_label();
    arena.add_antecedent(one, start);
    let replaced = arena.create_branch_label();
    arena.add_antecedent(replaced, start);
    arena.set_label_antecedents(replaced, vec![clause, assignment]);
    let reduce = arena.create_reduce_label(many, vec![assignment, start], clause);
    let empty_reduce = arena.create_reduce_label(one, Vec::new(), start);
    // The last label is empty: its span ends the pool.
    let empty = arena.create_branch_label();
    let unfrozen = arena.clone();
    let copy = arena.frozen();
    arena.freeze();
    for frozen in [&arena, &copy] {
        assert_eq!(frozen.payload(unreachable), FlowData::None);
        assert_eq!(frozen.payload(start), FlowData::None);
        assert_eq!(frozen.payload(assignment), FlowData::Node(node));
        assert_eq!(frozen.flow(assignment).payload_node(), Some(node));
        assert_eq!(frozen.antecedents(assignment), &[start]);
        assert_eq!(
            frozen.payload(clause),
            FlowData::SwitchClause {
                switch_statement,
                clause_start: 2,
                clause_end: 5,
            }
        );
        assert_eq!(frozen.flow(clause).payload_node(), None);
        assert_eq!(frozen.antecedents(clause), &[assignment]);
        assert_eq!(frozen.antecedents(many), &[start, assignment, clause]);
        assert_eq!(
            frozen.payload(many),
            FlowData::Label {
                antecedents: &[start, assignment, clause],
            }
        );
        assert_eq!(frozen.antecedents(one), &[start]);
        assert_eq!(frozen.antecedents(replaced), &[clause, assignment]);
        assert_eq!(
            frozen.payload(reduce),
            FlowData::ReduceLabel {
                target: many,
                antecedents: &[assignment, start],
            }
        );
        assert_eq!(frozen.antecedents(reduce), &[clause]);
        assert_eq!(
            frozen.payload(empty_reduce),
            FlowData::ReduceLabel {
                target: one,
                antecedents: &[],
            }
        );
        assert!(frozen.antecedents(empty).is_empty());
    }
    // The frozen copy left the arena it was taken from bindable.
    let mut unfrozen = unfrozen;
    unfrozen.add_antecedent(empty, start);
    assert_eq!(unfrozen.label_items(empty), vec![start]);
}

#[test]
fn freeze_sizes_the_pool_from_live_cells_and_reduce_headers() {
    let mut arena = FlowArena::for_bind();
    let start = arena.create_flow_node(FlowFlags::START, FlowPayload::None, None);
    let replaced = arena.create_branch_label();
    arena.add_antecedent(replaced, start);
    arena.add_antecedent(replaced, start);
    arena.set_label_antecedents(replaced, vec![start, start]);
    arena.set_label_antecedents(replaced, Vec::new());
    arena.add_antecedent(replaced, start);
    arena.create_reduce_label(replaced, Vec::new(), start);
    arena.create_reduce_label(replaced, vec![start, start], start);

    assert_eq!(arena.cells.len(), 6);
    assert_eq!(arena.abandoned_cells, 3);
    assert_eq!(arena.reduce_label_headers, 2);
    arena.freeze();
    assert_eq!(
        arena.pool.as_ref(),
        &[start, FlowId::new(0), FlowId::new(2), start, start]
    );

    let mut next = FlowArena::for_bind();
    assert!(next.cells.is_empty());
    next.freeze();
    assert!(next.pool.is_empty());
}
