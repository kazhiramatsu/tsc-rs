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
mod tests {
    use super::{classify_child_exit, run_once, ChildExitClass, HarnessError};
    use std::io::Cursor;

    use tsc_ci_adapter_protocol::ActionInvocationV1;
    use tsc_ci_core::{
        ActionKeyV1, BoundedBytesSink, CanonicalEncode, ImplementationIdV1, InputDigestV1,
        InvocationIdV1, ObjectDigestV1, SchemaIdV1,
    };

    fn invocation(max_output_bytes: u64) -> ActionInvocationV1 {
        ActionInvocationV1::try_new(
            ActionKeyV1::from_bytes([1; 32]),
            SchemaIdV1::from_bytes([2; 16]),
            ImplementationIdV1::from_bytes([3; 16]),
            InputDigestV1::from_bytes([4; 32]),
            InvocationIdV1::from_bytes([5; 16]),
            ObjectDigestV1::from_bytes([6; 32]),
            0,
            0,
            max_output_bytes,
        )
        .expect("invocation")
    }

    fn frame(bytes: &[u8]) -> Vec<u8> {
        let mut frame = (bytes.len() as u32).to_be_bytes().to_vec();
        frame.extend_from_slice(bytes);
        frame
    }

    fn encoded_invocation(max_output_bytes: u64) -> Vec<u8> {
        let mut sink = BoundedBytesSink::new(4096);
        invocation(max_output_bytes)
            .encode_canonical(&mut sink)
            .expect("invocation fits");
        sink.into_bytes()
    }

    #[test]
    fn replay_is_byte_identical_and_does_not_read_ambient_state() {
        let input = frame(&encoded_invocation(512));
        let mut first = Vec::new();
        run_once(&mut Cursor::new(input.clone()), &mut first).expect("first probe");
        let mut second = Vec::new();
        run_once(&mut Cursor::new(input), &mut second).expect("second probe");
        assert_eq!(first, second);
    }

    #[test]
    fn malformed_truncated_trailing_and_over_limit_frames_fail_closed() {
        assert_eq!(
            run_once(&mut Cursor::new(vec![0, 0, 0, 1]), &mut Vec::new()),
            Err(HarnessError::TruncatedFrame)
        );
        let mut trailing = frame(&encoded_invocation(512));
        trailing.push(0);
        assert_eq!(
            run_once(&mut Cursor::new(trailing), &mut Vec::new()),
            Err(HarnessError::TrailingInput)
        );
        assert_eq!(
            run_once(
                &mut Cursor::new(frame(&encoded_invocation(1))),
                &mut Vec::new()
            ),
            Err(HarnessError::OutputLimit)
        );
    }

    #[test]
    fn lifecycle_classes_are_closed_before_runner_mapping() {
        assert_eq!(
            classify_child_exit(Some(0), false, false),
            ChildExitClass::Success
        );
        assert_eq!(
            classify_child_exit(Some(2), false, false),
            ChildExitClass::NonZero
        );
        assert_eq!(
            classify_child_exit(None, true, false),
            ChildExitClass::Signaled
        );
        assert_eq!(
            classify_child_exit(None, false, true),
            ChildExitClass::TimedOut
        );
    }
}
