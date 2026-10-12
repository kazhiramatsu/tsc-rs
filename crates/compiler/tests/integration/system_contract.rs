//! The command line over a system without a disk: an in-memory file system,
//! a stepping clock and a fixed environment (`tsc_compiler::system`), the
//! way a test or a WebAssembly embedding runs it.

use std::collections::BTreeMap;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use tsc_harness::content_mappers::{
    package_json, ProjectLifecycle, TestSpawner, DYNAMIC_VERBATIM_MAPPER,
    SUPPLEMENTAL_DIAGNOSTICS_MAPPER, SUPPLEMENTAL_MAPPER, VERBATIM_MAPPER,
};
use tsc_program::content_mapper::host::{MapperProcess, ProcessControl, Spawner};

use tsc_compiler::system::{CommandLineTesting, ProgramReport, System};
use tsc_compiler::watch::{
    WatchBackend, WatchEvent, WatchEventKind, WatchEvents, WatchHandle, WatchRequest,
};
use tsc_compiler::{execute_command_line, SemanticDiagnosticsState};
use tsc_host::vfs::{Clock, FileSystem, MemFs, Seed, SteppingClock};

const LIBRARY: &str = "interface Array<T> { length: number; [n: number]: T; }\n\
    interface Boolean {}\ninterface Function {}\ninterface CallableFunction {}\n\
    interface NewableFunction {}\ninterface IArguments {}\ninterface Number {}\n\
    interface Object {}\ninterface RegExp {}\ninterface String {}\n";

struct MemorySystem {
    fs: MemFs,
    clock: Arc<SteppingClock>,
    env: BTreeMap<String, String>,
    output: Mutex<String>,
    programs: Mutex<Vec<ProgramReport>>,
    /// The directories a watch run watches, with their recursion and the
    /// queue their events go to.
    watches: Mutex<Vec<(String, bool, WatchEvents)>>,
    /// The test content mappers the system starts, when it starts any.
    spawner: Option<Arc<RecordingSpawner>>,
}

/// tsgo `recordingContentMapperSpawner`: the harness's in-process mappers,
/// counting the processes started and closed, and the projects the dynamic
/// mappers opened and closed (tsgo `ProjectLifecycle`).
struct RecordingSpawner {
    inner: TestSpawner,
    lifecycle: Arc<ProjectLifecycle>,
    spawns: AtomicUsize,
    closes: Arc<AtomicUsize>,
}

