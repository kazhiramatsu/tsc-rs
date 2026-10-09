//! tsgo's project tests on the standalone API path (P5-1b-1): synthetic
//! programs (`project/snapshot_test.go`, `api/session_createprogram_test.go`
//! at the snapshot layer), project IDs, and configured projects opened,
//! changed and ensured through API requests (`configfilechanges_test.go`
//! and `project_test.go` re-expressed with API requests instead of editor
//! events).

use std::collections::BTreeSet;
use std::sync::Arc;

use tsc_host::vfs::{FileSystem, MemFs, Seed, SystemClock};
use tsc_program::{CompilerOptions, LibraryCatalog, ProgramLoadLimits, ProgramOptions};
use tsc_project::{
    ApiSnapshotRequest, CreateProgramRequest, FileChangeSummary, ProgramUpdateKind, ProjectId,
    ProjectKind, ReconfigureProgramRequest, SessionOptions, Snapshot, SnapshotHost,
};

fn session(files: &[(&str, &str)]) -> (SnapshotHost, Arc<MemFs>) {
    let fs = Arc::new(
        MemFs::from_entries(
            files.iter().map(|(name, text)| (*name, Seed::file(text))),
            false,
            Arc::new(SystemClock),
        )
        .expect("build the file system"),
    );
    let host = SnapshotHost::new(
        SessionOptions {
            current_directory: "/".to_owned(),
            library_catalog: LibraryCatalog::typescript_7_1("/typescript/lib"),
            load_limits: ProgramLoadLimits::new(1_000, 10_000, 64, 1 << 20, 1 << 24),
        },
        Arc::clone(&fs) as Arc<dyn FileSystem>,
    );
    (host, fs)
}

fn no_lib(root_file_names: &[&str]) -> CreateProgramRequest {
    CreateProgramRequest {
        root_file_names: root_file_names
            .iter()
            .map(|name| (*name).to_owned())
            .collect(),
        program_options: ProgramOptions::default().with_no_lib(true),
        ..CreateProgramRequest::default()
    }
}

fn update(
    host: &SnapshotHost,
    base: &Snapshot,
    request: ApiSnapshotRequest,
) -> Result<Snapshot, String> {
    host.clone_snapshot(base, FileChangeSummary::default(), Some(&request))
        .map_err(|error| error.to_string())
}

fn changed(host: &SnapshotHost, base: &Snapshot, files: &[&str]) -> Snapshot {
    let changes = FileChangeSummary {
        changed: files.iter().map(|name| (*name).to_owned()).collect(),
        ..FileChangeSummary::default()
    };
    host.clone_snapshot(base, changes, None)
        .expect("apply the file changes")
}

fn open_projects(config_file_names: &[&str]) -> ApiSnapshotRequest {
    ApiSnapshotRequest {
        open_projects: config_file_names
            .iter()
            .map(|name| (*name).to_owned())
            .collect(),
        ..ApiSnapshotRequest::default()
    }
}

fn ensure(ids: &[&ProjectId]) -> ApiSnapshotRequest {
    ApiSnapshotRequest {
        ensure_programs: ids.iter().map(|id| (*id).clone()).collect(),
        ..ApiSnapshotRequest::default()
    }
}

fn configured(path: &str) -> ProjectId {
    ProjectId::configured(path).expect("a configured project ID")
}

#[test]
fn project_ids_narrow_as_tsgo_parses_them() {
    // TestProjectIDNarrowing.
    let configured_id = ProjectId::new("/project/tsconfig.json");
    assert_eq!(configured_id.kind(), Some(ProjectKind::Configured));
    assert_eq!(
        ProjectId::configured("/project/tsconfig.json"),
        Some(configured_id)
    );
    assert_eq!(ProjectId::inferred().kind(), Some(ProjectKind::Inferred));
    assert_eq!(ProjectId::configured("/dev/null/inferred"), None);
    let synthetic = ProjectId::synthetic(1);
    assert_eq!(synthetic.as_str(), "/dev/null/synthetic/1");
    assert_eq!(synthetic.kind(), Some(ProjectKind::Synthetic));
    assert_eq!(ProjectId::configured(synthetic.as_str()), None);
    assert_eq!(
        ProjectId::parse_synthetic("/dev/null/synthetic/01"),
        Some(ProjectId::synthetic(1))
    );
    assert_eq!(
        ProjectId::new("/dev/null/synthetic/invalid").kind(),
        Some(ProjectKind::Configured)
    );
    assert_eq!(ProjectId::parse_synthetic("/dev/null/synthetic/0"), None);
    assert_eq!(ProjectId::parse_synthetic("/dev/null/synthetic/-1"), None);
    assert_eq!(ProjectId::new("").kind(), None);
}

