//! tsgo `api/server.go` and `cmd/tsc/api.go` (19dadef8): `tsc-rs --api`,
//! one API session served over standard input and output, or over a Unix
//! domain socket. MessagePack with synchronous handling is the default;
//! `--async` serves JSON-RPC.

use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::Arc;

use tsc_compiler::content_mapper_process::NativeSpawner;
use tsc_compiler::system::{BundledFs, EMBEDDED_LIBRARY_DIRECTORY};
use tsc_compiler::DEFAULT_LOAD_LIMITS;
use tsc_host::flag::{self, Flag, Kind, Value};
use tsc_host::vfs::{FileSystem, OsFs};
use tsc_host::{CompilerHost, FsCompilerHost};
use tsc_program::LibraryCatalog;
use tsc_project::SessionOptions;

use crate::callback_fs::CallbackFs;
use crate::ipc::jsonrpc::JsonRpcProtocol;
use crate::ipc::{Conn, Mode, Protocol};
use crate::msgpack::MessagePackProtocol;
use crate::session::Session;

/// tsgo `StdioServerOptions`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ServerOptions {
    /// The session's current directory.
    pub current_directory: String,
    /// A Unix domain socket to accept the client on instead of standard
    /// input and output.
    pub pipe_path: Option<String>,
    /// The file system operations the client answers (tsgo's callback file
    /// system).
    pub callbacks: Vec<String>,
    /// JSON-RPC instead of MessagePack.
    pub async_protocol: bool,
    /// Whether each request's processing time is collected for
    /// `getServerTiming`.
    pub collect_timing: bool,
    /// tsgo `UseCaseSensitiveFileNames`: the file system's case sensitivity
    /// over the OS's (`--useCaseSensitiveFileNames`, which the command line
    /// always sets).
    pub use_case_sensitive_file_names: Option<bool>,
    /// tsgo `RunExternalCode`: the projects' content mappers may run
    /// (`--runExternalCode`).
    pub run_external_code: bool,
}

/// tsgo `StdioServer.Run`: serves one client until its input ends.
pub fn run_server(options: &ServerOptions) -> Result<(), String> {
    let case_sensitive = FsCompilerHost::from_process()
        .map_err(|error| format!("{error:?}"))?
        .use_case_sensitive_file_names();
    let bundled = BundledFs::new(OsFs::new(case_sensitive));
    // tsgo wraps the file system when callbacks or a case sensitivity are
    // asked for.
    let (fs, callback_fs): (Arc<dyn FileSystem>, _) =
        if options.callbacks.is_empty() && options.use_case_sensitive_file_names.is_none() {
            (Arc::new(bundled), None)
        } else {
            let callback_fs = Arc::new(CallbackFs::new(
                bundled,
                &options.callbacks,
                options.use_case_sensitive_file_names,
            )?);
            (
                Arc::clone(&callback_fs) as Arc<dyn FileSystem>,
                Some(callback_fs),
            )
        };
    let session = Arc::new(Session::new(
        SessionOptions {
            current_directory: options.current_directory.clone(),
            library_catalog: LibraryCatalog::typescript_7_1(Path::new(EMBEDDED_LIBRARY_DIRECTORY)),
            load_limits: DEFAULT_LOAD_LIMITS,
            run_external_code: options.run_external_code,
            // tsgo's API sessions: UTF-8 positions, no watching,
            // logging or published diagnostics.
            position_encoding: tsc_lsproto::PositionEncodingKind::UTF8,
            watch_enabled: false,
            logging_enabled: false,
            telemetry_enabled: false,
            push_diagnostics_enabled: false,
            debounce_delay: std::time::Duration::ZERO,
            client_capabilities: Default::default(),
            typings_location: None,
        },
        fs,
        // tsgo `ContentMapperSpawner: system`.
        Some(Arc::new(NativeSpawner)),
        // Only MessagePack sends binary responses.
        !options.async_protocol,
    ));
    let (reader, writer) = match &options.pipe_path {
        Some(path) => PipeTransport::new(path)
            .map_err(|error| format!("failed to create pipe transport: {error}"))?
            .accept()
            .and_then(|connection| connection.streams())
            .map_err(|error| format!("failed to accept connection: {error}"))?,
        None => (
            Box::new(io::stdin()) as Box<dyn Read + Send>,
            Box::new(io::stdout()) as Box<dyn Write + Send>,
        ),
    };
    let (protocol, mode): (Box<dyn Protocol>, _) = if options.async_protocol {
        (Box::new(JsonRpcProtocol::new(reader, writer)), Mode::Async)
    } else {
        (
            Box::new(MessagePackProtocol::new(reader, writer)),
            Mode::Sync,
        )
    };
    let mut conn = Conn::new(
        protocol,
        Arc::clone(&session) as Arc<dyn crate::ipc::Handler>,
        mode,
    );
    conn.set_collect_timing(options.collect_timing);
    let conn = Arc::new(conn);
    if let Some(callback_fs) = &callback_fs {
        callback_fs.set_connection(&conn);
    }
    session.set_connection(&conn);
    let result = conn.run().map_err(|error| error.to_string());
    // tsgo `defer session.Close()`.
    session.close();
    // Go's listener removes its socket when it closes.
    if let Some(path) = &options.pipe_path {
        let _ = std::fs::remove_file(path);
    }
    result
}

