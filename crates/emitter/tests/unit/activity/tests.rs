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

#[test]
fn h2_1c_profile_admits_only_the_three_completed_runtime_slices() {
    let mut canary = H2ActivityCanary::h2_1c_profile();
    for slice in [
        H2RuntimeSlice::H2_1a,
        H2RuntimeSlice::H2_1b,
        H2RuntimeSlice::H2_1c,
    ] {
        canary.observe_runtime_slice(slice);
        assert_eq!(canary.counters().runtime_slice(slice), 1);
    }

    for slice in H2RuntimeSlice::ALL {
        if matches!(
            slice,
            H2RuntimeSlice::H2_1a | H2RuntimeSlice::H2_1b | H2RuntimeSlice::H2_1c
        ) {
            continue;
        }
        let result = std::panic::catch_unwind(|| {
            H2ActivityCanary::h2_1c_profile().observe_runtime_slice(slice)
        });
        assert!(result.is_err(), "{} did not fail closed", slice.name());
    }
}

#[test]
fn h2_1d_profile_admits_only_the_four_completed_runtime_slices() {
    let mut canary = H2ActivityCanary::h2_1d_profile();
    for slice in [
        H2RuntimeSlice::H2_1a,
        H2RuntimeSlice::H2_1b,
        H2RuntimeSlice::H2_1c,
        H2RuntimeSlice::H2_1d,
    ] {
        canary.observe_runtime_slice(slice);
        assert_eq!(canary.counters().runtime_slice(slice), 1);
    }

    for slice in H2RuntimeSlice::ALL {
        if matches!(
            slice,
            H2RuntimeSlice::H2_1a
                | H2RuntimeSlice::H2_1b
                | H2RuntimeSlice::H2_1c
                | H2RuntimeSlice::H2_1d
        ) {
            continue;
        }
        let result = std::panic::catch_unwind(|| {
            H2ActivityCanary::h2_1d_profile().observe_runtime_slice(slice)
        });
        assert!(result.is_err(), "{} did not fail closed", slice.name());
    }
}

#[test]
fn h2_1e_profile_admits_only_the_five_completed_runtime_slices() {
    let mut canary = H2ActivityCanary::h2_1e_profile();
    for slice in [
        H2RuntimeSlice::H2_1a,
        H2RuntimeSlice::H2_1b,
        H2RuntimeSlice::H2_1c,
        H2RuntimeSlice::H2_1d,
        H2RuntimeSlice::H2_1e,
    ] {
        canary.observe_runtime_slice(slice);
        assert_eq!(canary.counters().runtime_slice(slice), 1);
    }

    for slice in H2RuntimeSlice::ALL {
        if matches!(
            slice,
            H2RuntimeSlice::H2_1a
                | H2RuntimeSlice::H2_1b
                | H2RuntimeSlice::H2_1c
                | H2RuntimeSlice::H2_1d
                | H2RuntimeSlice::H2_1e
        ) {
            continue;
        }
        let result = std::panic::catch_unwind(|| {
            H2ActivityCanary::h2_1e_profile().observe_runtime_slice(slice)
        });
        assert!(result.is_err(), "{} did not fail closed", slice.name());
    }
}

#[test]
fn h2_2a_profile_admits_only_the_six_completed_runtime_slices() {
    let mut canary = H2ActivityCanary::h2_2a_profile();
    for slice in [
        H2RuntimeSlice::H2_1a,
        H2RuntimeSlice::H2_1b,
        H2RuntimeSlice::H2_1c,
        H2RuntimeSlice::H2_1d,
        H2RuntimeSlice::H2_1e,
        H2RuntimeSlice::H2_2a,
    ] {
        canary.observe_runtime_slice(slice);
        assert_eq!(canary.counters().runtime_slice(slice), 1);
    }

    for slice in H2RuntimeSlice::ALL {
        if matches!(
            slice,
            H2RuntimeSlice::H2_1a
                | H2RuntimeSlice::H2_1b
                | H2RuntimeSlice::H2_1c
                | H2RuntimeSlice::H2_1d
                | H2RuntimeSlice::H2_1e
                | H2RuntimeSlice::H2_2a
        ) {
            continue;
        }
        let result = std::panic::catch_unwind(|| {
            H2ActivityCanary::h2_2a_profile().observe_runtime_slice(slice)
        });
        assert!(result.is_err(), "{} did not fail closed", slice.name());
    }
}

