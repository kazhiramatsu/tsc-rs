//! A build holds one project at a time. A project's program and checker
//! state are dropped when the project is built, as tsgo's are collected
//! while its build goes on; only the last project of a one-shot `tsc -b`,
//! after which the process exits, is left to the exit, and a build watch
//! leaves none. (Every project used to be left to the exit, as a one-shot
//! command's is: `tsc -b` over 100 small projects peaked at 5.7 GB where
//! tsgo peaks at 0.6 GB, and `tsc -b -w` grew with every rebuild.)
//!
//! The test counts the identity leases of the process — a live checker state
//! holds one for its types — so it is the only test of this binary.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use tsc_compiler::execute_command_line;
use tsc_compiler::system::{CommandLineTesting, System};
use tsc_compiler::watch::{
    WatchBackend, WatchEvent, WatchEventKind, WatchEvents, WatchHandle, WatchRequest,
};
use tsc_host::vfs::{Clock, FileSystem, MemFs, Seed, SteppingClock};
use tsc_types::{IdentityDomain, IdentitySpace};

const LIBRARY: &str = "interface Array<T> { length: number; [n: number]: T; }\n\
    interface Boolean {}\ninterface Function {}\ninterface CallableFunction {}\n\
    interface NewableFunction {}\ninterface IArguments {}\ninterface Number {}\n\
    interface Object {}\ninterface RegExp {}\ninterface String {}\n";

/// Deep enough for a watch: tsgo watches no directory near the root
/// (`CanWatchDirectory`).
const ROOT: &str = "/home/src/workspaces/solution";

const PROJECT_CONFIG: &str =
    r#"{ "compilerOptions": { "composite": true, "outDir": "out", "rootDir": "." } }"#;

/// The command line's system over a memory file system, one checker and one
/// worker, with the watches a run asks for kept for the test to feed.
struct MemorySystem {
    fs: MemFs,
    clock: Arc<SteppingClock>,
    env: BTreeMap<String, String>,
    output: Mutex<String>,
    watches: Mutex<Vec<(String, bool, WatchEvents)>>,
}

