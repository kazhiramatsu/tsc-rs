//! Content mappers in the API (tsgo `--runExternalCode`: the session's
//! spawner and `SessionOptions.RunExternalCode`), with the harness's
//! in-process test mappers for processes, and tsgo's project system tests
//! (`project/contentmapper_test.go`) over snapshot updates: the client's
//! file notifications stand for the language server's watch events, and a
//! snapshot the client released for one the language server dropped.

use std::sync::atomic::Ordering;

use base64::Engine;
use tsc_harness::content_mappers::{
    package_json, RecordingSpawner, DYNAMIC_VERBATIM_MAPPER, FAILING_MAPPER,
    MODULE_VERBATIM_MAPPER, SUPPLEMENTAL_MAPPER, TRANSFORMING_MAPPER, VERBATIM_MAPPER,
};
use tsc_program::content_mapper::host::Spawner;
use tsc_program::PreparedSourceFile;
use tsc_project::{CommandLine, ProgramUpdateKind, Project, ProjectProgram};

use super::*;
use crate::encoder::{
    HEADER_OFFSET_EXTENDED_DATA, HEADER_OFFSET_NODES, HEADER_OFFSET_STRING_DATA,
    HEADER_OFFSET_STRING_OFFSETS, NODE_DATA_STRING_INDEX_MASK, NODE_OFFSET_DATA, NODE_SIZE,
};

const CONFIG: &str = r#"{
    "compilerOptions": { "noLib": true, "types": [] },
    "contentMappers": [ { "package": "mapper", "extensions": [".box"] } ]
}"#;

const COMPONENT: &str = "export class C { property = 1 }\n";

/// tsgo's `#{target}` project (`TestContentMapperInProject`).
const TARGET_CONFIG: &str = r#"{
    "compilerOptions": { "target": "es2020", "module": "esnext", "moduleResolution": "bundler", "strict": true },
    "contentMappers": [ { "package": "mapper", "extensions": [".box"] } ]
}"#;

const TARGET_APP: &str = "export const version = #{target};\n";

/// A session over `files` whose content mappers run through `spawner`
/// under `run_external_code`.
fn mapper_session(
    files: Vec<(&str, Seed)>,
    run_external_code: bool,
    spawner: Arc<dyn Spawner>,
) -> (Session, Arc<MemFs>) {
    let fs = Arc::new(
        MemFs::from_entries(files, false, Arc::new(SystemClock)).expect("build the file system"),
    );
    let session = Session::new(
        SessionOptions {
            current_directory: "/".to_owned(),
            library_catalog: LibraryCatalog::typescript_7_1(Path::new(EMBEDDED_LIBRARY_DIRECTORY)),
            load_limits: DEFAULT_LOAD_LIMITS,
            run_external_code,
        },
        Arc::new(BundledFs::new(Arc::clone(&fs) as Arc<dyn FileSystem>)),
        Some(spawner),
        false,
    );
    (session, fs)
}

fn seeds<'a>(files: &[(&'a str, &str)]) -> Vec<(&'a str, Seed)> {
    files
        .iter()
        .map(|(name, text)| (*name, Seed::file(*text)))
        .collect()
}

/// The project of the first tests: `/p` with one mapped file.
fn project_files(mapper: &str) -> Vec<(&'static str, Seed)> {
    vec![
        ("/p/tsconfig.json", Seed::file(CONFIG)),
        (
            "/p/node_modules/mapper/package.json",
            Seed::file(package_json(mapper)),
        ),
        ("/p/component.box", Seed::file(COMPONENT)),
    ]
}

fn open_project(session: &Session, config: &str) -> Value {
    let response = call(
        session,
        "createSnapshot",
        json!({ "openProjects": [config] }),
    )
    .unwrap();
    json!({
        "snapshot": response["snapshot"],
        "project": response["projects"][0]["id"],
    })
}

fn with(base: &Value, fields: Value) -> Value {
    let mut params = base.clone();
    for (key, value) in fields.as_object().unwrap() {
        params[key] = value.clone();
    }
    params
}

/// The snapshot after the client's file notifications, with its programs
/// built (tsgo's watch events, then a language service request).
fn notify(session: &Session, snapshot: &Value, notifications: Value) -> Value {
    call(
        session,
        "updateSnapshot",
        json!({
            "snapshot": snapshot,
            "changes": { "fileNotifications": notifications, "ensurePrograms": true },
        }),
    )
    .unwrap()
}

fn release(session: &Session, snapshot: &Value) {
    assert_eq!(
        call(session, "release", json!({ "snapshot": snapshot })).unwrap(),
        true
    );
}

fn project(session: &Session, snapshot: &Value, config: &str) -> Arc<Project> {
    let snapshot = session.snapshot(snapshot.as_u64().unwrap()).unwrap();
    Arc::clone(snapshot.project(&ProjectId::new(config)).unwrap())
}

fn program(session: &Session, snapshot: &Value, config: &str) -> Arc<ProjectProgram> {
    Arc::clone(project(session, snapshot, config).program().unwrap())
}

/// The program's source of `file_name`, when the program has it.
fn source(program: &ProjectProgram, file_name: &str) -> Option<PreparedSourceFile> {
    program
        .prepared()
        .source_files()
        .iter()
        .find(|source| source.path().display() == JsStr::from_str(file_name))
        .cloned()
}