type Streams = (Box<dyn Read + Send>, Box<dyn Write + Send>);

/// tsgo `PipeTransport`: a Unix domain socket the clients connect to, a file
/// at its path replaced.
#[cfg(unix)]
pub struct PipeTransport {
    listener: std::os::unix::net::UnixListener,
    closer: PipeCloser,
}

/// What closes a [`PipeTransport`] from another thread (tsgo `Close`): an
/// accept waiting for a client ends with an error, as Go's listener's.
#[derive(Clone)]
pub struct PipeCloser {
    path: String,
    closed: Arc<std::sync::atomic::AtomicBool>,
}

impl PipeCloser {
    pub fn close(&self) {
        if !self.closed.swap(true, std::sync::atomic::Ordering::SeqCst) {
            // The waiting accept takes this connection and sees the close.
            #[cfg(unix)]
            let _ = std::os::unix::net::UnixStream::connect(&self.path);
        }
    }
}

/// A client's connection through a [`PipeTransport`].
#[cfg(unix)]
pub struct PipeConnection {
    stream: std::os::unix::net::UnixStream,
}

#[cfg(unix)]
impl PipeTransport {
    /// tsgo `NewPipeTransport`.
    pub fn new(path: &str) -> io::Result<Self> {
        let _ = std::fs::remove_file(path);
        Ok(Self {
            listener: std::os::unix::net::UnixListener::bind(path)?,
            closer: PipeCloser {
                path: path.to_owned(),
                closed: Arc::default(),
            },
        })
    }

    /// tsgo `Accept`: the next client.
    pub fn accept(&self) -> io::Result<PipeConnection> {
        let (stream, _) = self.listener.accept()?;
        if self.closer.closed.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(io::Error::other("use of closed network connection"));
        }
        Ok(PipeConnection { stream })
    }

    pub fn closer(&self) -> PipeCloser {
        self.closer.clone()
    }

    /// tsgo `Close`: Go's listener removes its socket when it closes.
    pub fn close(self) {
        self.closer
            .closed
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let _ = std::fs::remove_file(&self.closer.path);
    }
}

#[cfg(unix)]
impl PipeConnection {
    /// The streams a protocol reads and writes.
    pub fn streams(&self) -> io::Result<Streams> {
        let reader = self.stream.try_clone()?;
        let writer = self.stream.try_clone()?;
        Ok((Box::new(reader), Box::new(writer)))
    }

    /// tsgo `Close` of the connection: its reads end.
    pub fn close(&self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}

/// tsgo `ipc.GeneratePipePath`: the pipe of `name` in the temporary
/// directory (a named pipe on Windows).
pub fn generate_pipe_path(name: &str) -> String {
    if cfg!(windows) {
        return format!(r"\\.\pipe\{name}");
    }
    std::env::temp_dir()
        .join(name)
        .to_string_lossy()
        .into_owned()
}

/// tsgo's API connection of a language server's session
/// (`ipc.NewAsyncConn`): JSON-RPC over the client's connection, which the
/// session's callbacks go through, until it ends.
pub fn run_async_connection(
    session: &Arc<Session>,
    connection: &PipeConnection,
) -> Result<(), String> {
    let (reader, writer) = connection.streams().map_err(|error| error.to_string())?;
    let conn = Arc::new(Conn::new(
        Box::new(JsonRpcProtocol::new(reader, writer)),
        Arc::clone(session) as Arc<dyn crate::ipc::Handler>,
        Mode::Async,
    ));
    session.set_connection(&conn);
    conn.run().map_err(|error| error.to_string())
}

/// Windows named pipes are not ported yet.
#[cfg(not(unix))]
pub struct PipeTransport(std::convert::Infallible);

#[cfg(not(unix))]
pub struct PipeConnection(std::convert::Infallible);

#[cfg(not(unix))]
impl PipeTransport {
    pub fn new(_path: &str) -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "named pipes are not supported on this platform yet",
        ))
    }

    pub fn accept(&self) -> io::Result<PipeConnection> {
        match self.0 {}
    }

    pub fn closer(&self) -> PipeCloser {
        match self.0 {}
    }

    pub fn close(self) {}
}