impl RecordingSpawner {
    fn new() -> Self {
        let lifecycle = Arc::new(ProjectLifecycle::default());
        Self {
            inner: TestSpawner::with_project_lifecycle(Arc::clone(&lifecycle)),
            lifecycle,
            spawns: AtomicUsize::new(0),
            closes: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl Spawner for RecordingSpawner {
    fn spawn(
        &self,
        command: &[String],
        dir: &str,
        stderr: Box<dyn Write + Send>,
    ) -> Result<MapperProcess, String> {
        let process = self.inner.spawn(command, dir, stderr)?;
        self.spawns.fetch_add(1, Ordering::SeqCst);
        Ok(MapperProcess {
            stdout: process.stdout,
            stdin: process.stdin,
            control: Box::new(RecordingControl {
                inner: process.control,
                closes: Arc::clone(&self.closes),
                closed: AtomicBool::new(false),
            }),
        })
    }
}

/// A test mapper process whose first close is counted.
struct RecordingControl {
    inner: Box<dyn ProcessControl>,
    closes: Arc<AtomicUsize>,
    closed: AtomicBool,
}

impl ProcessControl for RecordingControl {
    fn close(&self) {
        if !self.closed.swap(true, Ordering::SeqCst) {
            self.closes.fetch_add(1, Ordering::SeqCst);
        }
        self.inner.close();
    }

    fn exit_code(&self) -> Option<i32> {
        self.inner.exit_code()
    }
}

impl MemorySystem {
    fn new(files: &[(&str, &str)]) -> Self {
        Self::from_seeds(
            files
                .iter()
                .map(|(path, text)| (path.to_string(), Seed::file(text)))
                .collect(),
            true,
        )
    }

    /// A system over the given entries (files and links), its file names
    /// compared with or without case.
    fn from_seeds(mut seeds: Vec<(String, Seed)>, case_sensitive: bool) -> Self {
        let clock = Arc::new(SteppingClock::new(
            SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000),
            Duration::from_secs(1),
        ));
        for name in ["lib.d.ts", "lib.es2026.full.d.ts"] {
            seeds.push((format!("/lib/{name}"), Seed::file(LIBRARY)));
        }
        let fs = MemFs::from_entries(seeds, case_sensitive, Arc::clone(&clock) as Arc<dyn Clock>)
            .expect("valid files");
        Self {
            fs,
            clock,
            env: BTreeMap::from([
                ("TSRS_WORKERS".to_owned(), "1".to_owned()),
                ("TSRS_CHECKERS".to_owned(), "1".to_owned()),
            ]),
            output: Mutex::new(String::new()),
            programs: Mutex::new(Vec::new()),
            watches: Mutex::new(Vec::new()),
            spawner: None,
        }
    }

    /// The system starts the harness's test content mappers.
    fn with_content_mappers(mut self) -> Self {
        self.spawner = Some(Arc::new(RecordingSpawner::new()));
        self
    }

    /// The (started, closed) counts of the mapper processes.
    fn mapper_processes(&self) -> (usize, usize) {
        let spawner = self.spawner.as_ref().expect("content mappers");
        (
            spawner.spawns.load(Ordering::SeqCst),
            spawner.closes.load(Ordering::SeqCst),
        )
    }

    /// The (opened, closed) counts of the dynamic mappers' projects.
    fn mapper_projects(&self) -> (i32, i32) {
        let lifecycle = &self.spawner.as_ref().expect("content mappers").lifecycle;
        (
            lifecycle.opens.load(Ordering::SeqCst),
            lifecycle.closes.load(Ordering::SeqCst),
        )
    }

    fn text(&self, path: &str) -> String {
        String::from_utf8(self.fs.read(path).expect(path)).expect("UTF-8")
    }
}

impl System for MemorySystem {
    fn fs(&self) -> &dyn FileSystem {
        &self.fs
    }

    fn current_directory(&self) -> &str {
        "/work"
    }

    fn default_library_path(&self) -> &str {
        "/lib"
    }

    fn now(&self) -> SystemTime {
        self.clock.now()
    }

    fn since_start(&self) -> Duration {
        Duration::ZERO
    }

    fn env_var(&self, name: &str) -> Option<String> {
        self.env.get(name).cloned()
    }

    fn output_is_terminal(&self) -> bool {
        false
    }

    fn terminal_width(&self) -> Option<usize> {
        None
    }

    fn write_output(&self, text: &str) {
        self.output.lock().unwrap().push_str(text);
    }

    fn write_error(&self, text: &str) {
        self.output.lock().unwrap().push_str(text);
    }

    fn content_mapper_spawner(&self) -> Arc<dyn Spawner> {
        match &self.spawner {
            Some(spawner) => Arc::clone(spawner) as Arc<dyn Spawner>,
            None => Arc::new(TestSpawner::default()),
        }
    }
}

impl CommandLineTesting for MemorySystem {
    fn on_program(&self, program: &ProgramReport) {
        self.programs.lock().unwrap().push(program.clone());
    }

    fn watch_backend(&self) -> Option<&dyn WatchBackend> {
        Some(self)
    }
}

struct NoHandle;

impl WatchHandle for NoHandle {}

impl WatchBackend for MemorySystem {
    fn watch_directories(
        &self,
        requests: Vec<WatchRequest>,
    ) -> std::io::Result<Vec<Box<dyn WatchHandle>>> {
        let mut watches = self.watches.lock().unwrap();
        Ok(requests
            .into_iter()
            .map(|request| {
                watches.push((request.directory, request.recursive, request.events));
                Box::new(NoHandle) as Box<dyn WatchHandle>
            })
            .collect())
    }
}

impl MemorySystem {
    /// Reports a change of `path` to the watch whose directory holds it.
    fn changed(&self, path: &str) {
        let fold = |text: &str| {
            if self.fs.case_sensitive() {
                text.to_owned()
            } else {
                text.to_lowercase()
            }
        };
        for (directory, recursive, events) in self.watches.lock().unwrap().iter() {
            let folded = fold(path);
            let Some(rest) = folded.strip_prefix(&format!("{}/", fold(directory))) else {
                continue;
            };
            if *recursive || !rest.contains('/') {
                events.send([WatchEvent {
                    kind: WatchEventKind::Update,
                    path: path.to_owned(),
                }]);
            }
        }
    }

    fn take_output(&self) -> String {
        std::mem::take(&mut *self.output.lock().unwrap())
    }
}

/// The exit status of a command line over `system`.
fn status_of(
    system: &dyn System,
    args: &[String],
    testing: Option<&dyn CommandLineTesting>,
) -> i32 {
    execute_command_line(system, args, testing).status
}

fn args(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_owned()).collect()
}

#[test]
fn a_project_compiles_into_the_memory_file_system() {
    let system = MemorySystem::new(&[
        (
            "/work/tsconfig.json",
            r#"{ "compilerOptions": { "outDir": "out" } }"#,
        ),
        ("/work/a.ts", "export const a: number = 1;\n"),
    ]);
    let status = status_of(&system, &[], None);
    assert_eq!(status, 0, "{}", system.output.lock().unwrap());
    assert_eq!(system.text("/work/out/a.js"), "export const a = 1;\n");
    assert!(system.output.lock().unwrap().is_empty());
}

#[test]
fn diagnostics_follow_the_system_and_command_line_errors_have_no_summary() {
    let system = MemorySystem::new(&[("/work/a.ts", "const x: string = 1;\n")]);
    let status = status_of(&system, &args(&["a.ts", "--noEmit"]), None);
    assert_eq!(status, 2);
    assert_eq!(
        *system.output.lock().unwrap(),
        "a.ts(1,7): error TS2322: Type 'number' is not assignable to type 'string'.\n"
    );

    // tsgo reports a command line's own errors and stops: no summary even
    // when the output is pretty.
    let system = MemorySystem::new(&[]);
    let status = status_of(&system, &args(&["--bogus", "--pretty"]), None);
    assert_eq!(status, 1);
    let output = system.output.lock().unwrap().clone();
    assert!(output.contains("TS5023"), "{output}");
    assert!(!output.contains("Found 1 error"), "{output}");
}

#[test]
fn an_incremental_run_reports_its_program_and_writes_its_build_info() {
    let system = MemorySystem::new(&[
        (
            "/work/tsconfig.json",
            r#"{ "compilerOptions": { "incremental": true, "outDir": "out" } }"#,
        ),
        ("/work/a.ts", "export const a = 1;\n"),
    ]);
    assert_eq!(status_of(&system, &[], Some(&system)), 0);
    assert!(system.fs.is_file("/work/out/tsconfig.tsbuildinfo"));
    let programs = system.programs.lock().unwrap().clone();
    assert_eq!(programs.len(), 1);
    assert_eq!(
        programs[0].config_file.as_deref(),
        Some("/work/tsconfig.json")
    );
    let states = programs[0]
        .files
        .iter()
        .map(|file| (file.file_name.as_str(), file.semantic_diagnostics))
        .collect::<Vec<_>>();
    assert_eq!(
        states,
        [
            (
                "/lib/lib.es2026.full.d.ts",
                SemanticDiagnosticsState::Refreshed
            ),
            ("/work/a.ts", SemanticDiagnosticsState::Refreshed),
        ]
    );

    // Nothing changed: the second run keeps every cached row.
    system.programs.lock().unwrap().clear();
    assert_eq!(status_of(&system, &[], Some(&system)), 0);
    let programs = system.programs.lock().unwrap().clone();
    assert!(programs[0]
        .files
        .iter()
        .all(|file| file.semantic_diagnostics == SemanticDiagnosticsState::Kept));
}

#[test]
fn a_build_takes_roots_in_a_referenced_project_from_its_output() {
    // The server includes the shared project's sources: tsgo holds the
    // shared project's output declarations in their place, and a second
    // build sees the recorded roots through `resolvedRoot`.
    let system = MemorySystem::new(&[
        (
            "/work/shared/tsconfig.json",
            r#"{ "compilerOptions": { "composite": true, "outDir": "dist" }, "include": ["src/**/*.ts"] }"#,
        ),
        ("/work/shared/src/a.ts", "export class A {}\n"),
        (
            "/work/server/tsconfig.json",
            r#"{ "compilerOptions": { "composite": true, "rootDir": "..", "outDir": "dist" }, "include": ["src/**/*.ts", "../shared/src/**/*.ts"], "references": [{ "path": "../shared" }] }"#,
        ),
        (
            "/work/server/src/b.ts",
            "import { A } from '../../shared/src/a';\nexport const b = new A();\n",
        ),
    ]);
    let build = args(&["-b", "server", "--verbose", "--pretty", "false"]);
    assert_eq!(status_of(&system, &build, Some(&system)), 0);
    assert!(system.fs.is_file("/work/server/dist/server/src/b.js"));
    system.output.lock().unwrap().clear();
    assert_eq!(status_of(&system, &build, Some(&system)), 0);
    let output = system.output.lock().unwrap().clone();
    assert!(
        output.contains("Project 'server/tsconfig.json' is up to date"),
        "{output}"
    );
}

/// The `(ph, name)` of every begin and end event of a trace, in order.
fn trace_spans(trace: &str) -> Vec<(String, String)> {
    trace
        .lines()
        .filter_map(|line| {
            let ph = line.split("\"ph\":\"").nth(1)?.split('"').next()?;
            let name = line.split("\"name\":\"").nth(1)?.split('"').next()?;
            matches!(ph, "B" | "E").then(|| (ph.to_owned(), name.to_owned()))
        })
        .collect()
}

#[test]
fn generate_trace_records_the_compilation_in_its_directory() {
    let system = MemorySystem::new(&[
        (
            "/work/tsconfig.json",
            r#"{ "compilerOptions": { "noEmit": true, "strict": true } }"#,
        ),
        (
            "/work/a.ts",
            "interface Person { name: string }\nconst p: Person = { name: \"a\" };\n",
        ),
    ]);
    // Under the test hooks the session is deterministic (tsgo's): the
    // timestamps count and no sampled event is written.
    let status = status_of(
        &system,
        &args(&["--generateTrace", "/work/trace", "--singleThreaded"]),
        Some(&system),
    );
    assert_eq!(status, 0, "{}", system.output.lock().unwrap());
    assert_eq!(
        system.text("/work/trace/legend.json"),
        "[\n  {\n    \"configFilePath\": \"/work/tsconfig.json\",\n    \
         \"tracePath\": \"/work/trace/trace.json\",\n    \
         \"typesPath\": \"/work/trace/types_0.json\",\n    \"checkerId\": 0\n  }\n]"
    );
    let trace = system.text("/work/trace/trace.json");
    assert!(trace.starts_with(
        "[\n{\"pid\":1,\"tid\":1,\"ph\":\"M\",\"cat\":\"__metadata\",\"ts\":1,\
         \"name\":\"process_name\",\"args\":{\"name\":\"tsgo\"}},\n"
    ));
    assert!(trace.ends_with("}\n]\n"));
    assert!(!trace.contains("\"ph\":\"X\""));
    let spans = trace_spans(&trace);
    let names = spans
        .iter()
        .map(|(ph, name)| format!("{ph} {name}"))
        .collect::<Vec<_>>();
    let expected = [
        "B createProgram",
        "B createSourceFile",
        "E createSourceFile",
        "B createSourceFile",
        "E createSourceFile",
        "E createProgram",
        "B bindSourceFiles",
        "B bindSourceFile",
        "E bindSourceFile",
        "B bindSourceFile",
        "E bindSourceFile",
        "E bindSourceFiles",
        "B checkSourceFiles",
        "B checkSourceFile",
        "E checkSourceFile",
        "B checkSourceFile",
        "E checkSourceFile",
        "E checkSourceFiles",
        "B emit",
        "E emit",
    ];
    assert_eq!(names, expected, "{trace}");
    assert!(trace.contains(
        "{\"pid\":1,\"tid\":2,\"ph\":\"M\",\"cat\":\"__metadata\",\"ts\":1,\
         \"name\":\"thread_name\",\"args\":{\"name\":\"checker:0\"}}"
    ));
    assert!(trace.contains(
        "\"name\":\"checkSourceFile\",\"args\":{\"checkerId\":0,\"path\":\"/work/a.ts\"}"
    ));
    let types = system.text("/work/trace/types_0.json");
    assert!(
        types.starts_with(
            "[{\"id\":1,\"intrinsicName\":\"any\",\"recursionId\":0,\"flags\":[\"Any\"]},\n"
        ),
        "{types}"
    );
    assert!(types.ends_with("}]\n"));
    // The interface's declared type, with its declaration's location.
    assert!(
        types.contains("\"symbolName\":\"Person\",\"recursionId\":"),
        "{types}"
    );
    assert!(types.contains(
        "\"firstDeclaration\":{\"path\":\"/work/a.ts\",\"start\":{\"line\":1,\"character\":1},\
         \"end\":{\"line\":1,\"character\":34}}"
    ));
}

#[test]
fn generate_trace_records_no_build() {
    // tsgo traces a command's compilation, never a build's projects.
    let system = MemorySystem::new(&[
        (
            "/work/tsconfig.json",
            r#"{ "compilerOptions": { "composite": true, "generateTrace": "trace" } }"#,
        ),
        ("/work/a.ts", "export const a = 1;\n"),
    ]);
    let status = status_of(&system, &args(&["-b"]), Some(&system));
    assert_eq!(status, 0, "{}", system.output.lock().unwrap());
    assert!(system.fs.read("/work/a.js").is_ok());
    assert!(system.fs.read("/work/trace/trace.json").is_err());

    // The command records the option the config sets.
    let status = status_of(&system, &[], Some(&system));
    assert_eq!(status, 0, "{}", system.output.lock().unwrap());
    assert!(system.fs.read("/work/trace/trace.json").is_ok());
}

#[test]
fn watch_builds_again_when_a_watched_file_changes() {
    // tsgo watches no directory as shallow as `/work` (`CanWatchDirectory`).
    const PROJECT: &str = "/home/src/workspaces/project";
    let system = MemorySystem::new(&[
        (
            "/home/src/workspaces/project/tsconfig.json",
            r#"{ "compilerOptions": { "outDir": "out", "rootDir": "." } }"#,
        ),
        (
            "/home/src/workspaces/project/a.ts",
            "export const a: number = 1;\n",
        ),
    ]);
    let result = execute_command_line(&system, &args(&["--watch", "-p", PROJECT]), Some(&system));
    assert_eq!(result.status, 0);
    let mut watcher = result.watcher.expect("--watch starts a watch");
    assert_eq!(
        system.text("/home/src/workspaces/project/out/a.js"),
        "export const a = 1;\n"
    );
    let output = system.take_output();
    assert!(
        output.starts_with("\x1b[2J\x1b[3J\x1b[H")
            && output.contains("Starting compilation in watch mode...")
            && output.contains("Found 0 errors. Watching for file changes."),
        "{output}"
    );
    // The include directory, recursively.
    let watched = system
        .watches
        .lock()
        .unwrap()
        .iter()
        .map(|(directory, recursive, _)| (directory.clone(), *recursive))
        .collect::<Vec<_>>();
    assert_eq!(watched, [(PROJECT.to_owned(), true)]);

    // A cycle without changes builds nothing and writes nothing.
    watcher.do_cycle();
    assert_eq!(system.take_output(), "");

    system
        .fs
        .write(
            "/home/src/workspaces/project/a.ts",
            b"export const a: number = 2;\n",
        )
        .unwrap();
    system.changed("/home/src/workspaces/project/a.ts");
    watcher.do_cycle();
    assert_eq!(
        system.text("/home/src/workspaces/project/out/a.js"),
        "export const a = 2;\n"
    );
    let output = system.take_output();
    assert!(
        output.contains("File change detected. Starting incremental compilation...")
            && output.contains("Found 0 errors. Watching for file changes."),
        "{output}"
    );

    // A file the include patterns match joins the program.
    system
        .fs
        .write(
            "/home/src/workspaces/project/b.ts",
            b"export const b: string = 1;\n",
        )
        .unwrap();
    system.changed("/home/src/workspaces/project/b.ts");
    watcher.do_cycle();
    let output = system.take_output();
    assert!(
        output.contains("b.ts(1,14): error TS2322")
            && output.contains("Found 1 error. Watching for file changes."),
        "{output}"
    );
}

#[test]
fn build_watch_rebuilds_the_projects_a_change_concerns() {
    const CORE: &str = "/home/src/workspaces/project/core";
    const APP: &str = "/home/src/workspaces/project/app";
    let system = MemorySystem::new(&[
        (
            "/home/src/workspaces/project/core/tsconfig.json",
            r#"{ "compilerOptions": { "composite": true, "outDir": "out", "rootDir": "." } }"#,
        ),
        (
            "/home/src/workspaces/project/core/index.ts",
            "export const answer: number = 42;\n",
        ),
        (
            "/home/src/workspaces/project/app/tsconfig.json",
            r#"{ "compilerOptions": { "composite": true, "outDir": "out", "rootDir": "." }, "references": [{ "path": "../core" }] }"#,
        ),
        (
            "/home/src/workspaces/project/app/index.ts",
            "import { answer } from \"../core/index.js\";\nexport const doubled = answer * 2;\n",
        ),
    ]);
    let result = execute_command_line(&system, &args(&["-b", APP, "--watch"]), Some(&system));
    assert_eq!(result.status, 0);
    let mut watcher = result.watcher.expect("-b --watch starts a watch");
    assert_eq!(
        system.text("/home/src/workspaces/project/core/out/index.d.ts"),
        "export declare const answer: number;\n"
    );
    assert_eq!(
        system.text("/home/src/workspaces/project/app/out/index.js"),
        "import { answer } from \"../core/index.js\";\nexport const doubled = answer * 2;\n"
    );
    let output = system.take_output();
    assert!(
        output.starts_with("\x1b[2J\x1b[3J\x1b[H")
            && output.contains("Starting compilation in watch mode...")
            && output.contains("Found 0 errors. Watching for file changes."),
        "{output}"
    );
    // Both projects' directories, recursively (their include patterns).
    let watched = system
        .watches
        .lock()
        .unwrap()
        .iter()
        .map(|(directory, recursive, _)| (directory.clone(), *recursive))
        .collect::<Vec<_>>();
    assert!(watched.contains(&(CORE.to_owned(), true)), "{watched:?}");
    assert!(watched.contains(&(APP.to_owned(), true)), "{watched:?}");

    // A cycle without changes builds nothing and writes nothing.
    watcher.do_cycle();
    assert_eq!(system.take_output(), "");

    // A change to the referenced project's declarations builds both.
    system
        .fs
        .write(
            "/home/src/workspaces/project/core/index.ts",
            b"export const answer: number = 43;\nexport const extra = 1;\n",
        )
        .unwrap();
    system.changed("/home/src/workspaces/project/core/index.ts");
    watcher.do_cycle();
    assert_eq!(
        system.text("/home/src/workspaces/project/core/out/index.d.ts"),
        "export declare const answer: number;\nexport declare const extra = 1;\n"
    );
    let output = system.take_output();
    assert!(
        output.contains("File change detected. Starting incremental compilation...")
            && output.contains("Found 0 errors. Watching for file changes."),
        "{output}"
    );

    // An error in the referencing project is reported, and only it builds.
    system
        .fs
        .write(
            "/home/src/workspaces/project/app/index.ts",
            b"import { answer } from \"../core/index.js\";\nexport const doubled: string = answer * 2;\n",
        )
        .unwrap();
    system.changed("/home/src/workspaces/project/app/index.ts");
    watcher.do_cycle();
    let output = system.take_output();
    assert!(
        output.contains("index.ts(2,14): error TS2322")
            && output.contains("Found 1 error. Watching for file changes."),
        "{output}"
    );
}

#[test]
fn the_bundled_file_system_serves_the_embedded_libraries_read_only() {
    // tsgo `bundled.WrapFS`: the libraries are files of `bundled:///libs`
    // over the base file system, which keeps everything else.
    use tsc_compiler::system::{BundledFs, EMBEDDED_LIBRARY_DIRECTORY};
    use tsc_host::vfs::{FileType, SystemClock};

    let base = MemFs::from_entries([("/a.ts", Seed::file("x"))], true, Arc::new(SystemClock))
        .expect("build the file system");
    let fs = BundledFs::new(base);
    let library = format!("{EMBEDDED_LIBRARY_DIRECTORY}/lib.es5.d.ts");
    let text = fs.read(&library).expect("read the library");
    assert!(String::from_utf8_lossy(&text).contains("interface Array<T>"));
    assert!(fs.is_file(&library));
    assert!(fs.is_dir(EMBEDDED_LIBRARY_DIRECTORY));
    assert_eq!(fs.canonicalize(&library).unwrap(), library);
    let entries = fs.read_dir(EMBEDDED_LIBRARY_DIRECTORY).unwrap();
    assert!(entries
        .iter()
        .any(|entry| entry.name() == "lib.dom.d.ts" && entry.file_type() == FileType::File));
    assert!(fs
        .read(&format!("{EMBEDDED_LIBRARY_DIRECTORY}/lib.none.d.ts"))
        .is_err());
    assert!(fs.write(&library, b"").is_err());
    assert_eq!(fs.read("/a.ts").unwrap(), b"x");
    fs.write("/b.ts", b"y").unwrap();
    assert_eq!(fs.read("/b.ts").unwrap(), b"y");
}

const MAPPED_PROJECT: &str = "/home/src/workspaces/project";

#[test]
fn a_build_starts_its_content_mapper_once_and_closes_it() {
    // tsgo TestContentMapperBuildLifecycle.
    let manifest = package_json(VERBATIM_MAPPER);
    let system = MemorySystem::new(&[
        (
            "/home/src/workspaces/project/tsconfig.json",
            r#"{
                "compilerOptions": { "composite": true },
                "contentMappers": [{ "package": "mapper", "extensions": [".vue"] }]
            }"#,
        ),
        ("/home/src/workspaces/project/app.vue", "export const app = 1;"),
        (
            "/home/src/workspaces/project/node_modules/mapper/package.json",
            &manifest,
        ),
    ])
    .with_content_mappers();
    let result = execute_command_line(
        &system,
        &args(&["--build", MAPPED_PROJECT, "--runExternalCode"]),
        Some(&system),
    );
    assert!(result.watcher.is_none());
    assert_eq!(system.mapper_processes(), (1, 1), "{}", system.take_output());
}

