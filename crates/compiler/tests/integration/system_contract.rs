//! The command line over a system without a disk: an in-memory file system,
//! a stepping clock and a fixed environment (`tsc_compiler::system`), the
//! way a test or a WebAssembly embedding runs it.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use tsc_compiler::system::{CommandLineTesting, ProgramReport, System};
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
}

impl MemorySystem {
    fn new(files: &[(&str, &str)]) -> Self {
        let clock = Arc::new(SteppingClock::new(
            SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000),
            Duration::from_secs(1),
        ));
        let mut seeds = files
            .iter()
            .map(|(path, text)| (path.to_string(), Seed::file(text)))
            .collect::<Vec<_>>();
        for name in ["lib.d.ts", "lib.es2026.full.d.ts"] {
            seeds.push((format!("/lib/{name}"), Seed::file(LIBRARY)));
        }
        let fs = MemFs::from_entries(seeds, true, Arc::clone(&clock) as Arc<dyn Clock>)
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
        }
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
}

impl CommandLineTesting for MemorySystem {
    fn on_program(&self, program: &ProgramReport) {
        self.programs.lock().unwrap().push(program.clone());
    }
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
    let status = execute_command_line(&system, &[], None);
    assert_eq!(status, 0, "{}", system.output.lock().unwrap());
    assert_eq!(system.text("/work/out/a.js"), "export const a = 1;\n");
    assert!(system.output.lock().unwrap().is_empty());
}

#[test]
fn diagnostics_follow_the_system_and_command_line_errors_have_no_summary() {
    let system = MemorySystem::new(&[("/work/a.ts", "const x: string = 1;\n")]);
    let status = execute_command_line(&system, &args(&["a.ts", "--noEmit"]), None);
    assert_eq!(status, 2);
    assert_eq!(
        *system.output.lock().unwrap(),
        "a.ts(1,7): error TS2322: Type 'number' is not assignable to type 'string'.\n"
    );

    // tsgo reports a command line's own errors and stops: no summary even
    // when the output is pretty.
    let system = MemorySystem::new(&[]);
    let status = execute_command_line(&system, &args(&["--bogus", "--pretty"]), None);
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
    assert_eq!(execute_command_line(&system, &[], Some(&system)), 0);
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
    assert_eq!(execute_command_line(&system, &[], Some(&system)), 0);
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
    assert_eq!(execute_command_line(&system, &build, Some(&system)), 0);
    assert!(system.fs.is_file("/work/server/dist/server/src/b.js"));
    system.output.lock().unwrap().clear();
    assert_eq!(execute_command_line(&system, &build, Some(&system)), 0);
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
    let status = execute_command_line(
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
    let status = execute_command_line(&system, &args(&["-b"]), Some(&system));
    assert_eq!(status, 0, "{}", system.output.lock().unwrap());
    assert!(system.fs.read("/work/a.js").is_ok());
    assert!(system.fs.read("/work/trace/trace.json").is_err());

    // The command records the option the config sets.
    let status = execute_command_line(&system, &[], Some(&system));
    assert_eq!(status, 0, "{}", system.output.lock().unwrap());
    assert!(system.fs.read("/work/trace/trace.json").is_ok());
}