#[test]
fn creates_and_removes_synthetic_programs() {
    // TestSnapshot "creates and removes synthetic programs" (the opened-file
    // part comes with the inferred project in P5-1b-2).
    let (host, _) = session(&[
        ("/a.ts", "export const a = 1;"),
        ("/b.ts", "export const b = 1;"),
    ]);
    let root = host.new_root_snapshot();
    let created = update(
        &host,
        &root,
        ApiSnapshotRequest {
            create_programs: vec![no_lib(&["/a.ts"]), no_lib(&["/b.ts"])],
            ..ApiSnapshotRequest::default()
        },
    )
    .expect("create the programs");
    let [first, second] = created.created_programs() else {
        panic!("two programs");
    };
    assert_eq!(first.as_str(), "/dev/null/synthetic/1");
    assert_eq!(second.as_str(), "/dev/null/synthetic/2");
    let first_project = Arc::clone(created.project(first).expect("the first program"));
    let second_project = Arc::clone(created.project(second).expect("the second program"));
    assert_eq!(first_project.config_file_name(), None);
    assert_eq!(second_project.config_file_name(), None);
    assert_eq!(first_project.root_file_names(), ["/a.ts"]);
    assert_eq!(second_project.root_file_names(), ["/b.ts"]);
    assert_eq!(created.projects().count(), 2);
    assert!(first_project.program().is_some());

    let removed = update(
        &host,
        &created,
        ApiSnapshotRequest {
            remove_programs: BTreeSet::from([first.clone()]),
            ..ApiSnapshotRequest::default()
        },
    )
    .expect("remove the first program");
    assert!(removed.project(first).is_none());
    assert!(Arc::ptr_eq(
        removed.project(second).expect("the second program stays"),
        &second_project
    ));
    assert_eq!(removed.projects().count(), 1);
    // The earlier snapshot keeps its programs.
    assert!(Arc::ptr_eq(
        created
            .project(first)
            .expect("still in the earlier snapshot"),
        &first_project
    ));
}

#[test]
fn a_failed_update_reports_its_error() {
    // TestSnapshot "failed API update is not adopted" and
    // TestCreateSnapshotRejectsRemovingProgramFromIndependentRoot.
    let (host, _) = session(&[("/a.ts", "export const a = 1;")]);
    let root = host.new_root_snapshot();
    let error = update(
        &host,
        &root,
        ApiSnapshotRequest {
            remove_programs: BTreeSet::from([ProjectId::synthetic(1)]),
            ..ApiSnapshotRequest::default()
        },
    )
    .err()
    .expect("the program does not exist");
    assert_eq!(
        error,
        "synthetic program not found for removal: /dev/null/synthetic/1"
    );
}

#[test]
fn independent_roots_number_their_snapshots() {
    // TestCreateSnapshotUsesIndependentRoots.
    let (host, _) = session(&[("/home/projects/p/src/index.ts", "export const x = 1;")]);
    let first = update(
        &host,
        &host.new_root_snapshot(),
        ApiSnapshotRequest {
            create_programs: vec![no_lib(&["/home/projects/p/src/index.ts"])],
            ..ApiSnapshotRequest::default()
        },
    )
    .expect("create a program");
    assert_eq!(first.id(), 1);
    assert_eq!(first.projects().count(), 1);
    let second = update(
        &host,
        &host.new_root_snapshot(),
        ApiSnapshotRequest::default(),
    )
    .expect("create an empty snapshot");
    assert_eq!(second.id(), 2);
    assert_eq!(second.projects().count(), 0);
}

