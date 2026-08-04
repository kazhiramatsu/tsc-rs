use std::collections::HashSet;

use tsc_types::{CompilerOptionNumber, CompilerOptions};

#[test]
fn compiler_option_number_has_stable_comparison_equivalent_identity() {
    assert_eq!(
        CompilerOptionNumber::new(-0.0),
        CompilerOptionNumber::new(0.0)
    );
    assert_eq!(
        CompilerOptionNumber::new(f64::NAN),
        CompilerOptionNumber::new(f64::from_bits(0x7ff8_0000_0000_0001))
    );

    let values = HashSet::from([
        CompilerOptionNumber::new(-0.0),
        CompilerOptionNumber::new(0.0),
        CompilerOptionNumber::new(f64::NAN),
        CompilerOptionNumber::new(f64::from_bits(0x7ff8_0000_0000_0001)),
        CompilerOptionNumber::new(f64::INFINITY),
        CompilerOptionNumber::new(f64::NEG_INFINITY),
        CompilerOptionNumber::new(1.5),
    ]);
    assert_eq!(values.len(), 5);
}

#[test]
fn compiler_option_number_round_trips_the_javascript_number_domain() {
    for value in [-1.5, -0.0, 0.0, 1.5, f64::NEG_INFINITY, f64::INFINITY] {
        let actual = CompilerOptionNumber::new(value).value();
        if value == 0.0 {
            assert_eq!(actual.to_bits(), 0.0_f64.to_bits());
        } else {
            assert_eq!(actual.to_bits(), value.to_bits());
        }
    }
    assert!(CompilerOptionNumber::new(f64::NAN).value().is_nan());
}

#[test]
fn node_module_depth_comparisons_keep_fraction_and_nan_ordering() {
    let with_maximum = |maximum| CompilerOptions {
        max_node_module_js_depth: Some(CompilerOptionNumber::new(maximum)),
        ..CompilerOptions::default()
    };

    let fractional = with_maximum(1.5);
    assert!(!fractional.node_modules_depth_exceeds_limit(1));
    assert!(fractional.node_modules_depth_exceeds_limit(2));
    assert!(fractional.node_modules_depth_below_limit(1));
    assert!(!fractional.node_modules_depth_below_limit(2));

    let nan = with_maximum(f64::NAN);
    assert!(!nan.node_modules_depth_exceeds_limit(256));
    assert!(!nan.node_modules_depth_below_limit(0));
}
