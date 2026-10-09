//! The requests that need no project: the command line and config requests,
//! a client's source files (tsgo `session_createsourcefile_test.go`) and
//! transpilation, pinned to tsgo's responses at 19dadef8.

use base64::Engine;

use super::*;
use crate::encoder::{
    kind_name, HEADER_OFFSET_EXTENDED_DATA, HEADER_OFFSET_NODES, HEADER_OFFSET_SOURCE_FILE_LEASE,
    HEADER_OFFSET_STRING_DATA, HEADER_OFFSET_STRING_OFFSETS, NODE_DATA_STRING_INDEX_MASK,
    NODE_OFFSET_DATA, NODE_OFFSET_FLAGS, NODE_OFFSET_KIND, NODE_SIZE,
};

/// What a test reads of an encoded source file: its lease, the source
/// file's names, text and script kind, and each node's kind and flags.
struct EncodedFile {
    lease: u64,
    file_name: String,
    path: String,
    text: String,
    language_variant: u32,
    script_kind: u32,
    nodes: Vec<(&'static str, u32)>,
}

impl EncodedFile {
    fn new(response: &Value) -> Self {
        let data = base64::engine::general_purpose::STANDARD
            .decode(response["data"].as_str().expect("a source file response"))
            .unwrap();
        let read = |offset: usize| u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
        let string = |index: u32| {
            let offsets = read(HEADER_OFFSET_STRING_OFFSETS) as usize + index as usize * 4;
            let strings = read(HEADER_OFFSET_STRING_DATA) as usize;
            String::from_utf8(
                data[strings + read(offsets) as usize..strings + read(offsets + 4) as usize]
                    .to_vec(),
            )
            .unwrap()
        };
        let nodes = read(HEADER_OFFSET_NODES) as usize;
        // The root's data is the offset of its extended data: the text,
        // file name and path strings, the language variant and the script
        // kind.
        let root = read(nodes + NODE_SIZE + NODE_OFFSET_DATA) & NODE_DATA_STRING_INDEX_MASK;
        let extended = read(HEADER_OFFSET_EXTENDED_DATA) as usize + root as usize;
        Self {
            lease: u64::from_le_bytes(
                data[HEADER_OFFSET_SOURCE_FILE_LEASE..HEADER_OFFSET_SOURCE_FILE_LEASE + 8]
                    .try_into()
                    .unwrap(),
            ),
            text: string(read(extended)),
            file_name: string(read(extended + 4)),
            path: string(read(extended + 8)),
            language_variant: read(extended + 12),
            script_kind: read(extended + 16),
            nodes: (nodes + NODE_SIZE..data.len())
                .step_by(NODE_SIZE)
                .map(|record| {
                    (
                        kind_name(read(record + NODE_OFFSET_KIND)),
                        read(record + NODE_OFFSET_FLAGS),
                    )
                })
                .collect(),
        }
    }

    fn count(&self, kind: &str) -> usize {
        self.nodes.iter().filter(|(name, _)| *name == kind).count()
    }

