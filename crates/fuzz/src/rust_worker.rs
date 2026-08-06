//! Same-binary Rust worker server.
//!
//! The controller launches the current producer executable with its
//! hidden worker argument. This module owns only the bounded
//! canonical request read and the phase/result protocol; OS deadline
//! and crash classification remain controller responsibilities.

use std::any::Any;
use std::io::{BufRead, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::adapters::tsrs::{
    execute_prepared_program, prepare_wire_program, PreparedProgram, TsrsAdapterOutput,
};
use crate::model::{EngineResult, TerminalBoundaryId, TerminalKind};
use crate::worker_protocol::{
    ExecuteCaseRequest, WireRenderSegment, WorkerFrame, WorkerPhase, WorkerRejectionKind,
    WorkerResult, WORKER_WIRE_SCHEMA,
};
use crate::{FoundationError, FoundationResult};

pub const TSRS_WORKER_IMPLEMENTATION: &str = "tsrs-worker";
pub const TSRS_WORKER_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const DEFAULT_MAX_REQUEST_LINE_BYTES: usize = 16 * 1024 * 1024;

pub fn trusted_hello_line() -> FoundationResult<Vec<u8>> {
    serde_json::to_vec(&WorkerFrame::Hello {
        schema: WORKER_WIRE_SCHEMA,
        implementation: TSRS_WORKER_IMPLEMENTATION.to_owned(),
        version: TSRS_WORKER_VERSION.to_owned(),
    })
    .map_err(|error| FoundationError::new(format!("cannot encode trusted Rust hello: {error}")))
}

/// Serve exactly one canonical request. A fresh same-binary process
/// emits one hello, one final result, and then exits; the controller
/// never retries the case implicitly.
pub fn serve_one(
    input: &mut impl BufRead,
    output: &mut impl Write,
    max_request_line_bytes: usize,
) -> FoundationResult<()> {
    serve_one_with(
        input,
        output,
        max_request_line_bytes,
        |prepared, observe_phase| execute_prepared_program(prepared, observe_phase),
    )
}

/// Standard-I/O entry used by the hidden same-binary worker command.
pub fn serve_stdio() -> FoundationResult<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    serve_one(
        &mut stdin.lock(),
        &mut stdout.lock(),
        DEFAULT_MAX_REQUEST_LINE_BYTES,
    )
}

fn serve_one_with(
    input: &mut impl BufRead,
    output: &mut impl Write,
    max_request_line_bytes: usize,
    execute: impl FnOnce(
        &PreparedProgram,
        &mut dyn FnMut(WorkerPhase),
    ) -> FoundationResult<TsrsAdapterOutput>,
) -> FoundationResult<()> {
    if max_request_line_bytes == 0 {
        return Err(FoundationError::new(
            "Rust worker request line limit must be positive",
        ));
    }
    let mut emitter = FrameEmitter::new(output);
    emitter.hello()?;

    let line = match read_bounded_line(input, max_request_line_bytes)? {
        BoundedRequestLine::Line(line) => line,
        BoundedRequestLine::Eof => {
            return Err(FoundationError::new(
                "Rust worker stdin reached EOF before a request",
            ));
        }
        BoundedRequestLine::Unterminated { bytes } => {
            return Err(FoundationError::new(format!(
                "Rust worker stdin ended with an unterminated {bytes}-byte request line"
            )));
        }
        BoundedRequestLine::TooLong => {
            return Err(FoundationError::new(format!(
                "Rust worker request line exceeds {max_request_line_bytes} bytes"
            )));
        }
    };
    let request = canonical_request(&line, max_request_line_bytes)?;
    emitter.bind(&request);

    let prepared = match prepare_wire_program(request.program()) {
        Ok(prepared) => prepared,
        Err(error) => {
            emitter.result(WorkerResult::Rejected {
                kind: WorkerRejectionKind::MalformedRequest,
                detail: safe_detail(error.to_string(), "invalid request"),
            })?;
            return Ok(());
        }
    };

    let mut last_phase = None;
    let mut phase_write_error = None;
    let execution = catch_unwind(AssertUnwindSafe(|| {
        let mut observe_phase = |phase| {
            last_phase = Some(phase);
            if phase_write_error.is_none() {
                if let Err(error) = emitter.phase(phase) {
                    phase_write_error = Some(error);
                }
            }
        };
        execute(&prepared, &mut observe_phase)
    }));
    if let Some(error) = phase_write_error {
        return Err(error);
    }

    match execution {
        Ok(Ok(adapter)) => {
            emitter.result(worker_result(adapter)?)?;
        }
        Ok(Err(error)) => {
            emitter.result(WorkerResult::Rejected {
                kind: WorkerRejectionKind::MalformedObservation,
                detail: safe_detail(error.to_string(), "invalid Rust observation"),
            })?;
        }
        Err(payload) => {
            let Some(phase) = last_phase else {
                return Err(FoundationError::new(format!(
                    "Rust worker panicked before the first phase: {}",
                    safe_detail(panic_detail(payload), "non-string Rust panic")
                )));
            };
            emitter.result(WorkerResult::Terminal {
                phase,
                kind: TerminalKind::Panic,
                boundary_id: TerminalBoundaryId::PhaseInvariant,
                detail: safe_detail(panic_detail(payload), "non-string Rust panic"),
            })?;
        }
    }
    Ok(())
}

