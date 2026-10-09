//! tsgo `api/server.go` and `cmd/tsc/api.go` (19dadef8): `tsc-rs --api`,
//! one API session served over standard input and output, or over a Unix
//! domain socket. MessagePack with synchronous handling is the default;
//! `--async` serves JSON-RPC.

use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::Arc;

use tsc_compiler::system::{BundledFs, EMBEDDED_LIBRARY_DIRECTORY};
use tsc_compiler::DEFAULT_LOAD_LIMITS;
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
}

/// tsgo `StdioServer.Run`: serves one client until its input ends.
pub fn run_server(options: &ServerOptions) -> Result<(), String> {
    let case_sensitive = FsCompilerHost::from_process()
        .map_err(|error| format!("{error:?}"))?
        .use_case_sensitive_file_names();
    let bundled = BundledFs::new(OsFs::new(case_sensitive));
    // tsgo wraps the file system with the client's callbacks when asked.
    let (fs, callback_fs): (Arc<dyn FileSystem>, _) = if options.callbacks.is_empty() {
        (Arc::new(bundled), None)
    } else {
        let callback_fs = Arc::new(CallbackFs::new(bundled, &options.callbacks)?);
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
        },
        fs,
        // Only MessagePack sends binary responses.
        !options.async_protocol,
    ));
    let (reader, writer) = match &options.pipe_path {
        Some(path) => accept_pipe(path)?,
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
    // Go's listener removes its socket when it closes.
    if let Some(path) = &options.pipe_path {
        let _ = std::fs::remove_file(path);
    }
    result
}

type Streams = (Box<dyn Read + Send>, Box<dyn Write + Send>);

/// tsgo `PipeTransport`: the first client of a Unix domain socket, a file
/// at `path` replaced.
#[cfg(unix)]
fn accept_pipe(path: &str) -> Result<Streams, String> {
    let _ = std::fs::remove_file(path);
    let listener = std::os::unix::net::UnixListener::bind(path)
        .map_err(|error| format!("failed to create pipe transport: {error}"))?;
    let (stream, _) = listener
        .accept()
        .map_err(|error| format!("failed to accept connection: {error}"))?;
    let reader = stream
        .try_clone()
        .map_err(|error| format!("failed to accept connection: {error}"))?;
    Ok((Box::new(reader), Box::new(stream)))
}

/// Windows named pipes are not ported yet.
#[cfg(not(unix))]
fn accept_pipe(_path: &str) -> Result<Streams, String> {
    Err(
        "failed to create pipe transport: named pipes are not supported on this platform yet"
            .to_owned(),
    )
}

/// tsgo `runAPI`: the API server's command line (`tsc-rs --api ...`) and
/// its exit status: 0, 1 when the server fails, 2 for invalid flags.
pub fn run_api(arguments: &[String]) -> i32 {
    let default_directory = std::env::current_dir()
        .map(|directory| directory.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    let options = match parse_api_flags(arguments, &default_directory) {
        Ok(options) => options,
        Err(message) => {
            let mut stderr = io::stderr().lock();
            if let Some(message) = message {
                let _ = writeln!(stderr, "{message}");
            }
            let _ = stderr.write_all(api_usage(&default_directory).as_bytes());
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

/// The flags of tsgo `parseAPIFlags`, sorted by name: name, value type
/// (empty for a boolean), usage.
const API_FLAGS: &[(&str, &str, &str)] = &[
    (
        "async",
        "",
        "use JSON-RPC protocol instead of MessagePack (for async API)",
    ),
    (
        "callbacks",
        "string",
        "comma-separated list of FS callbacks to enable (readFile,fileExists,directoryExists,getAccessibleEntries,realpath)",
    ),
    ("cwd", "string", "current working directory"),
    (
        "pipe",
        "string",
        "use named pipe or Unix domain socket for communication instead of stdio",
    ),
    (
        "runExternalCode",
        "",
        "allow projects to execute configured external plugins",
    ),
    (
        "timing",
        "",
        "collect per-request server processing time, folded into the client's timing snapshot",
    ),
];

/// tsgo `parseAPIFlags` with Go's `flag` package: `-name` or `--name`, a
/// value after `=` or in the next argument, parsing up to the first
/// argument that is not a flag. An error carries Go's message, or none for
/// `-h` and `-help`.
pub fn parse_api_flags(
    arguments: &[String],
    default_directory: &str,
) -> Result<ServerOptions, Option<String>> {
    let mut options = ServerOptions {
        current_directory: default_directory.to_owned(),
        ..ServerOptions::default()
    };
    let mut callbacks = String::new();
    let mut remaining = arguments.iter();
    while let Some(argument) = remaining.next() {
        let Some(flag) = argument
            .strip_prefix("--")
            .or_else(|| argument.strip_prefix('-'))
            .filter(|flag| argument.len() > 1 && !flag.is_empty())
        else {
            break;
        };
        if flag.starts_with('-') || flag.starts_with('=') {
            return Err(Some(format!("bad flag syntax: {argument}")));
        }
        let (name, value) = match flag.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (flag, None),
        };
        let Some((_, kind, _)) = API_FLAGS.iter().find(|(known, _, _)| *known == name) else {
            if name == "help" || name == "h" {
                return Err(None);
            }
            return Err(Some(format!("flag provided but not defined: -{name}")));
        };
        if kind.is_empty() {
            let enabled = match value {
                None => true,
                Some(value) => parse_go_bool(value).ok_or_else(|| {
                    Some(format!(
                        "invalid boolean value {value:?} for -{name}: parse error"
                    ))
                })?,
            };
            match name {
                "async" => options.async_protocol = enabled,
                "timing" => options.collect_timing = enabled,
                // Content mappers (external code) are not ported.
                _ => {}
            }
        } else {
            let value = match value {
                Some(value) => value.to_owned(),
                None => remaining
                    .next()
                    .cloned()
                    .ok_or_else(|| Some(format!("flag needs an argument: -{name}")))?,
            };
            match name {
                "cwd" => options.current_directory = value,
                "pipe" => options.pipe_path = Some(value).filter(|path| !path.is_empty()),
                _ => callbacks = value,
            }
        }
    }
    if !callbacks.is_empty() {
        options.callbacks = callbacks.split(',').map(str::to_owned).collect();
    }
    Ok(options)
}

/// Go's `strconv.ParseBool`.
fn parse_go_bool(value: &str) -> Option<bool> {
    match value {
        "1" | "t" | "T" | "TRUE" | "true" | "True" => Some(true),
        "0" | "f" | "F" | "FALSE" | "false" | "False" => Some(false),
        _ => None,
    }
}

/// Go's `FlagSet.PrintDefaults` under `Usage of api:`.
fn api_usage(default_directory: &str) -> String {
    let mut usage = "Usage of api:\n".to_owned();
    for (name, kind, text) in API_FLAGS {
        usage.push_str("  -");
        usage.push_str(name);
        if !kind.is_empty() {
            usage.push(' ');
            usage.push_str(kind);
        }
        usage.push_str("\n    \t");
        usage.push_str(text);
        if *name == "cwd" && !default_directory.is_empty() {
            usage.push_str(&format!(" (default {default_directory:?})"));
        }
        usage.push('\n');
    }
    usage
}
