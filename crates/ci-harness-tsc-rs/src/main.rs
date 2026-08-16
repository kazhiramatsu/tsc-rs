#![forbid(unsafe_code)]

use std::fmt;
use std::io::{self, Read, Write};

use tsc_ci_adapter_protocol::{ActionInvocationV1, ObservationEnvelopeV1};
use tsc_ci_core::{BoundedBytesSink, CanonicalEncode, CanonicalValue};

const MAX_FRAME_BYTES: u32 = 16 * 1024 * 1024;
const FRAME_HEADER_BYTES: usize = 4;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum HarnessError {
    InputTransport,
    OutputTransport,
    EmptyFrame,
    FrameTooLarge,
    TruncatedFrame,
    TrailingInput,
    InvalidInvocation,
    OutputLimit,
    InvalidOutput,
}

impl fmt::Display for HarnessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::InputTransport => "input-transport",
            Self::OutputTransport => "output-transport",
            Self::EmptyFrame => "empty-frame",
            Self::FrameTooLarge => "frame-too-large",
            Self::TruncatedFrame => "truncated-frame",
            Self::TrailingInput => "trailing-input",
            Self::InvalidInvocation => "invalid-invocation",
            Self::OutputLimit => "output-limit",
            Self::InvalidOutput => "invalid-output",
        };
        formatter.write_str(name)
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum ChildExitClass {
    Success,
    NonZero,
    Signaled,
    TimedOut,
}

#[cfg(test)]
fn classify_child_exit(exit_code: Option<i32>, signaled: bool, timed_out: bool) -> ChildExitClass {
    if timed_out {
        ChildExitClass::TimedOut
    } else if signaled {
        ChildExitClass::Signaled
    } else if exit_code == Some(0) {
        ChildExitClass::Success
    } else {
        ChildExitClass::NonZero
    }
}

fn read_frame<R: Read>(reader: &mut R) -> Result<Vec<u8>, HarnessError> {
    let mut header = [0; FRAME_HEADER_BYTES];
    reader
        .read_exact(&mut header)
        .map_err(|error| match error.kind() {
            io::ErrorKind::UnexpectedEof => HarnessError::TruncatedFrame,
            _ => HarnessError::InputTransport,
        })?;
    let length = u32::from_be_bytes(header);
    if length == 0 {
        return Err(HarnessError::EmptyFrame);
    }
    if length > MAX_FRAME_BYTES {
        return Err(HarnessError::FrameTooLarge);
    }
    let mut bytes = vec![0; length as usize];
    reader
        .read_exact(&mut bytes)
        .map_err(|error| match error.kind() {
            io::ErrorKind::UnexpectedEof => HarnessError::TruncatedFrame,
            _ => HarnessError::InputTransport,
        })?;
    Ok(bytes)
}

fn write_frame<W: Write>(writer: &mut W, bytes: &[u8]) -> Result<(), HarnessError> {
    if bytes.is_empty() {
        return Err(HarnessError::EmptyFrame);
    }
    let length = u32::try_from(bytes.len()).map_err(|_| HarnessError::FrameTooLarge)?;
    if length > MAX_FRAME_BYTES {
        return Err(HarnessError::FrameTooLarge);
    }
    writer
        .write_all(&length.to_be_bytes())
        .and_then(|_| writer.write_all(bytes))
        .and_then(|_| writer.flush())
        .map_err(|_| HarnessError::OutputTransport)
}

fn render_payload(diagnostic_count: usize, compiler_linked: bool) -> Result<Vec<u8>, HarnessError> {
    let payload = CanonicalValue::Object(vec![
        (
            "compiler_linked".to_owned(),
            CanonicalValue::Bool(compiler_linked),
        ),
        (
            "diagnostic_count".to_owned(),
            CanonicalValue::Unsigned(diagnostic_count as u64),
        ),
        (
            "kind".to_owned(),
            CanonicalValue::String("transport-probe".to_owned()),
        ),
    ]);
    let mut sink = BoundedBytesSink::new(u64::from(MAX_FRAME_BYTES));
    payload
        .encode_canonical(&mut sink)
        .map_err(|_| HarnessError::InvalidOutput)?;
    Ok(sink.into_bytes())
}

fn run_invocation(bytes: &[u8]) -> Result<Vec<u8>, HarnessError> {
    let invocation = ActionInvocationV1::decode_canonical(bytes, u64::from(MAX_FRAME_BYTES))
        .map_err(|_| HarnessError::InvalidInvocation)?;

    // This is deliberately a transport probe, not an H2 action. It proves
    // that the child may link the candidate assembly while keeping the
    // protected control process free of production/compiler dependencies.
    let check = tsc_harness::check_empty_program();
    let compiler_linked = tsc_compiler::NoEmitActivityCounters.all_zero();
    let payload = render_payload(check.diagnostics.len(), compiler_linked)?;
    let observation = ObservationEnvelopeV1::try_new(
        invocation.action(),
        invocation.schema(),
        invocation.implementation(),
        invocation.repetition(),
        payload,
        invocation.max_output_bytes(),
    )
    .map_err(|_| HarnessError::OutputLimit)?;
    let mut sink = BoundedBytesSink::new(u64::from(MAX_FRAME_BYTES));
    observation
        .encode_canonical(&mut sink)
        .map_err(|_| HarnessError::InvalidOutput)?;
    Ok(sink.into_bytes())
}

fn run_once<R: Read, W: Write>(reader: &mut R, writer: &mut W) -> Result<(), HarnessError> {
    let request = read_frame(reader)?;
    let response = run_invocation(&request)?;
    let mut trailing = [0; 1];
    if reader
        .read(&mut trailing)
        .map_err(|_| HarnessError::InputTransport)?
        != 0
    {
        return Err(HarnessError::TrailingInput);
    }
    write_frame(writer, &response)
}

fn main() {
    let result = run_once(&mut io::stdin().lock(), &mut io::stdout().lock());
    if let Err(error) = result {
        eprintln!("tsc-rs-ci-harness: {error}");
        std::process::exit(2);
    }
}

#[cfg(test)]
#[path = "../tests/unit/main_tests.rs"]
mod tests;