fn worker_result(adapter: TsrsAdapterOutput) -> FoundationResult<WorkerResult> {
    match adapter.result {
        EngineResult::Completed { outcome } => {
            if adapter.deduped_indices.len() != outcome.renderer.segments.len() {
                return Err(FoundationError::new(
                    "Rust adapter retained-index/segment length mismatch",
                ));
            }
            let segments = adapter
                .deduped_indices
                .iter()
                .copied()
                .zip(outcome.renderer.segments)
                .map(|(assembled_index, segment)| WireRenderSegment {
                    assembled_index,
                    raw_text: segment.raw_text,
                })
                .collect();
            Ok(WorkerResult::Completed {
                assembled: outcome.renderer.assembled,
                deduped_indices: adapter.deduped_indices,
                segments,
                aggregate_text: outcome.renderer.aggregate_text,
            })
        }
        EngineResult::Terminal { outcome } => Ok(WorkerResult::Terminal {
            phase: match outcome.phase {
                crate::model::TerminalPhase::Parse => WorkerPhase::Parse,
                crate::model::TerminalPhase::Bind => WorkerPhase::Bind,
                crate::model::TerminalPhase::Check => WorkerPhase::Check,
                crate::model::TerminalPhase::Format => WorkerPhase::Format,
            },
            kind: outcome.kind,
            boundary_id: outcome.boundary_id,
            detail: safe_detail(outcome.detail, "Rust terminal"),
        }),
    }
}

fn canonical_request(
    line: &[u8],
    max_request_line_bytes: usize,
) -> FoundationResult<ExecuteCaseRequest> {
    let request: ExecuteCaseRequest = serde_json::from_slice(line).map_err(|error| {
        FoundationError::new(format!("invalid Rust worker request JSON: {error}"))
    })?;
    request.validate_binding()?;
    let canonical = request.canonical_line(max_request_line_bytes)?;
    if canonical.get(..canonical.len().saturating_sub(1)) != Some(line) {
        return Err(FoundationError::new(
            "Rust worker request must use canonical compact schema-1 JSON bytes",
        ));
    }
    Ok(request)
}

struct FrameEmitter<'writer, W: Write> {
    writer: &'writer mut W,
    binding: Option<(crate::schema::CanonicalU64, String)>,
    hello_written: bool,
    result_written: bool,
}

impl<'writer, W: Write> FrameEmitter<'writer, W> {
    fn new(writer: &'writer mut W) -> Self {
        Self {
            writer,
            binding: None,
            hello_written: false,
            result_written: false,
        }
    }

    fn bind(&mut self, request: &ExecuteCaseRequest) {
        self.binding = Some((request.id(), request.case_sha256().to_owned()));
    }

    fn hello(&mut self) -> FoundationResult<()> {
        if self.hello_written {
            return Err(FoundationError::new(
                "Rust worker hello may be emitted exactly once",
            ));
        }
        self.write_frame(&WorkerFrame::Hello {
            schema: WORKER_WIRE_SCHEMA,
            implementation: TSRS_WORKER_IMPLEMENTATION.to_owned(),
            version: TSRS_WORKER_VERSION.to_owned(),
        })?;
        self.hello_written = true;
        Ok(())
    }