    fn flags_of(&self, kind: &str) -> Vec<u32> {
        self.nodes
            .iter()
            .filter(|(name, _)| *name == kind)
            .map(|(_, flags)| *flags)
            .collect()
    }
}

fn create(session: &Session, file_name: &str, text: &str, script_kind: u32) -> EncodedFile {
    EncodedFile::new(
        &call(
            session,
            "createSourceFile",
            json!({
                "fileName": file_name,
                "sourceText": text,
                "options": { "scriptKind": script_kind },
            }),
        )
        .unwrap(),
    )
}

#[test]
fn a_created_source_file_is_parsed_and_bound() {
    // TestCreateSourceFile/text.
    let (session, _) = session(&[]);
    let file = create(
        &session,
        "src/input.tsx",
        "export const element = <div />;",
        0,
    );
    assert_eq!(file.file_name, "/src/input.tsx");
    assert_eq!(file.path, "/src/input.tsx");
    assert_eq!(file.text, "export const element = <div />;");
    assert_eq!(file.script_kind, 4);
    assert_eq!(file.count("KindVariableStatement"), 1);
    assert_eq!(file.count("KindJsxSelfClosingElement"), 1);
    // The binder's flags (tsgo's encoding of the same text): `this` in a
    // function, its implicit and explicit returns, the source file's async
    // functions and an unreachable statement.
    let file = create(
        &session,
        "/a.ts",
        "function f() { this; return 1; }\nfunction g(x) { if (x) return 1; }\nexport const h = async () => { await 0; };\nwhile (true) {} let dead = 1;\n",
        0,
    );
    assert_eq!(file.flags_of("KindSourceFile"), [0x40000]);
    assert_eq!(file.flags_of("KindFunctionDeclaration"), [0x80, 0x300]);
    assert_eq!(file.flags_of("KindArrowFunction"), [0x100]);
    assert_eq!(
        file.flags_of("KindVariableStatement")
            .last()
            .copied()
            .map(|flags| flags & 0x800_0000),
        Some(0x800_0000)
    );
    // The path is the case-folded file name of a case-insensitive host.
    let file = create(&session, "/Src/Input.ts", "", 0);
    assert_eq!(file.file_name, "/Src/Input.ts");
    assert_eq!(file.path, "/src/input.ts");
}

#[test]
fn a_script_kind_overrides_the_extension() {
    // TestCreateSourceFile/script kind override and unknown extension.
    let (session, _) = session(&[]);
    let file = create(
        &session,
        "/src/component.txt",
        "export const element = <div />;",
        4,
    );
    assert_eq!(file.script_kind, 4);
    assert_eq!(file.count("KindJsxSelfClosingElement"), 1);
    let file = create(
        &session,
        "/src/component.txt",
        r#"export const value: string = "ok";"#,
        0,
    );
    assert_eq!(file.script_kind, 3);
    assert_eq!(file.count("KindStringKeyword"), 1);
    // JSON is the JSX variant, as tsgo's parser has every kind but TS.
    let file = create(&session, "/data.json", r#"{"a": [1]}"#, 0);
    assert_eq!((file.script_kind, file.language_variant), (6, 1));
    assert_eq!(file.count("KindObjectLiteralExpression"), 1);
}

#[test]
fn an_invalid_script_kind_is_a_client_error() {
    // TestCreateSourceFile/invalid script kind.
    let (session, _) = session(&[]);
    for kind in [5, 7, 999] {
        assert_eq!(
            call(
                &session,
                "createSourceFile",
                json!({ "fileName": "/src/input.ts", "sourceText": "", "options": { "scriptKind": kind } }),
            )
            .unwrap_err(),
            format!("api: client error: invalid scriptKind {kind}")
        );
    }
}

#[test]
fn a_source_file_reads_its_file() {
    // TestCreateSourceFile/from file and missing file.
    let (session, _) = session(&[("/src/input.ts", "export const fromFile = 1;")]);
    let file = EncodedFile::new(
        &call(
            &session,
            "createSourceFileFromFile",
            json!({ "fileName": "src/input.ts" }),
        )
        .unwrap(),
    );
    assert_eq!(file.file_name, "/src/input.ts");
    assert_eq!(file.text, "export const fromFile = 1;");
    assert_eq!(
        call(
            &session,
            "createSourceFileFromFile",
            json!({ "fileName": "/src/missing.ts" }),
        )
        .unwrap_err(),
        r#"api: client error: could not read file "/src/missing.ts""#
    );
}

#[test]
fn leases_are_released_once() {
    // TestCreateSourceFile/lease release.
    let (session, _) = session(&[]);
    let first = create(&session, "/src/lease-1.ts", "export {};", 0);
    let second = create(&session, "/src/lease-2.ts", "export {};", 0);
    assert_eq!((first.lease, second.lease), (1, 2));
    // A failed creation takes no lease.
    call(
        &session,
        "createSourceFile",
        json!({ "fileName": "/x.ts", "sourceText": "", "options": { "scriptKind": 7 } }),
    )
    .unwrap_err();
    assert_eq!(create(&session, "/src/lease-3.ts", "", 0).lease, 3);
    let release =
        |lease: Value| call_text(&session, "releaseSourceFile", &json!({ "lease": lease }));
    assert_eq!(release(json!(1)).unwrap(), "true");
    assert_eq!(
        release(json!(1)).unwrap_err(),
        "api: client error: source file lease 1 not found"
    );
    assert_eq!(release(json!(2)).unwrap(), "true");
    assert_eq!(
        release(json!(0)).unwrap_err(),
        "api: client error: empty source file lease"
    );
    assert_eq!(
        call_text(&session, "releaseSourceFile", &Value::Null).unwrap_err(),
        "api: client error: empty source file lease"
    );
}

#[test]
fn a_command_line_parses_as_tsgo_parses_it() {
    let (session, _) = session(&[("/args.txt", "--strict\n--target es2017\nresp.ts\n")]);
    let parse = |command_line: Value| {
        call_text(
            &session,
            "parseCommandLine",
            &json!({ "commandLine": command_line }),
        )
        .unwrap()
    };
    assert_eq!(
        parse(json!([])),
        r#"{"fileNames":[],"options":{},"errors":[]}"#
    );
    assert_eq!(
        parse(json!([
            "--strict",
            "a.ts",
            "--target",
            "es2020",
            "--lib",
            "es2020,dom",
            "b.ts"
        ])),
        r#"{"fileNames":["a.ts","b.ts"],"options":{"lib":["lib.es2020.d.ts","lib.dom.d.ts"],"strict":true,"target":7},"raw":{"strict":true,"target":7,"lib":["lib.es2020.d.ts","lib.dom.d.ts"]},"errors":[]}"#
    );
    // The command's own options are compiler options too; the raw values
    // are as written, a watch option's enum numbered as TypeScript's.
    assert_eq!(
        parse(json!([
            "-p",
            "proj",
            "--watch",
            "--watchFile",
            "useFsEvents",
            "--excludeDirectories",
            "a,b"
        ])),
        r#"{"fileNames":[],"options":{"project":"/proj","watch":true},"raw":{"project":"proj","watch":true,"watchFile":4,"excludeDirectories":["a","b"]},"errors":[]}"#
    );
    assert_eq!(
        parse(json!(["@args.txt", "--declaration"])),
        r#"{"fileNames":["resp.ts"],"options":{"declaration":true,"strict":true,"target":4},"raw":{"strict":true,"target":4,"declaration":true},"errors":[]}"#
    );
    // An unknown enum value is null, and the merge over a config's options
    // takes it as an explicit null.
    let response: Value = serde_json::from_str(&parse(json!([
        "--unknown",
        "--target",
        "nope",
        "x.ts",
        "--noEmit",
        "false"
    ])))
    .unwrap();
    assert_eq!(response["options"], json!({ "noEmit": false }));
    assert_eq!(response["raw"], json!({ "target": null, "noEmit": false }));
    assert_eq!(
        response["errors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|error| (
                error["code"].as_u64().unwrap(),
                error["pos"].as_i64().unwrap()
            ))
            .collect::<Vec<_>>(),
        [(5023, -1), (6046, -1)]
    );
    assert_eq!(
        parse(json!(["@missing.txt"])),
        r#"{"fileNames":[],"options":{},"errors":[{"pos":-1,"end":-1,"code":5083,"category":1,"text":"Cannot read file '/missing.txt'."}]}"#
    );
}

#[test]
fn a_config_file_reads_as_tsgo_reads_it() {
    let (session, _) = session(&[
        (
            "/proj/tsconfig.json",
            r#"{ "compilerOptions": { "strict": true, "target": "es2020" }, "compileOnSave": true }"#,
        ),
        (
            "/syntax/tsconfig.json",
            "{ \"compilerOptions\": { \"strict\": true,, }, \"files\": [\"a.ts\" }\n",
        ),
        ("/proj/a.ts", ""),
    ]);
    let read =
        |file: &str| call_text(&session, "readConfigFile", &json!({ "file": file })).unwrap();
    assert_eq!(
        read("proj/tsconfig.json"),
        r#"{"config":{"compilerOptions":{"strict":true,"target":"es2020"},"compileOnSave":true}}"#
    );
    assert_eq!(
        read("/missing.json"),
        r#"{"config":{},"error":{"pos":-1,"end":-1,"code":5083,"category":1,"text":"Cannot read file '/missing.json'."}}"#
    );
    let syntax: Value = serde_json::from_str(&read("/syntax/tsconfig.json")).unwrap();
    assert_eq!(
        syntax["config"],
        json!({ "compilerOptions": { "strict": true }, "files": ["a.ts"] })
    );
    assert_eq!(
        (
            &syntax["error"]["pos"],
            &syntax["error"]["end"],
            &syntax["error"]["code"]
        ),
        (&json!(38), &json!(39), &json!(1136))
    );

    let parse = |file: &str| call(&session, "parseConfigFile", json!({ "file": file }));
    assert_eq!(
        parse("/proj/tsconfig.json").unwrap(),
        json!({
            "fileNames": ["/proj/a.ts"],
            "options": { "strict": true, "target": 7, "configFilePath": "/proj/tsconfig.json" },
            "compileOnSave": true,
            "raw": { "compilerOptions": { "strict": true, "target": "es2020" }, "compileOnSave": true },
            "errors": [],
        })
    );
    assert_eq!(
        parse("/missing.json").unwrap_err(),
        r#"api: client error: could not read file "/missing.json""#
    );
}

#[test]
fn a_config_value_parses_in_its_directory_or_as_its_file() {
    let (session, _) = session(&[("/proj/a.ts", ""), ("/empty/.keep", "")]);
    let parse = |params: Value| call(&session, "parseJsonConfigFileContent", params);
    // A directory: no config file path.
    assert_eq!(
        parse(
            json!({ "json": { "compilerOptions": { "strict": true } }, "configDirectory": "proj" })
        )
        .unwrap(),
        json!({
            "fileNames": ["/proj/a.ts"],
            "options": { "strict": true },
            "compileOnSave": false,
            "raw": { "compilerOptions": { "strict": true } },
            "errors": [],
        })
    );
    // A file name: its path, and its name in the messages.
    let named = parse(json!({ "json": {}, "configFileName": "/empty/tsconfig.json" })).unwrap();
    assert_eq!(
        named["options"],
        json!({ "configFilePath": "/empty/tsconfig.json" })
    );
    assert_eq!(
        named["errors"][0]["text"],
        r#"No inputs were found in config file '/empty/tsconfig.json'. Specified 'include' paths were '["**/*"]' and 'exclude' paths were '[]'."#
    );
    let unnamed = parse(json!({ "json": {}, "configDirectory": "/empty" })).unwrap();
    assert_eq!(unnamed["options"], json!({}));
    assert_eq!(
        unnamed["errors"][0],
        json!({
            "pos": -1, "end": -1, "code": 18003, "category": 1,
            "text": r#"No inputs were found in config file ''. Specified 'include' paths were '["**/*"]' and 'exclude' paths were '[]'."#,
        })
    );
    // A value that is not an object is an empty one; the errors have no
    // location.
    assert_eq!(
        parse(json!({ "json": [1, 2], "configDirectory": "proj" })).unwrap()["fileNames"],
        json!(["/proj/a.ts"])
    );
    let invalid = parse(json!({
        "json": { "compilerOptions": { "unknownOption": 1 }, "files": 3 },
        "configDirectory": "proj",
    }))
    .unwrap();
    assert_eq!(
        invalid["errors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|error| (
                error["code"].as_u64().unwrap(),
                error["pos"].as_i64().unwrap(),
                error.get("fileName").is_some()
            ))
            .collect::<Vec<_>>(),
        [(5023, -1, false), (5024, -1, false)]
    );
    for params in [
        json!({ "json": {} }),
        json!({ "json": {}, "configDirectory": "proj", "configFileName": "proj/tsconfig.json" }),
    ] {
        assert_eq!(
            parse(params).unwrap_err(),
            "api: client error: exactly one of configDirectory or configFileName is required"
        );
    }
}