#[test]
fn h2_2b_profile_admits_only_the_seven_completed_runtime_slices() {
    let mut canary = H2ActivityCanary::h2_2b_profile();
    for slice in [
        H2RuntimeSlice::H2_1a,
        H2RuntimeSlice::H2_1b,
        H2RuntimeSlice::H2_1c,
        H2RuntimeSlice::H2_1d,
        H2RuntimeSlice::H2_1e,
        H2RuntimeSlice::H2_2a,
        H2RuntimeSlice::H2_2b,
    ] {
        canary.observe_runtime_slice(slice);
        assert_eq!(canary.counters().runtime_slice(slice), 1);
    }

    for slice in H2RuntimeSlice::ALL {
        if matches!(
            slice,
            H2RuntimeSlice::H2_1a
                | H2RuntimeSlice::H2_1b
                | H2RuntimeSlice::H2_1c
                | H2RuntimeSlice::H2_1d
                | H2RuntimeSlice::H2_1e
                | H2RuntimeSlice::H2_2a
                | H2RuntimeSlice::H2_2b
        ) {
            continue;
        }
        let result = std::panic::catch_unwind(|| {
            H2ActivityCanary::h2_2b_profile().observe_runtime_slice(slice)
        });
        assert!(result.is_err(), "{} did not fail closed", slice.name());
    }
}

#[test]
fn h2_2c_profile_admits_only_the_eight_completed_runtime_slices() {
    let mut canary = H2ActivityCanary::h2_2c_profile();
    for slice in [
        H2RuntimeSlice::H2_1a,
        H2RuntimeSlice::H2_1b,
        H2RuntimeSlice::H2_1c,
        H2RuntimeSlice::H2_1d,
        H2RuntimeSlice::H2_1e,
        H2RuntimeSlice::H2_2a,
        H2RuntimeSlice::H2_2b,
        H2RuntimeSlice::H2_2c,
    ] {
        canary.observe_runtime_slice(slice);
        assert_eq!(canary.counters().runtime_slice(slice), 1);
    }

    for slice in H2RuntimeSlice::ALL {
        if matches!(
            slice,
            H2RuntimeSlice::H2_1a
                | H2RuntimeSlice::H2_1b
                | H2RuntimeSlice::H2_1c
                | H2RuntimeSlice::H2_1d
                | H2RuntimeSlice::H2_1e
                | H2RuntimeSlice::H2_2a
                | H2RuntimeSlice::H2_2b
                | H2RuntimeSlice::H2_2c
        ) {
            continue;
        }
        let result = std::panic::catch_unwind(|| {
            H2ActivityCanary::h2_2c_profile().observe_runtime_slice(slice)
        });
        assert!(result.is_err(), "{} did not fail closed", slice.name());
    }
}

#[test]
fn h2_2d_profile_admits_only_the_nine_completed_runtime_slices() {
    let mut canary = H2ActivityCanary::h2_2d_profile();
    for slice in [
        H2RuntimeSlice::H2_1a,
        H2RuntimeSlice::H2_1b,
        H2RuntimeSlice::H2_1c,
        H2RuntimeSlice::H2_1d,
        H2RuntimeSlice::H2_1e,
        H2RuntimeSlice::H2_2a,
        H2RuntimeSlice::H2_2b,
        H2RuntimeSlice::H2_2c,
        H2RuntimeSlice::H2_2d,
    ] {
        canary.observe_runtime_slice(slice);
        assert_eq!(canary.counters().runtime_slice(slice), 1);
    }

    for slice in H2RuntimeSlice::ALL {
        if matches!(
            slice,
            H2RuntimeSlice::H2_1a
                | H2RuntimeSlice::H2_1b
                | H2RuntimeSlice::H2_1c
                | H2RuntimeSlice::H2_1d
                | H2RuntimeSlice::H2_1e
                | H2RuntimeSlice::H2_2a
                | H2RuntimeSlice::H2_2b
                | H2RuntimeSlice::H2_2c
                | H2RuntimeSlice::H2_2d
        ) {
            continue;
        }
        let result = std::panic::catch_unwind(|| {
            H2ActivityCanary::h2_2d_profile().observe_runtime_slice(slice)
        });
        assert!(result.is_err(), "{} did not fail closed", slice.name());
    }
}

