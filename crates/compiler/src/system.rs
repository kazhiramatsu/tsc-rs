//! What a command-line run needs from where it runs (tsgo `tsc.System`): a
//! file system, a current directory, a clock, the environment and an
//! output. The process provides [`NativeSystem`]; a test or a WebAssembly
//! embedding provides its own, and the same driver runs over it.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use tsc_diagnostics::{JsStr, JsString};
use tsc_host::vfs::{FileSystem, OsFs, VfsCompilerHost};
use tsc_host::{CompilerHost, FsCompilerHost, HostError, ParallelSourceReader};

use crate::EmitFileSystem;

mod embedded_libraries {
    include!(concat!(env!("OUT_DIR"), "/embedded_libraries.rs"));
}

/// Where a command-line run reads, writes and reports (tsgo `tsc.System`).
pub trait System: Send + Sync {
    /// The file system the run reads and writes.
    fn fs(&self) -> &dyn FileSystem;

    /// The directory relative paths are taken from.
    fn current_directory(&self) -> &str;

    /// The directory of the standard library files (tsgo
    /// `DefaultLibraryPath`).
    fn default_library_path(&self) -> &str;

    fn now(&self) -> SystemTime;

    /// The time since the run started.
    fn since_start(&self) -> Duration;

    fn env_var(&self, name: &str) -> Option<String>;

    /// Whether the output goes to a terminal (colors follow it).
    fn output_is_terminal(&self) -> bool;

    /// The terminal's width in columns, when known.
    fn terminal_width(&self) -> Option<usize>;

    /// Writes to the run's output (tsgo `Writer`).
    fn write_output(&self, text: &str);

    /// Writes to the run's error output (tsgo `ErrorWriter`).
    fn write_error(&self, text: &str);

    /// A host for one run's program construction over [`System::fs`].
    fn compiler_host(&self) -> Box<dyn CompilerHost + '_> {
        Box::new(VfsCompilerHost::new(self.fs(), self.current_directory()))
    }
}

/// What a test harness observes and adjusts during a run (tsgo
/// `tsc.CommandLineTesting`). Every hook defaults to doing nothing.
pub trait CommandLineTesting: Send + Sync {
    /// After a program's emit, with the files it wrote in order (tsgo
    /// `OnEmittedFiles`). Returns the files whose modification time the
    /// harness set, with the time.
    fn on_emitted_files(&self, _files: &[String]) -> Vec<(String, SystemTime)> {
        Vec::new()
    }

    fn on_list_files_start(&self, _output: &mut String) {}

    fn on_list_files_end(&self, _output: &mut String) {}

    fn on_statistics_start(&self, _output: &mut String) {}

    fn on_statistics_end(&self, _output: &mut String) {}

    fn on_build_status_report_start(&self, _output: &mut String) {}

    fn on_build_status_report_end(&self, _output: &mut String) {}

    /// Writes one resolution trace line (tsgo `GetTrace`); `shared_output`
    /// when it goes to the run's own output rather than a project's buffer.
    fn trace(&self, message: &str, output: &mut String, _shared_output: bool) {
        output.push_str(message);
        output.push('\n');
    }
}

/// The process: its file system, current directory, clock and environment.
/// The output is collected for [`NativeSystem::take_output`].
#[derive(Debug)]
pub struct NativeSystem {
    fs: OsFs,
    current_directory: String,
    started: Instant,
    output_is_terminal: bool,
    output: Mutex<String>,
    error: Mutex<String>,
}

impl NativeSystem {
    /// The process's system: its current directory, the case profile of the
    /// file system there, and whether standard output is a terminal.
    pub fn from_process() -> Result<Self, HostError> {
        use std::io::IsTerminal;
        let host = FsCompilerHost::from_process()?;
        let current_directory = host.current_directory_js()?.to_string_lossy().into_owned();
        Ok(Self {
            fs: OsFs::new(host.use_case_sensitive_file_names()),
            current_directory,
            started: Instant::now(),
            output_is_terminal: io::stdout().is_terminal(),
            output: Mutex::new(String::new()),
            error: Mutex::new(String::new()),
        })
    }

