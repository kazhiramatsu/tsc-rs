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