#[test]
fn a_build_notices_a_supplemental_file_written_to_disk() {
    // tsgo TestContentMapperBuildDetectsNewPhysicalSupplementalFile: the
    // build info lists the supplemental output, which is not a missing input
    // while it is virtual; once a file has its name, the project is built
    // again and the collision reported.
    let manifest = package_json(SUPPLEMENTAL_MAPPER);
    let system = MemorySystem::new(&[
        (
            "/home/src/workspaces/project/tsconfig.json",
            r#"{
                "compilerOptions": { "incremental": true },
                "files": ["app.vue"],
                "contentMappers": [{ "package": "mapper", "extensions": [".vue"] }]
            }"#,
        ),
        (
            "/home/src/workspaces/project/app.vue",
            "declare const value: number;",
        ),
        (
            "/home/src/workspaces/project/node_modules/mapper/package.json",
            &manifest,
        ),
    ])
    .with_content_mappers();
    let arguments = args(&[
        "--build",
        MAPPED_PROJECT,
        "--pretty",
        "false",
        "--runExternalCode",
    ]);
    let result = execute_command_line(&system, &arguments, Some(&system));
    assert_eq!(result.status, 0, "{}", system.take_output());
    system.take_output();

    system
        .fs
        .write("/home/src/workspaces/project/app.vue.0.ts", b"export {};\n")
        .unwrap();
    let result = execute_command_line(&system, &arguments, Some(&system));
    let output = system.take_output();
    assert_eq!(result.status, 2, "{output}");
    assert!(output.contains("TS18069"), "{output}");
    assert!(output.contains("conflicts with an existing file"), "{output}");
}

