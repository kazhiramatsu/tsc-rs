use super::*;

fn identity(code: u32) -> ShadowTierIdentity {
    ShadowTierIdentity {
        fixture: "tests/cases/conformance/a.ts".to_owned(),
        matrix_key: "default".to_owned(),
        diagnostic: T0Key {
            file: Some("/a.ts".to_owned()),
            code,
            line: Some(1),
            col: Some(code),
        },
    }
}

fn observation(
    universe: &str,
    t1: &[ShadowTierIdentity],
    t2: &[ShadowTierIdentity],
    t3: &[ShadowTierIdentity],
) -> ShadowTierObservation {
    ShadowTierObservation {
        schema: OBSERVATION_SCHEMA,
        oracle_universe_sha256: universe.to_owned(),
        t1_matched: t1.to_vec(),
        t2_matched: t2.to_vec(),
        t3_matched: t3.to_vec(),
    }
}

fn input(all: ShadowTierObservation, supported: ShadowTierObservation) -> ConformanceDiffInput {
    ConformanceDiffInput {
        band: "2xxx".to_owned(),
        shadow_t1_matched: all.t1_matched.len(),
        shadow_t2_matched: all.t2_matched.len(),
        shadow_t3_matched: all.t3_matched.len(),
        supported_t1_matched: supported.t1_matched.len(),
        supported_t2_matched: supported.t2_matched.len(),
        supported_t3_matched: supported.t3_matched.len(),
        shadow_tier_identities: all,
        supported_shadow_tier_identities: supported,
    }
}

#[test]
fn equal_counts_cannot_hide_identity_swaps() {
    let a = identity(2322);
    let b = identity(2323);
    let c = identity(2324);
    let before = observation(
        "same-universe",
        &[a.clone(), b.clone()],
        &[a.clone(), b.clone()],
        std::slice::from_ref(&a),
    );
    let after = observation(
        "same-universe",
        &[b.clone(), c.clone()],
        &[b.clone(), c.clone()],
        std::slice::from_ref(&b),
    );

    let report =
        diff_observations(input(before.clone(), before), input(after.clone(), after)).unwrap();
    assert_eq!(report.all_corpus.t1.before_matched, 2);
    assert_eq!(report.all_corpus.t1.after_matched, 2);
    assert_eq!(
        report.all_corpus.t1.lost.as_slice(),
        std::slice::from_ref(&a)
    );
    assert_eq!(
        report.all_corpus.t1.gained.as_slice(),
        std::slice::from_ref(&c)
    );
    assert_eq!(
        report.all_corpus.t2.lost.as_slice(),
        std::slice::from_ref(&a)
    );
    assert_eq!(
        report.all_corpus.t2.gained.as_slice(),
        std::slice::from_ref(&c)
    );
    assert_eq!(
        report.all_corpus.t3.lost.as_slice(),
        std::slice::from_ref(&a)
    );
    assert_eq!(
        report.all_corpus.t3.gained.as_slice(),
        std::slice::from_ref(&b)
    );
}

#[test]
fn all_corpus_universe_mismatch_is_rejected() {
    let a = identity(2322);
    let before = observation(
        "before-universe",
        std::slice::from_ref(&a),
        std::slice::from_ref(&a),
        std::slice::from_ref(&a),
    );
    let after = observation(
        "after-universe",
        std::slice::from_ref(&a),
        std::slice::from_ref(&a),
        std::slice::from_ref(&a),
    );
    let error = diff_observations(input(before.clone(), before), input(after.clone(), after))
        .unwrap_err()
        .to_string();
    assert!(error.contains("different all-corpus oracle universes"));
}

#[test]
fn universe_hash_is_order_independent_and_multiplicity_sensitive() {
    let a = b"a".to_vec();
    let b = b"b".to_vec();
    assert_eq!(
        oracle_universe_sha256(vec![a.clone(), b.clone()]),
        oracle_universe_sha256(vec![b.clone(), a.clone()])
    );
    assert_ne!(
        oracle_universe_sha256(vec![a.clone(), b.clone()]),
        oracle_universe_sha256(vec![a.clone(), b.clone(), b])
    );
}