fn text(program: &ProjectProgram, file_name: &str) -> String {
    source(program, file_name)
        .unwrap_or_else(|| panic!("{file_name} is in the program"))
        .text()
        .to_owned()
}

/// The diagnostic codes anywhere in a response.
fn codes(value: &Value) -> Vec<u64> {
    match value {
        Value::Array(values) => values.iter().flat_map(codes).collect(),
        Value::Object(fields) => fields
            .get("code")
            .and_then(Value::as_u64)
            .into_iter()
            .chain(fields.values().flat_map(codes))
            .collect(),
        _ => Vec::new(),
    }
}

fn read_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

/// tsgo `encodedString`: a string of the encoding's string table.
fn encoded_string(data: &[u8], index: u32) -> String {
    let offsets = read_u32(data, HEADER_OFFSET_STRING_OFFSETS) as usize;
    let strings = read_u32(data, HEADER_OFFSET_STRING_DATA) as usize;
    let start = read_u32(data, offsets + index as usize * 4) as usize;
    let end = read_u32(data, offsets + index as usize * 4 + 4) as usize;
    String::from_utf8(data[strings + start..strings + end].to_vec()).unwrap()
}

/// An encoded source file, and the offset of its extended data.
fn source_file_data(session: &Session, params: Value) -> (Vec<u8>, usize) {
    let response = call(session, "getSourceFile", params).unwrap();
    let data = base64::engine::general_purpose::STANDARD
        .decode(response["data"].as_str().unwrap())
        .unwrap();
    let nodes = read_u32(&data, HEADER_OFFSET_NODES) as usize;
    let root_data = read_u32(&data, nodes + NODE_SIZE + NODE_OFFSET_DATA);
    let extended = read_u32(&data, HEADER_OFFSET_EXTENDED_DATA) as usize
        + (root_data & NODE_DATA_STRING_INDEX_MASK) as usize;
    (data, extended)
}

#[test]
fn a_mapped_file_encodes_its_mapper_and_its_symbols_are_the_snapshots() {
    let (session, _) = mapper_session(
        project_files(VERBATIM_MAPPER),
        true,
        Arc::new(RecordingSpawner::new()),
    );
    let base = open_project(&session, "/p/tsconfig.json");
    // tsgo `recordExtendedData_SourceFile`: the mapper's identity, the
    // virtual name and the original text (the same as the verbatim
    // output's, so the text's string).
    let (data, extended) =
        source_file_data(&session, with(&base, json!({ "file": "/p/component.box" })));
    let text = read_u32(&data, extended);
    assert_eq!(read_u32(&data, extended + 48), text);
    assert_eq!(
        encoded_string(&data, read_u32(&data, extended + 64)),
        "mapper@1.0.0"
    );
    assert_eq!(
        encoded_string(&data, read_u32(&data, extended + 68)),
        "/p/component.box.ts"
    );
    // tsgo `symbolOwnerFile`: the parse cache does not hold a content
    // mapper's output, so its symbols are the snapshot's, and a reference
    // to one names no file.
    let class = call(
        &session,
        "getSymbolAtPosition",
        with(
            &base,
            json!({ "file": "/p/component.box", "position": COMPONENT.find("C {").unwrap() }),
        ),
    )
    .unwrap();
    assert_eq!(class["name"], "C");
    assert_eq!(class["reference"]["kind"], 1);
    assert_eq!(class["reference"].get("file"), None);
    assert_eq!(class["reference"]["snapshot"], base["snapshot"]);
    let property = call(
        &session,
        "getSymbolAtPosition",
        with(
            &base,
            json!({ "file": "/p/component.box", "position": COMPONENT.find("property").unwrap() }),
        ),
    )
    .unwrap();
    assert_eq!(property["name"], "property");
    assert_eq!(property["reference"]["kind"], 1);
    // tsgo `newSymbolReference`: the class's compact reference names no
    // file (`file` is omitted when empty).
    assert!(property["parent"].get("id").is_some());
    assert_eq!(property["parent"].get("file"), None);
}

