use tsc_ci_runner::{BoundedQueue, EffectPhase, InfraError, ResourceClaimV1, ResourcePolicyV1};

#[test]
fn resource_policy_admits_only_within_all_child_and_control_ceilings() {
    let policy =
        ResourcePolicyV1::new(100, 1024, 50, 512, 256, 2, 3).expect("positive resource policy");
    assert!(ResourceClaimV1::new(50, 512, 256, 2, 3).admitted_by(policy));
    assert!(!ResourceClaimV1::new(51, 512, 256, 2, 3).admitted_by(policy));
    assert!(!ResourceClaimV1::new(50, 512, 256, 3, 3).admitted_by(policy));
    assert_eq!(
        ResourcePolicyV1::new(0, 1024, 50, 512, 256, 2, 3),
        Err(InfraError::Quota {
            phase: EffectPhase::Acquire,
        })
    );
}

#[test]
fn bounded_queue_is_fifo_and_refuses_overflow() {
    let mut queue = BoundedQueue::new(2).expect("positive queue limit");
    queue.push(1).expect("first value");
    queue.push(2).expect("second value");
    assert_eq!(
        queue.push(3),
        Err(InfraError::Quota {
            phase: EffectPhase::Execute,
        })
    );
    assert_eq!(queue.pop(), Some(1));
    assert_eq!(queue.pop(), Some(2));
    assert!(queue.is_empty());
}

#[test]
fn runner_effect_surface_has_no_async_or_live_cache_entry() {
    let source = include_str!("../../src/lib.rs");
    assert!(!source.contains("async"));
    assert!(!source.contains("Runner::evaluate"));
    assert!(!source.contains("CasBackend"));
    assert!(!source.contains("ExactCacheBackend"));
}
