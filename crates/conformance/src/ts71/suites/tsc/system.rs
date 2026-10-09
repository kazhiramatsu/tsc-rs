//! tsgo's `TestSys` (internal/execute/tsctests/sys.go and fs.go) over a
//! [`MemFs`]: the test file system, the stepping clock, the environment and
//! terminal, the output and its sanitizer, the file-system differ
//! (testutil/fsbaselineutil/differ.go), and the hooks a command-line run
//! calls under test.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use tsc_compiler::system::{CommandLineTesting, ProgramReport, System};
use tsc_compiler::watch::{
    WatchBackend, WatchEvent, WatchEventKind, WatchEvents, WatchHandle, WatchRequest,
};
use tsc_host::vfs::{
    Clock, DirEntry, EntryContents, FileSystem, MemFs, Metadata, Seed, SteppingClock,
};
use tsc_incremental::{SemanticDiagnosticsState, SignatureUpdateKind};

use super::patience::diff_text;
use super::{build_info, Op, Scenario};

/// tsgo `tscLibPath`.
const LIBRARY_PATH: &str = "/home/src/tslibs/TS/Lib";
const DEFAULT_CURRENT_DIRECTORY: &str = "/home/src/workspaces/project";
/// tsgo `harnessutil.FakeTSVersion`.
pub(super) const FAKE_VERSION: &str = "FakeTSVersion";

/// tsgo `tscDefaultLibContent`: every library file of the test system.
const DEFAULT_LIBRARY_CONTENT: &str = r#"/// <reference no-default-lib="true"/>
interface Boolean {}
interface Function {}
interface CallableFunction {}
interface NewableFunction {}
interface IArguments {}
interface Number { toExponential: any; }
interface Object {}
interface RegExp {}
interface String { charAt: any; }
interface Array<T> { length: number; [n: number]: T; }
interface ReadonlyArray<T> {}
interface SymbolConstructor {
    (desc?: string | number): symbol;
    for(name: string): symbol;
    readonly toStringTag: symbol;
}
declare var Symbol: SymbolConstructor;
interface Symbol {
    readonly [Symbol.toStringTag]: string;
}
declare const console: { log(msg: any): void; };"#;

const LIST_FILES_START: &str = "!!! List files start";
const LIST_FILES_END: &str = "!!! List files end";
const STATISTICS_START: &str = "!!! Statistics start";
const STATISTICS_END: &str = "!!! Statistics end";
const BUILD_STATUS_START: &str = "!!! Build Status Report Start";
const BUILD_STATUS_END: &str = "!!! Build Status Report End";
const WATCH_STATUS_START: &str = "!!! Watch Status Report Start";
const WATCH_STATUS_END: &str = "!!! Watch Status Report End";
const TRACE_START: &str = "!!! Trace start";
const TRACE_END: &str = "!!! Trace end";

/// The first reading of the clock; tsgo starts at the wall clock, and the
/// times only meet the baselines through comparisons and the sanitized
/// status lines.
fn start_time() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_767_225_600)
}

/// tsgo `testFs`: the map file system with the library files no run has
/// read yet, the files written since the last baseline, and the build info
/// version kept fake on disk (with its readable form written beside it).
pub(super) struct TestFs {
    files: MemFs,
    default_libraries: Mutex<BTreeSet<String>>,
    written: Mutex<BTreeSet<String>>,
}

impl TestFs {
    fn forget_library(&self, path: &str) {
        lock(&self.default_libraries).remove(path);
    }

    fn write_build_info(&self, path: &str, contents: &[u8]) -> io::Result<()> {
        let text = std::str::from_utf8(contents)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let mut parsed: serde_json::Value = serde_json::from_str(text).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{path}: the build info does not parse: {error}"),
            )
        })?;
        let real = format!(r#"{{"version":"{}""#, tsc_types::TYPESCRIPT_VERSION);
        let text = if parsed["version"] == tsc_types::TYPESCRIPT_VERSION && text.starts_with(&real)
        {
            parsed["version"] = serde_json::Value::String(FAKE_VERSION.to_owned());
            format!(r#"{{"version":"{FAKE_VERSION}""#) + &text[real.len()..]
        } else {
            text.to_owned()
        };
        let readable = build_info::readable(&parsed, &sanitize_internal_symbol_name(&text));
        self.write(
            &format!("{path}.readable.baseline.txt"),
            readable.as_bytes(),
        )?;
        self.files.write_creating_dirs(path, text.as_bytes())
    }
}

impl FileSystem for TestFs {
    fn case_sensitive(&self) -> bool {
        self.files.case_sensitive()
    }