impl MemorySystem {
    /// `projects` independent composite projects `ROOT/p<n>` of one file
    /// each and a solution `ROOT/tsconfig.json` that references them all.
    fn workspace(projects: usize) -> Self {
        let clock = Arc::new(SteppingClock::new(
            SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000),
            Duration::from_secs(1),
        ));
        let mut seeds = vec![
            ("/lib/lib.d.ts".to_owned(), Seed::file(LIBRARY)),
            ("/lib/lib.es2026.full.d.ts".to_owned(), Seed::file(LIBRARY)),
        ];
        let mut references = Vec::new();
        for project in 0..projects {
            seeds.push((
                format!("{ROOT}/p{project}/tsconfig.json"),
                Seed::file(PROJECT_CONFIG),
            ));
            seeds.push((
                format!("{ROOT}/p{project}/index.ts"),
                Seed::file(format!(
                    "export const value{project}: number = {project};\n"
                )),
            ));
            references.push(format!(r#"{{ "path": "p{project}" }}"#));
        }
        seeds.push((
            format!("{ROOT}/tsconfig.json"),
            Seed::file(format!(
                r#"{{ "files": [], "references": [{}] }}"#,
                references.join(", ")
            )),
        ));
        Self {
            fs: MemFs::from_entries(seeds, true, Arc::clone(&clock) as Arc<dyn Clock>)
                .expect("valid files"),
            clock,
            env: BTreeMap::from([
                ("TSRS_WORKERS".to_owned(), "1".to_owned()),
                ("TSRS_CHECKERS".to_owned(), "1".to_owned()),
            ]),
            output: Mutex::new(String::new()),
            watches: Mutex::new(Vec::new()),
        }
    }

    /// Reports a change of `path` to the watches whose directory holds it.
    fn changed(&self, path: &str) {
        for (directory, recursive, events) in self.watches.lock().unwrap().iter() {
            let Some(rest) = path.strip_prefix(&format!("{directory}/")) else {
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

fn args(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_owned()).collect()
}

/// How many type tables hold a lease in the process: one per checker state
/// that is alive or was left to the process exit.
fn type_leases() -> usize {
    IdentityDomain::new()
        .stats()
        .expect("identity statistics")
        .space(IdentitySpace::Type)
        .active_ranges
}

/// Both reserved rooms and initialized source views must return with a project.
fn syntax_leases() -> [(usize, u64); 4] {
    let stats = IdentityDomain::new().stats().expect("identity statistics");
    [
        IdentitySpace::Node,
        IdentitySpace::NodeArray,
        IdentitySpace::NodeArrayItem,
        IdentitySpace::NodeText,
    ]
    .map(|space| {
        let stats = stats.space(space);
        (stats.active_ranges, stats.active_values)
    })
}

#[test]
fn a_build_holds_one_project_at_a_time() {
    // A single project's retained rooms are the reference for a larger build.
    let single = MemorySystem::workspace(1);
    let single_before = syntax_leases();
    let result = execute_command_line(&single, &args(&["-b", ROOT]), Some(&single));
    assert_eq!(result.status, 0, "{}", single.take_output());
    let single_after = syntax_leases();
    let retained = std::array::from_fn::<_, 4, _>(|index| {
        (
            single_after[index].0 - single_before[index].0,
            single_after[index].1 - single_before[index].1,
        )
    });
    assert!(retained
        .iter()
        .all(|&(ranges, values)| ranges > 0 && values > 0));

    // A one-shot build of five projects and the solution that references
    // them: four are dropped as the build goes on, the last program is left
    // to the process exit (the solution, last in the order, builds none).
    let system = MemorySystem::workspace(5);
    let syntax_before = syntax_leases();
    let before = type_leases();
    let result = execute_command_line(&system, &args(&["-b", ROOT]), Some(&system));
    let output = system.take_output();
    assert_eq!(result.status, 0, "{output}");
    for project in 0..5 {
        assert!(
            system
                .fs
                .read(&format!("{ROOT}/p{project}/out/index.d.ts"))
                .is_ok(),
            "project {project} was built: {output}"
        );
    }
    assert_eq!(
        type_leases() - before,
        1,
        "a one-shot build leaves its last program's checker state to the process exit, and no other"
    );

    let syntax_after = syntax_leases();
    assert_eq!(
        std::array::from_fn::<_, 4, _>(|index| (
            syntax_after[index].0 - syntax_before[index].0,
            syntax_after[index].1 - syntax_before[index].1,
        )),
        retained,
        "five projects retain only the last project's syntax rooms and views"
    );

    // A plain watch keeps incremental facts, while exact syntax leases belong
    // to each cycle. It must not inherit either one-shot command's rooms.
    let watched = MemorySystem::workspace(1);
    let syntax_before_watch = syntax_leases();
    let types_before_watch = type_leases();
    let result = execute_command_line(
        &watched,
        &args(&["-p", &format!("{ROOT}/p0"), "--watch"]),
        Some(&watched),
    );
    assert_eq!(result.status, 0, "{}", watched.take_output());
    let mut plain = result.watcher.expect("--watch starts a watcher");
    assert_eq!(syntax_leases(), syntax_before_watch);
    assert_eq!(type_leases(), types_before_watch);
    for cycle in 1..=3 {
        let path = format!("{ROOT}/p0/index.ts");
        watched
            .fs
            .write(
                &path,
                format!("export const value0: number = {cycle}0;\n").as_bytes(),
            )
            .unwrap();
        watched.changed(&path);
        plain.do_cycle();
        let output = watched.take_output();
        assert!(
            output.contains("Found 0 errors. Watching for file changes."),
            "{output}"
        );
        assert_eq!(
            syntax_leases(),
            syntax_before_watch,
            "plain watch cycle {cycle}"
        );
        assert_eq!(
            type_leases(),
            types_before_watch,
            "plain watch cycle {cycle}"
        );
    }
    drop(plain);

    // A build watch: neither its first build nor a rebuild leaves a state.
    let system = MemorySystem::workspace(3);
    let syntax_before = syntax_leases();
    let before = type_leases();
    let result = execute_command_line(&system, &args(&["-b", ROOT, "--watch"]), Some(&system));
    assert_eq!(result.status, 0, "{}", system.take_output());
    let mut watcher = result.watcher.expect("-b --watch starts a watch");
    let output = system.take_output();
    assert!(
        output.contains("Found 0 errors. Watching for file changes."),
        "{output}"
    );
    assert_eq!(
        type_leases(),
        before,
        "the first build of a watch keeps no checker state"
    );
    assert_eq!(
        syntax_leases(),
        syntax_before,
        "a build watch retains no syntax rooms after its initial build"
    );
    for cycle in 1..=4 {
        let path = format!("{ROOT}/p1/index.ts");
        system
            .fs
            .write(
                &path,
                format!("export const value1: number = {cycle}0;\n").as_bytes(),
            )
            .unwrap();
        system.changed(&path);
        watcher.do_cycle();
        let output = system.take_output();
        assert!(
            output.contains("Found 0 errors. Watching for file changes."),
            "cycle {cycle}: {output}"
        );
        assert_eq!(
            String::from_utf8(system.fs.read(&format!("{ROOT}/p1/out/index.js")).unwrap()).unwrap(),
            format!("export const value1 = {cycle}0;\n"),
            "cycle {cycle} rebuilt the project"
        );
        assert_eq!(
            type_leases(),
            before,
            "rebuild {cycle} of a build watch keeps no checker state"
        );
        assert_eq!(
            syntax_leases(),
            syntax_before,
            "rebuild {cycle} of a build watch retains no syntax rooms or views"
        );
    }
}
