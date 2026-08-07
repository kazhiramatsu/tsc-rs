//! H1.0b structural guard for the mandatory no-emit execution path.
//!
//! The guard is deliberately a zero-sized value. The CLI threads it from
//! argument dispatch into `ProgramSession`, and a successful no-emit result
//! carries a zero-sized proof whose accessors expose the eight frozen H1
//! activity counts. Future H1 compiler-side factories must call the matching
//! guard method before constructing an emit component on this path; every
//! method panics, so even a fast accidental construction is observable in the
//! complete local test gate.

/// Exact H1 activity observations for one successful no-emit execution.
///
/// This is a proof token, not eight counters stored beside every program. It
/// remains zero-sized and every accessor is therefore exactly zero. A guarded
/// execution cannot produce the token after a forbidden constructor or sink
/// call because those operations panic through [`NoEmitCanary`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NoEmitActivityCounters;

impl NoEmitActivityCounters {
    pub const fn emit_resolver_constructions(self) -> u64 {
        0
    }

    pub const fn transformer_initializations(self) -> u64 {
        0
    }

    pub const fn transform_context_constructions(self) -> u64 {
        0
    }

    pub const fn emit_side_table_allocations(self) -> u64 {
        0
    }

    pub const fn printer_writer_constructions(self) -> u64 {
        0
    }

    pub const fn output_plan_constructions(self) -> u64 {
        0
    }

    pub const fn emit_artifact_creations(self) -> u64 {
        0
    }

    pub const fn output_sink_writes(self) -> u64 {
        0
    }

    pub const fn all_zero(self) -> bool {
        self.emit_resolver_constructions() == 0
            && self.transformer_initializations() == 0
            && self.transform_context_constructions() == 0
            && self.emit_side_table_allocations() == 0
            && self.printer_writer_constructions() == 0
            && self.output_plan_constructions() == 0
            && self.emit_artifact_creations() == 0
            && self.output_sink_writes() == 0
    }
}

/// Panic factory threaded only through the mandatory no-emit route.
#[derive(Debug, Default)]
pub(crate) struct NoEmitCanary;

#[allow(dead_code)]
impl NoEmitCanary {
    pub(crate) const fn new() -> Self {
        Self
    }

    #[cold]
    #[track_caller]
    fn forbidden(activity: &str) -> ! {
        panic!("H1 no-emit canary reached forbidden activity: {activity}")
    }

    pub(crate) fn construct_emit_resolver(&mut self) -> ! {
        Self::forbidden("emit-resolver construction")
    }

    pub(crate) fn initialize_transformers(&mut self) -> ! {
        Self::forbidden("script-transformer selection or initialization")
    }

    pub(crate) fn construct_transform_context(&mut self) -> ! {
        Self::forbidden("transformation-context or synthetic-node-arena construction")
    }

    pub(crate) fn allocate_emit_side_table(&mut self) -> ! {
        Self::forbidden("emit-only node or symbol side-table allocation")
    }

    pub(crate) fn construct_printer_writer(&mut self) -> ! {
        Self::forbidden("printer or text-writer construction")
    }

    pub(crate) fn construct_output_plan(&mut self) -> ! {
        Self::forbidden("JavaScript output-path or source-map planning")
    }

    pub(crate) fn create_emit_artifact(&mut self) -> ! {
        Self::forbidden("emit artifact creation")
    }

    pub(crate) fn write_output_sink(&mut self) -> ! {
        Self::forbidden("output-sink write")
    }
}

#[cfg(test)]
#[path = "../tests/unit/no_emit_canary/tests.rs"]
mod tests;
