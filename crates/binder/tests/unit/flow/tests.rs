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
    assert!(arena.flow(multi).antecedent.is_empty());
    arena.add_antecedent(multi, start);
    arena.add_antecedent(multi, start);
    assert_eq!(arena.flow(multi).antecedent.len(), 1);
    // Second REFERENCE marks Shared.
    assert!(arena.flow(start).flags.intersects(FlowFlags::SHARED));

    let other = arena.create_flow_node(FlowFlags::START, FlowPayload::None, None);
    arena.add_antecedent(multi, other);
    assert_eq!(arena.finish_flow_label(multi, unreachable), multi);
}
