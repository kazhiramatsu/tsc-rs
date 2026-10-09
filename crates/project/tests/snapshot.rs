//! tsgo's project tests on the standalone API path (P5-1b): synthetic
//! programs (`project/snapshot_test.go`, `api/session_createprogram_test.go`
//! at the snapshot layer), project IDs, configured projects opened, changed
//! and ensured through API requests (`configfilechanges_test.go` and
//! `project_test.go` re-expressed with API requests instead of editor
//! events), files opened through the API, and programs over project
//! references (`projectreferencesprogram_test.go`).

use std::collections::BTreeSet;
use std::sync::Arc;

use tsc_host::vfs::{FileSystem, MemFs, Seed, SystemClock};
use tsc_program::go_json::compiler_options_bag;
use tsc_program::{ConfigOptionBag, ConfigProjectReference, LibraryCatalog, ProgramLoadLimits};
use tsc_project::{
    ApiSnapshotRequest, CreateProgramRequest, FileChangeSummary, ProgramUpdateKind, ProjectId,
    ProjectKind, ReconfigureProgramRequest, SessionOptions, Snapshot, SnapshotHost,
};

fn session(files: &[(&str, &str)]) -> (SnapshotHost, Arc<MemFs>) {
    session_with(
        files
            .iter()
            .map(|(name, text)| ((*name).to_owned(), Seed::file(text)))
            .collect(),
    )
}