    /// The output and error output written so far, taken.
    pub fn take_output(&self) -> (String, String) {
        (
            std::mem::take(&mut *self.output.lock().unwrap_or_else(PoisonError::into_inner)),
            std::mem::take(&mut *self.error.lock().unwrap_or_else(PoisonError::into_inner)),
        )
    }
}

impl System for NativeSystem {
    fn fs(&self) -> &dyn FileSystem {
        &self.fs
    }

    fn current_directory(&self) -> &str {
        &self.current_directory
    }

    fn default_library_path(&self) -> &str {
        EMBEDDED_LIBRARY_DIRECTORY
    }

    fn now(&self) -> SystemTime {
        SystemTime::now()
    }

    fn since_start(&self) -> Duration {
        self.started.elapsed()
    }

    fn env_var(&self, name: &str) -> Option<String> {
        std::env::var_os(name).map(|value| value.to_string_lossy().into_owned())
    }

    fn output_is_terminal(&self) -> bool {
        self.output_is_terminal
    }

    fn terminal_width(&self) -> Option<usize> {
        None
    }

    fn write_output(&self, text: &str) {
        self.output
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push_str(text);
    }

    fn write_error(&self, text: &str) {
        self.error
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push_str(text);
    }

    fn compiler_host(&self) -> Box<dyn CompilerHost + '_> {
        let filesystem = FsCompilerHost::new(&self.current_directory, self.fs.case_sensitive())
            .expect("the process current directory was read at construction");
        Box::new(NativeCompilerHost::new(filesystem))
    }
}

/// The emitter's file system over a [`System`]'s: the files the emit writes
/// and the directories it creates.
pub(crate) struct SystemEmitFileSystem<'a> {
    fs: &'a dyn FileSystem,
    /// Several artifacts may be written at once (a file system without
    /// observers, like the process's).
    concurrent: bool,
}

impl<'a> SystemEmitFileSystem<'a> {
    pub(crate) fn new(fs: &'a dyn FileSystem, concurrent: bool) -> Self {
        Self { fs, concurrent }
    }
}

impl tsc_emitter::SharedEmitFileSystem for SystemEmitFileSystem<'_> {
    fn write_file(&self, path: JsStr<'_>, bytes: &[u8]) -> Result<(), JsString> {
        let native = path.to_string_lossy();
        self.fs
            .write(&native, bytes)
            .map_err(|error| stable_io_message(&error, "open", path))
    }

    fn create_directory(&self, path: JsStr<'_>) -> Result<(), JsString> {
        let native = path.to_string_lossy();
        match self.fs.create_dir_all(&native) {
            Ok(()) => Ok(()),
            Err(error)
                if error.kind() == io::ErrorKind::AlreadyExists && self.fs.is_dir(&native) =>
            {
                Ok(())
            }
            Err(error) => Err(stable_io_message(&error, "mkdir", path)),
        }
    }

    fn directory_exists(&self, path: JsStr<'_>) -> bool {
        self.fs.is_dir(&path.to_string_lossy())
    }
}

impl EmitFileSystem for SystemEmitFileSystem<'_> {
    fn shared(&self) -> Option<&dyn tsc_emitter::SharedEmitFileSystem> {
        self.concurrent
            .then_some(self as &dyn tsc_emitter::SharedEmitFileSystem)
    }

    fn write_file(&mut self, path: JsStr<'_>, bytes: &[u8]) -> Result<(), JsString> {
        tsc_emitter::SharedEmitFileSystem::write_file(self, path, bytes)
    }

    fn create_directory(&mut self, path: JsStr<'_>) -> Result<(), JsString> {
        tsc_emitter::SharedEmitFileSystem::create_directory(self, path)
    }

    fn directory_exists(&mut self, path: JsStr<'_>) -> bool {
        tsc_emitter::SharedEmitFileSystem::directory_exists(self, path)
    }
}

fn stable_io_message(error: &io::Error, operation: &str, path: JsStr<'_>) -> JsString {
    #[cfg(unix)]
    let known = match error.raw_os_error() {
        Some(2) => Some(("ENOENT", "no such file or directory")),
        Some(13) => Some(("EACCES", "permission denied")),
        Some(17) => Some(("EEXIST", "file already exists")),
        Some(20) => Some(("ENOTDIR", "not a directory")),
        Some(21) => Some(("EISDIR", "illegal operation on a directory")),
        Some(28) => Some(("ENOSPC", "no space left on device")),
        Some(30) => Some(("EROFS", "read-only file system")),
        _ => None,
    };
    #[cfg(not(unix))]
    let known: Option<(&str, &str)> = None;

    if let Some((code, detail)) = known {
        let mut message = JsString::from(format!("{code}: {detail}, {operation} '"));
        message.push_js(path);
        message.push_str("'");
        message
    } else {
        error.to_string().into()
    }
}