#[test]
fn created_programs_keep_their_roots_and_options() {
    // TestCreateSnapshotCreatesPrograms.
    let a = "/home/projects/p/a.ts";
    let b = "/home/projects/p/b.ts";
    let (host, _) = session(&[(a, "export const a = 1;"), (b, "export const b = 1;")]);
    let mut strict = no_lib(&[a, b]);
    strict.compiler_options = CompilerOptions {
        strict: Some(true),
        ..CompilerOptions::default()
    };
    let snapshot = update(
        &host,
        &host.new_root_snapshot(),
        ApiSnapshotRequest {
            create_programs: vec![strict, no_lib(&[b])],
            ..ApiSnapshotRequest::default()
        },
    )
    .expect("create the programs");
    assert_eq!(
        snapshot.created_programs(),
        [ProjectId::synthetic(1), ProjectId::synthetic(2)]
    );
    let projects = snapshot.projects().collect::<Vec<_>>();
    assert_eq!(projects.len(), 2);
    assert_eq!(projects[0].root_file_names(), [a, b]);
    assert_eq!(
        projects[0]
            .command_line()
            .unwrap()
            .compiler_options()
            .strict,
        Some(true)
    );
    assert_eq!(projects[1].root_file_names(), [b]);
}

#[test]
fn a_reconfigured_program_keeps_its_id() {
    // TestUpdateSnapshotReconfiguresSyntheticProgram.
    let a = "/home/projects/p/a.ts";
    let b = "/home/projects/p/b.ts";
    let (host, _) = session(&[(a, "export const a = 1;"), (b, "export const b = 2;")]);
    let created = update(
        &host,
        &host.new_root_snapshot(),
        ApiSnapshotRequest {
            create_programs: vec![no_lib(&[a])],
            ..ApiSnapshotRequest::default()
        },
    )
    .expect("create the program");
    let id = created.created_programs()[0].clone();
    let mut program = no_lib(&[b]);
    program.compiler_options.strict = Some(true);
    let reconfigured = update(
        &host,
        &created,
        ApiSnapshotRequest {
            reconfigure_programs: vec![ReconfigureProgramRequest {
                program_id: id.clone(),
                program,
            }],
            ..ApiSnapshotRequest::default()
        },
    )
    .expect("reconfigure the program");
    let project = reconfigured.project(&id).expect("the same program");
    assert_eq!(project.root_file_names(), [b]);
    assert_eq!(
        project.command_line().unwrap().compiler_options().strict,
        Some(true)
    );
    assert_eq!(project.program_update_kind(), ProgramUpdateKind::NewFiles);
}

#[test]
fn reconfiguration_is_validated() {
    // TestReconfigureSyntheticProgramValidation, the project layer's checks.
    let (host, _) = session(&[]);
    let root = host.new_root_snapshot();
    let reconfigure = |id: &str| ReconfigureProgramRequest {
        program_id: ProjectId::new(id),
        program: CreateProgramRequest::default(),
    };
    let error = update(
        &host,
        &root,
        ApiSnapshotRequest {
            create_programs: vec![CreateProgramRequest::default()],
            reconfigure_programs: vec![reconfigure("/dev/null/synthetic/1")],
            ..ApiSnapshotRequest::default()
        },
    )
    .err();
    assert_eq!(
        error.as_deref(),
        Some("synthetic program not found for reconfiguration: /dev/null/synthetic/1")
    );
    // Each entry is checked in turn (a duplicate, then a removal, then a
    // missing program), so a duplicate shows once the program exists.
    let created = update(
        &host,
        &root,
        ApiSnapshotRequest {
            create_programs: vec![CreateProgramRequest::default()],
            ..ApiSnapshotRequest::default()
        },
    )
    .expect("create a program");
    let error = update(
        &host,
        &created,
        ApiSnapshotRequest {
            reconfigure_programs: vec![
                reconfigure("/dev/null/synthetic/1"),
                reconfigure("/dev/null/synthetic/01"),
            ],
            ..ApiSnapshotRequest::default()
        },
    )
    .err();
    assert_eq!(
        error.as_deref(),
        Some("synthetic program reconfigured more than once: /dev/null/synthetic/1")
    );
    let error = update(
        &host,
        &root,
        ApiSnapshotRequest {
            reconfigure_programs: vec![reconfigure("/dev/null/synthetic/1")],
            remove_programs: BTreeSet::from([ProjectId::synthetic(1)]),
            ..ApiSnapshotRequest::default()
        },
    )
    .err();
    assert_eq!(
        error.as_deref(),
        Some("synthetic program cannot be reconfigured and removed: /dev/null/synthetic/1")
    );
}