#[test]
fn a_build_whose_mapper_identity_fails_skips_its_outputs() {
    // tsgo TestContentMapperBuildIdentityFailureExitStatus: a dynamic
    // mapper's identity comes from its process; when it cannot start, the
    // project is out of date and is not built.
    let manifest = package_json(DYNAMIC_VERBATIM_MAPPER);
    let system = MemorySystem::new(&[
        (
            "/home/src/workspaces/project/tsconfig.json",
            r#"{
                "compilerOptions": { "composite": true },
                "contentMappers": [{ "package": "mapper", "extensions": [".vue"] }]
            }"#,
        ),
        ("/home/src/workspaces/project/app.vue", "export const app = 1;"),
        (
            "/home/src/workspaces/project/node_modules/mapper/package.json",
            &manifest,
        ),
    ])
    .with_content_mappers();
    let arguments = args(&["--build", MAPPED_PROJECT, "--runExternalCode"]);
    let result = execute_command_line(&system, &arguments, Some(&system));
    assert_eq!(result.status, 0, "{}", system.take_output());

    system
        .fs
        .write(
            "/home/src/workspaces/project/node_modules/mapper/package.json",
            br#"{
                "name": "mapper",
                "version": "1.0.0",
                "typescript": { "contentMapper": { "exec": ["missing-mapper"], "dynamicConfig": true } }
            }"#,
        )
        .unwrap();
    let result = execute_command_line(&system, &arguments, Some(&system));
    assert_eq!(result.status, 1, "{}", system.take_output());
}