#[test]
fn h2_3a_profile_admits_only_the_ten_completed_runtime_slices() {
    let mut canary = H2ActivityCanary::h2_3a_profile();
    for slice in [
        H2RuntimeSlice::H2_1a,
        H2RuntimeSlice::H2_1b,
        H2RuntimeSlice::H2_1c,
        H2RuntimeSlice::H2_1d,
        H2RuntimeSlice::H2_1e,
        H2RuntimeSlice::H2_2a,
        H2RuntimeSlice::H2_2b,
        H2RuntimeSlice::H2_2c,
        H2RuntimeSlice::H2_2d,
        H2RuntimeSlice::H2_3a,
    ] {
        canary.observe_runtime_slice(slice);
        assert_eq!(canary.counters().runtime_slice(slice), 1);
    }

    for slice in H2RuntimeSlice::ALL {
        if matches!(
            slice,
            H2RuntimeSlice::H2_1a
                | H2RuntimeSlice::H2_1b
                | H2RuntimeSlice::H2_1c
                | H2RuntimeSlice::H2_1d
                | H2RuntimeSlice::H2_1e
                | H2RuntimeSlice::H2_2a
                | H2RuntimeSlice::H2_2b
                | H2RuntimeSlice::H2_2c
                | H2RuntimeSlice::H2_2d
                | H2RuntimeSlice::H2_3a
        ) {
            continue;
        }
        let result = std::panic::catch_unwind(|| {
            H2ActivityCanary::h2_3a_profile().observe_runtime_slice(slice)
        });
        assert!(result.is_err(), "{} did not fail closed", slice.name());
    }
}

#[test]
fn h2_3b_profile_admits_only_the_eleven_completed_runtime_slices() {
    let mut canary = H2ActivityCanary::h2_3b_profile();
    for slice in [
        H2RuntimeSlice::H2_1a,
        H2RuntimeSlice::H2_1b,
        H2RuntimeSlice::H2_1c,
        H2RuntimeSlice::H2_1d,
        H2RuntimeSlice::H2_1e,
        H2RuntimeSlice::H2_2a,
        H2RuntimeSlice::H2_2b,
        H2RuntimeSlice::H2_2c,
        H2RuntimeSlice::H2_2d,
        H2RuntimeSlice::H2_3a,
        H2RuntimeSlice::H2_3b,
    ] {
        canary.observe_runtime_slice(slice);
        assert_eq!(canary.counters().runtime_slice(slice), 1);
    }

    for slice in H2RuntimeSlice::ALL {
        if matches!(
            slice,
            H2RuntimeSlice::H2_1a
                | H2RuntimeSlice::H2_1b
                | H2RuntimeSlice::H2_1c
                | H2RuntimeSlice::H2_1d
                | H2RuntimeSlice::H2_1e
                | H2RuntimeSlice::H2_2a
                | H2RuntimeSlice::H2_2b
                | H2RuntimeSlice::H2_2c
                | H2RuntimeSlice::H2_2d
                | H2RuntimeSlice::H2_3a
                | H2RuntimeSlice::H2_3b
        ) {
            continue;
        }
        let result = std::panic::catch_unwind(|| {
            H2ActivityCanary::h2_3b_profile().observe_runtime_slice(slice)
        });
        assert!(result.is_err(), "{} did not fail closed", slice.name());
    }
}

#[test]
fn h2_3c_profile_admits_only_the_twelve_completed_runtime_slices() {
    let mut canary = H2ActivityCanary::h2_3c_profile();
    for slice in [
        H2RuntimeSlice::H2_1a,
        H2RuntimeSlice::H2_1b,
        H2RuntimeSlice::H2_1c,
        H2RuntimeSlice::H2_1d,
        H2RuntimeSlice::H2_1e,
        H2RuntimeSlice::H2_2a,
        H2RuntimeSlice::H2_2b,
        H2RuntimeSlice::H2_2c,
        H2RuntimeSlice::H2_2d,
        H2RuntimeSlice::H2_3a,
        H2RuntimeSlice::H2_3b,
        H2RuntimeSlice::H2_3c,
    ] {
        canary.observe_runtime_slice(slice);
        assert_eq!(canary.counters().runtime_slice(slice), 1);
    }

    for slice in H2RuntimeSlice::ALL {
        if matches!(
            slice,
            H2RuntimeSlice::H2_1a
                | H2RuntimeSlice::H2_1b
                | H2RuntimeSlice::H2_1c
                | H2RuntimeSlice::H2_1d
                | H2RuntimeSlice::H2_1e
                | H2RuntimeSlice::H2_2a
                | H2RuntimeSlice::H2_2b
                | H2RuntimeSlice::H2_2c
                | H2RuntimeSlice::H2_2d
                | H2RuntimeSlice::H2_3a
                | H2RuntimeSlice::H2_3b
                | H2RuntimeSlice::H2_3c
        ) {
            continue;
        }
        let result = std::panic::catch_unwind(|| {
            H2ActivityCanary::h2_3c_profile().observe_runtime_slice(slice)
        });
        assert!(result.is_err(), "{} did not fail closed", slice.name());
    }
}

