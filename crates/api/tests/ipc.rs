//! tsgo's connection tests (`ipc/conn_sync_test.go`, the cases of
//! `ipc/conn_async_test.go` that hold when requests are handled one at a
//! time), a call that meets a nested request, and the framing of the
//! MessagePack and JSON-RPC protocols.

use std::collections::VecDeque;
use std::io::Cursor;
use std::sync::{Arc, Mutex};

use tsc_api::ipc::jsonrpc::JsonRpcProtocol;
use tsc_api::ipc::{Conn, Error, Handler, Id, Message, Mode, Payload, Protocol, ResponseError};
use tsc_api::msgpack::MessagePackProtocol;

/// What a test protocol wrote.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Written {
    Request(String, String),
    Response(String, Payload),
    Error(String, String),
}

/// tsgo `queuedProtocol`: messages to read, writes recorded, and an error
/// for every response written when set.
#[derive(Default)]
struct QueuedProtocol {
    messages: VecDeque<Message>,
    response_error: Option<String>,
    written: Arc<Mutex<Vec<Written>>>,
}

impl Protocol for QueuedProtocol {
    fn read_message(&mut self) -> Result<Option<Message>, Error> {
        Ok(self.messages.pop_front())
    }

    fn write_request(&mut self, id: &Id, method: &str, _params: &str) -> Result<(), Error> {
        self.written
            .lock()
            .unwrap()
            .push(Written::Request(id.to_string(), method.to_owned()));
        Ok(())
    }

    fn write_notification(&mut self, _method: &str, _params: &str) -> Result<(), Error> {
        Ok(())
    }

    fn write_response(&mut self, id: &Id, result: &Payload) -> Result<(), Error> {
        if let Some(error) = &self.response_error {
            return Err(Error::new(error.clone()));
        }
        self.written
            .lock()
            .unwrap()
            .push(Written::Response(id.to_string(), result.clone()));
        Ok(())
    }

    fn write_error(&mut self, id: &Id, error: &ResponseError) -> Result<(), Error> {
        if let Some(error) = &self.response_error {
            return Err(Error::new(error.clone()));
        }
        self.written
            .lock()
            .unwrap()
            .push(Written::Error(id.to_string(), error.message.clone()));
        Ok(())
    }
}

fn request(id: &str, method: &str) -> Message {
    Message {
        id: Some(Id::String(id.to_owned())),
        method: method.to_owned(),
        ..Message::default()
    }
}

/// tsgo `noOpHandler`.
struct NoOpHandler;

impl Handler for NoOpHandler {
    fn handle_request(&self, _method: &str, _params: &[u8]) -> Result<Payload, String> {
        Ok(Payload::null())
    }

    fn handle_notification(&self, _method: &str, _params: &[u8]) {}
}

/// tsgo `panicHandler`.
struct PanicHandler;

impl Handler for PanicHandler {
    fn handle_request(&self, _method: &str, _params: &[u8]) -> Result<Payload, String> {
        panic!("handler panic")
    }

    fn handle_notification(&self, _method: &str, _params: &[u8]) {}
}

fn failing_protocol() -> QueuedProtocol {
    QueuedProtocol {
        messages: VecDeque::from([request("1", "transform")]),
        response_error: Some("response write failed".to_owned()),
        ..QueuedProtocol::default()
    }
}

#[test]
fn a_failed_response_write_stops_the_sync_connection() {
    // TestSyncConnRunReturnsResponseWriteFailure.
    let conn = Conn::new(
        Box::new(failing_protocol()),
        Arc::new(NoOpHandler),
        Mode::Sync,
    );
    let error = conn.run().unwrap_err();
    assert!(error.message().contains("response write failed"), "{error}");
}

#[test]
fn a_failed_panic_response_write_names_the_panic() {
    // TestSyncConnRunReturnsPanicResponseWriteFailure.
    let conn = Conn::new(
        Box::new(failing_protocol()),
        Arc::new(PanicHandler),
        Mode::Sync,
    );
    let error = conn.run().unwrap_err();
    assert!(error.message().contains("response write failed"), "{error}");
    assert!(
        error.message().contains("original panic: handler panic"),
        "{error}"
    );
}

