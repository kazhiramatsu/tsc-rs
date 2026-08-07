use std::mem::size_of;
use std::panic::{catch_unwind, AssertUnwindSafe};

use super::{NoEmitActivityCounters, NoEmitCanary};

macro_rules! assert_forbidden {
    ($method:ident) => {{
        let mut canary = NoEmitCanary::new();
        assert!(catch_unwind(AssertUnwindSafe(|| canary.$method())).is_err());
    }};
}

#[test]
fn proof_is_zero_sized_and_every_frozen_activity_panics() {
    assert_eq!(size_of::<NoEmitActivityCounters>(), 0);
    assert!(NoEmitActivityCounters.all_zero());

    assert_forbidden!(construct_emit_resolver);
    assert_forbidden!(initialize_transformers);
    assert_forbidden!(construct_transform_context);
    assert_forbidden!(allocate_emit_side_table);
    assert_forbidden!(construct_printer_writer);
    assert_forbidden!(construct_output_plan);
    assert_forbidden!(create_emit_artifact);
    assert_forbidden!(write_output_sink);
}
