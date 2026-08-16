use std::io::Write;
use std::process::{Command, Stdio};

use tsc_ci_adapter_protocol::{ActionInvocationV1, ObservationEnvelopeV1};
use tsc_ci_core::{
    ActionKeyV1, BoundedBytesSink, CanonicalEncode, ImplementationIdV1, InputDigestV1,
    InvocationIdV1, ObjectDigestV1, SchemaIdV1,
};

fn invocation() -> ActionInvocationV1 {
    ActionInvocationV1::try_new(
        ActionKeyV1::from_bytes([1; 32]),
        SchemaIdV1::from_bytes([2; 16]),
        ImplementationIdV1::from_bytes([3; 16]),
        InputDigestV1::from_bytes([4; 32]),
        InvocationIdV1::from_bytes([5; 16]),
        ObjectDigestV1::from_bytes([6; 32]),
        0,
        0,
        512,
    )
    .expect("invocation")
}

fn request_frame() -> Vec<u8> {
    let mut payload = BoundedBytesSink::new(4096);
    invocation()
        .encode_canonical(&mut payload)
        .expect("invocation fits");
    let payload = payload.into_bytes();
    let mut frame = (payload.len() as u32).to_be_bytes().to_vec();
    frame.extend_from_slice(&payload);
    frame
}

fn run_with_input(input: &[u8], environment_value: &str) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_tsc-rs-ci-harness"))
        .env("TSCRS_FCI_AMBIENT_PROBE", environment_value)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn harness");
    child
        .stdin
        .take()
        .expect("stdin pipe")
        .write_all(input)
        .expect("write request");
    child.wait_with_output().expect("wait harness")
}

#[test]
fn process_boundary_returns_one_bounded_canonical_observation() {
    let output = run_with_input(&request_frame(), "first");
    assert!(output.status.success(), "stderr={:?}", output.stderr);
    assert!(output.stdout.len() > 4);
    let length = u32::from_be_bytes(output.stdout[..4].try_into().expect("header")) as usize;
    assert_eq!(length, output.stdout.len() - 4);
    let envelope = ObservationEnvelopeV1::decode_canonical(&output.stdout[4..], 16 * 1024 * 1024)
        .expect("canonical observation");
    assert_eq!(envelope.action(), invocation().action());
    assert_eq!(envelope.bytes()[0], b'{');
}

#[test]
fn malformed_and_trailing_requests_exit_nonzero() {
    let malformed = run_with_input(&[0, 0, 0, 1, b'{'], "malformed");
    assert!(!malformed.status.success());

    let mut trailing = request_frame();
    trailing.push(0);
    let trailing = run_with_input(&trailing, "trailing");
    assert!(!trailing.status.success());

    let over_limit = run_with_input(&[0x01, 0x00, 0x00, 0x01], "over-limit");
    assert!(!over_limit.status.success());
}

#[test]
fn output_is_invariant_under_ambient_environment_changes() {
    let first = run_with_input(&request_frame(), "first");
    let second = run_with_input(&request_frame(), "second");
    assert!(first.status.success());
    assert!(second.status.success());
    assert_eq!(first.stdout, second.stdout);
}
