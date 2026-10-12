//! tsgo conn_async_test.go's call and terminal-error cases, over in-memory
//! pipes, plus the matching of concurrent calls and a call's time limit.

use super::*;
use std::collections::VecDeque;
use std::sync::{Arc, Condvar};

use crate::ipc::{jsonrpc, Payload};

#[derive(Default)]
struct Pipe {
    state: Mutex<(VecDeque<u8>, bool)>,
    ready: Condvar,
}

/// The reading end of an in-memory pipe: blocks until bytes arrive or the
/// writing end is dropped.
struct PipeReader(Arc<Pipe>);

/// The writing end; dropping it ends the reader's input.
struct PipeWriter(Arc<Pipe>);

fn pipe() -> (PipeReader, PipeWriter) {
    let pipe = Arc::new(Pipe::default());
    (PipeReader(Arc::clone(&pipe)), PipeWriter(pipe))
}

impl Read for PipeReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let mut state = self.0.state.lock().unwrap();
        while state.0.is_empty() && !state.1 {
            state = self.0.ready.wait(state).unwrap();
        }
        let count = buffer.len().min(state.0.len());
        for (slot, byte) in buffer.iter_mut().zip(state.0.drain(..count)) {
            *slot = byte;
        }
        Ok(count)
    }
}

impl Write for PipeWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.state.lock().unwrap().0.extend(bytes);
        self.0.ready.notify_all();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for PipeWriter {
    fn drop(&mut self) {
        self.0.state.lock().unwrap().1 = true;
        self.0.ready.notify_all();
    }
}

struct Rejecting;

impl Handler for Rejecting {
    fn handle_request(&self, method: &str, _params: &[u8]) -> Result<Payload, String> {
        Err(format!("unexpected request {method}"))
    }

    fn handle_notification(&self, _method: &str, _params: &[u8]) {}
}

/// A connection whose peer is driven by the test through the returned
/// reader (the connection's requests) and writer (its input).
fn connected() -> (
    Arc<AsyncConn<PipeWriter>>,
    jsonrpc::MessageReader<PipeReader>,
    jsonrpc::MessageWriter<PipeWriter>,
    MessageReader<PipeReader>,
) {
    let (to_peer_reader, to_peer_writer) = pipe();
    let (from_peer_reader, from_peer_writer) = pipe();
    (
        Arc::new(AsyncConn::new(to_peer_writer, Box::new(Rejecting))),
        jsonrpc::MessageReader::new(to_peer_reader),
        jsonrpc::MessageWriter::new(from_peer_writer),
        MessageReader::new(from_peer_reader),
    )
}

#[test]
fn concurrent_calls_take_the_responses_of_their_ids() {
    let (conn, mut peer_in, mut peer_out, mut input) = connected();
    std::thread::scope(|scope| {
        let runner = Arc::clone(&conn);
        let run = scope.spawn(move || runner.run(&mut input));
        let calls: Vec<_> = (0..4)
            .map(|index| {
                let conn = Arc::clone(&conn);
                scope.spawn(move || conn.call(&format!("method{index}"), "", None))
            })
            .collect();
        // The peer answers in reverse order of arrival, each with its
        // method's name.
        let mut requests = Vec::new();
        for _ in 0..4 {
            requests.push(peer_in.read_message().unwrap().unwrap());
        }
        for request in requests.iter().rev() {
            peer_out
                .write_response(
                    request.id.as_ref().unwrap(),
                    &Payload::Json(format!("\"{}\"", request.method)),
                )
                .unwrap();
        }
        for (index, call) in calls.into_iter().enumerate() {
            assert_eq!(
                call.join().unwrap(),
                Ok(format!("\"method{index}\"").into_bytes())
            );
        }
        drop(peer_out);
        assert_eq!(run.join().unwrap(), Ok(()));
    });
}

#[test]
fn a_remote_error_fails_the_call_with_tsgo_s_message() {
    let (conn, mut peer_in, mut peer_out, mut input) = connected();
    std::thread::scope(|scope| {
        let runner = Arc::clone(&conn);
        scope.spawn(move || runner.run(&mut input));
        let call = {
            let conn = Arc::clone(&conn);
            scope.spawn(move || conn.call("transform", "{}", None))
        };
        let request = peer_in.read_message().unwrap().unwrap();
        assert_eq!(request.id, Some(Id::String("api1".to_owned())));
        assert_eq!(request.params, b"{}");
        peer_out
            .write_error(
                request.id.as_ref().unwrap(),
                &ResponseError {
                    code: -32601,
                    message: "no such method".to_owned(),
                },
            )
            .unwrap();
        assert_eq!(
            call.join().unwrap(),
            Err(CallError::Failed(Error::new(
                "ipc: remote error [-32601]: no such method"
            )))
        );
        drop(peer_out);
    });
}