    fn read(&self, path: &str) -> io::Result<Vec<u8>> {
        self.forget_library(path);
        let contents = self.files.read(path)?;
        if path.ends_with(".tsbuildinfo") {
            // tsgo `readFileHandlingBuildInfo`: a build info at
            // FakeTSVersion reads as the compiler's version, re-marshaled.
            let parsed = std::str::from_utf8(&contents)
                .ok()
                .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok());
            if let Some(mut info) = parsed.filter(|info| info["version"] == FAKE_VERSION) {
                info["version"] =
                    serde_json::Value::String(tsc_types::TYPESCRIPT_VERSION.to_owned());
                return Ok(info.to_string().into_bytes());
            }
        }
        Ok(contents)
    }

    fn metadata(&self, path: &str) -> io::Result<Metadata> {
        self.files.metadata(path)
    }

    fn read_dir(&self, path: &str) -> io::Result<Vec<DirEntry>> {
        self.files.read_dir(path)
    }

    fn canonicalize(&self, path: &str) -> io::Result<String> {
        self.files.canonicalize(path)
    }

    fn write(&self, path: &str, contents: &[u8]) -> io::Result<()> {
        self.forget_library(path);
        lock(&self.written).insert(path.to_owned());
        if path.ends_with(".tsbuildinfo") {
            return self.write_build_info(path, contents);
        }
        self.files.write_creating_dirs(path, contents)
    }

    fn append(&self, path: &str, contents: &[u8]) -> io::Result<()> {
        if self.files.append(path, contents).is_ok() {
            return Ok(());
        }
        self.files.create_dir_all(parent_of(path))?;
        self.files.append(path, contents)
    }

    fn create_dir_all(&self, path: &str) -> io::Result<()> {
        self.files.create_dir_all(path)
    }

    fn remove(&self, path: &str) -> io::Result<()> {
        self.forget_library(path);
        self.files.remove(path)
    }

    fn set_modified(&self, path: &str, modified: SystemTime) -> io::Result<()> {
        self.files.set_modified(path, modified)
    }
}

/// One file of a differ snapshot (tsgo `DiffEntry`).
#[derive(Clone, Debug)]
struct DiffEntry {
    content: String,
    modified: SystemTime,
    written: bool,
    symlink: Option<String>,
}

/// tsgo `fsbaselineutil.Snapshot`.
struct Snapshot {
    entries: BTreeMap<String, DiffEntry>,
    default_libraries: BTreeSet<String>,
}

/// tsgo `TestSys`.
pub(super) struct TestSystem {
    fs: TestFs,
    clock: Arc<SteppingClock>,
    current_directory: String,
    library_path: String,
    env: BTreeMap<String, String>,
    output_is_terminal: bool,
    output: Mutex<String>,
    /// tsgo `TracerForBaselining`'s package.json cache.
    traced_files: Mutex<BTreeSet<String>>,
    program_baselines: Mutex<String>,
    /// tsgo `FSDiffer.serializedDiff`.
    snapshot: Mutex<Option<Snapshot>>,
    /// tsgo `MockWatchBackend.Dirs`: every directory a watch run asked to
    /// watch, by its path (a later request replaces an earlier one).
    watches: Mutex<BTreeMap<String, MockWatch>>,
}

/// tsgo `MockWatch`: one registered directory watch.
struct MockWatch {
    recursive: bool,
    events: WatchEvents,
    closed: Arc<std::sync::atomic::AtomicBool>,
}

/// The handle of a mock watch: dropping it closes the watch.
struct MockWatchHandle {
    closed: Arc<std::sync::atomic::AtomicBool>,
}

impl WatchHandle for MockWatchHandle {}