#[test]
fn an_incremental_program_starts_over_when_its_mapper_changes() {
    // tsgo TestReadBuildInfoProgramContentMapperIdentityMismatch, through
    // the command line: the build info records the mapper's identity, and
    // the old program is not reused once the mapper's version changed.
    let manifest_file = "/home/src/workspaces/project/node_modules/mapper/package.json";
    let manifest = package_json(VERBATIM_MAPPER);
    let system = MemorySystem::new(&[
        (
            "/home/src/workspaces/project/tsconfig.json",
            r#"{
                "compilerOptions": { "incremental": true },
                "contentMappers": [{ "package": "mapper", "extensions": [".vue"] }]
            }"#,
        ),
        ("/home/src/workspaces/project/app.vue", "export const app = 1;"),
        (
            "/home/src/workspaces/project/index.ts",
            "export const local = 1;",
        ),
        (manifest_file, &manifest),
    ])
    .with_content_mappers();
    let arguments = args(&["-p", MAPPED_PROJECT, "--runExternalCode"]);
    let run = |expected: SemanticDiagnosticsState, identity: &str| {
        system.programs.lock().unwrap().clear();
        assert_eq!(
            status_of(&system, &arguments, Some(&system)),
            0,
            "{}",
            system.take_output()
        );
        let programs = system.programs.lock().unwrap().clone();
        assert!(
            programs[0]
                .files
                .iter()
                .all(|file| file.semantic_diagnostics == expected),
            "{:?}",
            programs[0].files
        );
        let build_info = system.text("/home/src/workspaces/project/tsconfig.tsbuildinfo");
        assert!(
            build_info.contains(&format!(r#""contentMapperIdentities":["{identity}:"#)),
            "{build_info}"
        );
    };
    run(SemanticDiagnosticsState::Refreshed, "mapper@1.0.0");
    run(SemanticDiagnosticsState::Kept, "mapper@1.0.0");
    system
        .fs
        .write(
            manifest_file,
            manifest
                .replace(r#""version": "1.0.0""#, r#""version": "2.0.0""#)
                .as_bytes(),
        )
        .unwrap();
    run(SemanticDiagnosticsState::Refreshed, "mapper@2.0.0");
}

const MAPPED_CONFIG: &str = "/home/src/workspaces/project/tsconfig.json";

/// tsgo TestContentMapperWatchLifecycle: a watch keeps its mapper's process,
/// replaces it when the configuration names another mapper, closes it when
/// the configuration has none, and closes what it has when it ends.
fn check_content_mapper_watch_lifecycle(arguments: &[&str]) {
    let mapper_a = package_json(VERBATIM_MAPPER);
    let mapper_b = mapper_a.replace(r#""version": "1.0.0""#, r#""version": "2.0.0""#);
    let configured = |mapper: &str| {
        format!(
            r#"{{
                "compilerOptions": {{ "composite": true }},
                "contentMappers": [{{ "package": "{mapper}", "extensions": [".vue"] }}]
            }}"#
        )
    };
    let system = MemorySystem::new(&[
        (MAPPED_CONFIG, &configured("mapper-a")),
        ("/home/src/workspaces/project/app.vue", "export const app = 1;"),
        (
            "/home/src/workspaces/project/node_modules/mapper-a/package.json",
            &mapper_a,
        ),
        (
            "/home/src/workspaces/project/node_modules/mapper-b/package.json",
            &mapper_b,
        ),
    ])
    .with_content_mappers();
    let result = execute_command_line(&system, &args(arguments), Some(&system));
    let mut watcher = result.watcher.expect("a watch");
    assert_eq!(system.mapper_processes(), (1, 0), "{}", system.take_output());

    for (config, processes) in [
        (configured("mapper-b"), (2, 1)),
        (r#"{ "compilerOptions": { "composite": true } }"#.to_owned(), (2, 2)),
        (configured("mapper-a"), (3, 2)),
    ] {
        system.fs.write(MAPPED_CONFIG, config.as_bytes()).unwrap();
        system.changed(MAPPED_CONFIG);
        watcher.do_cycle();
        assert_eq!(system.mapper_processes(), processes, "{}", system.take_output());
    }
    drop(watcher);
    assert_eq!(system.mapper_processes(), (3, 3));
}

#[test]
fn a_watch_starts_replaces_and_closes_its_content_mappers() {
    check_content_mapper_watch_lifecycle(&[
        "--watch",
        "-p",
        MAPPED_PROJECT,
        "--runExternalCode",
    ]);
}

#[test]
fn a_build_watch_starts_replaces_and_closes_its_content_mappers() {
    check_content_mapper_watch_lifecycle(&[
        "--build",
        MAPPED_PROJECT,
        "--watch",
        "--runExternalCode",
    ]);
}

#[test]
fn supplemental_diagnostics_name_the_original_file() {
    // tsgo TestContentMapperSupplementalDiagnosticUsesOriginalFileName.
    let manifest = package_json(SUPPLEMENTAL_DIAGNOSTICS_MAPPER);
    let system = MemorySystem::new(&[
        (
            MAPPED_CONFIG,
            r#"{
                "compilerOptions": { "noEmit": true },
                "contentMappers": [{ "package": "mapper", "extensions": [".astro"] }]
            }"#,
        ),
        (
            "/home/src/workspaces/project/app.astro",
            "const value: string = 1;",
        ),
        (
            "/home/src/workspaces/project/node_modules/mapper/package.json",
            &manifest,
        ),
    ])
    .with_content_mappers();
    let result = execute_command_line(
        &system,
        &args(&["-p", MAPPED_PROJECT, "--pretty", "false", "--runExternalCode"]),
        Some(&system),
    );
    let output = system.take_output();
    assert_eq!(result.status, 2, "{output}");
    assert!(output.contains("app.astro(1,1): error TS2304"), "{output}");
    assert!(output.contains("app.astro(1,7): error TS2322"), "{output}");
    assert!(!output.contains("app.astro.0.ts"), "{output}");
}

#[test]
fn a_watch_builds_again_when_a_supplemental_name_collides() {
    // tsgo TestContentMapperSupplementalCollisionWatch: a file with a
    // supplemental output's name, created and removed again, builds the
    // project each time (a build of this watch is always a full one).
    let manifest = package_json(SUPPLEMENTAL_MAPPER);
    let supplemental = "/home/src/workspaces/project/app.vue.0.ts";
    let system = MemorySystem::new(&[
        (
            MAPPED_CONFIG,
            r#"{
                "compilerOptions": { "noLib": true },
                "contentMappers": [{ "package": "mapper", "extensions": [".vue"] }]
            }"#,
        ),
        (
            "/home/src/workspaces/project/app.vue",
            "declare const value: number;",
        ),
        (
            "/home/src/workspaces/project/node_modules/mapper/package.json",
            &manifest,
        ),
    ])
    .with_content_mappers();
    let result = execute_command_line(
        &system,
        &args(&["--watch", "-p", MAPPED_PROJECT, "--runExternalCode"]),
        Some(&system),
    );
    let mut watcher = result.watcher.expect("a watch");
    system.take_output();

    system.fs.write(supplemental, b"export {};\n").unwrap();
    system.changed(supplemental);
    watcher.do_cycle();
    let output = system.take_output();
    assert!(output.contains("File change detected"), "{output}");
    assert!(output.contains("TS18069"), "{output}");

    system.fs.remove(supplemental).unwrap();
    system.changed(supplemental);
    watcher.do_cycle();
    let output = system.take_output();
    assert!(output.contains("File change detected"), "{output}");
    assert!(!output.contains("TS18069"), "{output}");
}

