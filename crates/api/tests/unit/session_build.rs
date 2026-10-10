//! Build orchestrators (P5-6): `createBuildOrchestrator` over the
//! session's file system; builds and cleans of the whole graph, of one
//! project and of a project's references; an unknown project, a reference
//! cycle and a missing configuration; the errors of an unknown
//! orchestrator; the build and compiler options read only under their keys.
//! Pinned to tsgo's responses at 19dadef8 for the same files.

use super::*;

const GRAPH: &[(&str, &str)] = &[
    (
        "/a/tsconfig.json",
        r#"{"compilerOptions":{"composite":true,"outDir":"dist","rootDir":"src"},"files":["src/index.ts"]}"#,
    ),
    ("/a/src/index.ts", "export const a = 1; // keep"),
    (
        "/b/tsconfig.json",
        r#"{"compilerOptions":{"composite":true,"outDir":"dist","rootDir":"src"},"files":["src/index.ts"]}"#,
    ),
    ("/b/src/index.ts", "export const b = 2;"),
    (
        "/c/tsconfig.json",
        r#"{"compilerOptions":{"composite":true,"outDir":"dist","rootDir":"src"},"files":["src/index.ts"],"references":[{"path":"../a"},{"path":"../b"}]}"#,
    ),
    ("/c/src/index.ts", "export const c = 3;"),
];

/// A new orchestrator's ID.
fn create(session: &Session, params: Value) -> u64 {
    call(session, "createBuildOrchestrator", params).unwrap()["buildOrchestratorID"]
        .as_u64()
        .unwrap()
}

/// A build or clean request's JSON text.
fn step(session: &Session, method: &str, id: u64, project: Option<&str>) -> String {
    let params = match project {
        Some(project) => json!({ "buildOrchestratorID": id, "project": project }),
        None => json!({ "buildOrchestratorID": id }),
    };
    call_text(session, method, &params).unwrap()
}

fn read(fs: &MemFs, name: &str) -> Option<String> {
    fs.read(name)
        .ok()
        .map(|bytes| String::from_utf8(bytes).unwrap())
}

#[test]
fn builds_and_cleans_a_graph_and_its_parts() {
    let (session, fs) = session(GRAPH);
    let id = create(
        &session,
        json!({ "rootNames": ["/c/tsconfig.json"], "cwd": "/" }),
    );
    assert_eq!(
        step(&session, "build", id, None),
        r#"{"status":0,"statistics":{"Projects":3,"ProjectsBuilt":3,"TimestampUpdates":0}}"#
    );
    assert_eq!(
        read(&fs, "/a/dist/index.js").as_deref(),
        Some("export const a = 1; // keep\n")
    );
    assert_eq!(
        step(&session, "cleanBuild", id, Some("/a/tsconfig.json")),
        r#"{"status":0,"statistics":{"Projects":1,"ProjectsBuilt":0,"TimestampUpdates":0},"filesDeleted":["/a/dist/index.js","/a/dist/index.d.ts","/a/tsconfig.tsbuildinfo"]}"#
    );
    assert_eq!(read(&fs, "/a/dist/index.js"), None);
    assert_eq!(
        step(&session, "build", id, Some("/a/tsconfig.json")),
        r#"{"status":0,"statistics":{"Projects":1,"ProjectsBuilt":1,"TimestampUpdates":0}}"#
    );
    assert_eq!(
        step(&session, "cleanReferences", id, Some("/c/tsconfig.json")),
        r#"{"status":0,"statistics":{"Projects":2,"ProjectsBuilt":0,"TimestampUpdates":0},"filesDeleted":["/a/dist/index.js","/a/dist/index.d.ts","/a/tsconfig.tsbuildinfo","/b/dist/index.js","/b/dist/index.d.ts","/b/tsconfig.tsbuildinfo"]}"#
    );
    assert!(read(&fs, "/c/dist/index.js").is_some());
    assert_eq!(
        step(&session, "buildReferences", id, Some("/c/tsconfig.json")),
        r#"{"status":0,"statistics":{"Projects":2,"ProjectsBuilt":2,"TimestampUpdates":0}}"#
    );
    // References of every project, and a project the graph does not have.
    assert_eq!(
        step(&session, "buildReferences", id, None),
        r#"{"status":3,"statistics":{"Projects":0,"ProjectsBuilt":0,"TimestampUpdates":0}}"#
    );
    assert_eq!(
        step(&session, "build", id, Some("/x/tsconfig.json")),
        r#"{"status":3,"statistics":{"Projects":0,"ProjectsBuilt":0,"TimestampUpdates":0}}"#
    );
    assert_eq!(
        step(&session, "cleanBuild", id, Some("/x/tsconfig.json")),
        r#"{"status":3,"statistics":{"Projects":0,"ProjectsBuilt":0,"TimestampUpdates":0}}"#
    );
    assert_eq!(
        step(&session, "cleanBuild", id, None),
        r#"{"status":0,"statistics":{"Projects":3,"ProjectsBuilt":0,"TimestampUpdates":0},"filesDeleted":["/a/dist/index.js","/a/dist/index.d.ts","/a/tsconfig.tsbuildinfo","/b/dist/index.js","/b/dist/index.d.ts","/b/tsconfig.tsbuildinfo","/c/dist/index.js","/c/dist/index.d.ts","/c/tsconfig.tsbuildinfo"]}"#
    );
    // A project named relative to the orchestrator's directory.
    assert_eq!(
        step(&session, "build", id, Some("c")),
        r#"{"status":0,"statistics":{"Projects":3,"ProjectsBuilt":3,"TimestampUpdates":0}}"#
    );
}