#[test]
fn a_panic_becomes_an_error_response() {
    let written = Arc::new(Mutex::new(Vec::new()));
    let protocol = QueuedProtocol {
        messages: VecDeque::from([request("transform", "transform")]),
        written: Arc::clone(&written),
        ..QueuedProtocol::default()
    };
    let conn = Conn::new(Box::new(protocol), Arc::new(PanicHandler), Mode::Sync);
    conn.run().unwrap();
    assert_eq!(
        *written.lock().unwrap(),
        [Written::Error(
            "transform".to_owned(),
            "panic: handler panic".to_owned()
        )]
    );
}

#[test]
fn a_failed_response_write_stops_the_async_connection() {
    // TestAsyncConnResponseWriteFailureWithNilTransport.
    let conn = Conn::new(
        Box::new(failing_protocol()),
        Arc::new(NoOpHandler),
        Mode::Async,
    );
    let error = conn.run().unwrap_err();
    assert!(error.message().contains("response write failed"), "{error}");
}

#[test]
fn a_stopped_async_connection_fails_calls_with_its_cause_once() {
    // TestAsyncConnTerminalErrorIncludesResponseWriteFailure and
    // TestAsyncConnCallAfterReadLoopFailureReturnsImmediately.
    let conn = Conn::new(
        Box::new(failing_protocol()),
        Arc::new(NoOpHandler),
        Mode::Async,
    );
    conn.run().unwrap_err();
    let error = conn.call("transform", "null").unwrap_err();
    assert!(
        error.message().starts_with("ipc: connection closed"),
        "{error}"
    );
    assert_eq!(error.message().matches("response write failed").count(), 1);
    let error = conn.notify("changed", "null").unwrap_err();
    assert!(error.message().contains("response write failed"), "{error}");
}

#[test]
fn an_async_connection_that_read_an_invalid_header_is_closed() {
    // TestAsyncConnCallAfterReadLoopFailureReturnsImmediately.
    let protocol = JsonRpcProtocol::new(Cursor::new(b"oops\n".to_vec()), Vec::new());
    let conn = Conn::new(Box::new(protocol), Arc::new(NoOpHandler), Mode::Async);
    let error = conn.run().unwrap_err();
    assert!(error.message().contains("invalid header"), "{error}");
    let error = conn.call("transform", "null").unwrap_err();
    assert!(
        error.message().starts_with("ipc: connection closed"),
        "{error}"
    );
}

#[test]
fn a_sync_connection_rejects_a_response_nobody_waits_for() {
    let protocol = QueuedProtocol {
        messages: VecDeque::from([Message {
            id: Some(Id::String("x".to_owned())),
            ..Message::default()
        }]),
        ..QueuedProtocol::default()
    };
    let conn = Conn::new(Box::new(protocol), Arc::new(NoOpHandler), Mode::Sync);
    assert_eq!(
        conn.run().unwrap_err().message(),
        "ipc: unexpected response message in sync connection"
    );
}

/// A handler that calls the client back while it handles `outer`.
struct CallingHandler {
    conn: Mutex<Option<Arc<Conn>>>,
    seen: Mutex<Vec<String>>,
}