#[test]
fn transpilation_answers_as_tsgo_does() {
    let (session, _) = session(&[(
        "/src/file.ts",
        "export function f(a: number): string { return `${a}`; }\n",
    )]);
    let transpile = |method: &str, params: Value| call(&session, method, params).unwrap();
    assert_eq!(
        transpile(
            "transpileModule",
            json!({ "input": "export const x: number = 1;\n", "options": {} })
        ),
        json!({ "outputText": "export const x = 1;\n" })
    );
    let module = transpile(
        "transpileModule",
        json!({
            "input": "import a from \"a\";\nexport default a as number;\nlet y = ;",
            "options": { "compilerOptions": { "module": 1, "sourceMap": true }, "fileName": "m.ts", "reportDiagnostics": true },
        }),
    );
    assert!(module["outputText"]
        .as_str()
        .unwrap()
        .ends_with("let y = ;\n//# sourceMappingURL=m.js.map"));
    assert_eq!(
        (
            &module["diagnostics"][0]["fileName"],
            &module["diagnostics"][0]["code"],
            &module["diagnostics"][0]["sourceLines"]
        ),
        (
            &json!("/m.ts"),
            &json!(1109),
            &json!([{ "line": 2, "text": "let y = ;" }])
        )
    );
    assert!(module["sourceMapText"]
        .as_str()
        .unwrap()
        .starts_with(r#"{"version":3,"file":"m.js""#));
    // tsgo numbers `newLine` 1 for CRLF.
    assert_eq!(
        transpile(
            "transpileModule",
            json!({ "input": "let a = 1;\nlet b = 2;\n", "options": { "compilerOptions": { "newLine": 1 } } })
        ),
        json!({ "outputText": "\"use strict\";\r\nlet a = 1;\r\nlet b = 2;\r\n" })
    );
    assert_eq!(
        transpile(
            "transpileDeclarationFromFile",
            json!({ "fileName": "src/file.ts", "options": {} })
        ),
        json!({ "outputText": "export declare function f(a: number): string;\n" })
    );
    assert_eq!(
        call(
            &session,
            "transpileModuleFromFile",
            json!({ "fileName": "/src/missing.ts", "options": {} })
        )
        .unwrap_err(),
        r#"api: client error: could not read file "/src/missing.ts""#
    );
}