impl Drop for MockWatchHandle {
    fn drop(&mut self) {
        self.closed
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// tsgo `fsbaselineutil.FileChange`.
pub(super) struct FileChange {
    path: String,
    deleted: bool,
}

impl TestSystem {
    /// tsgo `newTestSys`.
    pub(super) fn new(
        scenario: &Scenario,
        libraries: &[String],
        _for_incremental_correctness: bool,
    ) -> Result<Self, String> {
        let current_directory = if scenario.cwd.is_empty() {
            DEFAULT_CURRENT_DIRECTORY.to_owned()
        } else {
            scenario.cwd.clone()
        };
        let library_path = if scenario.windows_style_root.is_empty() {
            LIBRARY_PATH.to_owned()
        } else {
            format!("{}{}", scenario.windows_style_root, &LIBRARY_PATH[1..])
        };
        let clock = Arc::new(SteppingClock::new(start_time(), Duration::from_secs(1)));
        let seeds = scenario
            .files
            .iter()
            .map(|file| {
                Ok((
                    file.path.clone(),
                    match &file.symlink {
                        Some(target) => Seed::symlink(target),
                        None => Seed::file(file.contents.bytes()?),
                    },
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let files = MemFs::from_entries(
            seeds,
            !scenario.ignore_case,
            Arc::clone(&clock) as Arc<dyn Clock>,
        )
        .map_err(|error| error.to_string())?;
        let fs = TestFs {
            files,
            default_libraries: Mutex::new(BTreeSet::new()),
            written: Mutex::new(BTreeSet::new()),
        };
        // tsgo `ensureLibPathExists`: lib.d.ts, then the other libraries.
        let mut names = vec!["lib.d.ts"];
        names.extend(
            libraries
                .iter()
                .map(String::as_str)
                .filter(|name| *name != "lib.d.ts"),
        );
        for name in names {
            let path = format!("{library_path}/{name}");
            if fs.files.read(&path).is_err() {
                lock(&fs.default_libraries).insert(path.clone());
                fs.files
                    .write_creating_dirs(&path, DEFAULT_LIBRARY_CONTENT.as_bytes())
                    .map_err(|error| error.to_string())?;
            }
        }
        Ok(Self {
            fs,
            clock,
            current_directory,
            library_path,
            env: scenario.env.clone().unwrap_or_default(),
            output_is_terminal: scenario.output_is_tty.unwrap_or(true),
            output: Mutex::new(String::new()),
            traced_files: Mutex::new(BTreeSet::new()),
            program_baselines: Mutex::new(String::new()),
            snapshot: Mutex::new(None),
            watches: Mutex::new(BTreeMap::new()),
        })
    }

    /// tsgo `FSDiffer.ChangedPaths`: the regular files created, modified or
    /// touched since the last baseline of the file system, and the entries
    /// deleted.
    pub(super) fn changed_paths(&self) -> Vec<FileChange> {
        let snapshot = lock(&self.snapshot);
        let Some(previous) = snapshot.as_ref() else {
            return Vec::new();
        };
        let mut changes = Vec::new();
        for entry in self.fs.files.entries() {
            let EntryContents::File(bytes) = entry.contents() else {
                continue;
            };
            let changed = match previous.entries.get(entry.path()) {
                None => true,
                Some(old) => {
                    old.symlink.is_some()
                        || old.content
                            != sanitize_internal_symbol_name(&String::from_utf8_lossy(bytes))
                        || old.modified != entry.modified()
                }
            };
            if changed {
                changes.push(FileChange {
                    path: entry.path().to_owned(),
                    deleted: false,
                });
            }
        }
        for path in previous.entries.keys() {
            if self.fs.files.entry(path).is_none() {
                changes.push(FileChange {
                    path: path.clone(),
                    deleted: true,
                });
            }
        }
        changes
    }

    /// tsgo `MockWatchBackend.SendChangedPaths`: an event per change and an
    /// update of each parent directory, delivered to the watches whose
    /// directory contains the path.
    pub(super) fn send_changed_paths(&self, changes: &[FileChange]) {
        let mut events = Vec::with_capacity(changes.len() * 2);
        let mut seen_directories = BTreeSet::new();
        for change in changes {
            events.push(WatchEvent {
                kind: if change.deleted {
                    WatchEventKind::Delete
                } else {
                    WatchEventKind::Update
                },
                path: change.path.clone(),
            });
            let mut directory = parent_of(&change.path).to_owned();
            while !directory.is_empty() && directory != "/" && directory != "." {
                if !seen_directories.insert(directory.clone()) {
                    break;
                }
                events.push(WatchEvent {
                    kind: WatchEventKind::Update,
                    path: directory.clone(),
                });
                let parent = parent_of(&directory).to_owned();
                if parent == directory {
                    break;
                }
                directory = parent;
            }
        }
        let watches = lock(&self.watches);
        for (directory, watch) in watches.iter() {
            if watch.closed.load(std::sync::atomic::Ordering::Relaxed) {
                continue;
            }
            let matching = events
                .iter()
                .filter(|event| {
                    path_is_under(
                        &event.path,
                        directory,
                        watch.recursive,
                        self.fs.case_sensitive(),
                    )
                })
                .cloned()
                .collect::<Vec<_>>();
            if !matching.is_empty() {
                watch.events.send(matching);
            }
        }
    }

    /// tsgo `MockWatchBackend.HasWatches`.
    pub(super) fn has_watches(&self) -> bool {
        !lock(&self.watches).is_empty()
    }

    /// tsgo `MockWatchBackend.WatchState`.
    pub(super) fn watch_state(&self) -> String {
        let mut state = String::from("Watch Registrations::\nDirectory watches::\n");
        let watches = lock(&self.watches);
        let open = watches
            .iter()
            .filter(|(_, watch)| !watch.closed.load(std::sync::atomic::Ordering::Relaxed))
            .collect::<Vec<_>>();
        if open.is_empty() {
            state.push_str("  (none)\n");
        }
        for (directory, watch) in open {
            if watch.recursive {
                state.push_str(&format!("  {directory} (recursive)\n"));
            } else {
                state.push_str(&format!("  {directory}\n"));
            }
        }
        state
    }

    pub(super) fn current_directory_text(&self) -> &str {
        &self.current_directory
    }

    /// tsgo `clearOutput`.
    pub(super) fn clear_output(&self) {
        lock(&self.output).clear();
        lock(&self.traced_files).clear();
    }

    /// tsgo `serializeState`: the output, then the file system's changes.
    pub(super) fn serialize_state(&self, baseline: &mut String) {
        baseline.push_str("\nOutput::\n");
        baseline.push_str(&self.sanitized_output(false));
        self.baseline_fs_with_diff(baseline);
    }

    /// tsgo `baselinePrograms`: the programs' state since the last call.
    /// Returns the include-reason problems (none are recorded yet).
    pub(super) fn baseline_programs(&self, baseline: &mut String, _header: &str) -> String {
        baseline.push_str(&std::mem::take(&mut *lock(&self.program_baselines)));
        String::new()
    }

    /// The edit's operations, as the test's `TestSys` helpers perform them
    /// on the map file system.
    pub(super) fn apply(&self, ops: &[Op]) -> Result<(), String> {
        let files = &self.fs.files;
        let read = |path: &str| -> Result<String, String> {
            files
                .read(path)
                .map(|bytes| decode_bytes(&bytes))
                .map_err(|_| format!("File not found: {path}"))
        };
        let write = |path: &str, text: &[u8]| -> Result<(), String> {
            files
                .write_creating_dirs(path, text)
                .map_err(|error| error.to_string())
        };
        for op in ops {
            match op {
                Op::Write { path, contents } => write(path, &contents.bytes()?)?,
                Op::Append { path, contents } => {
                    let mut text = read(path)?.into_bytes();
                    text.extend(contents.bytes()?);
                    write(path, &text)?;
                }
                Op::Prepend { path, contents } => {
                    let mut text = contents.bytes()?;
                    text.extend(read(path)?.into_bytes());
                    write(path, &text)?;
                }
                Op::Replace { path, old, new } => {
                    write(path, read(path)?.replacen(old.as_str(), new, 1).as_bytes())?
                }
                Op::ReplaceAll { path, old, new } => {
                    write(path, read(path)?.replace(old.as_str(), new).as_bytes())?
                }
                Op::Remove { path } => files.remove(path).map_err(|error| error.to_string())?,
                Op::Rename { path, to } => {
                    let text = read(path)?;
                    write(to, text.as_bytes())?;
                    files.remove(path).map_err(|error| error.to_string())?;
                }
                Op::Touch { path } => {
                    let now = self.clock.now();
                    files
                        .set_modified(path, now)
                        .map_err(|error| error.to_string())?;
                }
            }
        }
        Ok(())
    }

    /// tsgo `FSDiffer.BaselineFSwithDiff`.
    pub(super) fn baseline_fs_with_diff(&self, baseline: &mut String) {
        let written = std::mem::take(&mut *lock(&self.fs.written));
        let default_libraries = lock(&self.fs.default_libraries).clone();
        let mut snapshot = lock(&self.snapshot);
        let mut entries = BTreeMap::new();
        let mut diffs = BTreeMap::new();
        for entry in self.fs.files.entries() {
            let current = match entry.contents() {
                EntryContents::Symlink(target) => DiffEntry {
                    content: String::new(),
                    modified: SystemTime::UNIX_EPOCH,
                    written: false,
                    symlink: Some(if target.starts_with('/') {
                        target.clone()
                    } else {
                        format!("/{target}")
                    }),
                },
                EntryContents::File(bytes) => DiffEntry {
                    content: sanitize_internal_symbol_name(&String::from_utf8_lossy(bytes)),
                    modified: entry.modified(),
                    written: written.contains(entry.path()),
                    symlink: None,
                },
                EntryContents::Directory => continue,
            };
            let path = entry.path().to_owned();
            if let Some(diff) =
                entry_diff(snapshot.as_ref(), &default_libraries, &path, Some(&current))
            {
                diffs.insert(path.clone(), diff);
            }
            entries.insert(path, current);
        }
        if let Some(previous) = snapshot.as_ref() {
            for path in previous.entries.keys() {
                if self.fs.files.entry(path).is_none() {
                    if let Some(diff) =
                        entry_diff(snapshot.as_ref(), &default_libraries, path, None)
                    {
                        diffs.insert(path.clone(), diff);
                    }
                }
            }
        }
        *snapshot = Some(Snapshot {
            entries,
            default_libraries,
        });
        for (path, diff) in diffs {
            baseline.push_str(&format!("//// [{path}] {diff}\n"));
        }
        baseline.push('\n');
    }

    /// tsgo `getOutput`: the output with its unstable parts sanitized and,
    /// `for_comparing`, the parts that only a baseline shows left out.
    pub(super) fn sanitized_output(&self, for_comparing: bool) -> String {
        let output = lock(&self.output).clone();
        let lines = output.split('\n').collect::<Vec<_>>();
        let mut sanitizer = OutputSanitizer {
            for_comparing,
            lines: &lines,
            index: 0,
            output: Vec::with_capacity(lines.len()),
        };
        sanitizer.transform()
    }

    /// The files the runs wrote since the last baseline.
    fn written_files(&self) -> Vec<String> {
        lock(&self.fs.written).iter().cloned().collect()
    }

    fn read_text(&self, path: &str) -> Option<String> {
        self.fs
            .files
            .read(path)
            .ok()
            .map(|bytes| decode_bytes(&bytes))
    }
}

/// tsgo `FSDiffer.addFsEntryDiff`.
fn entry_diff(
    previous: Option<&Snapshot>,
    default_libraries: &BTreeSet<String>,
    path: &str,
    current: Option<&DiffEntry>,
) -> Option<String> {
    let old = previous.and_then(|snapshot| snapshot.entries.get(path));
    match (old, current) {
        (None, Some(current)) => {
            if default_libraries.contains(path) {
                None
            } else if let Some(target) = &current.symlink {
                Some(format!("-> {target} *new*"))
            } else {
                Some(format!("*new* \n{}", current.content))
            }
        }
        (Some(_), None) => Some("*deleted*".to_owned()),
        (Some(old), Some(current)) => {
            if current.content != old.content {
                Some(format!("*modified* \n{}", current.content))
            } else if current.written {
                Some("*rewrite with same content*".to_owned())
            } else if current.modified != old.modified {
                Some("*mTime changed*".to_owned())
            } else if previous.is_some_and(|snapshot| snapshot.default_libraries.contains(path))
                && !default_libraries.contains(path)
            {
                Some(format!("*Lib*\n{}", current.content))
            } else {
                None
            }
        }
        (None, None) => None,
    }
}

impl System for TestSystem {
    fn fs(&self) -> &dyn FileSystem {
        &self.fs
    }

    fn current_directory(&self) -> &str {
        &self.current_directory
    }

    fn default_library_path(&self) -> &str {
        &self.library_path
    }

    fn now(&self) -> SystemTime {
        self.clock.now()
    }

    fn since_start(&self) -> Duration {
        self.clock
            .now()
            .duration_since(start_time())
            .unwrap_or_default()
    }

    fn env_var(&self, name: &str) -> Option<String> {
        match self.env.get(name) {
            Some(value) => Some(value.clone()),
            // tsgo's reference runs single-threaded: one worker and one
            // checker.
            None if name == "TSRS_WORKERS" || name == "TSRS_CHECKERS" => Some("1".to_owned()),
            None => None,
        }
    }

    fn output_is_terminal(&self) -> bool {
        self.output_is_terminal
    }

    fn terminal_width(&self) -> Option<usize> {
        self.env
            .get("TS_TEST_TERMINAL_WIDTH")
            .and_then(|width| width.parse().ok())
    }

    fn write_output(&self, text: &str) {
        lock(&self.output).push_str(text);
    }

    fn write_error(&self, text: &str) {
        lock(&self.output).push_str(text);
    }
}

impl WatchBackend for TestSystem {
    /// tsgo `MockWatchBackend.WatchDirectories`: fails for a directory that
    /// does not exist, records the others.
    fn watch_directories(
        &self,
        requests: Vec<WatchRequest>,
    ) -> io::Result<Vec<Box<dyn WatchHandle>>> {
        let mut watches = lock(&self.watches);
        for request in &requests {
            if !self.fs.files.is_dir(&request.directory) {
                return Err(io::Error::other(format!(
                    "directory does not exist: {}",
                    request.directory
                )));
            }
        }
        let mut handles: Vec<Box<dyn WatchHandle>> = Vec::with_capacity(requests.len());
        for request in requests {
            let closed = Arc::new(std::sync::atomic::AtomicBool::new(false));
            watches.insert(
                request.directory,
                MockWatch {
                    recursive: request.recursive,
                    events: request.events,
                    closed: Arc::clone(&closed),
                },
            );
            handles.push(Box::new(MockWatchHandle { closed }));
        }
        Ok(handles)
    }
}

/// tsgo `pathIsUnder`: `path` is in `directory` (a direct entry unless
/// `recursive`).
fn path_is_under(path: &str, directory: &str, recursive: bool, case_sensitive: bool) -> bool {
    let (path, directory) = if case_sensitive {
        (path.to_owned(), directory.to_owned())
    } else {
        (
            tsc_host::to_file_name_lower_case(path),
            tsc_host::to_file_name_lower_case(directory),
        )
    };
    let Some(rest) = path.strip_prefix(&directory) else {
        return false;
    };
    let Some(rest) = rest.strip_prefix('/') else {
        return false;
    };
    !rest.is_empty() && (recursive || !rest.contains('/'))
}

impl CommandLineTesting for TestSystem {
    /// tsgo `TestSys.OnEmittedFiles`: every emitted file gets the next
    /// time, unless the build put back the time the last baseline saw.
    fn on_emitted_files(&self, files: &[String]) -> Vec<(String, SystemTime)> {
        let mut stamped = Vec::new();
        for file in files {
            let modified = self
                .fs
                .files
                .entry(file)
                .map_or(SystemTime::UNIX_EPOCH, |entry| entry.modified());
            let reverted = lock(&self.snapshot).as_ref().is_some_and(|snapshot| {
                snapshot
                    .entries
                    .get(file)
                    .is_some_and(|entry| entry.modified == modified)
            });
            if reverted {
                continue;
            }
            let now = self.clock.now();
            if self.fs.files.set_modified(file, now).is_ok() {
                stamped.push((file.clone(), now));
            }
        }
        stamped
    }

    fn on_list_files_start(&self, output: &mut String) {
        push_line(output, LIST_FILES_START);
    }

    fn on_list_files_end(&self, output: &mut String) {
        push_line(output, LIST_FILES_END);
    }

    fn on_statistics_start(&self, output: &mut String) {
        push_line(output, STATISTICS_START);
    }

    fn on_statistics_end(&self, output: &mut String) {
        push_line(output, STATISTICS_END);
    }

    fn on_build_status_report_start(&self, output: &mut String) {
        push_line(output, BUILD_STATUS_START);
    }

    fn on_build_status_report_end(&self, output: &mut String) {
        push_line(output, BUILD_STATUS_END);
    }

    fn watch_backend(&self) -> Option<&dyn WatchBackend> {
        Some(self)
    }

    fn on_watch_status_report_start(&self, output: &mut String) {
        push_line(output, WATCH_STATUS_START);
    }

    fn on_watch_status_report_end(&self, output: &mut String) {
        push_line(output, WATCH_STATUS_END);
    }

    /// tsgo `TestSys.OnProgram`: the config, then the files whose semantic
    /// diagnostics were computed or are not cached, and the files whose
    /// signature was updated.
    fn on_program(&self, program: &ProgramReport) {
        let mut baselines = lock(&self.program_baselines);
        if !baselines.is_empty() {
            baselines.push('\n');
        }
        if let Some(config) = &program.config_file {
            baselines.push_str(&relative_path_from_directory(
                &self.current_directory,
                config,
                self.fs.case_sensitive(),
            ));
            baselines.push_str("::\n");
        }
        baselines.push_str("SemanticDiagnostics::\n");
        for file in &program.files {
            match file.semantic_diagnostics {
                SemanticDiagnosticsState::Refreshed => {
                    baselines.push_str(&format!("*refresh*    {}\n", file.file_name));
                }
                SemanticDiagnosticsState::NotCached => {
                    baselines.push_str(&format!("*not cached* {}\n", file.file_name));
                }
                SemanticDiagnosticsState::Kept => {}
            }
        }
        baselines.push_str("Signatures::\n");
        for file in &program.files {
            let kind = match file.signature_update {
                Some(SignatureUpdateKind::ComputedDts) => "(computed .d.ts) ",
                Some(SignatureUpdateKind::StoredAtEmit) => "(stored at emit) ",
                Some(SignatureUpdateKind::UsedVersion) => "(used version)   ",
                None => continue,
            };
            baselines.push_str(kind);
            baselines.push_str(&file.file_name);
            baselines.push('\n');
        }
    }

    /// tsgo `TestSys.GetTrace` with `TracerForBaselining.sanitizeTrace`.
    fn trace(&self, message: &str, output: &mut String, shared_output: bool) {
        push_line(output, TRACE_START);
        let line = self.sanitize_trace(message, shared_output);
        push_line(output, &line);
        push_line(output, TRACE_END);
    }
}

impl TestSystem {
    fn sanitize_trace(&self, message: &str, use_cache: bool) -> String {
        let version = format!("'{}'", tsc_types::TYPESCRIPT_VERSION);
        if message.contains(&version) {
            return message.replacen(&version, &format!("'{FAKE_VERSION}'"), 1);
        }
        let key = |file: &str| {
            let absolute = if file.starts_with('/') {
                file.to_owned()
            } else {
                format!("{}/{file}", self.current_directory)
            };
            if self.fs.case_sensitive() {
                absolute
            } else {
                tsc_host::to_file_name_lower_case(&absolute)
            }
        };
        let mut cache = lock(&self.traced_files);
        if let Some(rest) =
            message.strip_suffix("' does not exist according to earlier cached lookups.")
        {
            let file = rest.strip_prefix("File '").unwrap_or(rest);
            if use_cache && !cache.insert(key(file)) {
                return message.to_owned();
            }
            return format!("File '{file}' does not exist.");
        }
        if let Some(rest) = message.strip_suffix("' exists according to earlier cached lookups.") {
            let file = rest.strip_prefix("File '").unwrap_or(rest);
            if use_cache && !cache.insert(key(file)) {
                return message.to_owned();
            }
            return format!("Found 'package.json' at '{file}'.");
        }
        if use_cache {
            if let Some(rest) = message.strip_suffix("' does not exist.") {
                let file = rest.strip_prefix("File '").unwrap_or(rest);
                if cache.insert(key(file)) {
                    return message.to_owned();
                }
                return format!(
                    "File '{file}' does not exist according to earlier cached lookups."
                );
            }
            if let Some(rest) = message.strip_prefix("Found 'package.json' at '") {
                let file = rest.strip_suffix("'.").unwrap_or(rest);
                if cache.insert(key(file)) {
                    return message.to_owned();
                }
                return format!("File '{file}' exists according to earlier cached lookups.");
            }
        }
        message.to_owned()
    }
}

/// tsgo `outputSanitizer`.
struct OutputSanitizer<'a> {
    for_comparing: bool,
    lines: &'a [&'a str],
    index: usize,
    output: Vec<String>,
}

impl OutputSanitizer<'_> {
    /// tsgo `addOutputLine`: the version in quotes, the English and the
    /// Czech `Version {0}` (the locale test) become `FakeTSVersion`.
    fn add(&mut self, line: &str) {
        let version = tsc_compiler::CLI_VERSION;
        let czech = |version: &str| {
            tsc_diagnostics::gen::Version_0
                .format_in(tsc_compiler::locale::Locale::Czech.messages(), &[version])
                .to_string_lossy()
                .into_owned()
        };
        let line = line
            .replace(&format!("'{version}'"), &format!("'{FAKE_VERSION}'"))
            .replace(
                &format!("Version {version}"),
                &format!("Version {FAKE_VERSION}"),
            )
            .replace(&czech(version), &czech(FAKE_VERSION));
        self.output.push(sanitize_internal_symbol_name(&line));
    }

    fn sanitized_status_line(&self) -> String {
        const FAKE_TIME: &str = "HH:MM:SS AM";
        let line = self.lines[self.index];
        match line.find(':') {
            Some(separator) if separator >= 2 => {
                let tail = (separator + FAKE_TIME.len() - 2).min(line.len());
                format!("{}{FAKE_TIME}{}", &line[..separator - 2], &line[tail..])
            }
            _ => line.to_owned(),
        }
    }

    fn transform(&mut self) -> String {
        while self.index < self.lines.len() {
            let line = self.lines[self.index];
            if line.starts_with("build starting at ") {
                if !self.for_comparing {
                    self.add("build starting at HH:MM:SS AM");
                }
            } else if line.starts_with("build finished in ") {
                if !self.for_comparing {
                    self.add("build finished in d.ddds");
                }
            } else if !self.block(LIST_FILES_START, LIST_FILES_END, false, false)
                && !self.block(STATISTICS_START, STATISTICS_END, true, false)
                && !self.block(TRACE_START, TRACE_END, false, false)
                && !self.block(BUILD_STATUS_START, BUILD_STATUS_END, false, true)
                && !self.block(WATCH_STATUS_START, WATCH_STATUS_END, false, true)
            {
                self.add(line);
            }
            self.index += 1;
        }
        self.output.join("\n")
    }

    /// tsgo `addOrSkipLinesForComparing`.
    fn block(&mut self, start: &str, end: &str, skip: bool, timed: bool) -> bool {
        if self.lines[self.index] != start {
            return false;
        }
        self.index += 1;
        let mut first = true;
        while self.index < self.lines.len() {
            if self.lines[self.index] == end {
                return true;
            }
            if !self.for_comparing && !skip {
                let line = if first && timed {
                    first = false;
                    self.sanitized_status_line()
                } else {
                    self.lines[self.index].to_owned()
                };
                self.add(&line);
            }
            self.index += 1;
        }
        true
    }
}

/// tsgo `getDiffForIncremental`: what a clean build with every edit wrote
/// or printed differently.
pub(super) fn diff_for_incremental(incremental: &TestSystem, clean: &TestSystem) -> String {
    let mut diff = String::new();
    for output in clean.written_files() {
        if output.ends_with(".tsbuildinfo") || output.ends_with(".readable.baseline.txt") {
            if !incremental.fs.files.is_file(&output) {
                diff.push_str(&diff_text(
                    &format!("nonIncremental {output}"),
                    &format!("incremental {output}"),
                    "Exists",
                    "",
                ));
                diff.push('\n');
            }
        } else {
            let expected = clean.read_text(&output).unwrap_or_default();
            let actual = incremental.read_text(&output);
            if actual.as_deref() != Some(expected.as_str()) {
                diff.push_str(&diff_text(
                    &format!("nonIncremental {output}"),
                    &format!("incremental {output}"),
                    &expected,
                    actual.as_deref().unwrap_or_default(),
                ));
                diff.push('\n');
            }
        }
    }
    let incremental_output = incremental.sanitized_output(true);
    let clean_output = clean.sanitized_output(true);
    if incremental_output != clean_output {
        diff.push_str(&diff_text(
            "nonIncremental.output.txt",
            "incremental.output.txt",
            &clean_output,
            &incremental_output,
        ));
    }
    diff
}

/// tsgo `SanitizeInternalSymbolName`: `\u{FFFD}@name@123` becomes
/// `\u{FFFD}@name@<symbolId>`.
pub(super) fn sanitize_internal_symbol_name(text: &str) -> String {
    const PREFIX: &str = "\u{FFFD}@";
    if !text.contains(PREFIX) {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(PREFIX) {
        let after = &rest[start + PREFIX.len()..];
        let name_end = after.find('@').filter(|&end| end > 0);
        let digits = name_end.map(|end| {
            after[end + 1..]
                .bytes()
                .take_while(u8::is_ascii_digit)
                .count()
        });
        match (name_end, digits) {
            (Some(end), Some(count)) if count > 0 => {
                out.push_str(&rest[..start + PREFIX.len() + end + 1]);
                out.push_str("<symbolId>");
                rest = &after[end + 1 + count..];
            }
            _ => {
                out.push_str(&rest[..start + PREFIX.len()]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// tsgo `vfs` `decodeBytes`: UTF-16 by its byte order mark, UTF-8 without
/// its mark.
fn decode_bytes(bytes: &[u8]) -> String {
    match bytes {
        [0xFF, 0xFE, rest @ ..] => decode_utf16(rest, u16::from_le_bytes),
        [0xFE, 0xFF, rest @ ..] => decode_utf16(rest, u16::from_be_bytes),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

fn decode_utf16(bytes: &[u8], unit: fn([u8; 2]) -> u16) -> String {
    let units = bytes
        .chunks_exact(2)
        .map(|pair| unit([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    String::from_utf16_lossy(&units)
}

/// tsgo `tspath.GetRelativePathFromDirectory` of two absolute paths.
fn relative_path_from_directory(directory: &str, path: &str, case_sensitive: bool) -> String {
    let from = directory
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let to = path
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let same = |left: &str, right: &str| {
        if case_sensitive {
            left == right
        } else {
            tsc_host::to_file_name_lower_case(left) == tsc_host::to_file_name_lower_case(right)
        }
    };
    let common = from
        .iter()
        .zip(&to)
        .take_while(|(left, right)| same(left, right))
        .count();
    let mut parts = vec![".."; from.len() - common];
    parts.extend(&to[common..]);
    parts.join("/")
}

fn parent_of(path: &str) -> &str {
    match path.rfind('/') {
        Some(0) => "/",
        Some(index) => &path[..index],
        None => path,
    }
}

fn push_line(output: &mut String, line: &str) {
    output.push_str(line);
    output.push('\n');
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_symbol_names_lose_their_ids() {
        assert_eq!(
            sanitize_internal_symbol_name("a \u{FFFD}@sym@123 b \u{FFFD}@x@ c"),
            "a \u{FFFD}@sym@<symbolId> b \u{FFFD}@x@ c"
        );
        assert_eq!(sanitize_internal_symbol_name("plain"), "plain");
    }

    #[test]
    fn status_blocks_keep_their_lines_with_a_fake_time() {
        let lines = [
            "x",
            BUILD_STATUS_START,
            "[\u{1b}[90m12:34:56 PM\u{1b}[0m] Projects in this build: ",
            "",
            BUILD_STATUS_END,
            STATISTICS_START,
            "Files: 1",
            STATISTICS_END,
            "",
        ];
        let mut sanitizer = OutputSanitizer {
            for_comparing: false,
            lines: &lines,
            index: 0,
            output: Vec::new(),
        };
        assert_eq!(
            sanitizer.transform(),
            "x\n[\u{1b}[90mHH:MM:SS AM\u{1b}[0m] Projects in this build: \n\n"
        );
    }
}