#[test]
fn an_unknown_orchestrator_is_an_error() {
    let (session, _) = session(GRAPH);
    let id = create(
        &session,
        json!({ "rootNames": ["/c/tsconfig.json"], "cwd": "/" }),
    );
    let dispose = json!({ "buildOrchestratorID": id });
    assert_eq!(
        call_text(&session, "disposeBuildOrchestrator", &dispose).unwrap(),
        r#"true"#
    );
    assert_eq!(
        call_text(&session, "disposeBuildOrchestrator", &dispose).unwrap_err(),
        "build orchestrator not found while disposing"
    );
    let failure = |method: &str, project: Option<&str>| {
        let params = match project {
            Some(project) => json!({ "buildOrchestratorID": id, "project": project }),
            None => json!({ "buildOrchestratorID": id }),
        };
        call_text(&session, method, &params).unwrap_err()
    };
    assert_eq!(
        failure("build", Some("/c/tsconfig.json")),
        "build orchestrator not found while building /c/tsconfig.json"
    );
    assert_eq!(
        failure("buildReferences", None),
        "build orchestrator not found for building references for "
    );
    assert_eq!(
        failure("cleanBuild", Some("p")),
        "build orchestrator not found while cleaning p"
    );
    assert_eq!(
        failure("cleanReferences", None),
        "build orchestrator not found while cleaning references for "
    );
}