/// The directory of the embedded standard library: tsgo's bundled
/// library path (`bundled:///libs`, internal/bundled/embed.go), a URL that
/// no filesystem path equals. Diagnostics and `--listFiles` name a library
/// file `bundled:///libs/lib.dom.d.ts`, as tsgo does, and such a name sorts
/// after every absolute path, which orders the diagnostics as tsgo's.
const EMBEDDED_LIBRARY_DIRECTORY: &str = "bundled:///libs";

/// The native system's compiler host: the process filesystem with the
/// immutable, binary-owned TypeScript 7.1 standard-library directory. User/config/package paths retain ordinary
/// filesystem semantics; only exact immediate children of this private
/// directory are intercepted.
#[derive(Clone, Debug)]
struct NativeCompilerHost {
    filesystem: FsCompilerHost,
    library_directory: PathBuf,
}

impl NativeCompilerHost {
    fn new(filesystem: FsCompilerHost) -> Self {
        Self {
            filesystem,
            library_directory: PathBuf::from(EMBEDDED_LIBRARY_DIRECTORY),
        }
    }

    fn embedded_file_name<'a>(&self, path: &'a Path) -> Option<&'a str> {
        (path.parent() == Some(self.library_directory.as_path()))
            .then(|| path.file_name().and_then(|name| name.to_str()))
            .flatten()
    }

    fn embedded_bytes(&self, path: &Path) -> Option<&'static [u8]> {
        let name = self.embedded_file_name(path)?;
        embedded_libraries::EMBEDDED_LIBRARIES
            .binary_search_by_key(&name, |(candidate, _)| *candidate)
            .ok()
            .map(|index| embedded_libraries::EMBEDDED_LIBRARIES[index].1)
    }
}

impl CompilerHost for NativeCompilerHost {
    fn current_directory_js(&self) -> Result<JsString, HostError> {
        self.filesystem.current_directory_js()
    }
    fn read_file_js(&self, path: JsStr<'_>) -> Result<Option<Vec<u8>>, HostError> {
        if let Some(path) = path
            .as_str()
            .map(Path::new)
            .filter(|path| self.embedded_file_name(path).is_some())
        {
            return Ok(self.embedded_bytes(path).map(<[u8]>::to_vec));
        }
        self.filesystem.read_file_js(path)
    }
    fn file_size_hint_js(&self, path: JsStr<'_>) -> Result<Option<u64>, HostError> {
        if let Some(path) = path
            .as_str()
            .map(Path::new)
            .filter(|path| self.embedded_file_name(path).is_some())
        {
            return Ok(self.embedded_bytes(path).map(|bytes| bytes.len() as u64));
        }
        self.filesystem.file_size_hint_js(path)
    }
    fn file_exists_js(&self, path: JsStr<'_>) -> Result<bool, HostError> {
        if let Some(path) = path
            .as_str()
            .map(Path::new)
            .filter(|path| self.embedded_file_name(path).is_some())
        {
            return Ok(self.embedded_bytes(path).is_some());
        }
        self.filesystem.file_exists_js(path)
    }
    fn directory_exists_js(&self, path: JsStr<'_>) -> Result<bool, HostError> {
        if path
            .as_str()
            .is_some_and(|path| Path::new(path) == self.library_directory)
        {
            return Ok(true);
        }
        self.filesystem.directory_exists_js(path)
    }
    fn read_directory_js(&self, path: JsStr<'_>) -> Result<Vec<JsString>, HostError> {
        if path
            .as_str()
            .is_some_and(|path| Path::new(path) == self.library_directory)
        {
            return Ok(embedded_libraries::EMBEDDED_LIBRARIES
                .iter()
                .map(|(name, _)| {
                    let mut entry = path.to_owned();
                    entry.push_str("/");
                    entry.push_str(name);
                    entry
                })
                .collect());
        }
        self.filesystem.read_directory_js(path)
    }
    fn read_directory_listing_js(
        &self,
        path: JsStr<'_>,
    ) -> Result<Vec<tsc_host::DirectoryListingEntry>, HostError> {
        if path
            .as_str()
            .is_some_and(|path| Path::new(path) == self.library_directory)
        {
            return Ok(embedded_libraries::EMBEDDED_LIBRARIES
                .iter()
                .map(|(name, _)| {
                    let mut entry = path.to_owned();
                    entry.push_str("/");
                    entry.push_str(name);
                    tsc_host::DirectoryListingEntry {
                        path: entry,
                        kind: tsc_host::DirectoryListingKind::File,
                        symlink: false,
                    }
                })
                .collect());
        }
        self.filesystem.read_directory_listing_js(path)
    }
    fn get_directories_js(&self, path: JsStr<'_>) -> Result<Vec<JsString>, HostError> {
        if path
            .as_str()
            .is_some_and(|path| Path::new(path) == self.library_directory)
        {
            return Ok(Vec::new());
        }
        self.filesystem.get_directories_js(path)
    }
    fn realpath_js(&self, path: JsStr<'_>) -> Result<Option<JsString>, HostError> {
        if let Some(native) = path.as_str().map(Path::new) {
            if native == self.library_directory || self.embedded_bytes(native).is_some() {
                return Ok(Some(path.to_owned()));
            }
            if self.embedded_file_name(native).is_some() {
                return Ok(None);
            }
        }
        self.filesystem.realpath_js(path)
    }