/// tsgo TestDynamicContentMapperWatchDependency and its build-watch twin: a
/// change of the configuration a dynamic mapper watches refreshes its
/// project (closed and opened again) in the same process and builds again.
fn check_dynamic_mapper_watch_dependency(arguments: &[&str]) {
    let manifest = package_json(DYNAMIC_VERBATIM_MAPPER);
    let mapper_config = "/home/src/workspaces/project/mapper.config.json";
    let system = MemorySystem::new(&[
        (
            MAPPED_CONFIG,
            r#"{
                "compilerOptions": { "composite": true },
                "contentMappers": [{ "package": "mapper", "extensions": [".vue"] }]
            }"#,
        ),
        (mapper_config, r#"{ "version": 1 }"#),
        ("/home/src/workspaces/project/app.vue", "export const app = 1;"),
        (
            "/home/src/workspaces/project/node_modules/mapper/package.json",
            &manifest,
        ),
    ])
    .with_content_mappers();
    let result = execute_command_line(&system, &args(arguments), Some(&system));
    let mut watcher = result.watcher.expect("a watch");
    assert_eq!(system.mapper_projects(), (1, 0), "{}", system.take_output());

    system.fs.write(mapper_config, br#"{ "version": 2 }"#).unwrap();
    system.changed(mapper_config);
    watcher.do_cycle();
    let output = system.take_output();
    assert!(
        output.contains("File change detected. Starting incremental compilation..."),
        "{output}"
    );
    assert_eq!(system.mapper_projects(), (2, 1), "{output}");
    assert_eq!(system.mapper_processes(), (1, 0), "{output}");
}

#[test]
fn a_watch_refreshes_a_dynamic_mapper_when_its_configuration_changes() {
    check_dynamic_mapper_watch_dependency(&[
        "--watch",
        "-p",
        MAPPED_PROJECT,
        "--runExternalCode",
    ]);
}

#[test]
fn a_build_watch_refreshes_a_dynamic_mapper_when_its_configuration_changes() {
    check_dynamic_mapper_watch_dependency(&[
        "--build",
        MAPPED_PROJECT,
        "--watch",
        "--runExternalCode",
    ]);
}

#[test]
fn a_watch_builds_a_mixed_batch_over_the_new_mapped_file() {
    // tsgo TestContentMapperMixedWatchBatchForcesFullRebuild.
    let manifest = package_json(VERBATIM_MAPPER);
    let mapped = "/home/src/workspaces/project/app.vue";
    let main = "/home/src/workspaces/project/main.ts";
    let system = MemorySystem::new(&[
        (
            MAPPED_CONFIG,
            r#"{
                "compilerOptions": { "noLib": true },
                "contentMappers": [{ "package": "mapper", "extensions": [".vue"] }]
            }"#,
        ),
        (mapped, "export const marker = 1 as const;"),
        (
            main,
            r#"import { marker } from "./app.vue"; const check: 1 = marker;"#,
        ),
        (
            "/home/src/workspaces/project/node_modules/mapper/package.json",
            &manifest,
        ),
    ])
    .with_content_mappers();
    let result = execute_command_line(
        &system,
        &args(&[
            "--watch",
            "-p",
            MAPPED_PROJECT,
            "--pretty",
            "false",
            "--runExternalCode",
        ]),
        Some(&system),
    );
    let mut watcher = result.watcher.expect("a watch");
    system.take_output();
    system
        .fs
        .write(mapped, b"export const marker = 2 as const;")
        .unwrap();
    system
        .fs
        .write(
            main,
            br#"import { marker } from "./app.vue"; const check: 2 = marker;"#,
        )
        .unwrap();
    system.changed(mapped);
    system.changed(main);
    watcher.do_cycle();
    let output = system.take_output();
    assert!(
        !output.contains("Type '1' is not assignable to type '2'"),
        "{output}"
    );
}