#[test]
fn errors_of_a_build_a_cycle_and_a_missing_configuration() {
    {
        let (session, _) = session(&[
            (
                "/a/tsconfig.json",
                r#"{"compilerOptions":{"composite":true,"noEmitOnError":true,"outDir":"dist","rootDir":"src"},"files":["src/index.ts"]}"#,
            ),
            ("/a/src/index.ts", "export const value: string = 1;"),
        ]);
        let id = create(
            &session,
            json!({ "rootNames": ["/a/tsconfig.json"], "cwd": "/" }),
        );
        // A project with errors reports them again when nothing changed.
        assert_eq!(
            step(&session, "build", id, None),
            r#"{"status":1,"diagnostics":[{"fileName":"/a/src/index.ts","pos":13,"end":18,"startPosition":{"line":0,"character":13},"endPosition":{"line":0,"character":18},"sourceLines":[{"line":0,"text":"export const value: string = 1;"}],"code":2322,"category":1,"text":"Type 'number' is not assignable to type 'string'."}],"statistics":{"Projects":1,"ProjectsBuilt":1,"TimestampUpdates":0}}"#
        );
        assert_eq!(
            step(&session, "build", id, None),
            r#"{"status":1,"diagnostics":[{"fileName":"/a/src/index.ts","pos":13,"end":18,"startPosition":{"line":0,"character":13},"endPosition":{"line":0,"character":18},"sourceLines":[{"line":0,"text":"export const value: string = 1;"}],"code":2322,"category":1,"text":"Type 'number' is not assignable to type 'string'."}],"statistics":{"Projects":1,"ProjectsBuilt":1,"TimestampUpdates":0}}"#
        );
    }
    {
        let (session, _) = session(&[
            (
                "/e/tsconfig.json",
                r#"{"compilerOptions":{"composite":true},"files":["index.ts"],"references":[{"path":"../f"}]}"#,
            ),
            ("/e/index.ts", "export const e = 1;"),
            (
                "/f/tsconfig.json",
                r#"{"compilerOptions":{"composite":true},"files":["index.ts"],"references":[{"path":"../e"}]}"#,
            ),
            ("/f/index.ts", "export const f = 1;"),
        ]);
        let id = create(
            &session,
            json!({ "rootNames": ["/e/tsconfig.json"], "cwd": "/" }),
        );
        assert_eq!(
            step(&session, "cleanBuild", id, None),
            r#"{"status":4,"diagnostics":[{"pos":-1,"end":-1,"code":6202,"category":1,"text":"Project references may not form a circular graph. Cycle detected: /e/tsconfig.json\n/f/tsconfig.json"}],"statistics":{"Projects":0,"ProjectsBuilt":0,"TimestampUpdates":0}}"#
        );
        assert_eq!(
            step(&session, "build", id, None),
            r#"{"status":4,"diagnostics":[{"pos":-1,"end":-1,"code":6202,"category":1,"text":"Project references may not form a circular graph. Cycle detected: /e/tsconfig.json\n/f/tsconfig.json"}],"statistics":{"Projects":0,"ProjectsBuilt":0,"TimestampUpdates":0}}"#
        );
        assert_eq!(
            step(&session, "buildReferences", id, None),
            r#"{"status":4,"diagnostics":[{"pos":-1,"end":-1,"code":6202,"category":1,"text":"Project references may not form a circular graph. Cycle detected: /e/tsconfig.json\n/f/tsconfig.json"}],"statistics":{"Projects":0,"ProjectsBuilt":0,"TimestampUpdates":0}}"#
        );
    }
    {
        // A missing configuration fails a build but not a clean.
        let (session, _) = session(&[]);
        let id = create(
            &session,
            json!({ "rootNames": ["/missing/tsconfig.json"], "cwd": "/" }),
        );
        assert_eq!(
            step(&session, "cleanBuild", id, None),
            r#"{"status":0,"diagnostics":[{"pos":-1,"end":-1,"code":6053,"category":1,"text":"File '/missing/tsconfig.json' not found."}],"statistics":{"Projects":1,"ProjectsBuilt":0,"TimestampUpdates":0}}"#
        );
        assert_eq!(
            step(&session, "build", id, None),
            r#"{"status":1,"diagnostics":[{"pos":-1,"end":-1,"code":6053,"category":1,"text":"File '/missing/tsconfig.json' not found."}],"statistics":{"Projects":1,"ProjectsBuilt":0,"TimestampUpdates":0}}"#
        );
    }
}

#[test]
fn the_options_are_read_under_their_keys() {
    let (session, fs) = session(GRAPH);
    let id = create(
        &session,
        json!({
            "rootNames": ["/a/tsconfig.json"],
            "cwd": "/",
            "compilerOptions": { "removeComments": true },
            "buildOptions": { "verbose": true },
        }),
    );
    assert_eq!(
        step(&session, "build", id, None),
        r#"{"status":0,"statistics":{"Projects":1,"ProjectsBuilt":1,"TimestampUpdates":0}}"#
    );
    assert_eq!(
        read(&fs, "/a/dist/index.js").as_deref(),
        Some("export const a = 1;\n")
    );
    // `buildOptions` sets a dry clean; the options a client spreads over the
    // params are not read.
    let dry = create(
        &session,
        json!({ "rootNames": ["/a/tsconfig.json"], "cwd": "/", "buildOptions": { "dry": true }, "dry": false }),
    );
    assert_eq!(
        step(&session, "cleanBuild", dry, None),
        r#"{"status":0,"statistics":{"Projects":1,"ProjectsBuilt":0,"TimestampUpdates":0},"filesDeleted":["/a/dist/index.js","/a/dist/index.d.ts","/a/tsconfig.tsbuildinfo"]}"#
    );
    assert_eq!(
        read(&fs, "/a/dist/index.js").as_deref(),
        Some("export const a = 1;\n")
    );
    let spread = create(
        &session,
        json!({ "rootNames": ["/a/tsconfig.json"], "cwd": "/", "dry": true, "removeComments": false }),
    );
    assert_eq!(
        step(&session, "cleanBuild", spread, None),
        r#"{"status":0,"statistics":{"Projects":1,"ProjectsBuilt":0,"TimestampUpdates":0},"filesDeleted":["/a/dist/index.js","/a/dist/index.d.ts","/a/tsconfig.tsbuildinfo"]}"#
    );
    assert_eq!(read(&fs, "/a/dist/index.js"), None);
}