    fn current_directory(&self) -> Result<PathBuf, HostError> {
        self.filesystem.current_directory()
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        self.filesystem.use_case_sensitive_file_names()
    }

    fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, HostError> {
        if self.embedded_file_name(path).is_some() {
            return Ok(self.embedded_bytes(path).map(<[u8]>::to_vec));
        }
        self.filesystem.read_file(path)
    }

    fn file_exists(&self, path: &Path) -> Result<bool, HostError> {
        if self.embedded_file_name(path).is_some() {
            return Ok(self.embedded_bytes(path).is_some());
        }
        self.filesystem.file_exists(path)
    }

    fn directory_exists(&self, path: &Path) -> Result<bool, HostError> {
        if path == self.library_directory {
            return Ok(true);
        }
        self.filesystem.directory_exists(path)
    }

    fn read_directory(&self, path: &Path) -> Result<Vec<PathBuf>, HostError> {
        if path == self.library_directory {
            return Ok(embedded_libraries::EMBEDDED_LIBRARIES
                .iter()
                .map(|(name, _)| path.join(name))
                .collect());
        }
        self.filesystem.read_directory(path)
    }

    fn get_directories(&self, path: &Path) -> Result<Vec<PathBuf>, HostError> {
        if path == self.library_directory {
            return Ok(Vec::new());
        }
        self.filesystem.get_directories(path)
    }

    fn realpath(&self, path: &Path) -> Result<Option<PathBuf>, HostError> {
        if path == self.library_directory || self.embedded_bytes(path).is_some() {
            return Ok(Some(path.to_path_buf()));
        }
        if self.embedded_file_name(path).is_some() {
            return Ok(None);
        }
        self.filesystem.realpath(path)
    }

    /// Embedded library bytes are immutable; everything else delegates to the
    /// filesystem host's own answer.
    fn permits_source_read_ahead(&self) -> bool {
        self.filesystem.permits_source_read_ahead()
    }

    fn parallel_source_reader(&self) -> Option<&(dyn ParallelSourceReader + Sync)> {
        self.filesystem
            .parallel_source_reader()
            .is_some()
            .then_some(self as &(dyn ParallelSourceReader + Sync))
    }

    fn parallel_resolution_host(&self) -> Option<&(dyn CompilerHost + Sync)> {
        self.filesystem
            .parallel_resolution_host()
            .is_some()
            .then_some(self as &(dyn CompilerHost + Sync))
    }
}

impl ParallelSourceReader for NativeCompilerHost {
    fn read_source_js(&self, path: JsStr<'_>) -> Result<Option<Vec<u8>>, HostError> {
        CompilerHost::read_file_js(self, path)
    }
}