fn session_with(entries: Vec<(String, Seed)>) -> (SnapshotHost, Arc<MemFs>) {
    let fs = Arc::new(
        MemFs::from_entries(entries, false, Arc::new(SystemClock)).expect("build the file system"),
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

/// tsgo `core.CompilerOptions` from its JSON, as the API takes them.
fn options(json: serde_json::Value) -> ConfigOptionBag {
    compiler_options_bag(json.as_object().expect("options are an object"))
        .expect("the options convert")
}

fn no_lib(root_file_names: &[&str]) -> CreateProgramRequest {
    CreateProgramRequest {
        root_file_names: root_file_names
            .iter()
            .map(|name| (*name).to_owned())
            .collect(),
        options: options(serde_json::json!({ "noLib": true })),
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
    strict.options = options(serde_json::json!({ "noLib": true, "strict": true }));
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
    program.options = options(serde_json::json!({ "noLib": true, "strict": true }));
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

fn open_files(file_names: &[&str]) -> ApiSnapshotRequest {
    let files = file_names
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<BTreeSet<_>>();
    ApiSnapshotRequest {
        open_files: Some(files.clone()),
        // The API ensures every file it opens.
        ensure_files: files,
        ..ApiSnapshotRequest::default()
    }
}

#[test]
fn an_opened_file_without_a_config_goes_to_the_inferred_project() {
    // TestSnapshot "creates and removes synthetic programs", the opened-file
    // part: a synthetic program does not take an opened file.
    let (host, _) = session(&[
        ("/a.ts", "export const a = 1;"),
        ("/b.ts", "export const b = 1;"),
    ]);
    let created = update(
        &host,
        &host.new_root_snapshot(),
        ApiSnapshotRequest {
            create_programs: vec![no_lib(&["/a.ts"]), no_lib(&["/b.ts"])],
            ..ApiSnapshotRequest::default()
        },
    )
    .expect("create the programs");
    assert!(created.inferred_project().is_none());
    assert!(created.default_project("/a.ts").is_none());
    let first = Arc::clone(created.project(&ProjectId::synthetic(1)).unwrap());

    let opened = update(
        &host,
        &created,
        ApiSnapshotRequest {
            open_files: Some(BTreeSet::from(["/a.ts".to_owned()])),
            ..ApiSnapshotRequest::default()
        },
    )
    .expect("open the file");
    let inferred = opened.inferred_project().expect("an inferred project");
    assert_eq!(inferred.id(), &ProjectId::inferred());
    assert_eq!(inferred.kind(), ProjectKind::Inferred);
    assert_eq!(inferred.config_file_name(), None);
    assert_eq!(inferred.root_file_names(), ["/a.ts"]);
    assert!(Arc::ptr_eq(
        opened.default_project("/a.ts").expect("a default project"),
        inferred
    ));
    assert!(Arc::ptr_eq(
        opened.project(&ProjectId::synthetic(1)).unwrap(),
        &first
    ));
}

#[test]
fn opening_and_ensuring_a_file_lists_an_inferred_root_twice() {
    // tsgo (probed): the API's open also ensures the file, and ensuring
    // adds the file to the inferred roots that already have it.
    let (host, _) = session(&[("/home/p/a.ts", "export const a = 1;")]);
    let opened = update(
        &host,
        &host.new_root_snapshot(),
        open_files(&["/home/p/a.ts"]),
    )
    .expect("open the file");
    let inferred = opened.inferred_project().expect("an inferred project");
    assert_eq!(inferred.root_file_names(), ["/home/p/a.ts", "/home/p/a.ts"]);
    assert_eq!(
        inferred.program_update_kind(),
        ProgramUpdateKind::SameFileNames
    );
}

#[test]
fn an_opened_file_in_a_project_belongs_to_it() {
    let (host, _) = session(&[
        (
            "/p/tsconfig.json",
            r#"{ "compilerOptions": { "noLib": true } }"#,
        ),
        ("/p/src/a.ts", "export const a = 1;"),
    ]);
    let opened = update(
        &host,
        &host.new_root_snapshot(),
        open_files(&["/p/src/a.ts"]),
    )
    .expect("open the file");
    let id = configured("/p/tsconfig.json");
    let project = opened.project(&id).expect("the configured project");
    assert!(!project.is_dirty());
    assert!(Arc::ptr_eq(
        opened
            .default_project("/p/src/a.ts")
            .expect("a default project"),
        project
    ));
    assert!(opened.inferred_project().is_none());

    // Ensuring the file alone creates its project too.
    let ensured = update(
        &host,
        &host.new_root_snapshot(),
        ApiSnapshotRequest {
            ensure_files: BTreeSet::from(["/p/src/a.ts".to_owned()]),
            ..ApiSnapshotRequest::default()
        },
    )
    .expect("ensure the file");
    assert!(ensured.project(&id).is_some());
}

#[test]
fn an_opened_file_is_not_searched_for_above_its_nearest_config() {
    // tsgo (probed): the nearest config does not have the file, and a file
    // the API opens gets no ancestor (solution) search, so it is inferred.
    let (host, _) = session(&[
        (
            "/home/p/tsconfig.json",
            r#"{ "compilerOptions": { "noLib": true } }"#,
        ),
        (
            "/home/p/sub/tsconfig.json",
            r#"{ "compilerOptions": { "noLib": true }, "files": ["other.ts"] }"#,
        ),
        ("/home/p/sub/other.ts", "export {};"),
        ("/home/p/sub/x.ts", "export const x = 1;"),
    ]);
    let opened = update(
        &host,
        &host.new_root_snapshot(),
        open_files(&["/home/p/sub/x.ts"]),
    )
    .expect("open the file");
    assert!(opened
        .project(&configured("/home/p/tsconfig.json"))
        .is_none());
    let default = opened
        .default_project("/home/p/sub/x.ts")
        .expect("a default project");
    assert_eq!(default.kind(), ProjectKind::Inferred);

    // Opening it again (the API's open state already has it, so only the
    // ensure remains) keeps it there; tsgo crashes here.
    let reopened = update(
        &host,
        &opened,
        ApiSnapshotRequest {
            open_files: Some(BTreeSet::new()),
            ensure_files: BTreeSet::from(["/home/p/sub/x.ts".to_owned()]),
            ..ApiSnapshotRequest::default()
        },
    )
    .expect("open the file again");
    assert_eq!(
        reopened
            .default_project("/home/p/sub/x.ts")
            .expect("a default project")
            .kind(),
        ProjectKind::Inferred
    );
}

#[test]
fn closing_a_file_drops_the_projects_no_open_file_needs() {
    // tsgo (probed): closing y.ts removes /home/b's project.
    let (host, _) = session(&[
        (
            "/home/a/tsconfig.json",
            r#"{ "compilerOptions": { "noLib": true } }"#,
        ),
        ("/home/a/x.ts", "export const x = 1;"),
        (
            "/home/b/tsconfig.json",
            r#"{ "compilerOptions": { "noLib": true } }"#,
        ),
        ("/home/b/y.ts", "export const y = 1;"),
    ]);
    let opened = update(
        &host,
        &host.new_root_snapshot(),
        open_files(&["/home/a/x.ts", "/home/b/y.ts"]),
    )
    .expect("open the files");
    assert!(opened
        .project(&configured("/home/a/tsconfig.json"))
        .is_some());
    assert!(opened
        .project(&configured("/home/b/tsconfig.json"))
        .is_some());
    let closed = update(
        &host,
        &opened,
        ApiSnapshotRequest {
            close_files: Some(BTreeSet::from(["/home/b/y.ts".to_owned()])),
            ..ApiSnapshotRequest::default()
        },
    )
    .expect("close a file");
    assert!(closed
        .project(&configured("/home/a/tsconfig.json"))
        .is_some());
    assert!(closed
        .project(&configured("/home/b/tsconfig.json"))
        .is_none());
}

#[test]
fn closing_a_project_leaves_its_opened_file_without_one() {
    // tsgo (probed): closeProjects does not spare the project of a file the
    // API opened.
    let (host, _) = session(&[
        (
            "/home/a/tsconfig.json",
            r#"{ "compilerOptions": { "noLib": true } }"#,
        ),
        ("/home/a/x.ts", "export const x = 1;"),
    ]);
    let mut request = open_files(&["/home/a/x.ts"]);
    request.open_projects = BTreeSet::from(["/home/a/tsconfig.json".to_owned()]);
    let opened = update(&host, &host.new_root_snapshot(), request).expect("open");
    let closed = update(
        &host,
        &opened,
        ApiSnapshotRequest {
            close_projects: BTreeSet::from(["/home/a/tsconfig.json".to_owned()]),
            ..ApiSnapshotRequest::default()
        },
    )
    .expect("close the project");
    assert!(closed
        .project(&configured("/home/a/tsconfig.json"))
        .is_none());
    assert!(closed.default_project("/home/a/x.ts").is_none());
}

#[test]
fn an_opened_file_needs_a_project() {
    let (host, _) = session(&[("/p/readme.md", "# readme")]);
    let error = update(
        &host,
        &host.new_root_snapshot(),
        open_files(&["/p/readme.md"]),
    )
    .err()
    .expect("no project takes the file");
    assert_eq!(error, "no project found for opened file: /p/readme.md");
}

const MY_PROJECT: &str = "/user/username/projects/myproject";

/// tsgo `filesForReferencedProjectProgram`.
fn files_for_referenced_project_program(
    disable_source_of_project_reference_redirect: bool,
) -> Vec<(String, Seed)> {
    let disable = if disable_source_of_project_reference_redirect {
        r#", "disableSourceOfProjectReferenceRedirect": true"#
    } else {
        ""
    };
    vec![
        (
            format!("{MY_PROJECT}/main/tsconfig.json"),
            Seed::file(format!(
                r#"{{ "compilerOptions": {{ "composite": true{disable} }}, "references": [{{ "path": "../dependency" }}] }}"#
            )),
        ),
        (
            format!("{MY_PROJECT}/main/main.ts"),
            Seed::file(
                "import { fn1, fn2, fn3, fn4, fn5 } from '../decls/fns'\nfn1();\nfn2();\nfn3();\nfn4();\nfn5();\n",
            ),
        ),
        (
            format!("{MY_PROJECT}/dependency/tsconfig.json"),
            Seed::file(r#"{ "compilerOptions": { "composite": true, "declarationDir": "../decls" }, }"#),
        ),
        (
            format!("{MY_PROJECT}/dependency/fns.ts"),
            Seed::file(
                "export function fn1() { }\nexport function fn2() { }\nexport function fn3() { }\nexport function fn4() { }\nexport function fn5() { }\n",
            ),
        ),
    ]
}

/// tsgo `addConfigForPackage`.
fn add_config_for_package(
    files: &mut Vec<(String, Seed)>,
    package_name: &str,
    preserve_symlinks: bool,
    references: &[&str],
) {
    let preserve = if preserve_symlinks {
        r#", "preserveSymlinks": true"#
    } else {
        ""
    };
    let references = references
        .iter()
        .map(|path| format!(r#"{{ "path": "{path}" }}"#))
        .collect::<Vec<_>>()
        .join(", ");
    files.push((
        format!("{MY_PROJECT}/packages/{package_name}/tsconfig.json"),
        Seed::file(format!(
            r#"{{ "compilerOptions": {{ "outDir": "lib", "rootDir": "src", "composite": true{preserve} }}, "include": ["src"], "references": [{references}] }}"#
        )),
    ));
}

/// tsgo `filesForSymlinkReferences`: the test file, foo and bar.
fn files_for_symlink_references(
    preserve_symlinks: bool,
    scope: &str,
) -> (Vec<(String, Seed)>, String, String, String) {
    let a_test = format!("{MY_PROJECT}/packages/A/src/index.ts");
    let b_foo = format!("{MY_PROJECT}/packages/B/src/index.ts");
    let b_bar = format!("{MY_PROJECT}/packages/B/src/bar.ts");
    let mut files = vec![
        (
            format!("{MY_PROJECT}/packages/B/package.json"),
            Seed::file(r#"{ "main": "lib/index.js", "types": "lib/index.d.ts" }"#),
        ),
        (
            a_test.clone(),
            Seed::file(format!(
                "import {{ foo }} from '{scope}b';\nimport {{ bar }} from '{scope}b/lib/bar';\nfoo();\nbar();\n"
            )),
        ),
        (b_foo.clone(), Seed::file("export function foo() { }")),
        (b_bar.clone(), Seed::file("export function bar() { }")),
        (
            format!("{MY_PROJECT}/node_modules/{scope}b"),
            Seed::symlink(format!("{MY_PROJECT}/packages/B")),
        ),
    ];
    add_config_for_package(&mut files, "A", preserve_symlinks, &["../B"]);
    add_config_for_package(&mut files, "B", preserve_symlinks, &[]);
    (files, a_test, b_foo, b_bar)
}

/// tsgo `filesForSymlinkReferencesInSubfolder`.
fn files_for_symlink_references_in_subfolder(
    preserve_symlinks: bool,
    scope: &str,
) -> (Vec<(String, Seed)>, String, String, String) {
    let a_test = format!("{MY_PROJECT}/packages/A/src/test.ts");
    let b_foo = format!("{MY_PROJECT}/packages/B/src/foo.ts");
    let b_bar = format!("{MY_PROJECT}/packages/B/src/bar/foo.ts");
    let mut files = vec![
        (
            format!("{MY_PROJECT}/packages/B/package.json"),
            Seed::file("{}"),
        ),
        (
            a_test.clone(),
            Seed::file(format!(
                "import {{ foo }} from '{scope}b/lib/foo';\nimport {{ bar }} from '{scope}b/lib/bar/foo';\nfoo();\nbar();\n"
            )),
        ),
        (b_foo.clone(), Seed::file("export function foo() { }")),
        (b_bar.clone(), Seed::file("export function bar() { }")),
        (
            format!("{MY_PROJECT}/node_modules/{scope}b"),
            Seed::symlink(format!("{MY_PROJECT}/packages/B")),
        ),
    ];
    add_config_for_package(&mut files, "A", preserve_symlinks, &["../B"]);
    add_config_for_package(&mut files, "B", preserve_symlinks, &[]);
    (files, a_test, b_foo, b_bar)
}

/// tsgo `filesForDirectorySubpathSymlinkReferences`.
fn files_for_directory_subpath_symlink_references(
    scope: &str,
) -> (Vec<(String, Seed)>, String, String) {
    let a_index = format!("{MY_PROJECT}/packages/a/src/index.ts");
    let b_file = format!("{MY_PROJECT}/packages/b/src/File/index.ts");
    let mut files = vec![
        (
            format!("{MY_PROJECT}/packages/b/package.json"),
            Seed::file(r#"{ "main": "lib/index.js", "types": "lib/index.d.ts" }"#),
        ),
        (
            a_index.clone(),
            Seed::file(format!(
                "import {{ helper }} from \"{scope}b/lib/File\";\nexport const result: number = helper();\n"
            )),
        ),
        (
            b_file.clone(),
            Seed::file("export function helper(): number { return 1; }"),
        ),
        (
            format!("{MY_PROJECT}/node_modules/{scope}b"),
            Seed::symlink(format!("{MY_PROJECT}/packages/b")),
        ),
    ];
    add_config_for_package(&mut files, "a", false, &["../b"]);
    add_config_for_package(&mut files, "b", false, &[]);
    (files, a_index, b_file)
}

/// The only project of a snapshot that opened `file_name`.
fn open_one(files: Vec<(String, Seed)>, file_name: &str) -> (SnapshotHost, Arc<MemFs>, Snapshot) {
    let (host, fs) = session_with(files);
    let opened =
        update(&host, &host.new_root_snapshot(), open_files(&[file_name])).expect("open the file");
    assert_eq!(opened.projects().count(), 1);
    assert_eq!(
        opened.projects().next().unwrap().kind(),
        ProjectKind::Configured
    );
    (host, fs, opened)
}

fn has_file(snapshot: &Snapshot, file_name: &str) -> bool {
    let program = snapshot.projects().next().unwrap().program().unwrap();
    program
        .prepared()
        .source_files()
        .iter()
        .any(|source| source.path().display().to_string_lossy() == file_name)
}

#[test]
fn a_program_reads_the_sources_of_its_referenced_project() {
    // "program for referenced project".
    let main = format!("{MY_PROJECT}/main/main.ts");
    let (_, _, opened) = open_one(files_for_referenced_project_program(false), &main);
    assert!(has_file(
        &opened,
        &format!("{MY_PROJECT}/dependency/fns.ts")
    ));
    assert!(!has_file(&opened, &format!("{MY_PROJECT}/decls/fns.d.ts")));
}

#[test]
fn disabling_the_source_redirect_reads_the_outputs() {
    // "program with disableSourceOfProjectReferenceRedirect".
    let mut files = files_for_referenced_project_program(true);
    files.push((
        format!("{MY_PROJECT}/decls/fns.d.ts"),
        Seed::file(
            "export declare function fn1(): void;\nexport declare function fn2(): void;\nexport declare function fn3(): void;\nexport declare function fn4(): void;\nexport declare function fn5(): void;\n",
        ),
    ));
    let main = format!("{MY_PROJECT}/main/main.ts");
    let (_, _, opened) = open_one(files, &main);
    assert!(!has_file(
        &opened,
        &format!("{MY_PROJECT}/dependency/fns.ts")
    ));
    assert!(has_file(&opened, &format!("{MY_PROJECT}/decls/fns.d.ts")));
}

#[test]
fn references_through_symlinked_packages_reach_the_sources() {
    // "references through symlink with index and typings" and "...
    // referencing from subFolder", each with and without preserveSymlinks
    // and with a scoped package.
    for preserve_symlinks in [false, true] {
        for scope in ["", "@issue/"] {
            for subfolder in [false, true] {
                let (files, a_test, b_foo, b_bar) = if subfolder {
                    files_for_symlink_references_in_subfolder(preserve_symlinks, scope)
                } else {
                    files_for_symlink_references(preserve_symlinks, scope)
                };
                let (_, _, opened) = open_one(files, &a_test);
                let case = format!(
                    "preserveSymlinks {preserve_symlinks}, scope {scope:?}, subfolder {subfolder}"
                );
                assert!(has_file(&opened, &b_foo), "{case}: {b_foo}");
                assert!(has_file(&opened, &b_bar), "{case}: {b_bar}");
            }
        }
    }
}

#[test]
fn a_directory_index_subpath_through_a_symlink_reaches_the_source() {
    // "references through symlink with directory index subpath (issue
    // 4373)" and its scoped package.
    for scope in ["", "@issue/"] {
        let (files, a_index, b_file) = files_for_directory_subpath_symlink_references(scope);
        let (_, _, opened) = open_one(files, &a_index);
        assert!(has_file(&opened, &b_file), "scope {scope:?}");
        let program = opened.projects().next().unwrap().program().unwrap();
        let diagnostics = program.with_live(|live| {
            let file = live.file_index(a_index.as_str()).expect("the file");
            live.semantic_diagnostics(file).expect("check the file")
        });
        assert_eq!(diagnostics, [], "scope {scope:?}");
    }
}

#[test]
fn a_file_added_to_a_referenced_project_rebuilds_the_program() {
    // "when new file is added to referenced project".
    let main = format!("{MY_PROJECT}/main/main.ts");
    let (host, fs, opened) = open_one(files_for_referenced_project_program(false), &main);
    let before = Arc::clone(opened.projects().next().unwrap().program().unwrap());
    let added = format!("{MY_PROJECT}/dependency/fns2.ts");
    fs.write(&added, b"export const x = 2;")
        .expect("write the new file");
    let created = host
        .clone_snapshot(
            &opened,
            FileChangeSummary {
                created: BTreeSet::from([added]),
                ..FileChangeSummary::default()
            },
            None,
        )
        .expect("apply the creation");
    let ensured = update(
        &host,
        &created,
        ApiSnapshotRequest {
            ensure_files: BTreeSet::from([main]),
            ..ApiSnapshotRequest::default()
        },
    )
    .expect("ensure the file");
    assert_eq!(ensured.projects().count(), 1);
    assert!(!Arc::ptr_eq(
        ensured.projects().next().unwrap().program().unwrap(),
        &before
    ));
}

#[test]
fn a_dropped_reference_releases_the_referenced_config() {
    // "dropped project reference does not crash on later change to the
    // dropped config".
    let main = format!("{MY_PROJECT}/main/main.ts");
    let other = format!("{MY_PROJECT}/other/other.ts");
    let main_config = format!("{MY_PROJECT}/main/tsconfig.json");
    let dependency_config = format!("{MY_PROJECT}/dependency/tsconfig.json");
    let (host, fs) = session(&[
        (
            &main_config,
            r#"{ "compilerOptions": { "composite": true }, "references": [{ "path": "../dependency" }] }"#,
        ),
        (&main, "import { fn1 } from '../dependency/fns'\nfn1();\n"),
        (
            &dependency_config,
            r#"{ "compilerOptions": { "composite": true } }"#,
        ),
        (
            &format!("{MY_PROJECT}/dependency/fns.ts"),
            "export function fn1() { }",
        ),
        (
            &format!("{MY_PROJECT}/other/tsconfig.json"),
            r#"{ "compilerOptions": { "composite": true } }"#,
        ),
        (&other, "export const y = 1;"),
    ]);

    // 1. The main project's program takes the dependency's config.
    let opened =
        update(&host, &host.new_root_snapshot(), open_files(&[&main])).expect("open main.ts");
    assert_eq!(opened.projects().count(), 1);
    assert!(opened.config(&dependency_config).is_some());

    // 2. The reference goes from the config, and the program is built again.
    fs.write(
        &main_config,
        br#"{ "compilerOptions": { "composite": true } }"#,
    )
    .expect("write the config");
    let dropped = changed(&host, &opened, &[&main_config]);
    let ensured = update(
        &host,
        &dropped,
        ApiSnapshotRequest {
            ensure_files: BTreeSet::from([main.clone()]),
            ..ApiSnapshotRequest::default()
        },
    )
    .expect("ensure main.ts");
    // Released, the config stays until a clean-up (tsgo, probed).
    assert!(ensured.config(&dependency_config).is_some());

    // 3. Closing main.ts and opening another file deletes the main project;
    // nothing keeps the dependency's config.
    let mut reopened = open_files(&[&other]);
    reopened.close_files = Some(BTreeSet::from([main.clone()]));
    let reopened = update(&host, &ensured, reopened).expect("close main.ts, open other.ts");
    assert!(reopened.project(&configured(&main_config)).is_none());
    assert!(reopened.config(&dependency_config).is_none());

    // 4. A later change to the dropped config concerns no project.
    fs.write(
        &dependency_config,
        br#"{ "compilerOptions": { "composite": true, "strict": true } }"#,
    )
    .expect("write the config");
    let later = changed(&host, &reopened, &[&dependency_config]);
    let later = update(
        &host,
        &later,
        ApiSnapshotRequest {
            ensure_files: BTreeSet::from([other.clone()]),
            ..ApiSnapshotRequest::default()
        },
    )
    .expect("ensure other.ts");
    assert_eq!(
        later
            .default_project(&other)
            .map(|project| project.id().clone()),
        Some(configured(&format!("{MY_PROJECT}/other/tsconfig.json")))
    );
}

const DUMMY: &str = "/user/username/workspaces/dummy/dummy.ts";

/// tsgo `filesForSolutionConfigFile`, with the dummy file the tests open
/// last.
fn files_for_solution_config_file(
    solution_references: &[&str],
    compiler_options: &str,
    own_files: &[&str],
) -> Vec<(String, Seed)> {
    let compiler_options = if compiler_options.is_empty() {
        String::new()
    } else {
        format!(r#""compilerOptions": {{ {compiler_options} }},"#)
    };
    let references = solution_references
        .iter()
        .map(|path| format!(r#"{{ "path": "{path}" }}"#))
        .collect::<Vec<_>>()
        .join(", ");
    let own_files = own_files.join(", ");
    vec![
        (
            format!("{MY_PROJECT}/tsconfig.json"),
            Seed::file(format!(
                r#"{{ {compiler_options} "files": [{own_files}], "references": [{references}] }}"#
            )),
        ),
        (
            format!("{MY_PROJECT}/tsconfig-src.json"),
            Seed::file(
                r#"{ "compilerOptions": { "composite": true, "outDir": "./target", }, "include": ["./src/**/*"] }"#,
            ),
        ),
        (
            format!("{MY_PROJECT}/src/main.ts"),
            Seed::file("import { foo } from './helpers/functions';\nexport { foo };"),
        ),
        (
            format!("{MY_PROJECT}/src/helpers/functions.ts"),
            Seed::file("export const foo = 1;"),
        ),
        (DUMMY.to_owned(), Seed::file("const x = 1;")),
    ]
}

/// tsgo `applyIndirectProjectFiles`.
fn add_indirect_project(files: &mut Vec<(String, Seed)>, index: usize, compiler_options: &str) {
    files.push((
        format!("{MY_PROJECT}/tsconfig-indirect{index}.json"),
        Seed::file(format!(
            r#"{{ "compilerOptions": {{ "composite": true, "outDir": "./target/", {compiler_options} }}, "files": ["./indirect{index}/main.ts"], "references": [{{ "path": "./tsconfig-src.json" }}] }}"#
        )),
    ));
    files.push((
        format!("{MY_PROJECT}/indirect{index}/main.ts"),
        Seed::file("export const indirect = 1;"),
    ));
}

/// The projects of a snapshot (IDs), its default project for `file_name`
/// and which of `configs` it keeps.
fn search_state(
    snapshot: &Snapshot,
    file_name: Option<&str>,
    configs: &[&str],
) -> (Vec<String>, Option<String>, Vec<bool>) {
    (
        snapshot
            .projects()
            .map(|project| project.id().as_str().to_owned())
            .collect(),
        file_name.and_then(|file_name| {
            snapshot
                .default_project(file_name)
                .map(|project| project.id().as_str().to_owned())
        }),
        configs
            .iter()
            .map(|config| snapshot.config(config).is_some())
            .collect(),
    )
}

/// Open `file_name`, close it and open the dummy file, each through the API
/// (tsgo's builder tests with API requests instead of editor events): the
/// state after each request.
fn open_close_and_open_dummy(
    files: Vec<(String, Seed)>,
    file_name: &str,
    configs: &[&str],
) -> [(Vec<String>, Option<String>, Vec<bool>); 3] {
    let (host, _) = session_with(files);
    let opened =
        update(&host, &host.new_root_snapshot(), open_files(&[file_name])).expect("open the file");
    let closed = update(
        &host,
        &opened,
        ApiSnapshotRequest {
            close_files: Some(BTreeSet::from([file_name.to_owned()])),
            ..ApiSnapshotRequest::default()
        },
    )
    .expect("close the file");
    let dummy = update(&host, &closed, open_files(&[DUMMY])).expect("open the dummy file");
    [
        search_state(&opened, Some(file_name), configs),
        search_state(&closed, None, configs),
        search_state(&dummy, None, configs),
    ]
}

fn ids(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|id| (*id).to_owned()).collect()
}

#[test]
fn a_solution_leads_to_the_project_that_has_the_file() {
    // projectcollectionbuilder_test "when project found is solution
    // referencing default project directly" / "... indirectly" / "...
    // through disableReferencedProjectLoad in one but without it in
    // another", with API requests (tsgo, probed). A file the API opens
    // keeps no config, so only the project's own config stays.
    let src = format!("{MY_PROJECT}/tsconfig-src.json");
    let configs = [
        format!("{MY_PROJECT}/tsconfig.json"),
        src.clone(),
        format!("{MY_PROJECT}/tsconfig-indirect1.json"),
        format!("{MY_PROJECT}/tsconfig-indirect2.json"),
    ];
    let configs = configs.iter().map(String::as_str).collect::<Vec<_>>();
    let main = format!("{MY_PROJECT}/src/main.ts");

    let direct = files_for_solution_config_file(&["./tsconfig-src.json"], "", &[]);
    let mut indirect = files_for_solution_config_file(
        &["./tsconfig-indirect1.json", "./tsconfig-indirect2.json"],
        "",
        &[],
    );
    add_indirect_project(&mut indirect, 1, "");
    add_indirect_project(&mut indirect, 2, "");
    let mut mixed = files_for_solution_config_file(
        &["./tsconfig-indirect1.json", "./tsconfig-indirect2.json"],
        "",
        &[],
    );
    add_indirect_project(&mut mixed, 1, r#""disableReferencedProjectLoad": true"#);
    add_indirect_project(&mut mixed, 2, "");

    for (case, files) in [("direct", direct), ("indirect", indirect), ("mixed", mixed)] {
        let [opened, closed, dummy] = open_close_and_open_dummy(files, &main, &configs);
        assert_eq!(
            opened,
            (
                ids(&[&src]),
                Some(src.clone()),
                vec![false, true, false, false]
            ),
            "{case}"
        );
        assert_eq!(closed, (ids(&[]), None, vec![false; 4]), "{case}");
        assert_eq!(
            dummy,
            (ids(&["/dev/null/inferred"]), None, vec![false; 4]),
            "{case}"
        );
    }
}

#[test]
fn a_solution_that_does_not_load_its_references_leaves_the_file_inferred() {
    // projectcollectionbuilder_test "... solution with
    // disableReferencedProjectLoad referencing default project directly",
    // "... indirectly through disableReferencedProjectLoad" and "project
    // lookup terminates" (a cycle of references), with API requests. tsgo
    // panics here: its first search deletes the configs it kept for no
    // project, and the open's ensure acquires them again in the same build
    // (tsgo, probed; opening without the ensure leaves the file inferred).
    // The port keeps the file in the inferred project.
    let main = format!("{MY_PROJECT}/src/main.ts");
    let src = format!("{MY_PROJECT}/tsconfig-src.json");
    let direct = files_for_solution_config_file(
        &["./tsconfig-src.json"],
        r#""disableReferencedProjectLoad": true"#,
        &[],
    );
    let mut indirect = files_for_solution_config_file(&["./tsconfig-indirect1.json"], "", &[]);
    add_indirect_project(&mut indirect, 1, r#""disableReferencedProjectLoad": true"#);
    let cycle = vec![
        (
            "/tsconfig.json".to_owned(),
            Seed::file(
                r#"{ "files": [], "references": [ { "path": "./packages/pkg1" }, { "path": "./packages/pkg2" }, ] }"#,
            ),
        ),
        (
            "/packages/pkg1/tsconfig.json".to_owned(),
            Seed::file(
                r#"{ "include": ["src/**/*.ts"], "compilerOptions": { "composite": true, }, "references": [ { "path": "../pkg2" }, ] }"#,
            ),
        ),
        (
            "/packages/pkg2/tsconfig.json".to_owned(),
            Seed::file(
                r#"{ "include": ["src/**/*.ts"], "compilerOptions": { "composite": true, }, "references": [ { "path": "../pkg1" }, ] }"#,
            ),
        ),
        ("/script.ts".to_owned(), Seed::file("export const a = 1;")),
        (DUMMY.to_owned(), Seed::file("const x = 1;")),
    ];
    for (case, files, file_name) in [
        ("direct", direct, main.as_str()),
        ("indirect", indirect, main.as_str()),
        ("cycle", cycle, "/script.ts"),
    ] {
        let [opened, _, dummy] = open_close_and_open_dummy(files, file_name, &[&src]);
        assert_eq!(opened.0, ids(&["/dev/null/inferred"]), "{case}");
        assert_eq!(opened.1.as_deref(), Some("/dev/null/inferred"), "{case}");
        assert_eq!(opened.2, [false], "{case}");
        assert_eq!(dummy.0, ids(&["/dev/null/inferred"]), "{case}");
    }
}

#[test]
fn a_project_that_reads_the_file_as_a_referenced_source_is_not_its_default() {
    // projectcollectionbuilder_test "when project found is project with own
    // files referencing the file from referenced project", with API
    // requests (tsgo, probed): the solution's program has the file as a
    // source of its referenced project, so the search goes on to that
    // project; the solution stays as the path to it.
    let solution = format!("{MY_PROJECT}/tsconfig.json");
    let src = format!("{MY_PROJECT}/tsconfig-src.json");
    let mut files =
        files_for_solution_config_file(&["./tsconfig-src.json"], "", &[r#""./own/main.ts""#]);
    files.push((
        format!("{MY_PROJECT}/own/main.ts"),
        Seed::file("import { foo } from '../src/main';\nfoo;\nexport function bar() {}\n"),
    ));
    let main = format!("{MY_PROJECT}/src/main.ts");
    let [opened, closed, dummy] =
        open_close_and_open_dummy(files, &main, &[solution.as_str(), src.as_str()]);
    assert_eq!(
        opened,
        (ids(&[&src, &solution]), Some(src.clone()), vec![true, true])
    );
    assert_eq!(closed, (ids(&[]), None, vec![false, false]));
    assert_eq!(
        dummy,
        (ids(&["/dev/null/inferred"]), None, vec![false, false])
    );
}

#[test]
fn a_declaration_file_beside_a_referenced_source_is_inferred() {
    // projectcollectionbuilder_test "when dts file is next to ts file and
    // included as root in referenced project", with API requests (tsgo,
    // probed): the root config's index.d.ts is the referenced project's
    // output of index.ts, so its program reads index.ts instead, and the
    // referenced project lists only index.ts.
    let root = "/home/src/projects/project/tsconfig.json";
    let node = "/home/src/projects/project/tsconfig.node.json";
    let declaration = "/home/src/projects/project/src/index.d.ts";
    let files = vec![
        (
            declaration.to_owned(),
            Seed::file(
                "declare global {\n    interface Window {\n        electron: ElectronAPI\n        api: unknown\n    }\n}\n",
            ),
        ),
        (
            "/home/src/projects/project/src/index.ts".to_owned(),
            Seed::file("const api = {}"),
        ),
        (
            root.to_owned(),
            Seed::file(
                r#"{ "include": [ "src/*.d.ts", ], "references": [{ "path": "./tsconfig.node.json" }], }"#,
            ),
        ),
        (
            node.to_owned(),
            Seed::file(r#"{ "include": ["src/**/*"], "compilerOptions": { "composite": true, }, }"#),
        ),
        (DUMMY.to_owned(), Seed::file("const x = 1;")),
    ];
    let [opened, closed, dummy] = open_close_and_open_dummy(files, declaration, &[root, node]);
    assert_eq!(
        opened,
        (
            ids(&[root, "/dev/null/inferred"]),
            Some("/dev/null/inferred".to_owned()),
            vec![true, true]
        )
    );
    assert_eq!(closed.2, [false, false]);
    assert_eq!(
        dummy,
        (ids(&["/dev/null/inferred"]), None, vec![false, false])
    );
}

#[test]
fn a_synthetic_program_reads_the_sources_of_its_references() {
    // The client's "createProgram includes project references" fixture with
    // an import of the referenced project (tsgo, probed): the program reads
    // the source and does not check it; the config stays until a clean-up
    // after the program goes.
    let (host, _) = session(&[
        (
            "/src/index.ts",
            "import { lib } from \"../lib/index\";\nexport const value = lib;\n",
        ),
        (
            "/lib/tsconfig.json",
            r#"{ "compilerOptions": { "composite": true, "noLib": true, "outDir": "out" }, "files": ["index.ts"] }"#,
        ),
        ("/lib/index.ts", "export const lib = 1;"),
    ]);
    let mut program = no_lib(&["/src/index.ts"]);
    program.project_references = vec![ConfigProjectReference {
        path: "/lib/tsconfig.json".into(),
        original_path: "/lib/tsconfig.json".into(),
        prepend: None,
        circular: None,
    }];
    let created = update(
        &host,
        &host.new_root_snapshot(),
        ApiSnapshotRequest {
            create_programs: vec![program],
            ..ApiSnapshotRequest::default()
        },
    )
    .expect("create the program");
    let project = created.project(&ProjectId::synthetic(1)).unwrap();
    let program = project.program().unwrap();
    let files = program
        .prepared()
        .source_files()
        .iter()
        .map(|source| source.path().display().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(files, ["/lib/index.ts", "/src/index.ts"]);
    let diagnostics = program.with_live(|live| {
        files
            .iter()
            .map(|file_name| {
                let file = live.file_index(file_name.as_str()).expect("the file");
                live.semantic_diagnostics(file)
                    .expect("check the file")
                    .len()
            })
            .collect::<Vec<_>>()
    });
    assert_eq!(diagnostics, [0, 0]);
    assert!(created.config("/lib/tsconfig.json").is_some());

    let removed = update(
        &host,
        &created,
        ApiSnapshotRequest {
            remove_programs: BTreeSet::from([ProjectId::synthetic(1)]),
            ..ApiSnapshotRequest::default()
        },
    )
    .expect("remove the program");
    assert_eq!(removed.projects().count(), 0);
    assert!(removed.config("/lib/tsconfig.json").is_some());
    let cleaned = update(
        &host,
        &removed,
        ApiSnapshotRequest {
            close_files: Some(BTreeSet::new()),
            ..ApiSnapshotRequest::default()
        },
    )
    .expect("clean up");
    assert!(cleaned.config("/lib/tsconfig.json").is_none());
}

#[test]
fn a_program_built_again_keeps_the_documents_of_unchanged_files() {
    // tsgo's parse cache through the snapshots: the program built after a
    // change has the same parsed and bound file for every file that did not
    // change (the API reports the files whose documents differ), and the
    // documents go with the snapshots that hold them.
    let config = "/src/tsconfig.json";
    let (host, fs) = session(&[
        (config, r#"{ "compilerOptions": { "noLib": true } }"#),
        ("/src/a.ts", "export const a = 1;"),
        ("/src/b.ts", "export const b = 1;"),
    ]);
    let id = configured(config);
    let opened = update(&host, &host.new_root_snapshot(), open_projects(&[config]))
        .expect("open the project");
    fs.write("/src/b.ts", b"export const b = 2;")
        .expect("write the file");
    let marked = changed(&host, &opened, &["/src/b.ts"]);
    let ensured = update(&host, &marked, ensure(&[&id])).expect("ensure the project");
    let document = |snapshot: &Snapshot, file_name: &str| {
        let program = snapshot.project(&id).unwrap().program().unwrap();
        program
            .with_live(|live| {
                live.file_index(file_name)
                    .and_then(|index| live.document(index))
                    .cloned()
            })
            .expect("the file's document")
    };
    assert!(!Arc::ptr_eq(
        opened.project(&id).unwrap().program().unwrap(),
        ensured.project(&id).unwrap().program().unwrap()
    ));
    assert!(Arc::ptr_eq(
        &document(&opened, "/src/a.ts"),
        &document(&ensured, "/src/a.ts")
    ));
    assert!(!Arc::ptr_eq(
        &document(&opened, "/src/b.ts"),
        &document(&ensured, "/src/b.ts")
    ));
    assert_eq!(host.documents().len(), 3);
    drop((opened, marked, ensured));
    host.documents().purge();
    assert!(host.documents().is_empty());
}

#[test]
fn a_one_file_change_reuses_the_program_in_place() {
    // project_test "Cloned on single-file change" and snapshot_test
    // "compilerHost gets frozen with snapshot's FS only once" with API
    // requests: a file changed without changing its requests is read again
    // into the program (tsgo UpdateProgram), which shares the other files;
    // the inferred project, which the change does not concern, keeps its
    // program (tsgo, probed).
    let config = "/home/projects/TS/p1/tsconfig.json";
    let index = "/home/projects/TS/p1/index.ts";
    let helper = "/home/projects/TS/p1/helper.ts";
    let other = "/home/projects/other.ts";
    let (host, fs) = session(&[
        (config, r#"{ "compilerOptions": { "noLib": true } }"#),
        (index, "console.log('Hello, world!');"),
        (helper, "export const helper = 1;"),
        (other, "export {};"),
    ]);
    // The file system ignores case, so the project's ID is its lower-case path.
    let id = configured("/home/projects/ts/p1/tsconfig.json");
    let opened = update(
        &host,
        &host.new_root_snapshot(),
        open_files(&[index, other]),
    )
    .expect("open the files");
    let inferred = Arc::clone(opened.inferred_project().expect("an inferred project"));

    fs.write(index, b"console.log('Hello, world!')\n;")
        .expect("write the file");
    let marked = changed(&host, &opened, &[index]);
    let ensured = update(
        &host,
        &marked,
        ApiSnapshotRequest {
            ensure_files: BTreeSet::from([index.to_owned()]),
            ..ApiSnapshotRequest::default()
        },
    )
    .expect("ensure the file");
    let project = ensured.project(&id).unwrap();
    assert_eq!(project.program_update_kind(), ProgramUpdateKind::Cloned);
    assert!(!project.is_dirty());
    let program = project.program().unwrap();
    assert_eq!(
        program
            .prepared()
            .source_files()
            .iter()
            .find(|source| source.path().display().to_string_lossy() == index)
            .map(|source| source.text().to_owned()),
        Some("console.log('Hello, world!')\n;".to_owned())
    );
    let document = |snapshot: &Snapshot, file_name: &str| {
        snapshot
            .project(&id)
            .unwrap()
            .program()
            .unwrap()
            .with_live(|live| {
                live.file_index(file_name)
                    .and_then(|index| live.document(index))
                    .cloned()
            })
            .expect("the file's document")
    };
    assert!(Arc::ptr_eq(
        &document(&opened, helper),
        &document(&ensured, helper)
    ));
    assert!(!Arc::ptr_eq(
        &document(&opened, index),
        &document(&ensured, index)
    ));
    // The inferred project keeps the program (and the update kind) of the
    // open; the API's open also ensures the file, which builds it a second
    // time (SameFileNames, where tsgo's editor test sees NewFiles).
    let inferred_after = ensured.inferred_project().expect("the inferred project");
    assert!(Arc::ptr_eq(inferred_after, &inferred));
    assert_eq!(
        inferred_after.program_update_kind(),
        ProgramUpdateKind::SameFileNames
    );
}

#[test]
fn a_change_to_a_files_requests_builds_the_program_again() {
    // canReplaceFileInProgram: a new import, or another check directive,
    // needs a full build (the same files: SameFileNames; tsgo, probed).
    let config = "/p/tsconfig.json";
    let index = "/p/index.ts";
    let (host, fs) = session(&[
        (config, r#"{ "compilerOptions": { "noLib": true } }"#),
        (index, "export const a = 1;"),
        ("/p/b.ts", "export const b = 1;"),
    ]);
    let id = configured(config);
    let mut snapshot =
        update(&host, &host.new_root_snapshot(), open_files(&[index])).expect("open the file");
    for text in [
        "import './b';\nexport const a = 1;",
        "// @ts-nocheck\nimport './b';\nexport const a = 1;",
        "// @ts-nocheck\nimport './b';\nexport const a = 2;",
    ] {
        fs.write(index, text.as_bytes()).expect("write the file");
        let marked = changed(&host, &snapshot, &[index]);
        snapshot = update(
            &host,
            &marked,
            ApiSnapshotRequest {
                ensure_files: BTreeSet::from([index.to_owned()]),
                ..ApiSnapshotRequest::default()
            },
        )
        .expect("ensure the file");
        let expected = if text.ends_with("= 2;") {
            ProgramUpdateKind::Cloned
        } else {
            ProgramUpdateKind::SameFileNames
        };
        assert_eq!(
            snapshot.project(&id).unwrap().program_update_kind(),
            expected,
            "{text:?}"
        );
    }
}
