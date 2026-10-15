//! tsgo `runLSP` (`cmd/tsc/lsp.go`): `tsc-rs --lsp --stdio` serves the
//! language server protocol over the standard streams.

use std::io::{self, Write};
use std::sync::{Arc, OnceLock, Weak};
use std::time::Duration;

use tsc_compiler::system::{BundledFs, EMBEDDED_LIBRARY_DIRECTORY};
use tsc_host::flag::{self, Flag, Kind, Value};
use tsc_host::vfs::{FileSystem, OsFs};
use tsc_host::{CompilerHost, FsCompilerHost};

use crate::server::{Server, ServerOptions};

/// The flags of tsgo `runLSP`. `--pipe` and `--socket` are accepted and
/// ignored as in tsgo; `--pprofDir` writes a CPU profile (tsgo's memory
/// profile is not supported).
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
    let mut pprof_dir = String::new();
    for (name, value) in values {
        match (name, value) {
            ("stdio", Value::Bool(enabled)) => stdio = enabled,
            ("clientProcessId", Value::Int(id)) => client_process_id = id,
            ("pprofDir", Value::String(directory)) => pprof_dir = directory,
            _ => {}
        }
    }
    if !stdio {
        let _ = writeln!(io::stderr(), "only stdio is supported");
        return 1;
    }
    let profile = (!pprof_dir.is_empty()).then(|| {
        let _ = writeln!(
            io::stderr(),
            "pprof profiles will be written to: {pprof_dir}"
        );
        tsc_compiler::pprof::begin_profiling(&pprof_dir)
    });
    // tsgo `bundled.WrapFS(osvfs.FS())` and `bundled.LibPath()`.
    let case_sensitive = FsCompilerHost::from_process()
        .map(|host| host.use_case_sensitive_file_names())
        .unwrap_or(true);
    let fs: Arc<dyn FileSystem> = Arc::new(BundledFs::new(OsFs::new(case_sensitive)));
    let cwd = std::env::current_dir()
        .map(|directory| tsc_program::uri::rooted_path_from_absolute(&directory.to_string_lossy()))
        .unwrap_or_default();
    let cwd_for_typings = cwd.clone();
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
        fs,
        default_library_path: EMBEDDED_LIBRARY_DIRECTORY.to_owned(),
        spawner: Some(Arc::new(
            tsc_compiler::content_mapper_process::NativeSpawner,
        )),
        // tsgo `cmd/tsc/lsp.go`.
        progress_delay: std::time::Duration::from_millis(250),
        typings_location: Some(global_typings_cache_location(&cwd_for_typings)),
        npm_executor: Some(Arc::new(NpmCommand)),
    });
    let _ = server_cell.set(Arc::downgrade(&server));
    let code = match server.run(Box::new(io::stdin()), Box::new(io::stdout())) {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(io::stderr(), "{error}");
            1
        }
    };
    if let Some(profile) = profile {
        profile.stop(&mut io::stderr());
    }
    code
}

/// tsgo `osvfs.GetGlobalTypingsCacheLocation`: `typescript/<major.minor>`
/// (`Microsoft/TypeScript/<major.minor>` on Windows) in the user's cache
/// directory (Go's `os.UserCacheDir`), else in the temporary directory;
/// against `cwd` (tsgo `ToRootedDirectoryPath`).
fn global_typings_cache_location(cwd: &str) -> String {
    let user_cache_dir = if cfg!(windows) {
        std::env::var_os("LocalAppData").map(std::path::PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|home| std::path::Path::new(&home).join("Library/Caches"))
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(std::path::PathBuf::from)
            .filter(|directory| directory.is_absolute())
            .or_else(|| {
                std::env::var_os("HOME").map(|home| std::path::Path::new(&home).join(".cache"))
            })
    };
    let cache_dir = user_cache_dir.unwrap_or_else(std::env::temp_dir);
    let subdir = if cfg!(windows) {
        "Microsoft/TypeScript"
    } else {
        "typescript"
    };
    let location = format!(
        "{}/{subdir}/{}",
        cache_dir.to_string_lossy(),
        tsc_types::TYPESCRIPT_VERSION_MAJOR_MINOR
    );
    tsc_program::get_normalized_absolute_path(
        tsc_diagnostics::JsStr::from_str(&location),
        tsc_diagnostics::JsStr::from_str(cwd),
    )
    .to_string_lossy()
    .into_owned()
}

/// tsgo `cmd/tsc/lsp.go`'s `NpmInstall`: `npm` with the arguments in `cwd`,
/// its standard output; a failed run's status is the error.
struct NpmCommand;

impl tsc_project::ata::NpmExecutor for NpmCommand {
    fn npm_install(&self, cwd: &str, args: &[String]) -> Result<Vec<u8>, String> {
        let output = std::process::Command::new("npm")
            .args(args)
            .current_dir(cwd)
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|error| error.to_string())?;
        if output.status.success() {
            Ok(output.stdout)
        } else {
            Err(output.status.to_string())
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
