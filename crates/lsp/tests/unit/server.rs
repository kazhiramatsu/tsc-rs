//! tsgo `recover`: a request's panic answered as an internal error, logged
//! with its stack and, with telemetry on, reported with the stack sanitized.

use std::sync::Arc;

use tsc_host::vfs::{MemFs, SystemClock};

use super::*;

fn server() -> Arc<Server> {
    Server::new(ServerOptions {
        stderr: Box::new(std::io::sink()),
        cwd: "/".to_owned(),
        set_parent_process_id: None,
        fs: Arc::new(MemFs::new(false, Arc::new(SystemClock))),
        default_library_path: "/lib".to_owned(),
        spawner: None,
        progress_delay: Duration::ZERO,
    })
}

fn panic_in_request() -> Box<dyn std::any::Any + Send> {
    std::panic::catch_unwind(|| panic!("boom")).unwrap_err()
}

#[test]
fn a_panic_is_an_internal_error_reported_to_telemetry() {
    let server = server();
    lock(&server.state).telemetry_enabled = true;
    let failure = server.recover("textDocument/hover", panic_in_request());
    let Failure::Error(error) = failure else {
        panic!("an error");
    };
    assert_eq!(error.code, ErrorCode::INTERNAL_ERROR);
    assert_eq!(
        error.message,
        "InternalError: panic handling request textDocument/hover: boom"
    );
    let Some(Message::Notification(notification)) = server.outgoing.get() else {
        panic!("a telemetry notification");
    };
    assert_eq!(notification.method, "telemetry/event");
    let params: serde_json::Value =
        serde_json::from_str(notification.params.as_ref().unwrap().get()).unwrap();
    assert_eq!(params["eventName"], "languageServer.errorResponse");
    assert_eq!(params["telemetryPurpose"], "error");
    assert_eq!(params["properties"]["errorCode"], "InternalError");
    assert_eq!(params["properties"]["requestMethod"], "textDocument.hover");
    // The test's own frame is tsc-rs's; the panic machinery's are not.
    let stack = params["properties"]["stack"].as_str().unwrap();
    assert!(stack.contains("(REDACTED FRAME)"), "{stack}");
    assert!(stack.contains("tsc_lsp::"), "{stack}");
}

#[test]
fn without_telemetry_a_panic_is_only_answered() {
    let server = server();
    let failure = server.recover("textDocument/hover", panic_in_request());
    assert!(matches!(failure, Failure::Error(_)));
    server.outgoing.close();
    assert!(server.outgoing.get().is_none());
}
