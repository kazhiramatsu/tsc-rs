use super::*;

#[test]
fn unknown_failures_are_semantic_and_transient_markers_are_environment() {
    assert_eq!(
        classify_failure("reported diagnostics differ"),
        FailureClass::Semantic
    );
    assert_eq!(
        classify_failure("worker process exited: timed out"),
        FailureClass::Environment
    );
    assert_eq!(
        classify_failure("No such file or directory"),
        FailureClass::Environment
    );
}

#[test]
fn stable_message_removes_only_the_workspace_prefix() {
    let workspace = Path::new("/tmp/workspace");
    assert_eq!(
        stable_message(workspace, "/tmp/workspace/ts-tests/a.ts: mismatch"),
        "<workspace>/ts-tests/a.ts: mismatch"
    );
}

#[test]
fn failure_messages_are_bounded_on_utf8_boundaries() {
    let input = "😀".repeat(200_000);
    let (message, truncated) = bounded_message(&input);
    assert!(truncated);
    assert!(message.len() <= 256 * 1024);
    assert!(message.is_char_boundary(message.len()));
}