#[test]
fn removing_and_creating_in_one_request_reuses_the_number() {
    // tsgo crashes here (nextSyntheticProjectID picks the number removed in
    // the same build); the port gives the new program that number.
    let (host, _) = session(&[("/a.ts", "export const a = 1;"), ("/b.ts", "export {};")]);
    let created = update(
        &host,
        &host.new_root_snapshot(),
        ApiSnapshotRequest {
            create_programs: vec![no_lib(&["/a.ts"])],
            ..ApiSnapshotRequest::default()
        },
    )
    .expect("create the program");
    let replaced = update(
        &host,
        &created,
        ApiSnapshotRequest {
            remove_programs: BTreeSet::from([ProjectId::synthetic(1)]),
            create_programs: vec![no_lib(&["/b.ts"])],
            ..ApiSnapshotRequest::default()
        },
    )
    .expect("replace the program");
    assert_eq!(replaced.created_programs(), [ProjectId::synthetic(1)]);
    assert_eq!(
        replaced
            .project(&ProjectId::synthetic(1))
            .unwrap()
            .root_file_names(),
        ["/b.ts"]
    );
}

#[test]
fn a_changed_file_marks_its_program_dirty_until_ensured() {
    // TestUpdateSnapshotEnsuresSyntheticProgram.
    let file_name = "/home/projects/p/index.ts";
    let (host, fs) = session(&[(file_name, "export const value = 1;")]);
    let created = update(
        &host,
        &host.new_root_snapshot(),
        ApiSnapshotRequest {
            create_programs: vec![no_lib(&[file_name])],
            ..ApiSnapshotRequest::default()
        },
    )
    .expect("create the program");
    let id = created.created_programs()[0].clone();
    assert!(!created.project(&id).unwrap().is_dirty());

    fs.write(file_name, b"export const value = 2;")
        .expect("write the file");
    let dirty = changed(&host, &created, &[file_name]);
    assert!(dirty.project(&id).unwrap().is_dirty());

    let ensured = update(&host, &dirty, ensure(&[&id])).expect("ensure the program");
    assert!(!ensured.project(&id).unwrap().is_dirty());
}

#[test]
fn a_change_without_new_text_leaves_the_program_alone() {
    // TestSnapshot "no-op watch change does not rebuild program".
    let file_name = "/p/index.ts";
    let (host, fs) = session(&[(file_name, "export const value = 1;")]);
    let created = update(
        &host,
        &host.new_root_snapshot(),
        ApiSnapshotRequest {
            create_programs: vec![no_lib(&[file_name])],
            ..ApiSnapshotRequest::default()
        },
    )
    .expect("create the program");
    let id = created.created_programs()[0].clone();
    let program = Arc::clone(created.project(&id).unwrap().program().unwrap());

    let unchanged = changed(&host, &created, &[file_name]);
    assert!(!unchanged.project(&id).unwrap().is_dirty());
    let ensured = update(&host, &unchanged, ensure(&[&id])).expect("ensure the program");
    assert!(Arc::ptr_eq(
        ensured.project(&id).unwrap().program().unwrap(),
        &program
    ));

    fs.write(file_name, b"export const value = 2;")
        .expect("write the file");
    let edited = changed(&host, &ensured, &[file_name]);
    assert!(edited.project(&id).unwrap().is_dirty());
    let rebuilt = update(&host, &edited, ensure(&[&id])).expect("ensure the program");
    assert!(!Arc::ptr_eq(
        rebuilt.project(&id).unwrap().program().unwrap(),
        &program
    ));
}