#[test]
fn without_run_external_code_a_configs_content_mappers_are_rejected() {
    let (session, _) = mapper_session(
        project_files(VERBATIM_MAPPER),
        false,
        Arc::new(RecordingSpawner::new()),
    );
    let response = call(
        &session,
        "createSnapshot",
        json!({ "openProjects": ["/p/tsconfig.json"] }),
    )
    .unwrap();
    // tsgo's response: TS18068, then TS18003 for the include that matched
    // no file.
    let command_line = &response["projects"][0]["parsedCommandLine"];
    let codes = command_line["errors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|error| error["code"].as_u64().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(codes, [18068, 18003]);
    assert_eq!(command_line["fileNames"], json!([]));
}

#[test]
fn under_run_external_code_a_configs_content_mappers_register() {
    // tsgo's response: the config parsed with `runExternalCode` as an
    // existing option lists the mapped file, and the option follows the
    // config's.
    let (session, _) = mapper_session(
        project_files(VERBATIM_MAPPER),
        true,
        Arc::new(RecordingSpawner::new()),
    );
    let text = call_text(
        &session,
        "createSnapshot",
        &json!({ "openProjects": ["/p/tsconfig.json"] }),
    )
    .unwrap();
    assert!(
        text.contains(r#""options":{"noLib":true,"types":[],"configFilePath":"/p/tsconfig.json","runExternalCode":true}"#),
        "{text}"
    );
    let response: Value = serde_json::from_str(&text).unwrap();
    let command_line = &response["projects"][0]["parsedCommandLine"];
    assert_eq!(command_line["fileNames"], json!(["/p/component.box"]));
    assert_eq!(command_line["errors"], json!([]));
}

#[test]
fn a_programs_mapper_project_closes_when_no_snapshot_holds_it() {
    // tsgo `Snapshot.dispose`: the program's content mapper project closes
    // with the last snapshot of the program, and the session closes the
    // processes.
    let spawner = Arc::new(RecordingSpawner::new());
    let (session, _) = mapper_session(
        project_files(DYNAMIC_VERBATIM_MAPPER),
        true,
        Arc::clone(&spawner) as Arc<dyn Spawner>,
    );
    let base = open_project(&session, "/p/tsconfig.json");
    assert_eq!(spawner.lifecycle.opens.load(Ordering::SeqCst), 1);
    assert_eq!(spawner.lifecycle.closes.load(Ordering::SeqCst), 0);
    release(&session, &base["snapshot"]);
    assert_eq!(spawner.lifecycle.closes.load(Ordering::SeqCst), 1);
    session.close();
    assert_eq!(spawner.closes.load(Ordering::SeqCst), 1);
}

#[test]
fn a_project_without_mapped_files_starts_no_mapper() {
    // tsgo `TestContentMapperProjectWithoutMappedFiles`: the program has a
    // content mapper project when its config has mappers, and no process
    // starts for a program without mapped files.
    for has_mapper in [false, true] {
        let config = if has_mapper {
            r#"{
                "compilerOptions": { "noLib": true },
                "contentMappers": [ { "package": "mapper", "extensions": [".box"] } ]
            }"#
        } else {
            r#"{"compilerOptions": {"noLib": true}}"#
        };
        let manifest = package_json(TRANSFORMING_MAPPER);
        let spawner = Arc::new(RecordingSpawner::new());
        let (session, _) = mapper_session(
            seeds(&[
                ("/home/project/tsconfig.json", config),
                ("/home/project/node_modules/mapper/package.json", &manifest),
                ("/home/project/main.ts", "export {};"),
            ]),
            true,
            Arc::clone(&spawner) as Arc<dyn Spawner>,
        );
        let base = open_project(&session, "/home/project/tsconfig.json");
        let program = program(&session, &base["snapshot"], "/home/project/tsconfig.json");
        assert_eq!(
            program
                .prepared()
                .program_options()
                .content_mapper_project()
                .is_some(),
            has_mapper
        );
        assert_eq!(spawner.spawns.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn mapped_files_load_in_parallel() {
    // tsgo `TestContentMapperParallelFileLoading`: parallel parsing reads
    // the mapper project's identity while another file initializes it.
    let manifest = package_json(TRANSFORMING_MAPPER);
    let names = (0..32)
        .map(|index| format!("/home/project/file{index}.box"))
        .collect::<Vec<_>>();
    let mut files = seeds(&[
        (
            "/home/project/tsconfig.json",
            r#"{
                "compilerOptions": { "target": "es2020", "noLib": true },
                "contentMappers": [ { "package": "mapper", "extensions": [".box"] } ]
            }"#,
        ),
        ("/home/project/node_modules/mapper/package.json", &manifest),
        ("/home/project/main.ts", "export {};"),
    ]);
    for name in &names {
        files.push((name, Seed::file(TARGET_APP)));
    }
    let (session, _) = mapper_session(files, true, Arc::new(RecordingSpawner::new()));
    let base = open_project(&session, "/home/project/tsconfig.json");
    let program = program(&session, &base["snapshot"], "/home/project/tsconfig.json");
    assert!(program
        .prepared()
        .program_options()
        .content_mapper_project()
        .is_some());
    for name in &names {
        assert_eq!(
            text(&program, name),
            "const __VERSION = \"1.0.0\";\nexport const version = 7;\n"
        );
    }
}

/// tsgo `TestContentMapperInProject`'s project, its session trusted or not.
fn target_project(run_external_code: bool) -> (Session, Arc<MemFs>, Arc<RecordingSpawner>) {
    let manifest = package_json(TRANSFORMING_MAPPER);
    let spawner = Arc::new(RecordingSpawner::new());
    let (session, fs) = mapper_session(
        seeds(&[
            ("/home/project/tsconfig.json", TARGET_CONFIG),
            ("/home/project/node_modules/mapper/package.json", &manifest),
            ("/home/project/app.box", TARGET_APP),
            (
                "/home/project/main.ts",
                "import { version } from \"./app.box\";\nexport const twice: number = version * 2;\n",
            ),
        ]),
        run_external_code,
        Arc::clone(&spawner) as Arc<dyn Spawner>,
    );
    (session, fs, spawner)
}

#[test]
fn a_trusted_session_transforms_the_mapped_file() {
    // tsgo `TestContentMapperInProject`: the mapper substitutes the
    // es2020 target (7) for the `#{target}` token.
    let (session, _, _) = target_project(true);
    let base = open_project(&session, "/home/project/tsconfig.json");
    let program = program(&session, &base["snapshot"], "/home/project/tsconfig.json");
    assert!(text(&program, "/home/project/app.box").contains("export const version = 7;"));
}

#[test]
fn an_untrusted_session_does_not_run_the_mapper() {
    // tsgo `TestContentMapperInProject`: without `runExternalCode` the
    // mappers do not register, so `.box` is no supported extension and the
    // file never enters the program.
    let (session, _, spawner) = target_project(false);
    let base = open_project(&session, "/home/project/tsconfig.json");
    let program = program(&session, &base["snapshot"], "/home/project/tsconfig.json");
    assert!(source(&program, "/home/project/app.box").is_none());
    assert_eq!(spawner.spawns.load(Ordering::SeqCst), 0);
}

#[test]
fn a_changed_mapped_file_runs_through_its_mapper_again_in_place() {
    // tsgo `TestContentMapperInProject` ("watch change to a content-mapped
    // file updates the program"): the program is cloned with the file
    // transformed again, and only the file changes.
    let (session, fs, _) = target_project(true);
    let base = open_project(&session, "/home/project/tsconfig.json");
    let original = program(&session, &base["snapshot"], "/home/project/tsconfig.json");
    let original = source(&original, "/home/project/app.box").unwrap();
    fs.write(
        "/home/project/app.box",
        b"export const version = #{target};\nexport const watched = true;\n",
    )
    .unwrap();
    let ensured = notify(
        &session,
        &base["snapshot"],
        json!({ "changed": ["/home/project/app.box"] }),
    );
    assert_eq!(
        ensured["changes"],
        json!({ "changedProjects": { "/home/project/tsconfig.json": { "changedFiles": ["/home/project/app.box"] } } })
    );
    let project = project(
        &session,
        &ensured["snapshot"],
        "/home/project/tsconfig.json",
    );
    assert_eq!(project.program_update_kind(), ProgramUpdateKind::Cloned);
    assert_eq!(
        project.program_last_update(),
        ensured["snapshot"].as_u64().unwrap()
    );
    let updated = source(project.program().unwrap(), "/home/project/app.box").unwrap();
    assert!(!Arc::ptr_eq(
        original.content_mapped().unwrap(),
        updated.content_mapped().unwrap()
    ));
    assert!(updated.text().contains("export const version = 7;"));
    assert!(updated.text().contains("export const watched = true;"));
    // The API encodes the new text.
    let (data, extended) = source_file_data(
        &session,
        json!({ "snapshot": ensured["snapshot"], "project": "/home/project/tsconfig.json", "file": "/home/project/app.box" }),
    );
    assert_eq!(
        encoded_string(&data, read_u32(&data, extended)),
        updated.text()
    );
}

#[test]
fn an_unchanged_mapped_file_is_reused_across_a_full_rebuild() {
    // tsgo `TestContentMapperInProject` ("unchanged content-mapped file is
    // reused from the cache across a full rebuild"): an option the mapper
    // does not depend on builds the program again, and the file's outputs
    // come from the content-mapped parse cache, not from the mapper.
    let (session, fs, _) = target_project(true);
    let base = open_project(&session, "/home/project/tsconfig.json");
    let before = program(&session, &base["snapshot"], "/home/project/tsconfig.json");
    let box_file = source(&before, "/home/project/app.box").unwrap();
    assert!(box_file.text().contains("export const version = 7;"));
    let document = before.with_live(|live| {
        let index = live.file_index("/home/project/app.box").unwrap();
        Arc::clone(live.document(index).unwrap())
    });
    fs.write(
        "/home/project/tsconfig.json",
        TARGET_CONFIG
            .replace(r#""strict": true"#, r#""strict": false"#)
            .as_bytes(),
    )
    .unwrap();
    let ensured = notify(
        &session,
        &base["snapshot"],
        json!({ "changed": ["/home/project/tsconfig.json"] }),
    );
    let project = project(
        &session,
        &ensured["snapshot"],
        "/home/project/tsconfig.json",
    );
    assert_ne!(project.program_update_kind(), ProgramUpdateKind::Cloned);
    let after = project.program().unwrap();
    assert!(!Arc::ptr_eq(&before, after));
    let rebuilt = source(after, "/home/project/app.box").unwrap();
    assert!(Arc::ptr_eq(
        box_file.content_mapped().unwrap(),
        rebuilt.content_mapped().unwrap()
    ));
    assert!(Arc::ptr_eq(box_file.snapshot(), rebuilt.snapshot()));
    after.with_live(|live| {
        let index = live.file_index("/home/project/app.box").unwrap();
        assert!(Arc::ptr_eq(&document, live.document(index).unwrap()));
    });
}

#[test]
fn a_mapper_manifest_change_reloads_the_config() {
    // tsgo `TestContentMapperPackageManifestChangeReloadsConfig`: the
    // mapper's package is a link, its real directory holds the manifest,
    // and a change to the manifest parses the config again.
    let invalid = r#"{
        "name": "mapper",
        "version": "1.0.0",
        "typescript": { "contentMapper": { "exec": ["compiler-test-mapper"] } }
    }"#;
    let (session, fs) = mapper_session(
        vec![
            (
                "/home/project/tsconfig.json",
                Seed::file(
                    r#"{
                        "compilerOptions": { "target": "es2020", "module": "esnext", "moduleResolution": "bundler" },
                        "contentMappers": [{ "package": "mapper", "extensions": [".box"] }]
                    }"#,
                ),
            ),
            (
                "/home/project/node_modules/mapper",
                Seed::symlink("/home/mapper"),
            ),
            ("/home/mapper/package.json", Seed::file(invalid)),
            ("/home/project/app.box", Seed::file(TARGET_APP)),
            (
                "/home/project/main.ts",
                Seed::file("import { version } from \"./app.box\";\n"),
            ),
        ],
        true,
        Arc::new(RecordingSpawner::new()),
    );
    let base = open_project(&session, "/home/project/tsconfig.json");
    let project = project(&session, &base["snapshot"], "/home/project/tsconfig.json");
    let Some(CommandLine::Config(plan)) = project.command_line() else {
        panic!("a configured project");
    };
    let [mapper] = plan.content_mappers() else {
        panic!("one mapper");
    };
    assert_eq!(mapper.package_directory, "/home/mapper");
    let fixed = package_json(TRANSFORMING_MAPPER).replacen(
        r#""version": "1.0.0""#,
        r#""version": "2.0.0""#,
        1,
    );
    assert!(fixed.contains(r#""version": "2.0.0""#));
    fs.write("/home/mapper/package.json", fixed.as_bytes())
        .unwrap();
    let ensured = notify(
        &session,
        &base["snapshot"],
        json!({ "changed": ["/home/mapper/package.json"] }),
    );
    let program = program(
        &session,
        &ensured["snapshot"],
        "/home/project/tsconfig.json",
    );
    assert!(text(&program, "/home/project/app.box").contains("export const version = 7;"));
}

#[test]
fn an_edit_through_a_supplemental_mapper_is_cloned() {
    // tsgo `TestContentMapperSupplementalFileClonedOnEdit`: the edit
    // replaces the canonical output and its supplemental one in place; a
    // reference in the edit builds the program again.
    let manifest = package_json(SUPPLEMENTAL_MAPPER);
    let (session, fs) = mapper_session(
        seeds(&[
            (
                "/home/project/tsconfig.json",
                r#"{ "compilerOptions": { "strict": true }, "contentMappers": [{ "package": "mapper", "extensions": [".box"] }] }"#,
            ),
            ("/home/project/node_modules/mapper/package.json", &manifest),
            (
                "/home/project/app.box",
                "declare const supplementalValue: number;\n",
            ),
            ("/home/project/extra.d.ts", "interface Extra {}\n"),
            (
                "/home/project/main.ts",
                "const value: number = supplementalValue;\n",
            ),
        ]),
        true,
        Arc::new(RecordingSpawner::new()),
    );
    let base = open_project(&session, "/home/project/tsconfig.json");
    let old_program = program(&session, &base["snapshot"], "/home/project/tsconfig.json");
    let old_canonical = source(&old_program, "/home/project/app.box").unwrap();
    assert_eq!(
        old_canonical.content_mapped().unwrap().supplemental,
        ["/home/project/app.box.0.ts"]
    );
    let old_supplemental = source(&old_program, "/home/project/app.box.0.ts").unwrap();

    fs.write(
        "/home/project/app.box",
        b"declare const supplementalValue: string;\n",
    )
    .unwrap();
    let ensured = notify(
        &session,
        &base["snapshot"],
        json!({ "changed": ["/home/project/app.box"] }),
    );
    let cloned = project(
        &session,
        &ensured["snapshot"],
        "/home/project/tsconfig.json",
    );
    assert_eq!(cloned.program_update_kind(), ProgramUpdateKind::Cloned);
    let new_program = cloned.program().unwrap();
    let new_canonical = source(new_program, "/home/project/app.box").unwrap();
    assert_eq!(
        new_canonical.content_mapped().unwrap().supplemental,
        ["/home/project/app.box.0.ts"]
    );
    let new_supplemental = source(new_program, "/home/project/app.box.0.ts").unwrap();
    assert!(!Arc::ptr_eq(
        old_canonical.content_mapped().unwrap(),
        new_canonical.content_mapped().unwrap()
    ));
    assert!(!Arc::ptr_eq(
        old_supplemental.content_mapped().unwrap(),
        new_supplemental.content_mapped().unwrap()
    ));
    assert!(new_supplemental
        .text()
        .contains("supplementalValue: string"));
    let diagnostics = call(
        &session,
        "getSemanticDiagnostics",
        json!({
            "snapshot": ensured["snapshot"],
            "project": "/home/project/tsconfig.json",
            "files": ["/home/project/main.ts"],
        }),
    )
    .unwrap();
    assert!(codes(&diagnostics).contains(&2322), "{diagnostics}");

    // A change to the supplemental output's references builds the program
    // again.
    fs.write(
        "/home/project/app.box",
        b"/// <reference path=\"./extra.d.ts\" />\ndeclare const supplementalValue: string;\n",
    )
    .unwrap();
    let rebuilt = notify(
        &session,
        &ensured["snapshot"],
        json!({ "changed": ["/home/project/app.box"] }),
    );
    let project = project(
        &session,
        &rebuilt["snapshot"],
        "/home/project/tsconfig.json",
    );
    assert_eq!(
        project.program_update_kind(),
        ProgramUpdateKind::SameFileNames
    );
}

#[test]
fn an_unrelated_edit_keeps_a_module_mapped_file() {
    // tsgo `TestContentMapperModuleExtensionClonedOnUnrelatedEdit`: a
    // module output (its virtual name `.mts`, so a module) is kept by a
    // program cloned for another file.
    let manifest = package_json(MODULE_VERBATIM_MAPPER);
    let (session, fs) = mapper_session(
        seeds(&[
            (
                "/home/project/tsconfig.json",
                r#"{ "contentMappers": [{ "package": "mapper", "extensions": [".box"] }] }"#,
            ),
            ("/home/project/node_modules/mapper/package.json", &manifest),
            ("/home/project/app.box", "export const value = 1;\n"),
            (
                "/home/project/main.ts",
                r#"import { value } from "./app.box"; value;"#,
            ),
        ]),
        true,
        Arc::new(RecordingSpawner::new()),
    );
    let base = open_project(&session, "/home/project/tsconfig.json");
    let before = program(&session, &base["snapshot"], "/home/project/tsconfig.json");
    let mapped = source(&before, "/home/project/app.box").unwrap();
    assert_eq!(
        mapped.content_mapped().unwrap().virtual_file_name,
        "/home/project/app.box.mts"
    );
    assert_eq!(mapped.parse_file_name(), "/home/project/app.box.mts");

    fs.write(
        "/home/project/main.ts",
        br#"import { value } from "./app.box"; value + 1;"#,
    )
    .unwrap();
    let ensured = notify(
        &session,
        &base["snapshot"],
        json!({ "changed": ["/home/project/main.ts"] }),
    );
    let project = project(
        &session,
        &ensured["snapshot"],
        "/home/project/tsconfig.json",
    );
    assert_eq!(project.program_update_kind(), ProgramUpdateKind::Cloned);
    let kept = source(project.program().unwrap(), "/home/project/app.box").unwrap();
    assert!(Arc::ptr_eq(
        mapped.content_mapped().unwrap(),
        kept.content_mapped().unwrap()
    ));
    assert!(Arc::ptr_eq(mapped.snapshot(), kept.snapshot()));
}

/// tsgo's dynamic mapper project: `mapper.config.json` beside the config
/// is the file the mapper watches.
fn dynamic_project(config: &str) -> (Session, Arc<MemFs>, Arc<RecordingSpawner>, Value) {
    let manifest = package_json(DYNAMIC_VERBATIM_MAPPER);
    let spawner = Arc::new(RecordingSpawner::new());
    let (session, fs) = mapper_session(
        seeds(&[
            ("/home/project/tsconfig.json", config),
            ("/home/project/node_modules/mapper/package.json", &manifest),
            ("/home/project/mapper.config.json", r#"{ "version": 1 }"#),
            ("/home/project/app.box", "export const value = 1;\n"),
            (
                "/home/project/main.ts",
                r#"import { value } from "./app.box"; value;"#,
            ),
        ]),
        true,
        Arc::clone(&spawner) as Arc<dyn Spawner>,
    );
    let base = open_project(&session, "/home/project/tsconfig.json");
    (session, fs, spawner, base)
}

#[test]
fn a_dynamic_mappers_watched_file_builds_the_program_again() {
    // tsgo `TestDynamicContentMapperInProject`: a change to the file the
    // dynamic mapper watches refreshes its project and builds a new
    // program.
    let (session, fs, _, base) = dynamic_project(
        r#"{ "contentMappers": [ { "package": "mapper", "extensions": [".box"], "options": { "mode": "project" } } ] }"#,
    );
    let before = program(&session, &base["snapshot"], "/home/project/tsconfig.json");
    let mapped = source(&before, "/home/project/app.box").unwrap();
    assert!(!mapped.content_mapped().unwrap().is_failure_stub());
    fs.write("/home/project/mapper.config.json", br#"{ "version": 2 }"#)
        .unwrap();
    let ensured = notify(
        &session,
        &base["snapshot"],
        json!({ "changed": ["/home/project/mapper.config.json"] }),
    );
    let after = program(
        &session,
        &ensured["snapshot"],
        "/home/project/tsconfig.json",
    );
    assert!(!Arc::ptr_eq(&before, &after));
    let mapped = source(&after, "/home/project/app.box").unwrap();
    assert!(!mapped.content_mapped().unwrap().is_failure_stub());
}

#[test]
fn mixed_watch_batches_refresh_a_dynamic_mapper() {
    // tsgo `TestDynamicContentMapperRefreshesForMixedWatchBatches`: an
    // excessive batch refreshes every project, and the mapper's watched
    // file deleted or created along with other changes refreshes the
    // project too.
    let mut excessive = vec![
        "/home/project/mapper.config.json".to_owned(),
        "/home/project/main.ts".to_owned(),
    ];
    excessive.extend((2..1001).map(|index| format!("/home/project/noise-{index}.ts")));
    let batches = [
        ("excessive events", json!({ "changed": excessive })),
        (
            "changed files make project fully dirty before mapper deletion",
            json!({
                "changed": ["/home/project/main.ts", "/home/project/app.box"],
                "deleted": ["/home/project/mapper.config.json"],
            }),
        ),
        (
            "changed files make project fully dirty before mapper creation",
            json!({
                "changed": ["/home/project/main.ts", "/home/project/app.box"],
                "created": ["/home/project/mapper.config.json"],
            }),
        ),
    ];
    for (name, batch) in batches {
        let (session, _, spawner, base) = dynamic_project(
            r#"{ "contentMappers": [{ "package": "mapper", "extensions": [".box"] }] }"#,
        );
        assert_eq!(spawner.lifecycle.opens.load(Ordering::SeqCst), 1, "{name}");
        assert_eq!(spawner.lifecycle.closes.load(Ordering::SeqCst), 0, "{name}");
        let ensured = notify(&session, &base["snapshot"], batch);
        program(
            &session,
            &ensured["snapshot"],
            "/home/project/tsconfig.json",
        );
        assert_eq!(spawner.lifecycle.closes.load(Ordering::SeqCst), 1, "{name}");
        assert_eq!(spawner.lifecycle.opens.load(Ordering::SeqCst), 2, "{name}");
    }
}

#[test]
fn an_unused_dynamic_mapper_is_not_opened() {
    // tsgo `TestUnusedDynamicContentMapperIsNotOpened`.
    let manifest = package_json(DYNAMIC_VERBATIM_MAPPER);
    let spawner = Arc::new(RecordingSpawner::new());
    let (session, _) = mapper_session(
        seeds(&[
            (
                "/home/project/tsconfig.json",
                r#"{ "contentMappers": [{ "package": "mapper", "extensions": [".box"] }] }"#,
            ),
            ("/home/project/node_modules/mapper/package.json", &manifest),
            ("/home/project/main.ts", "export const value = 1;"),
        ]),
        true,
        Arc::clone(&spawner) as Arc<dyn Spawner>,
    );
    open_project(&session, "/home/project/tsconfig.json");
    assert_eq!(spawner.lifecycle.opens.load(Ordering::SeqCst), 0);
}

#[test]
fn a_mapper_process_is_shared_across_projects() {
    // tsgo `TestContentMapperProcessSharedAcrossProjects`: two projects of
    // the same mapper share its process, which closes with its last owner.
    let config = |extension: &str| {
        format!(
            r#"{{
                "compilerOptions": {{ "target": "es2020", "module": "esnext", "moduleResolution": "bundler" }},
                "contentMappers": [ {{ "package": "mapper", "extensions": ["{extension}"] }} ]
            }}"#
        )
    };
    let (config_a, config_b) = (config(".box"), config(".panel"));
    let manifest = package_json(TRANSFORMING_MAPPER);
    let spawner = Arc::new(RecordingSpawner::new());
    let (session, fs) = mapper_session(
        seeds(&[
            ("/home/a/tsconfig.json", &config_a),
            ("/home/a/node_modules/mapper/package.json", &manifest),
            ("/home/a/app.box", "export const a = 1;\n"),
            ("/home/b/tsconfig.json", &config_b),
            ("/home/b/node_modules/mapper/package.json", &manifest),
            ("/home/b/app.panel", "export const b = 1;\n"),
        ]),
        true,
        Arc::clone(&spawner) as Arc<dyn Spawner>,
    );
    let response = call(
        &session,
        "createSnapshot",
        json!({ "openProjects": ["/home/a/tsconfig.json", "/home/b/tsconfig.json"] }),
    )
    .unwrap();
    let first = response["snapshot"].clone();
    assert_eq!(spawner.spawns.load(Ordering::SeqCst), 1);

    fs.write("/home/a/tsconfig.json", b"{}").unwrap();
    let second = notify(
        &session,
        &first,
        json!({ "changed": ["/home/a/tsconfig.json"] }),
    );
    release(&session, &first);
    assert_eq!(spawner.closes.load(Ordering::SeqCst), 0);

    fs.write("/home/b/tsconfig.json", b"{}").unwrap();
    let third = notify(
        &session,
        &second["snapshot"],
        json!({ "changed": ["/home/b/tsconfig.json"] }),
    );
    release(&session, &second["snapshot"]);
    assert_eq!(spawner.closes.load(Ordering::SeqCst), 1);
    release(&session, &third["snapshot"]);
}

#[test]
fn resolution_over_a_snapshot_takes_its_mappers_extensions() {
    // tsgo `handleResolveModuleName` (its responses): over a snapshot the
    // resolver has the extensions of the snapshot's content mappers (tsgo
    // `ContentMapperExtensions`), so `./app.box` is the file itself; over
    // the session it is not resolved. The program's own resolution says it
    // used them too (tsgo `getResolvedModule`).
    let manifest = package_json(VERBATIM_MAPPER);
    let (session, _) = mapper_session(
        seeds(&[
            (
                "/home/project/tsconfig.json",
                r#"{
                    "compilerOptions": { "noLib": true, "types": [], "module": "esnext", "moduleResolution": "bundler" },
                    "contentMappers": [ { "package": "mapper", "extensions": [".box"] } ]
                }"#,
            ),
            ("/home/project/node_modules/mapper/package.json", &manifest),
            ("/home/project/app.box", "export const value = 1;\n"),
            (
                "/home/project/main.ts",
                "import { value } from \"./app.box\";\nvalue;\n",
            ),
        ]),
        true,
        Arc::new(RecordingSpawner::new()),
    );
    let base = open_project(&session, "/home/project/tsconfig.json");
    assert_eq!(
        call(
            &session,
            "getResolvedModule",
            with(
                &base,
                json!({ "file": "/home/project/main.ts", "moduleName": "./app.box", "mode": 99 }),
            ),
        )
        .unwrap(),
        json!({
            "resolvedFileName": "/home/project/app.box",
            "extension": ".box",
            "resolvedUsingTsExtension": false,
            "resolvedUsingExtraExtensions": true,
            "isExternalLibraryImport": false,
        })
    );
    let resolver = call(
        &session,
        "createModuleResolver",
        json!({ "compilerOptions": { "module": 99, "moduleResolution": 100, "traceResolution": true } }),
    )
    .unwrap();
    let request = json!({ "resolver": resolver, "moduleName": "./app.box", "containingDirectory": "/home/project" });
    assert_eq!(
        call(
            &session,
            "resolveModuleName",
            with(&request, json!({ "snapshot": base["snapshot"] })),
        )
        .unwrap(),
        json!({
            "resolvedModule": {
                "resolvedFileName": "/home/project/app.box",
                "extension": ".box",
                "resolvedUsingTsExtension": false,
                "resolvedUsingExtraExtensions": true,
                "isExternalLibraryImport": false,
            },
            "trace": [
                "======== Resolving module './app.box' from '/home/project'. ========",
                "Explicitly specified module resolution kind: 'Bundler'.",
                "Resolving in CJS mode with conditions 'import', 'types'.",
                "Loading module as file / folder, candidate module location '/home/project/app.box', target file types: TypeScript, JavaScript, Declaration, JSON.",
                "File name '/home/project/app.box' has a '.box' extension - stripping it.",
                "File '/home/project/app.box' exists - use it as a name resolution result.",
                "======== Module name './app.box' was successfully resolved to '/home/project/app.box'. ========",
            ],
        })
    );
    assert_eq!(
        call(&session, "resolveModuleName", request).unwrap(),
        json!({
            "trace": [
                "======== Resolving module './app.box' from '/home/project'. ========",
                "Explicitly specified module resolution kind: 'Bundler'.",
                "Resolving in CJS mode with conditions 'import', 'types'.",
                "Loading module as file / folder, candidate module location '/home/project/app.box', target file types: TypeScript, JavaScript, Declaration, JSON.",
                "File name '/home/project/app.box' has a '.box' extension - stripping it.",
                "File '/home/project/app.d.box.ts' does not exist.",
                "File '/home/project/app.box.ts' does not exist.",
                "File '/home/project/app.box.tsx' does not exist.",
                "File '/home/project/app.box.d.ts' does not exist.",
                "File '/home/project/app.box.js' does not exist.",
                "File '/home/project/app.box.jsx' does not exist.",
                "Directory '/home/project/app.box' does not exist, skipping all lookups in it.",
                "======== Module name './app.box' was not resolved. ========",
            ],
        })
    );
}

#[test]
fn a_failure_stub_is_kept_by_a_cloned_program_and_remade_by_a_new_one() {
    // tsgo `ReuseProgram` keeps every other file of the program, a mapper's
    // failure stub included, while a program built again makes its own
    // stub (`emptyContentMappedFile`): an edit elsewhere changes only the
    // edited file.
    let manifest = package_json(FAILING_MAPPER);
    let (session, fs) = mapper_session(
        seeds(&[
            ("/home/project/tsconfig.json", TARGET_CONFIG),
            ("/home/project/node_modules/mapper/package.json", &manifest),
            ("/home/project/app.box", "export const value = 1;\n"),
            ("/home/project/main.ts", "export const main = 1;\n"),
        ]),
        true,
        Arc::new(RecordingSpawner::new()),
    );
    let base = open_project(&session, "/home/project/tsconfig.json");
    let stub_document = |snapshot: &Value| {
        let program = program(&session, snapshot, "/home/project/tsconfig.json");
        let stub = source(&program, "/home/project/app.box").unwrap();
        assert!(stub.content_mapped().unwrap().is_failure_stub());
        program.with_live(|live| {
            let index = live.file_index("/home/project/app.box").unwrap();
            Arc::clone(live.document(index).unwrap())
        })
    };
    let first = stub_document(&base["snapshot"]);
    fs.write("/home/project/main.ts", b"export const main = 2;\n")
        .unwrap();
    let cloned = notify(
        &session,
        &base["snapshot"],
        json!({ "changed": ["/home/project/main.ts"] }),
    );
    assert_eq!(
        cloned["changes"],
        json!({ "changedProjects": { "/home/project/tsconfig.json": { "changedFiles": ["/home/project/main.ts"] } } })
    );
    assert_eq!(
        project(&session, &cloned["snapshot"], "/home/project/tsconfig.json").program_update_kind(),
        ProgramUpdateKind::Cloned
    );
    assert!(Arc::ptr_eq(&first, &stub_document(&cloned["snapshot"])));
    fs.write(
        "/home/project/tsconfig.json",
        TARGET_CONFIG
            .replace(r#""strict": true"#, r#""strict": false"#)
            .as_bytes(),
    )
    .unwrap();
    let rebuilt = notify(
        &session,
        &cloned["snapshot"],
        json!({ "changed": ["/home/project/tsconfig.json"] }),
    );
    assert!(!Arc::ptr_eq(&first, &stub_document(&rebuilt["snapshot"])));
}