    fn phase(&mut self, phase: WorkerPhase) -> FoundationResult<()> {
        if !self.hello_written || self.result_written {
            return Err(FoundationError::new(
                "Rust worker phase is outside hello/result boundaries",
            ));
        }
        let (id, case_sha256) = self
            .binding
            .clone()
            .ok_or_else(|| FoundationError::new("Rust worker phase has no request binding"))?;
        self.write_frame(&WorkerFrame::Phase {
            schema: WORKER_WIRE_SCHEMA,
            id,
            case_sha256,
            phase,
        })
    }

    fn result(&mut self, result: WorkerResult) -> FoundationResult<()> {
        if !self.hello_written || self.result_written {
            return Err(FoundationError::new(
                "Rust worker must emit exactly one result",
            ));
        }
        let (id, case_sha256) = self
            .binding
            .clone()
            .ok_or_else(|| FoundationError::new("Rust worker result has no request binding"))?;
        self.write_frame(&WorkerFrame::Result {
            schema: WORKER_WIRE_SCHEMA,
            id,
            case_sha256,
            result,
        })?;
        self.result_written = true;
        Ok(())
    }

    fn write_frame(&mut self, frame: &WorkerFrame) -> FoundationResult<()> {
        serde_json::to_writer(&mut *self.writer, frame).map_err(|error| {
            FoundationError::new(format!("cannot serialize/write Rust worker frame: {error}"))
        })?;
        self.writer.write_all(b"\n").map_err(|error| {
            FoundationError::new(format!("cannot delimit Rust worker frame: {error}"))
        })?;
        self.writer.flush().map_err(|error| {
            FoundationError::new(format!("cannot flush Rust worker frame: {error}"))
        })
    }
}

enum BoundedRequestLine {
    Line(Vec<u8>),
    Eof,
    Unterminated { bytes: usize },
    TooLong,
}

fn read_bounded_line(
    input: &mut impl BufRead,
    limit: usize,
) -> FoundationResult<BoundedRequestLine> {
    let mut line = Vec::new();
    loop {
        let available = input.fill_buf().map_err(|error| {
            FoundationError::new(format!("cannot read Rust worker stdin: {error}"))
        })?;
        if available.is_empty() {
            return Ok(if line.is_empty() {
                BoundedRequestLine::Eof
            } else {
                BoundedRequestLine::Unterminated { bytes: line.len() }
            });
        }
        if let Some(newline) = available.iter().position(|byte| *byte == b'\n') {
            let total = line.len().checked_add(newline).ok_or_else(|| {
                FoundationError::new("Rust worker request length overflows usize")
            })?;
            if total > limit {
                input.consume(newline + 1);
                return Ok(BoundedRequestLine::TooLong);
            }
            line.extend_from_slice(&available[..newline]);
            input.consume(newline + 1);
            return Ok(BoundedRequestLine::Line(line));
        }
        let total = line
            .len()
            .checked_add(available.len())
            .ok_or_else(|| FoundationError::new("Rust worker request length overflows usize"))?;
        if total > limit {
            let consumed = available.len();
            input.consume(consumed);
            return Ok(BoundedRequestLine::TooLong);
        }
        line.extend_from_slice(available);
        let consumed = available.len();
        input.consume(consumed);
    }
}

fn panic_detail(payload: Box<dyn Any + Send>) -> String {
    if let Some(detail) = payload.downcast_ref::<&str>() {
        (*detail).to_owned()
    } else if let Some(detail) = payload.downcast_ref::<String>() {
        detail.clone()
    } else {
        "non-string Rust panic payload".to_owned()
    }
}

fn safe_detail(detail: String, fallback: &str) -> String {
    let detail = if detail.is_empty() {
        fallback.to_owned()
    } else {
        detail
    };
    detail.replace('\0', "\\0")
}

#[cfg(test)]
#[path = "../tests/unit/rust_worker/tests.rs"]
mod tests;