#[cfg(not(unix))]
impl PipeConnection {
    pub fn streams(&self) -> io::Result<Streams> {
        match self.0 {}
    }

    pub fn close(&self) {
        match self.0 {}
    }
}

/// tsgo `runAPI`: the API server's command line (`tsc-rs --api ...`) and
/// its exit status: 0, 1 when the server fails, 2 for invalid flags.
pub fn run_api(arguments: &[String]) -> i32 {
    let default_directory = std::env::current_dir()
        .map(|directory| directory.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    // tsgo's flag defaults to the OS file system's case sensitivity.
    let default_case_sensitive = FsCompilerHost::from_process()
        .map(|host| host.use_case_sensitive_file_names())
        .unwrap_or(true);
    let options = match parse_api_flags(arguments, &default_directory, default_case_sensitive) {
        Ok(options) => options,
        Err(message) => {
            let mut stderr = io::stderr().lock();
            if let Some(message) = message {
                let _ = writeln!(stderr, "{message}");
            }
            let _ =
                stderr.write_all(api_usage(&default_directory, default_case_sensitive).as_bytes());
            return 2;
        }
    };
    match run_server(&options) {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(io::stderr(), "{error}");
            1
        }
    }
}

/// The flags of tsgo `parseAPIFlags`.
const API_FLAGS: &[Flag] = &[
    Flag {
        name: "async",
        kind: Kind::Bool,
        usage: "use JSON-RPC protocol instead of MessagePack (for async API)",
    },
    Flag {
        name: "callbacks",
        kind: Kind::String,
        usage: "comma-separated list of FS callbacks and defaults to enable",
    },
    Flag {
        name: "cwd",
        kind: Kind::String,
        usage: "current working directory",
    },
    Flag {
        name: "pipe",
        kind: Kind::String,
        usage: "use named pipe or Unix domain socket for communication instead of stdio",
    },
    Flag {
        name: "runExternalCode",
        kind: Kind::Bool,
        usage: "allow projects to execute configured external plugins",
    },
    Flag {
        name: "timing",
        kind: Kind::Bool,
        usage:
            "collect per-request server processing time, folded into the client's timing snapshot",
    },
    Flag {
        name: "useCaseSensitiveFileNames",
        kind: Kind::Bool,
        usage: "treat filesystem paths as case-sensitive",
    },
];

/// tsgo `parseAPIFlags` with Go's `flag` package. An error carries Go's
/// message, or none for `-h` and `-help`.
pub fn parse_api_flags(
    arguments: &[String],
    default_directory: &str,
    default_case_sensitive: bool,
) -> Result<ServerOptions, Option<String>> {
    let mut options = ServerOptions {
        current_directory: default_directory.to_owned(),
        use_case_sensitive_file_names: Some(default_case_sensitive),
        ..ServerOptions::default()
    };
    let mut callbacks = String::new();
    for (name, value) in flag::parse(arguments, API_FLAGS)? {
        match (name, value) {
            ("async", Value::Bool(enabled)) => options.async_protocol = enabled,
            ("timing", Value::Bool(enabled)) => options.collect_timing = enabled,
            ("useCaseSensitiveFileNames", Value::Bool(enabled)) => {
                options.use_case_sensitive_file_names = Some(enabled);
            }
            ("runExternalCode", Value::Bool(enabled)) => options.run_external_code = enabled,
            ("cwd", Value::String(value)) => options.current_directory = value,
            ("pipe", Value::String(value)) => {
                options.pipe_path = Some(value).filter(|path| !path.is_empty());
            }
            (_, Value::String(value)) => callbacks = value,
            _ => {}
        }
    }
    if !callbacks.is_empty() {
        options.callbacks = callbacks.split(',').map(str::to_owned).collect();
    }
    // tsgo `tspath.ToRootedDirectoryPath(flags.cwd, system.cwd)`: normalized,
    // without a trailing separator.
    options.current_directory = tsc_program::get_normalized_absolute_path(
        tsc_diagnostics::JsStr::from_str(&options.current_directory),
        tsc_diagnostics::JsStr::from_str(default_directory),
    )
    .to_string_lossy()
    .into_owned();
    Ok(options)
}

/// Go's `FlagSet.PrintDefaults` under `Usage of api:`: a default other than
/// the zero value is printed.
fn api_usage(default_directory: &str, default_case_sensitive: bool) -> String {
    flag::usage("api", API_FLAGS, |name| match name {
        "cwd" if !default_directory.is_empty() => Some(default_directory.to_owned()),
        "useCaseSensitiveFileNames" if default_case_sensitive => Some("true".to_owned()),
        _ => None,
    })
}