#[test]
fn an_update_without_changes_shares_the_snapshot_contents() {
    // TestSnapshotUpdateCarriesHostFileSystemWithoutOverride.
    let (host, _) = session(&[
        (
            "/p/tsconfig.json",
            r#"{ "compilerOptions": { "noLib": true } }"#,
        ),
        ("/p/index.ts", "export const value = 1;"),
    ]);
    let opened = update(
        &host,
        &host.new_root_snapshot(),
        open_projects(&["/p/tsconfig.json"]),
    )
    .expect("open the project");
    let id = configured("/p/tsconfig.json");
    let project = Arc::clone(opened.project(&id).expect("the configured project"));
    let next = update(&host, &opened, ApiSnapshotRequest::default()).expect("an empty update");
    assert!(Arc::ptr_eq(next.project(&id).unwrap(), &project));
}

#[test]
fn opening_a_project_builds_its_program() {
    let (host, _) = session(&[
        (
            "/p/tsconfig.json",
            r#"{ "compilerOptions": { "noLib": true, "strict": true } }"#,
        ),
        ("/p/src/a.ts", "export const a = 1;"),
        ("/p/src/b.ts", "export const b = 1;"),
    ]);
    let opened = update(
        &host,
        &host.new_root_snapshot(),
        open_projects(&["/p/tsconfig.json"]),
    )
    .expect("open the project");
    let project = opened
        .project(&configured("/p/tsconfig.json"))
        .expect("the configured project");
    assert_eq!(project.kind(), ProjectKind::Configured);
    assert_eq!(project.config_file_name(), Some("/p/tsconfig.json"));
    assert_eq!(project.current_directory(), "/p");
    assert_eq!(project.root_file_names(), ["/p/src/a.ts", "/p/src/b.ts"]);
    assert_eq!(project.program_update_kind(), ProgramUpdateKind::NewFiles);
    assert_eq!(project.program_last_update(), opened.id());
    assert!(!project.is_dirty());
    assert_eq!(
        project.command_line().unwrap().compiler_options().strict,
        Some(true)
    );
}

#[test]
fn closing_a_project_removes_it() {
    let (host, _) = session(&[
        (
            "/p/tsconfig.json",
            r#"{ "compilerOptions": { "noLib": true } }"#,
        ),
        ("/p/index.ts", "export {};"),
    ]);
    let opened = update(
        &host,
        &host.new_root_snapshot(),
        open_projects(&["/p/tsconfig.json"]),
    )
    .expect("open the project");
    let closed = update(
        &host,
        &opened,
        ApiSnapshotRequest {
            close_projects: BTreeSet::from(["/p/tsconfig.json".to_owned()]),
            ..ApiSnapshotRequest::default()
        },
    )
    .expect("close the project");
    assert_eq!(closed.projects().count(), 0);
    // Closing and opening in one request keeps the project.
    let reopened = update(
        &host,
        &opened,
        ApiSnapshotRequest {
            open_projects: BTreeSet::from(["/p/tsconfig.json".to_owned()]),
            close_projects: BTreeSet::from(["/p/tsconfig.json".to_owned()]),
            ..ApiSnapshotRequest::default()
        },
    )
    .expect("close and open the project");
    assert_eq!(reopened.projects().count(), 1);
}

#[test]
fn opening_a_missing_config_is_no_error() {
    // tsgo: the project is created, its config cannot be read, and the
    // project is deleted again; the request succeeds.
    let (host, _) = session(&[]);
    let snapshot = update(
        &host,
        &host.new_root_snapshot(),
        open_projects(&["/missing/tsconfig.json"]),
    )
    .expect("open a missing project");
    assert_eq!(snapshot.projects().count(), 0);
}

#[test]
fn a_config_change_updates_the_options_when_ensured() {
    // configfilechanges_test "should update program options on config file
    // change", and project_test "SameFileNames on config change without root
    // changes".
    let config = "/src/tsconfig.json";
    let (host, fs) = session(&[
        (
            config,
            r#"{ "compilerOptions": { "noLib": true, "target": "es2015" } }"#,
        ),
        ("/src/index.ts", "export const x = 1;"),
    ]);
    let opened = update(&host, &host.new_root_snapshot(), open_projects(&[config]))
        .expect("open the project");
    let id = configured(config);
    fs.write(
        config,
        br#"{ "compilerOptions": { "noLib": true, "target": "esnext" } }"#,
    )
    .expect("write the config");
    let dirty = changed(&host, &opened, &[config]);
    assert!(dirty.project(&id).unwrap().is_dirty());
    let ensured = update(&host, &dirty, ensure(&[&id])).expect("ensure the project");
    let project = ensured.project(&id).unwrap();
    assert!(!project.is_dirty());
    assert_eq!(
        project.command_line().unwrap().compiler_options().target,
        Some(99)
    );
    assert_eq!(
        project.program_update_kind(),
        ProgramUpdateKind::SameFileNames
    );
}

