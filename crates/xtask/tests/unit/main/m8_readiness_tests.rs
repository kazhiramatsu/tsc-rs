use super::*;

fn family(
    name: &str,
    owner: &str,
    supported_false_negative: usize,
    canaries_passed: usize,
    canaries_total: usize,
) -> M8FamilyReadiness {
    M8FamilyReadiness {
        name: name.to_owned(),
        owner: owner.to_owned(),
        supported_false_negative,
        canaries_passed,
        canaries_total,
    }
}

#[test]
fn aggregate_m7_gate_cannot_hide_a_red_owned_family() {
    let mut gates = vec![M8ReadinessGate {
        name: "m7-gate".to_owned(),
        ready: true,
        detail: "T0=99% FP=0 T1-ratchet-active=true".to_owned(),
    }];
    let report = M8FamiliesReport {
        schema: 1,
        map_status: "frozen".to_owned(),
        families: vec![
            family("checker-grammar", "M7 8.1", 1, 3, 4),
            family("m8-tail", "M8", 9, 0, 1),
        ],
    };

    gates.push(m7_family_readiness_gate(&report));

    assert!(gates[0].ready);
    assert!(!gates[1].ready);
    assert_eq!(gates[1].name, "m7-family-rollup");
    assert!(gates[1]
        .detail
        .contains("checker-grammar(FN=1,canaries=3/4)"));
    assert!(!gates[1].detail.contains("m8-tail"));
}

#[test]
fn m7_family_gate_requires_a_frozen_nonempty_complete_rollup() {
    let complete = vec![
        family("checker-grammar", "M7 8.1", 0, 4, 4),
        family("unused", "M7 8.3+8.4", 0, 3, 3),
    ];
    let ready = m7_family_readiness_gate(&M8FamiliesReport {
        schema: 1,
        map_status: "frozen".to_owned(),
        families: complete,
    });
    assert!(ready.ready);
    assert_eq!(ready.detail, "map-status=frozen complete=2/2");

    let draft = m7_family_readiness_gate(&M8FamiliesReport {
        schema: 1,
        map_status: "draft".to_owned(),
        families: vec![family("checker-grammar", "M7 8.1", 0, 4, 4)],
    });
    assert!(!draft.ready);

    let empty = m7_family_readiness_gate(&M8FamiliesReport {
        schema: 1,
        map_status: "frozen".to_owned(),
        families: vec![family("m8-tail", "M8", 0, 1, 1)],
    });
    assert!(!empty.ready);
}