#[test]
fn a_call_returns_when_the_peer_closes() {
    let (conn, mut peer_in, peer_out, mut input) = connected();
    std::thread::scope(|scope| {
        let runner = Arc::clone(&conn);
        let run = scope.spawn(move || runner.run(&mut input));
        let call = {
            let conn = Arc::clone(&conn);
            scope.spawn(move || conn.call("transform", "", None))
        };
        peer_in.read_message().unwrap().unwrap();
        drop(peer_out);
        assert_eq!(run.join().unwrap(), Ok(()));
        assert_eq!(
            call.join().unwrap(),
            Err(CallError::Failed(Error::new(CONN_CLOSED)))
        );
    });
}

#[test]
fn calls_after_a_read_failure_fail_at_once_with_its_cause() {
    let (to_peer_reader, to_peer_writer) = pipe();
    let (from_peer_reader, mut from_peer_writer) = pipe();
    let conn = AsyncConn::new(to_peer_writer, Box::new(Rejecting));
    from_peer_writer.write_all(b"oops\n").unwrap();
    drop(from_peer_writer);
    let error = conn
        .run(&mut MessageReader::new(from_peer_reader))
        .expect_err("a malformed header fails the read loop");
    let terminal = format!("{CONN_CLOSED}\n{error}");
    // Not the time limit: the call fails at once.
    assert_eq!(
        conn.call("transform", "", Some(Duration::from_secs(5))),
        Err(CallError::Failed(Error::new(terminal.clone())))
    );
    assert_eq!(conn.notify("changed", ""), Err(Error::new(terminal)));
    drop(to_peer_reader);
}

#[test]
fn an_incoming_request_is_answered_by_the_handler() {
    let (conn, mut peer_in, mut peer_out, mut input) = connected();
    std::thread::scope(|scope| {
        let runner = Arc::clone(&conn);
        let run = scope.spawn(move || runner.run(&mut input));
        peer_out
            .write_request(&Id::Int(7), "openFile", "{}")
            .unwrap();
        peer_out.write_notification("log", "\"hi\"").unwrap();
        let answer = peer_in.read_message().unwrap().unwrap();
        assert_eq!(answer.id, Some(Id::Int(7)));
        assert_eq!(
            answer.error,
            Some(ResponseError {
                code: CODE_INTERNAL_ERROR,
                message: "unexpected request openFile".to_owned(),
            })
        );
        drop(peer_out);
        assert_eq!(run.join().unwrap(), Ok(()));
    });
}

struct FailingWriter;

impl Write for FailingWriter {
    fn write(&mut self, _bytes: &[u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("response write failed"))
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_response_write_failure_ends_the_connection_and_is_its_cause_once() {
    let (from_peer_reader, from_peer_writer) = pipe();
    let mut peer_out = jsonrpc::MessageWriter::new(from_peer_writer);
    peer_out
        .write_request(&Id::Int(1), "transform", "")
        .unwrap();
    drop(peer_out);
    let conn = AsyncConn::new(FailingWriter, Box::new(Rejecting));
    let error = conn
        .run(&mut MessageReader::new(from_peer_reader))
        .expect_err("the response cannot be written");
    assert!(error.message().contains("response write failed"), "{error}");
    let Err(CallError::Failed(terminal)) = conn.call("transform", "", None) else {
        panic!("a call after the failure fails");
    };
    assert_eq!(
        terminal.message().matches("response write failed").count(),
        1
    );
}

#[test]
fn a_call_without_a_response_times_out() {
    let (conn, mut peer_in, peer_out, mut input) = connected();
    std::thread::scope(|scope| {
        let runner = Arc::clone(&conn);
        let run = scope.spawn(move || runner.run(&mut input));
        assert_eq!(
            conn.call("initialize", "{}", Some(Duration::from_millis(20))),
            Err(CallError::TimedOut)
        );
        peer_in.read_message().unwrap().unwrap();
        drop(peer_out);
        assert_eq!(run.join().unwrap(), Ok(()));
    });
}
