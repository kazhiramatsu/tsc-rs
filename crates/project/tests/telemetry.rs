//! tsgo's project information telemetry (`collectProjectInfoTelemetry` in
//! project/session.go; tsgo has no test of it): one event per project the
//! session builds, with its kind, config file name and shape, compiler
//! options and file counts.

mod common;

use common::*;
use tsc_host::vfs::Seed;
use tsc_lsproto::TelemetryEvent;

fn project_info_events(client: &ClientMock) -> Vec<tsc_lsproto::ProjectInfoTelemetryEvent> {
    client
        .telemetry_calls
        .lock()
        .unwrap()
        .iter()
        .filter_map(|event| match event {
            TelemetryEvent::ProjectInfoTelemetryEvent(event) => Some(event.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_configured_project_sends_its_information_once() {
    let (session, _fs, client) = setup_with_telemetry(
        vec![
            (
                "/home/projects/tsconfig.json".to_owned(),
                Seed::file(
                    r#"{ "compilerOptions": { "strict": true, "target": "es2020", "module": "nodenext", "allowJs": false }, "include": ["src"] }"#,
                ),
            ),
            (
                "/home/projects/src/a.ts".to_owned(),
                Seed::file("export const a = 1;"),
            ),
            (
                "/home/projects/src/b.d.ts".to_owned(),
                Seed::file("declare const b: number;"),
            ),
        ],
        "/home/projects",
    );
    open(
        &session,
        "file:///home/projects/src/a.ts",
        "export const a = 1;",
    );
    program(&session, "file:///home/projects/src/a.ts");
    session.wait_for_background_tasks();
    let events = project_info_events(&client);
    assert_eq!(events.len(), 1, "{events:?}");
    let properties = &events[0].properties;
    assert_eq!(properties["configFileName"], "tsconfig.json");
    assert_eq!(properties["projectType"], "configured");
    assert_eq!(properties["version"], tsc_types::TYPESCRIPT_VERSION);
    assert_eq!(
        properties["compilerOptions"],
        r#"{"allowJs":false,"module":"NodeNext","strict":true,"target":"ES2020"}"#
    );
    assert_eq!(properties["extends"], "false");
    assert_eq!(properties["files"], "false");
    assert_eq!(properties["include"], "true");
    assert_eq!(properties["exclude"], "false");
    let measurements = &events[0].measurements;
    assert_eq!(measurements.ts_file_count, 1.0);
    assert_eq!(
        measurements.ts_file_size,
        "export const a = 1;".len() as f64
    );
    // The declaration files with the default libraries (tsgo counts the
    // program's every file).
    let built = program(&session, "file:///home/projects/src/a.ts");
    let declarations = built
        .prepared()
        .source_files()
        .iter()
        .filter(|source| source.path().display().to_string_lossy().ends_with(".d.ts"))
        .count();
    assert!(declarations > 1);
    assert_eq!(measurements.dts_file_count, declarations as f64);
    // Another build of the project sends nothing.
    open(
        &session,
        "file:///home/projects/src/b.d.ts",
        "declare const b: number;",
    );
    program(&session, "file:///home/projects/src/b.d.ts");
    session.wait_for_background_tasks();
    assert_eq!(project_info_events(&client).len(), 1);
}

#[test]
fn an_inferred_project_has_no_config_shape() {
    let (session, _fs, client) = setup_with_telemetry(
        vec![(
            "/home/projects/a.js".to_owned(),
            Seed::file("export const a = 1;"),
        )],
        "/home/projects",
    );
    open_as(
        &session,
        "file:///home/projects/a.js",
        "export const a = 1;",
        tsc_lsproto::LanguageKind::JAVA_SCRIPT,
    );
    program(&session, "file:///home/projects/a.js");
    session.wait_for_background_tasks();
    let events = project_info_events(&client);
    assert_eq!(events.len(), 1, "{events:?}");
    let properties = &events[0].properties;
    assert_eq!(properties["configFileName"], "other");
    assert_eq!(properties["projectType"], "inferred");
    assert!(!properties.contains_key("include"));
    // tsgo's inferred project's options.
    assert_eq!(
        properties["compilerOptions"],
        r#"{"allowJs":true,"jsx":"react-jsx","module":"ESNext","moduleResolution":"Bundler","strictFunctionTypes":true,"strictNullChecks":true,"target":"ES2026"}"#
    );
    assert_eq!(events[0].measurements.js_file_count, 1.0);
}