#[test]
fn an_extended_config_change_reaches_the_extending_project() {
    // configfilechanges_test "should update project on extended config file
    // change" and "on doubly extended config file change".
    let (host, fs) = session(&[
        (
            "/src/tsconfig.json",
            r#"{ "extends": "./middle.json", "compilerOptions": { "noLib": true } }"#,
        ),
        ("/src/middle.json", r#"{ "extends": "./base.json" }"#),
        (
            "/src/base.json",
            r#"{ "compilerOptions": { "strict": true } }"#,
        ),
        ("/src/index.ts", "export const x = 1;"),
    ]);
    let opened = update(
        &host,
        &host.new_root_snapshot(),
        open_projects(&["/src/tsconfig.json"]),
    )
    .expect("open the project");
    let id = configured("/src/tsconfig.json");
    assert_eq!(
        opened
            .project(&id)
            .unwrap()
            .command_line()
            .unwrap()
            .compiler_options()
            .strict,
        Some(true)
    );
    fs.write(
        "/src/base.json",
        br#"{ "compilerOptions": { "strict": false } }"#,
    )
    .expect("write the base config");
    let dirty = changed(&host, &opened, &["/src/base.json"]);
    assert!(dirty.project(&id).unwrap().is_dirty());
    let ensured = update(&host, &dirty, ensure(&[&id])).expect("ensure the project");
    assert_eq!(
        ensured
            .project(&id)
            .unwrap()
            .command_line()
            .unwrap()
            .compiler_options()
            .strict,
        Some(false)
    );
}

#[test]
fn a_created_file_the_includes_match_is_a_new_root() {
    // project_test "NewFiles on root addition".
    let config = "/src/tsconfig.json";
    let (host, fs) = session(&[
        (config, r#"{ "compilerOptions": { "noLib": true } }"#),
        ("/src/index.ts", "export const x = 1;"),
    ]);
    let opened = update(&host, &host.new_root_snapshot(), open_projects(&[config]))
        .expect("open the project");
    let id = configured(config);
    fs.write("/src/other.ts", b"export const y = 2;")
        .expect("write the new file");
    let created = host
        .clone_snapshot(
            &opened,
            FileChangeSummary {
                created: BTreeSet::from(["/src/other.ts".to_owned()]),
                ..FileChangeSummary::default()
            },
            None,
        )
        .expect("apply the creation");
    assert!(created.project(&id).unwrap().is_dirty());
    let ensured = update(&host, &created, ensure(&[&id])).expect("ensure the project");
    let project = ensured.project(&id).unwrap();
    assert_eq!(
        project.root_file_names(),
        ["/src/index.ts", "/src/other.ts"]
    );
    assert_eq!(project.program_update_kind(), ProgramUpdateKind::NewFiles);
}

#[test]
fn a_missing_file_is_not_read() {
    // TestSnapshot "GetFile returns nil for non-existent files".
    let (host, _) = session(&[
        (
            "/home/projects/TS/p1/tsconfig.json",
            r#"{ "compilerOptions": { "noLib": true } }"#,
        ),
        (
            "/home/projects/TS/p1/index.ts",
            "console.log('Hello, world!');",
        ),
    ]);
    let snapshot = update(
        &host,
        &host.new_root_snapshot(),
        open_projects(&["/home/projects/TS/p1/tsconfig.json"]),
    )
    .expect("open the project");
    assert_eq!(
        snapshot.read_file("/home/projects/TS/p1/nonexistent.ts"),
        None
    );
    assert_eq!(
        snapshot
            .read_file("/home/projects/TS/p1/index.ts")
            .as_deref(),
        Some(&b"console.log('Hello, world!');"[..])
    );
}

#[test]
fn snapshots_move_between_threads() {
    // The API server answers requests on snapshots from any thread.
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Snapshot>();
    assert_send_sync::<SnapshotHost>();
}
