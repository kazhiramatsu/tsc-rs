use super::{H2ActivityCanary, H2RuntimeSlice};

#[test]
fn h1_profile_has_positive_wiring_and_zero_h2_runtime_activity() {
    let mut canary = H2ActivityCanary::h1_profile();
    canary.construct_emit_session();
    canary.construct_output_plan();
    canary.borrow_emit_resolver();
    canary.construct_script_transformer_list();
    canary.construct_transform_typescript();
    canary.construct_transform_class_fields();
    canary.construct_transform_ecmascript_module();
    canary.construct_transform_context();
    canary.construct_printer();
    canary.create_javascript_artifact();
    canary.attempt_output_sink_write();

    let counters = canary.counters();
    assert_eq!(counters.emit_session_constructions(), 1);
    assert_eq!(counters.output_plan_constructions(), 1);
    assert_eq!(counters.emit_resolver_borrows(), 1);
    assert_eq!(counters.script_transformer_list_constructions(), 1);
    assert_eq!(counters.transform_typescript_constructions(), 1);
    assert_eq!(counters.transform_class_fields_constructions(), 1);
    assert_eq!(counters.transform_ecmascript_module_constructions(), 1);
    assert_eq!(counters.transform_context_constructions(), 1);
    assert_eq!(counters.printer_constructions(), 1);
    assert_eq!(counters.javascript_artifact_creations(), 1);
    assert_eq!(counters.output_sink_write_attempts(), 1);
    assert_eq!(counters.output_sink_failures(), 0);
    assert!(counters.h2_runtime_is_zero());
}

#[test]
fn every_h2_runtime_slice_fails_closed_before_admission() {
    for slice in H2RuntimeSlice::ALL {
        let result = std::panic::catch_unwind(|| {
            H2ActivityCanary::h1_profile().observe_runtime_slice(slice)
        });
        assert!(result.is_err(), "{} did not fail closed", slice.name());
    }
}

#[test]
fn h2_1b_profile_admits_only_the_two_completed_runtime_slices() {
    let mut canary = H2ActivityCanary::h2_1b_profile();
    canary.observe_runtime_slice(H2RuntimeSlice::H2_1a);
    canary.observe_runtime_slice(H2RuntimeSlice::H2_1b);
    let counters = canary.counters();
    assert_eq!(counters.runtime_slice(H2RuntimeSlice::H2_1a), 1);
    assert_eq!(counters.runtime_slice(H2RuntimeSlice::H2_1b), 1);

    for slice in H2RuntimeSlice::ALL {
        if matches!(slice, H2RuntimeSlice::H2_1a | H2RuntimeSlice::H2_1b) {
            continue;
        }
        let result = std::panic::catch_unwind(|| {
            H2ActivityCanary::h2_1b_profile().observe_runtime_slice(slice)
        });
        assert!(result.is_err(), "{} did not fail closed", slice.name());
    }
}
