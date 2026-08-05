use super::*;

#[test]
fn snapshot_parser_sorts_targets_and_rejects_duplicate_flags() {
    let parsed = parse_snapshot_args(
        [
            "--slice",
            "p10-tail",
            "--targets",
            "ts-tests/tests/cases/conformance/b.ts,ts-tests/tests/cases/conformance/a.ts",
            "--band",
            "2xxx",
            "--out-dir",
            "/tmp/evidence",
        ]
        .into_iter()
        .map(str::to_owned),
    )
    .unwrap();
    assert_eq!(parsed.slice, "p10-tail");
    assert_eq!(
        parsed.targets,
        [
            "ts-tests/tests/cases/conformance/a.ts",
            "ts-tests/tests/cases/conformance/b.ts"
        ]
    );
    assert!(parse_snapshot_args(
        [
            "--slice",
            "a",
            "--slice",
            "b",
            "--targets",
            "a.ts",
            "--band",
            "all",
            "--out-dir",
            "/tmp/evidence",
        ]
        .into_iter()
        .map(str::to_owned),
    )
    .is_err());
}

#[test]
fn rejects_unsafe_names_and_paths() {
    assert!(validate_slice_name("../slice").is_err());
    assert!(validate_slice_name("phase 10").is_err());
    assert!(validate_slice_name("phase-10.1_tail").is_ok());
    assert!(parse_targets("../outside.ts").is_err());
    assert!(parse_targets("/absolute.ts").is_err());
    assert!(parse_band("23xx").is_err());
}

#[test]
fn wider_gains_are_compared_in_both_scope_views() {
    fn identity(code: u32) -> tsc_conformance::ShadowTierIdentity {
        tsc_conformance::ShadowTierIdentity {
            fixture: "a.ts".to_owned(),
            matrix_key: "default".to_owned(),
            diagnostic: tsc_conformance::T0Key {
                file: Some("/a.ts".to_owned()),
                code,
                line: Some(1),
                col: Some(1),
            },
        }
    }
    let input = DiffIdentityInput {
        all_corpus: TierIdentityInput {
            t1: TierGainInput {
                gained: vec![identity(1), identity(2)],
            },
            t2: TierGainInput {
                gained: vec![identity(1)],
            },
            t3: TierGainInput { gained: vec![] },
        },
        supported: TierIdentityInput {
            t1: TierGainInput {
                gained: vec![identity(1), identity(3)],
            },
            t2: TierGainInput { gained: vec![] },
            t3: TierGainInput { gained: vec![] },
        },
    };
    let target = GainSets {
        all_corpus: [
            BTreeSet::from([identity(1)]),
            BTreeSet::from([identity(1)]),
            BTreeSet::new(),
        ],
        supported: [
            BTreeSet::from([identity(1)]),
            BTreeSet::new(),
            BTreeSet::new(),
        ],
    };
    let counts = input.gains_not_in(&target);
    assert_eq!(counts.all_corpus.t1_gained, 1);
    assert_eq!(counts.supported.t1_gained, 1);
    assert_eq!(counts.all_corpus.t2_gained, 0);
    assert_eq!(counts.supported.t3_gained, 0);
}
