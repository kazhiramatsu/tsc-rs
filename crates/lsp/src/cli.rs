//! tsgo `runLSP` (`cmd/tsc/lsp.go`): `tsc-rs --lsp --stdio` serves the
//! language server protocol over the standard streams.

use std::io::{self, Write};
use std::sync::{Arc, OnceLock, Weak};
use std::time::Duration;

use tsc_host::flag::{self, Flag, Kind, Value};

use crate::server::{Server, ServerOptions};

/// The flags of tsgo `runLSP`. `--pipe` and `--socket` are accepted and
/// ignored as in tsgo; tsc-rs writes no Go profiles for `--pprofDir`, as for
/// the compiler's option of that name.
const LSP_FLAGS: &[Flag] = &[
    Flag {
        name: "stdio",
        kind: Kind::Bool,
        usage: "use stdio for communication",
    },
    Flag {
        name: "pprofDir",
        kind: Kind::String,
        usage: "Generate pprof CPU/memory profiles to the given directory.",
    },
    Flag {
        name: "pipe",
        kind: Kind::String,
        usage: "use named pipe for communication",
    },
    Flag {
        name: "socket",
        kind: Kind::String,
        usage: "use socket for communication",
    },
    Flag {
        name: "clientProcessId",
        kind: Kind::Int,
        usage: "use the given PID for the parent process watchdog",
    },
];

/// tsgo `runLSP`: 0 when the client's input ends, 1 when the server stops
/// otherwise (`exit` included, as tsgo's `context canceled`), 2 for invalid
/// flags. `process_alive` is tsgo `isProcessAlive` where the platform has
/// one; the parent process watchdog runs only then.
pub fn run_lsp(arguments: &[String], process_alive: Option<fn(i32) -> bool>) -> i32 {
    let values = match flag::parse(arguments, LSP_FLAGS) {
        Ok(values) => values,
        Err(message) => {
            let mut stderr = io::stderr().lock();
            if let Some(message) = message {
                let _ = writeln!(stderr, "{message}");
            }
            let _ = stderr.write_all(flag::usage("lsp", LSP_FLAGS, |_| None).as_bytes());
            return 2;
        }
    };
    let mut stdio = false;
    let mut client_process_id = 0;
    for (name, value) in values {
        match (name, value) {
            ("stdio", Value::Bool(enabled)) => stdio = enabled,
            ("clientProcessId", Value::Int(id)) => client_process_id = id,
            _ => {}
        }
    }
    if !stdio {
        let _ = writeln!(io::stderr(), "only stdio is supported");
        return 1;
    }
    let cwd = std::env::current_dir()
        .map(|directory| tsc_program::uri::rooted_path_from_absolute(&directory.to_string_lossy()))
        .unwrap_or_default();
    let server_cell: Arc<OnceLock<Weak<Server>>> = Arc::default();
    let set_parent_process_id = process_alive.and_then(|alive| {
        if client_process_id > 0 {
            start_parent_process_watchdog(&server_cell, alive, client_process_id);
            return None;
        }
        let cell = Arc::clone(&server_cell);
        let watch: Box<dyn Fn(i32) + Send + Sync> =
            Box::new(move |id| start_parent_process_watchdog(&cell, alive, i64::from(id)));
        Some(watch)
    });
    let server = Server::new(ServerOptions {
        stderr: Box::new(io::stderr()),
        cwd,
        set_parent_process_id,
    });
    let _ = server_cell.set(Arc::downgrade(&server));
    match server.run(Box::new(io::stdin()), Box::new(io::stdout())) {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(io::stderr(), "{error}");
            1
        }
    }
}

/// tsgo `startParentProcessWatchdog`: every five seconds, stops the server
/// once the client's process has exited.
fn start_parent_process_watchdog(
    server: &Arc<OnceLock<Weak<Server>>>,
    alive: fn(i32) -> bool,
    parent: i64,
) {
    let Ok(parent) = i32::try_from(parent) else {
        return;
    };
    if parent <= 0 {
        return;
    }
    let server = Arc::clone(server);
    let _ = std::thread::Builder::new()
        .name("tsc-rs-lsp-watchdog".to_owned())
        .spawn(move || loop {
            std::thread::sleep(Duration::from_secs(5));
            let Some(server) = server.get().and_then(Weak::upgrade) else {
                continue;
            };
            if server.is_stopped() {
                return;
            }
            if !alive(parent) {
                let _ = writeln!(
                    io::stderr(),
                    "Parent process {parent} has exited, shutting down."
                );
                server.stop();
                return;
            }
        });
}
