use tsc_ci_testkit::{composite_fixture, flat_fixture};

#[test]
fn two_structurally_different_shapes_use_the_same_core_records() {
    let flat = flat_fixture();
    let composite = composite_fixture();
    assert_eq!(flat.len(), 1);
    assert_eq!(composite.len(), 2);
    assert_eq!(flat[0].class(), tsc_ci_core::NodeClass::Input);
    assert_eq!(composite[1].dependencies().len(), 1);
}

#[test]
fn testkit_has_no_repository_semantic_literal() {
    let source = include_str!("../src/lib.rs");
    for forbidden in ["tsc-rs", "TypeScript", "Cargo", "compiler", "oracle", "H2"] {
        assert!(!source.contains(forbidden), "testkit literal: {forbidden}");
    }
}
