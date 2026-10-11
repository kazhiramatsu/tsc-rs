//! The program's information: its files, their metadata and encodings,
//! its config files, the resolution modes and the resolved modules and type
//! reference directives, and node handles. Pinned to tsgo's responses at
//! 19dadef8 for the same project on disk.

use base64::Engine;

use super::*;
use crate::encoder::{build_node_index_table, HEADER_OFFSET_HASH_LO0, HEADER_OFFSET_PARSE_OPTIONS};

const APP: &[(&str, &str)] = &[
    ("/app/base.json", r#"{ "compilerOptions": { "strict": true } }"#),
    (
        "/app/tsconfig.json",
        r#"{ "extends": "./base.json", "compilerOptions": { "module": "nodenext", "resolveJsonModule": true, "allowJs": true, "types": [] }, "include": ["src"] }"#,
    ),
    ("/app/package.json", r#"{ "name": "app", "type": "module" }"#),
    (
        "/app/src/index.ts",
        "/// <reference types=\"extra\" />\nimport { dep } from \"dep\";\nimport data from \"./data.json\" with { type: \"json\" };\nimport type { T } from \"./types.cjs\";\nimport * as js from \"./helper.js\";\nexport const x = dep + data.a + js.h;\nconst lazy = import(\"dep/sub\");\ndeclare module \"dep\" { export const more: number; }\nexport type U = T;\n",
    ),
    ("/app/src/helper.js", "export const h = 1;\n"),
    ("/app/src/types.cts", "export type T = string;\n"),
    ("/app/src/data.json", r#"{ "a": 1 }"#),
    (
        "/app/node_modules/dep/package.json",
        r#"{ "name": "dep", "version": "1.2.3", "type": "commonjs", "exports": { ".": "./index.js", "./sub": "./sub.js" } }"#,
    ),
    ("/app/node_modules/dep/index.js", "exports.dep = 1;\n"),
    ("/app/node_modules/dep/index.d.ts", "export declare const dep: number;\n"),
    ("/app/node_modules/dep/sub.js", "exports.s = 1;\n"),
    ("/app/node_modules/dep/sub.d.ts", "export declare const s: number;\n"),
    (
        "/app/node_modules/@types/extra/index.d.ts",
        "declare const extraGlobal: number;\n",
    ),
    (
        "/app/node_modules/@types/extra/package.json",
        r#"{ "name": "@types/extra", "version": "0.0.1" }"#,
    ),
];

/// A session with the app's project open: the session, the snapshot and
/// the project.
fn app() -> (Session, Value, Value) {
    let (session, _) = session(APP);
    let response = call(
        &session,
        "createSnapshot",
        json!({ "openProjects": ["/app/tsconfig.json"] }),
    )
    .unwrap();
    let project = response["projects"][0]["id"].clone();
    (session, response["snapshot"].clone(), project)
}

#[test]
fn a_program_lists_its_files_in_order() {
    let (session, snapshot, project) = app();
    let names = call(
        &session,
        "getSourceFileNames",
        json!({ "snapshot": snapshot, "project": project }),
    )
    .unwrap();
    let names = names.as_array().unwrap();
    assert!(names[0].as_str().unwrap().starts_with("bundled:///libs/"));
    assert_eq!(
        names
            .iter()
            .filter_map(Value::as_str)
            .filter(|name| !name.starts_with("bundled:"))
            .collect::<Vec<_>>(),
        [
            "/app/src/helper.js",
            "/app/node_modules/@types/extra/index.d.ts",
            "/app/node_modules/dep/index.d.ts",
            "/app/src/data.json",
            "/app/src/types.cts",
            "/app/node_modules/dep/sub.d.ts",
            "/app/src/index.ts",
        ]
    );
    assert_eq!(
        call(
            &session,
            "getConfigFileNames",
            json!({ "snapshot": snapshot, "project": project }),
        )
        .unwrap(),
        json!(["/app/tsconfig.json", "/app/base.json"])
    );
}

#[test]
fn a_file_has_the_programs_metadata() {
    let (session, snapshot, project) = app();
    let metadata = |file: Value| {
        call(
            &session,
            "getSourceFileMetadata",
            json!({ "snapshot": snapshot, "project": project, "file": file }),
        )
        .unwrap()
    };
    let expected = |library: bool, external: bool, kind: &str, directory: &str, format: u32| {
        json!({
            "isDefaultLibrary": library,
            "isFromExternalLibrary": external,
            "packageJsonType": kind,
            "packageJsonDirectory": directory,
            "impliedNodeFormat": format,
        })
    };
    // Names are relative to the project's directory.
    assert_eq!(
        metadata(json!("src/index.ts")),
        expected(false, false, "module", "/app", 99)
    );
    assert_eq!(
        metadata(json!({ "uri": "file:///app/src/helper.js" })),
        expected(false, false, "module", "/app", 99)
    );
    assert_eq!(
        metadata(json!("src/data.json")),
        expected(false, false, "module", "/app", 0)
    );
    assert_eq!(
        metadata(json!("node_modules/dep/index.d.ts")),
        expected(false, true, "commonjs", "/app/node_modules/dep", 1)
    );
    // An extension that sets the format leaves the package's type out.
    assert_eq!(
        metadata(json!("src/types.cts")),
        expected(false, false, "", "/app", 1)
    );
    assert_eq!(
        metadata(json!("node_modules/@types/extra/index.d.ts")),
        expected(false, true, "", "/app/node_modules/@types/extra", 1)
    );
    assert_eq!(
        metadata(json!("bundled:///libs/lib.es5.d.ts")),
        expected(true, false, "", "", 1)
    );
    assert_eq!(metadata(json!("missing.ts")), Value::Null);
    assert_eq!(metadata(json!("app/src/index.ts")), Value::Null);
}

#[test]
fn a_programs_file_encodes_as_the_parse_cache_holds_it() {
    let (session, snapshot, project) = app();
    let encoded = |file: &str| {
        let response = call(
            &session,
            "getSourceFile",
            json!({ "snapshot": snapshot, "project": project, "file": file }),
        )
        .unwrap();
        response["data"].as_str().map(|data| {
            base64::engine::general_purpose::STANDARD
                .decode(data)
                .unwrap()
        })
    };
    let read = |data: &[u8], offset: usize| {
        u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
    };
    // nodenext forces every file but a declaration file to be a module,
    // a JSON file too, and the parse cache hashed the text.
    for file in ["src/index.ts", "src/helper.js", "src/data.json"] {
        let data = encoded(file).unwrap();
        assert_eq!(read(&data, HEADER_OFFSET_PARSE_OPTIONS), 2, "{file}");
        assert_ne!(read(&data, HEADER_OFFSET_HASH_LO0), 0, "{file}");
    }
    let declaration = encoded("node_modules/dep/index.d.ts").unwrap();
    assert_eq!(read(&declaration, HEADER_OFFSET_PARSE_OPTIONS), 0);
    assert_eq!(encoded("missing.ts"), None);
    // A config source has no hash.
    let config = |file: &str| {
        call(
            &session,
            "getConfigSourceFile",
            json!({ "snapshot": snapshot, "project": project, "file": file }),
        )
        .unwrap()
    };
    for file in ["tsconfig.json", "base.json", "/app/base.json"] {
        let data = base64::engine::general_purpose::STANDARD
            .decode(config(file)["data"].as_str().unwrap())
            .unwrap();
        assert_eq!(read(&data, HEADER_OFFSET_HASH_LO0), 0, "{file}");
    }
    assert_eq!(config("package.json"), Value::Null);
}

#[test]
fn resolution_modes_and_resolutions_answer_as_tsgo_does() {
    let (session, snapshot, project) = app();
    let base = json!({ "snapshot": snapshot, "project": project });
    let with = |extra: Value| {
        let mut params = base.clone();
        params
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        params
    };
    // The imports, the dynamic import and the string module augmentation.
    for index in 0..6 {
        assert_eq!(
            call(
                &session,
                "getModeForResolutionAtIndex",
                with(json!({ "file": "src/index.ts", "index": index })),
            )
            .unwrap(),
            json!(99)
        );
    }
    for index in [-1, 6] {
        assert_eq!(
            call(
                &session,
                "getModeForResolutionAtIndex",
                with(json!({ "file": "src/index.ts", "index": index })),
            )
            .unwrap_err(),
            "api: client error: invalid resolution index"
        );
    }
    let module = |name: &str, mode: u32| {
        call(
            &session,
            "getResolvedModule",
            with(json!({ "file": "src/index.ts", "moduleName": name, "mode": mode })),
        )
        .unwrap()
    };
    assert_eq!(
        module("dep", 99),
        json!({
            "resolvedFileName": "/app/node_modules/dep/index.d.ts",
            "extension": ".d.ts",
            "resolvedUsingTsExtension": false,
            "resolvedUsingExtraExtensions": false,
            "packageId": { "name": "dep", "subModuleName": "index.d.ts", "version": "1.2.3", "peerDependencies": "" },
            "isExternalLibraryImport": true,
        })
    );
    // The program resolved it in its usage's mode only.
    assert_eq!(module("dep", 1), Value::Null);
    assert_eq!(module("dep", 0), Value::Null);
    assert_eq!(
        module("./types.cjs", 99),
        json!({
            "resolvedFileName": "/app/src/types.cts",
            "extension": ".cts",
            "resolvedUsingTsExtension": false,
            "resolvedUsingExtraExtensions": false,
            "isExternalLibraryImport": false,
        })
    );
    assert_eq!(module("nope", 99), Value::Null);
    let extra = json!({
        "primary": false,
        "resolvedFileName": "/app/node_modules/@types/extra/index.d.ts",
        "packageId": { "name": "@types/extra", "subModuleName": "index.d.ts", "version": "0.0.1", "peerDependencies": "" },
        "isExternalLibraryImport": true,
    });
    let directive = |mode: u32| {
        call(
            &session,
            "getResolvedTypeReferenceDirective",
            with(json!({ "file": "src/index.ts", "typeDirectiveName": "extra", "mode": mode })),
        )
        .unwrap()
    };
    assert_eq!(directive(0), Value::Null);
    assert_eq!(directive(99), extra);
    // From a reference, no mode is the file's default.
    assert_eq!(
        call(
            &session,
            "getResolvedTypeReferenceDirectiveFromTypeReferenceDirective",
            with(json!({ "sourceFile": "src/index.ts", "typeDirectiveName": "extra", "resolutionMode": 0 })),
        )
        .unwrap(),
        extra
    );
}

#[test]
fn node_handles_name_a_files_encoded_nodes() {
    let (session, snapshot, project) = app();
    let base = json!({ "snapshot": snapshot, "project": project });
    let with = |extra: Value| {
        let mut params = base.clone();
        params
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        params
    };
    // The index of the `"dep"` specifier among the encoded nodes.
    let (_, program) = session
        .program(
            serde_json::from_value(snapshot.clone()).unwrap(),
            project.as_str().unwrap(),
        )
        .unwrap();
    let file = session.source_file(&program, "src/index.ts").unwrap();
    let source = file.document.source();
    let table = build_node_index_table(source);
    let specifier = (0..table.nodes().len())
        .find(|&index| {
            table.nodes()[index]
                .is_some_and(|node| literal_text(source, node).as_deref() == Some("dep"))
        })
        .unwrap();
    let handle = format!("{specifier}.11./app/src/index.ts");
    assert_eq!(
        call(
            &session,
            "getModeForUsageLocation",
            with(json!({ "file": "src/index.ts", "usage": handle })),
        )
        .unwrap(),
        json!(99)
    );
    assert_eq!(
        call(
            &session,
            "getResolvedModuleFromModuleSpecifier",
            with(json!({ "moduleSpecifier": handle })),
        )
        .unwrap()["resolvedFileName"],
        "/app/node_modules/dep/index.d.ts"
    );
    for (handle, error) in [
        ("bad", r#"invalid node handle "bad""#),
        ("1.x", r#"invalid node handle "1.x""#),
        (
            "x.1./app/src/index.ts",
            r#"invalid node handle "x.1./app/src/index.ts": strconv.ParseUint: parsing "x": invalid syntax"#,
        ),
        (
            "99999999999.1./app/src/index.ts",
            r#"invalid node handle "99999999999.1./app/src/index.ts": strconv.ParseUint: parsing "99999999999": value out of range"#,
        ),
        (
            "1.1./app/src/missing.ts",
            r#"node handle "1.1./app/src/missing.ts" could not be resolved (file may not be loaded or handle may be stale)"#,
        ),
    ] {
        assert_eq!(
            call(
                &session,
                "getResolvedModuleFromModuleSpecifier",
                with(json!({ "moduleSpecifier": handle })),
            )
            .unwrap_err(),
            format!("api: client error: {error}")
        );
    }
    // The source file node is not a module specifier.
    assert_eq!(
        call(
            &session,
            "getModeForUsageLocation",
            with(json!({ "file": "src/index.ts", "usage": "1.308./app/src/index.ts" })),
        )
        .unwrap_err(),
        "api: client error: usage must be a StringLiteralLike node"
    );
}

#[test]
fn a_missing_project_or_file_is_a_client_error() {
    let (session, snapshot, project) = app();
    assert_eq!(
        call(
            &session,
            "getSourceFile",
            json!({ "snapshot": snapshot, "project": "nope", "file": "x.ts" }),
        )
        .unwrap_err(),
        "api: client error: project nope not found"
    );
    assert_eq!(
        call(
            &session,
            "getSourceFile",
            json!({ "snapshot": 999, "project": project, "file": "x.ts" }),
        )
        .unwrap_err(),
        "api: client error: snapshot 999 not found"
    );
    assert_eq!(
        call(
            &session,
            "getResolvedModule",
            json!({ "snapshot": snapshot, "project": project, "file": "app/src/index.ts", "moduleName": "dep" }),
        )
        .unwrap_err(),
        "api: client error: source file not found: app/src/index.ts"
    );
    // A program without a config has no config files.
    let synthetic = call(
        &session,
        "updateSnapshot",
        json!({ "snapshot": snapshot, "changes": { "createPrograms": [{ "rootFiles": ["/app/src/helper.js"], "compilerOptions": { "allowJs": true } }] } }),
    )
    .unwrap();
    let synthetic_project = synthetic["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|project| project["configFileName"] == "")
        .unwrap()["id"]
        .clone();
    assert_eq!(
        call(
            &session,
            "getConfigFileNames",
            json!({ "snapshot": synthetic["snapshot"], "project": synthetic_project }),
        )
        .unwrap(),
        json!([])
    );
}

#[test]
fn a_host_link_into_node_modules_rebuilds_the_program() {
    // The client's `Snapshot.update host symlinks bypass an inherited full
    // filesystem`: the program looked for the missing node_modules, so a
    // link created there rebuilds it.
    let (session, _) = session(&[(
        "/host/node_modules/pkg/index.d.ts",
        "export declare const value: string;",
    )]);
    let created = call(
        &session,
        "createSnapshot",
        json!({
            "openProjects": ["/project/tsconfig.json"],
            "fileSystem": { "kind": "full", "files": {
                "/project/tsconfig.json": r#"{ "compilerOptions": { "noLib": true, "moduleResolution": "node" }, "files": ["index.ts"] }"#,
                "/project/index.ts": r#"import { value } from "pkg"; export { value };"#,
            } },
        }),
    )
    .unwrap();
    let updated = call(
        &session,
        "updateSnapshot",
        json!({ "snapshot": created["snapshot"], "changes": {
            "ensurePrograms": true,
            "fileSystem": { "kind": "layer", "symlinks": {
                "/project/node_modules": { "target": "/host/node_modules", "host": true },
            } },
        } }),
    )
    .unwrap();
    assert_eq!(ids(&updated), ["/project/tsconfig.json"]);
    assert_eq!(
        call(
            &session,
            "getSourceFileNames",
            json!({ "snapshot": updated["snapshot"], "project": "/project/tsconfig.json" }),
        )
        .unwrap(),
        json!(["/host/node_modules/pkg/index.d.ts", "/project/index.ts"])
    );
    assert_eq!(
        call(
            &session,
            "getSourceFileMetadata",
            json!({ "snapshot": updated["snapshot"], "project": "/project/tsconfig.json", "file": "/host/node_modules/pkg/index.d.ts" }),
        )
        .unwrap()["isFromExternalLibrary"],
        true
    );
}