impl Handler for CallingHandler {
    fn handle_request(&self, method: &str, _params: &[u8]) -> Result<Payload, String> {
        self.seen.lock().unwrap().push(method.to_owned());
        if method == "outer" {
            let conn = self.conn.lock().unwrap().clone().unwrap();
            let result = conn
                .call("readFile", r#"{"fileName":"/a.ts"}"#)
                .map_err(|error| error.to_string())?;
            return Ok(Payload::Json(String::from_utf8(result).unwrap()));
        }
        Ok(Payload::Json(r#""inner""#.to_owned()))
    }

    fn handle_notification(&self, _method: &str, _params: &[u8]) {}
}

#[test]
fn a_call_handles_the_requests_that_come_before_its_response() {
    // tsgo `SyncConn.Call`: a client callback may make a nested request,
    // answered before the call's response is read.
    let written = Arc::new(Mutex::new(Vec::new()));
    let protocol = QueuedProtocol {
        messages: VecDeque::from([
            request("outer", "outer"),
            request("inner", "inner"),
            Message {
                id: Some(Id::String("readFile".to_owned())),
                result: br#""text""#.to_vec(),
                ..Message::default()
            },
        ]),
        written: Arc::clone(&written),
        ..QueuedProtocol::default()
    };
    let handler = Arc::new(CallingHandler {
        conn: Mutex::new(None),
        seen: Mutex::new(Vec::new()),
    });
    let conn = Arc::new(Conn::new(
        Box::new(protocol),
        Arc::clone(&handler) as Arc<dyn Handler>,
        Mode::Sync,
    ));
    *handler.conn.lock().unwrap() = Some(Arc::clone(&conn));
    conn.run().unwrap();
    assert_eq!(*handler.seen.lock().unwrap(), ["outer", "inner"]);
    assert_eq!(
        *written.lock().unwrap(),
        [
            Written::Request("readFile".to_owned(), "readFile".to_owned()),
            Written::Response("inner".to_owned(), Payload::Json(r#""inner""#.to_owned())),
            Written::Response("outer".to_owned(), Payload::Json(r#""text""#.to_owned())),
        ]
    );
    *handler.conn.lock().unwrap() = None;
}

#[test]
fn a_call_fails_with_the_clients_error() {
    let protocol = QueuedProtocol {
        messages: VecDeque::from([Message {
            id: Some(Id::String("readFile".to_owned())),
            error: Some(ResponseError::internal("no such file")),
            ..Message::default()
        }]),
        ..QueuedProtocol::default()
    };
    let conn = Conn::new(Box::new(protocol), Arc::new(NoOpHandler), Mode::Sync);
    assert_eq!(
        conn.call("readFile", "null").unwrap_err().message(),
        "ipc: remote error [-32603]: no such file"
    );
}

#[test]
fn the_connection_answers_the_timing_requests() {
    let written = Arc::new(Mutex::new(Vec::new()));
    let protocol = QueuedProtocol {
        messages: VecDeque::from([
            request("getServerTiming", "getServerTiming"),
            request("resetServerTiming", "resetServerTiming"),
        ]),
        written: Arc::clone(&written),
        ..QueuedProtocol::default()
    };
    Conn::new(Box::new(protocol), Arc::new(NoOpHandler), Mode::Sync)
        .run()
        .unwrap();
    assert_eq!(
        *written.lock().unwrap(),
        [
            Written::Response(
                "getServerTiming".to_owned(),
                Payload::Json(
                    r#"{"enabled":false,"totals":{"requestCount":0,"totalProcessingTimeMs":0},"recentRequests":[]}"#
                        .to_owned()
                )
            ),
            Written::Response("resetServerTiming".to_owned(), Payload::null()),
        ]
    );
}

/// A MessagePack binary string of `data`.
fn bin(data: &[u8]) -> Vec<u8> {
    let mut bytes = match data.len() {
        length if length < 256 => vec![0xC4, length as u8],
        length if length < 65_536 => {
            let mut bytes = vec![0xC5];
            bytes.extend_from_slice(&(length as u16).to_be_bytes());
            bytes
        }
        length => {
            let mut bytes = vec![0xC6];
            bytes.extend_from_slice(&(length as u32).to_be_bytes());
            bytes
        }
    };
    bytes.extend_from_slice(data);
    bytes
}

fn tuple(message_type: &[u8], method: &str, payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0x93];
    bytes.extend_from_slice(message_type);
    bytes.extend(bin(method.as_bytes()));
    bytes.extend(bin(payload));
    bytes
}

#[test]
fn message_pack_requests_and_responses_frame_like_tsgo() {
    // tsgo `MessagePackProtocol`: a request's method is its ID, a uint8
    // message type is read too, and a payload's length takes the smallest
    // binary string.
    let long = vec![b'x'; 300];
    let mut input = tuple(&[1], "ping", b"");
    input.extend(tuple(&[0xCC, 2], "readFile", &long));
    input.extend(tuple(&[3], "readFile", b"no such file"));
    let mut output = Vec::new();
    {
        let mut protocol = MessagePackProtocol::new(Cursor::new(input), &mut output);
        let request = protocol.read_message().unwrap().unwrap();
        assert!(request.is_request());
        assert_eq!(request.id, Some(Id::String("ping".to_owned())));
        let response = protocol.read_message().unwrap().unwrap();
        assert!(response.is_response());
        assert_eq!(response.result, long);
        let error = protocol.read_message().unwrap().unwrap();
        assert_eq!(error.error.unwrap().message, "no such file");
        assert_eq!(protocol.read_message().unwrap(), None);
        protocol
            .write_response(
                &Id::String("ping".to_owned()),
                &Payload::Json(r#""pong""#.to_owned()),
            )
            .unwrap();
        protocol
            .write_error(&Id::String("x".to_owned()), &ResponseError::internal("bad"))
            .unwrap();
        protocol
            .write_request(
                &Id::String("readFile".to_owned()),
                "readFile",
                &"y".repeat(70_000),
            )
            .unwrap();
    }
    let mut expected = tuple(&[4], "ping", br#""pong""#);
    expected.extend(tuple(&[5], "x", b"bad"));
    expected.extend(tuple(&[6], "readFile", "y".repeat(70_000).as_bytes()));
    assert_eq!(output, expected);
}

#[test]
fn malformed_message_pack_is_an_invalid_request() {
    for (input, expected) in [
        (
            vec![0x92],
            "api: invalid request: expected fixed 3-element array (0x93), received: 0x92",
        ),
        (
            vec![0x93, 0xCD],
            "api: invalid request: expected positive fixint or uint8 marker, received: 0xcd",
        ),
        (
            vec![0x93, 7],
            "api: invalid request: unknown message type: 7",
        ),
        (
            vec![0x93, 1, 0xA4],
            "api: invalid request: expected binary data (0xc4-0xc6), received: 0xa4",
        ),
        (vec![0x93, 1, 0xC4, 4, b'p'], "unexpected EOF"),
        (tuple(&[4], "x", b""), "unexpected message type: 4"),
    ] {
        let mut protocol = MessagePackProtocol::new(Cursor::new(input), Vec::new());
        assert_eq!(protocol.read_message().unwrap_err().message(), expected);
    }
}

#[test]
fn json_rpc_messages_frame_like_tsgo() {
    let body = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":null}"#;
    let input = format!("Content-Length: {}\r\nOther: x\r\n\r\n{body}", body.len());
    let mut output = Vec::new();
    {
        let mut protocol = JsonRpcProtocol::new(Cursor::new(input.into_bytes()), &mut output);
        let message = protocol.read_message().unwrap().unwrap();
        assert_eq!(message.id, Some(Id::Int(1)));
        assert_eq!(message.method, "initialize");
        assert_eq!(message.params, b"null");
        assert_eq!(protocol.read_message().unwrap(), None);
        protocol
            .write_response(&Id::Int(1), &Payload::Json("true".to_owned()))
            .unwrap();
        protocol
            .write_error(&Id::String("a".to_owned()), &ResponseError::internal("bad"))
            .unwrap();
        protocol
            .write_request(&Id::String("api1".to_owned()), "readFile", "")
            .unwrap();
    }
    let expected = [
        r#"{"jsonrpc":"2.0","id":1,"result":true}"#,
        r#"{"jsonrpc":"2.0","id":"a","error":{"code":-32603,"message":"bad"}}"#,
        r#"{"jsonrpc":"2.0","id":"api1","method":"readFile"}"#,
    ]
    .iter()
    .map(|body| format!("Content-Length: {}\r\n\r\n{body}", body.len()))
    .collect::<String>();
    assert_eq!(String::from_utf8(output).unwrap(), expected);
}

#[test]
fn malformed_json_rpc_framing_is_an_error() {
    for (input, expected) in [
        ("oops\n", "jsonrpc: invalid header: \"oops\\n\""),
        ("Content-Length: -5\r\n\r\n", "jsonrpc: invalid content length: negative value -5"),
        ("Content-Length: x\r\n\r\n", "jsonrpc: invalid content length: parse error: strconv.ParseInt: parsing \"x\": invalid syntax"),
        ("Other: 1\r\n\r\n", "jsonrpc: no content length"),
        ("Content-Length: 5\r\n\r\n{}", "jsonrpc: read content: unexpected EOF"),
        ("Content-Length: 17\r\n\r\n{\"jsonrpc\":\"1.0\"}", "invalid JSON-RPC version"),
    ] {
        let mut protocol = JsonRpcProtocol::new(Cursor::new(input.as_bytes().to_vec()), Vec::new());
        assert_eq!(
            protocol.read_message().unwrap_err().message(),
            expected,
            "{input:?}"
        );
    }
    // A header cut off before its newline ends the input.
    let mut protocol = JsonRpcProtocol::new(Cursor::new(b"Content-Length: 5".to_vec()), Vec::new());
    assert_eq!(protocol.read_message().unwrap(), None);
}
