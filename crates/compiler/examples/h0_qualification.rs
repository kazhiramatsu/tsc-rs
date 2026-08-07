//! Allocation-observed H0 command runner used only by L0/H1 qualification.
//!
//! The production `tsc-rs` binary keeps the system allocator directly and
//! contains no measurement branch. This example links the same `run_cli`
//! entry under `stats_alloc`, takes one region snapshot around the command,
//! and emits one machine-readable observation after the command is complete.

use std::alloc::System;

use serde_json::json;
use stats_alloc::{Region, StatsAlloc, INSTRUMENTED_SYSTEM};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

fn main() {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    let region = Region::new(GLOBAL);
    let output = tsc_compiler::run_cli(&arguments);
    let stats = region.change();

    if output.exit_code() != 0 || !output.stdout().is_empty() || !output.stderr().is_empty() {
        eprintln!(
            "qualification workload failed: exit={} stdout={:?} stderr={:?}",
            output.exit_code(),
            output.stdout(),
            output.stderr()
        );
        std::process::exit(1);
    }

    let observation = json!({
        "schema": 2,
        "exit_code": output.exit_code(),
        "allocations": stats.allocations,
        "deallocations": stats.deallocations,
        "reallocations": stats.reallocations,
        "bytes_allocated": stats.bytes_allocated,
        "bytes_deallocated": stats.bytes_deallocated,
        "bytes_reallocated": stats.bytes_reallocated,
        "work": {
            "parsed_documents": output.work_counters().parsed_documents(),
            "bound_documents": output.work_counters().bound_documents(),
            "full_text_copies": output.work_counters().full_text_copies(),
            "full_text_bytes_copied": output.work_counters().full_text_bytes_copied(),
        },
        "h1_no_emit": {
            "emit_resolver_constructions": output.no_emit_activity().emit_resolver_constructions(),
            "transformer_initializations": output.no_emit_activity().transformer_initializations(),
            "transform_context_constructions": output.no_emit_activity().transform_context_constructions(),
            "emit_side_table_allocations": output.no_emit_activity().emit_side_table_allocations(),
            "printer_writer_constructions": output.no_emit_activity().printer_writer_constructions(),
            "output_plan_constructions": output.no_emit_activity().output_plan_constructions(),
            "emit_artifact_creations": output.no_emit_activity().emit_artifact_creations(),
            "output_sink_writes": output.no_emit_activity().output_sink_writes(),
        },
    });
    println!("{observation}");
}