/// A composite project whose `.vue` files the `mapper` package maps.
const COMPOSITE_MAPPED_CONFIG: &str = r#"{
    "compilerOptions": { "composite": true },
    "contentMappers": [{ "package": "mapper", "extensions": [".vue"] }]
}"#;

/// The project of tsgo's symlinked-manifest tests: the mapper package is a
/// link to a directory outside the project.
fn linked_mapper_system(
    config: &str,
    target: &str,
    link_target: &str,
    case_sensitive: bool,
) -> MemorySystem {
    MemorySystem::from_seeds(
        vec![
            (MAPPED_CONFIG.to_owned(), Seed::file(config)),
            (
                "/home/src/workspaces/project/app.vue".to_owned(),
                Seed::file("export const app = 1;"),
            ),
            (
                "/home/src/workspaces/project/node_modules/mapper".to_owned(),
                Seed::symlink(link_target),
            ),
            (target.to_owned(), Seed::file(package_json(VERBATIM_MAPPER))),
        ],
        case_sensitive,
    )
    .with_content_mappers()
}

#[test]
fn a_build_watch_follows_a_linked_mapper_manifest() {
    // tsgo TestContentMapperBuildWatchSymlinkedManifestChange.
    let manifest = "/home/src/workspaces/mapper/package.json";
    let system = linked_mapper_system(
        COMPOSITE_MAPPED_CONFIG,
        manifest,
        "/home/src/workspaces/mapper",
        true,
    );
    let result = execute_command_line(
        &system,
        &args(&["--build", MAPPED_PROJECT, "--watch", "--runExternalCode"]),
        Some(&system),
    );
    let mut watcher = result.watcher.expect("a watch");
    assert_eq!(system.mapper_processes(), (1, 0), "{}", system.take_output());
    let updated = package_json(VERBATIM_MAPPER).replace(r#""version": "1.0.0""#, r#""version": "2.0.0""#);
    system.fs.write(manifest, updated.as_bytes()).unwrap();
    system.changed(manifest);
    watcher.do_cycle();
    assert_eq!(system.mapper_processes(), (2, 1), "{}", system.take_output());
}

#[test]
fn a_watch_matches_a_mapper_manifest_change_without_case() {
    // tsgo TestContentMapperWatchManifestChangeIgnoresCase.
    let target = "/home/src/workspaces/Mapper/package.json";
    let event = "/home/src/workspaces/mapper/package.json";
    let system = linked_mapper_system(
        r#"{ "contentMappers": [{ "package": "mapper", "extensions": [".vue"] }] }"#,
        target,
        "/home/src/workspaces/Mapper",
        false,
    );
    let result = execute_command_line(
        &system,
        &args(&["--watch", "-p", MAPPED_PROJECT, "--runExternalCode"]),
        Some(&system),
    );
    let mut watcher = result.watcher.expect("a watch");
    assert_eq!(system.mapper_processes(), (1, 0), "{}", system.take_output());
    let updated = package_json(VERBATIM_MAPPER).replace(r#""version": "1.0.0""#, r#""version": "2.0.0""#);
    system.fs.write(event, updated.as_bytes()).unwrap();
    system.changed(event);
    watcher.do_cycle();
    assert_eq!(system.mapper_processes(), (2, 1), "{}", system.take_output());
}

#[test]
fn a_build_watch_reports_a_deleted_mapper_manifest() {
    // tsgo TestContentMapperBuildWatchSymlinkedManifestDelete.
    let manifest = "/home/src/workspaces/mapper/package.json";
    let system = linked_mapper_system(
        COMPOSITE_MAPPED_CONFIG,
        manifest,
        "/home/src/workspaces/mapper",
        true,
    );
    let result = execute_command_line(
        &system,
        &args(&["--build", MAPPED_PROJECT, "--watch", "--runExternalCode"]),
        Some(&system),
    );
    let mut watcher = result.watcher.expect("a watch");
    assert_eq!(system.mapper_processes(), (1, 0), "{}", system.take_output());
    system.take_output();
    system.fs.remove(manifest).unwrap();
    system.changed(manifest);
    watcher.do_cycle();
    let output = system.take_output();
    assert_eq!(system.mapper_processes(), (1, 1), "{output}");
    assert!(
        output.contains("The content mapper package 'mapper' could not be resolved."),
        "{output}"
    );
}

#[test]
fn a_build_watch_shares_a_mapper_process_between_projects() {
    // tsgo TestContentMapperBuildWatchSharedLifecycle: two projects use one
    // mapper's process, which closes when the last stops using it.
    let manifest = package_json(VERBATIM_MAPPER);
    let mapped = r#"{
        "compilerOptions": { "composite": true },
        "contentMappers": [{ "package": "mapper", "extensions": [".vue"] }]
    }"#;
    let system = MemorySystem::new(&[
        (
            MAPPED_CONFIG,
            r#"{ "files": [], "references": [{ "path": "a" }, { "path": "b" }] }"#,
        ),
        ("/home/src/workspaces/project/a/tsconfig.json", mapped),
        ("/home/src/workspaces/project/a/app.vue", "export const a = 1;"),
        ("/home/src/workspaces/project/b/tsconfig.json", mapped),
        ("/home/src/workspaces/project/b/app.vue", "export const b = 1;"),
        (
            "/home/src/workspaces/project/node_modules/mapper/package.json",
            &manifest,
        ),
    ])
    .with_content_mappers();
    let result = execute_command_line(
        &system,
        &args(&["--build", MAPPED_PROJECT, "--watch", "--runExternalCode"]),
        Some(&system),
    );
    let mut watcher = result.watcher.expect("a watch");
    assert_eq!(system.mapper_processes(), (1, 0), "{}", system.take_output());
    for (project, closes) in [("a", 0), ("b", 1)] {
        let config = format!("/home/src/workspaces/project/{project}/tsconfig.json");
        system
            .fs
            .write(&config, br#"{ "compilerOptions": { "composite": true } }"#)
            .unwrap();
        system.changed(&config);
        watcher.do_cycle();
        assert_eq!(system.mapper_processes().1, closes, "{}", system.take_output());
    }
}