#[test]
fn h2_3d_profile_admits_only_the_thirteen_completed_runtime_slices() {
    let mut canary = H2ActivityCanary::h2_3d_profile();
    for slice in [
        H2RuntimeSlice::H2_1a,
        H2RuntimeSlice::H2_1b,
        H2RuntimeSlice::H2_1c,
        H2RuntimeSlice::H2_1d,
        H2RuntimeSlice::H2_1e,
        H2RuntimeSlice::H2_2a,
        H2RuntimeSlice::H2_2b,
        H2RuntimeSlice::H2_2c,
        H2RuntimeSlice::H2_2d,
        H2RuntimeSlice::H2_3a,
        H2RuntimeSlice::H2_3b,
        H2RuntimeSlice::H2_3c,
        H2RuntimeSlice::H2_3d,
    ] {
        canary.observe_runtime_slice(slice);
        assert_eq!(canary.counters().runtime_slice(slice), 1);
    }

    for slice in H2RuntimeSlice::ALL {
        if slice <= H2RuntimeSlice::H2_3d {
            continue;
        }
        let result = std::panic::catch_unwind(|| {
            H2ActivityCanary::h2_3d_profile().observe_runtime_slice(slice)
        });
        assert!(result.is_err(), "{} did not fail closed", slice.name());
    }
}

#[test]
fn h2_4a_profile_admits_only_the_fourteen_completed_runtime_slices() {
    let mut canary = H2ActivityCanary::h2_4a_profile();
    for slice in H2RuntimeSlice::ALL {
        if slice <= H2RuntimeSlice::H2_4a {
            canary.observe_runtime_slice(slice);
            assert_eq!(canary.counters().runtime_slice(slice), 1);
        } else {
            let result = std::panic::catch_unwind(|| {
                H2ActivityCanary::h2_4a_profile().observe_runtime_slice(slice)
            });
            assert!(result.is_err(), "{} did not fail closed", slice.name());
        }
    }
}

#[test]
fn h2_4b_profile_admits_only_the_fifteen_completed_runtime_slices() {
    let mut canary = H2ActivityCanary::h2_4b_profile();
    for slice in H2RuntimeSlice::ALL {
        if slice <= H2RuntimeSlice::H2_4b {
            canary.observe_runtime_slice(slice);
            assert_eq!(canary.counters().runtime_slice(slice), 1);
        } else {
            let result = std::panic::catch_unwind(|| {
                H2ActivityCanary::h2_4b_profile().observe_runtime_slice(slice)
            });
            assert!(result.is_err(), "{} did not fail closed", slice.name());
        }
    }
}

#[test]
fn h2_5f_profile_admits_only_the_twenty_one_completed_runtime_slices() {
    let mut canary = H2ActivityCanary::h2_5f_profile();
    for slice in H2RuntimeSlice::ALL {
        if slice <= H2RuntimeSlice::H2_5f {
            canary.observe_runtime_slice(slice);
            assert_eq!(canary.counters().runtime_slice(slice), 1);
        } else {
            let result = std::panic::catch_unwind(|| {
                H2ActivityCanary::h2_5f_profile().observe_runtime_slice(slice)
            });
            assert!(result.is_err(), "{} did not fail closed", slice.name());
        }
    }
}

#[test]
fn h2_5g_profile_admits_only_the_twenty_two_completed_runtime_slices() {
    let mut canary = H2ActivityCanary::h2_5g_profile();
    for slice in H2RuntimeSlice::ALL {
        if slice <= H2RuntimeSlice::H2_5g {
            canary.observe_runtime_slice(slice);
            assert_eq!(canary.counters().runtime_slice(slice), 1);
        } else {
            let result = std::panic::catch_unwind(|| {
                H2ActivityCanary::h2_5g_profile().observe_runtime_slice(slice)
            });
            assert!(result.is_err(), "{} did not fail closed", slice.name());
        }
    }
}
