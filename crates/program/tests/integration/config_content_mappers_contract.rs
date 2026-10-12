//! A config's `contentMappers`: tsgo tsconfigparsing_test.go's content
//! mapper cases (through both of its APIs: a JSON object, and the config's
//! source file), contentmappers_test.go's mapper lookup, and the
//! diagnostics the tsgo binary at the vendored commit prints for configs
//! with every kind of invalid entry (`tsc -p . [--runExternalCode]`).

use tsc_diagnostics::{JsStr, JsString};
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    command_line_option_bag, parse_config_root_plan_of_json_object,
    parse_config_root_plan_with_command_line, CompilerConfigHost, ConfigExtendedCache,
    ConfigRootPlan, ConfigRootPlanRequest, JsonValue,
};

#[derive(Clone, Copy, Debug)]
enum Api {
    Json,
    JsonSourceFile,
}

const APIS: [Api; 2] = [Api::Json, Api::JsonSourceFile];

fn host(files: &[(&str, &str)], case_sensitive: bool) -> MemoryCompilerHost {
    files
        .iter()
        .fold(
            MemoryCompilerHost::builder("/").case_sensitive(case_sensitive),
            |builder, (path, text)| builder.file(path, text.as_bytes()),
        )
        .build()
        .unwrap()
}

/// The plan of `<base>/tsconfig.json`, with `--runExternalCode` when
/// `run_external_code`.
fn plan_with(
    host: &MemoryCompilerHost,
    api: Api,
    base_path: &str,
    run_external_code: bool,
) -> ConfigRootPlan {
    let config_file_name = format!("{}/tsconfig.json", base_path.trim_end_matches('/'));
    let text = String::from_utf8(
        tsc_host::CompilerHost::read_file(host, std::path::Path::new(&config_file_name))
            .unwrap()
            .expect("the config exists"),
    )
    .unwrap();
    let options = command_line_option_bag(
        &run_external_code
            .then(|| ("runExternalCode".to_owned(), JsonValue::Bool(true)))
            .into_iter()
            .collect::<Vec<_>>(),
        JsStr::from(base_path),
    );
    let request = ConfigRootPlanRequest {
        file_name: JsString::from(config_file_name.as_str()),
        text,
        base_path: JsString::from(base_path),
    };
    let host = CompilerConfigHost::new(host);
    match api {
        Api::Json => parse_config_root_plan_of_json_object(&host, request, Some(&options)),
        Api::JsonSourceFile => parse_config_root_plan_with_command_line(
            &host,
            request,
            &options,
            &mut ConfigExtendedCache::default(),
        ),
    }
    .unwrap()
}

fn plan(host: &MemoryCompilerHost, api: Api, base_path: &str) -> ConfigRootPlan {
    plan_with(host, api, base_path, true)
}

fn codes(plan: &ConfigRootPlan) -> Vec<u32> {
    plan.errors().iter().map(|error| error.code()).collect()
}

fn extensions(plan: &ConfigRootPlan) -> Vec<&str> {
    plan.content_mapper_extensions().collect()
}

/// tsgo `TestContentMappers`.
#[test]
fn content_mappers_are_parsed_resolved_and_their_files_listed() {
    let host = host(
        &[
            (
                "/tsconfig.json",
                r#"{
                    "contentMappers": [
                        { "package": "vue-mapper", "extensions": [".vue"], "options": { "strictTemplates": true } }
                    ],
                    "include": ["src"]
                }"#,
            ),
            ("/src/app.ts", "export {}"),
            ("/src/Component.vue", "<template></template>"),
            (
                "/node_modules/vue-mapper/package.json",
                r#"{ "name": "vue-mapper", "version": "1.2.3", "typescript": { "contentMapper": { "exec": ["node", "./mapper.js"], "dynamicConfig": true } } }"#,
            ),
        ],
        true,
    );
    for api in APIS {
        let plan = plan(&host, api, "/");
        assert!(plan.errors().is_empty(), "{api:?}: {:?}", plan.errors());
        let [mapper] = plan.content_mappers() else {
            panic!("{api:?}: one mapper");
        };
        assert_eq!(mapper.definition.package, "vue-mapper");
        assert_eq!(
            mapper.definition.extensions.as_deref(),
            Some(&[".vue".to_owned()][..])
        );
        assert_eq!(
            mapper.definition.options.as_deref(),
            Some(r#"{"strictTemplates":true}"#)
        );
        assert_eq!(extensions(&plan), [".vue"]);
        // The package.json is resolved while parsing.
        assert_eq!(mapper.manifest.name, "vue-mapper");
        assert_eq!(mapper.manifest.version, "1.2.3");
        assert_eq!(
            mapper.manifest.exec.as_deref(),
            Some(&["node".to_owned(), "./mapper.js".to_owned()][..])
        );
        assert!(mapper.manifest.dynamic_config);
        assert_eq!(mapper.package_directory, "/node_modules/vue-mapper");
        // The registered extension is listed by the include glob (in the
        // host's listing order).
        let mut file_names = plan
            .file_names()
            .iter()
            .map(|name| name.as_str().unwrap())
            .collect::<Vec<_>>();
        file_names.sort_unstable();
        assert_eq!(file_names, ["/src/Component.vue", "/src/app.ts"], "{api:?}");
    }
}

/// tsgo `TestContentMapperOptionDiagnosticLocation`.
#[test]
fn an_option_diagnostic_points_into_the_mapper_s_options() {
    let text = r#"{
        "contentMappers": [{
            "package": "mapper",
            "extensions": [".vue"],
            "options": { "plugins": [{ "name": 1 }] }
        }]
    }"#;
    let host = host(
        &[
            ("/tsconfig.json", text),
            ("/index.ts", "export {};"),
            (
                "/node_modules/mapper/package.json",
                r#"{ "name": "mapper", "version": "1.0.0", "typescript": { "contentMapper": { "exec": ["mapper"] } } }"#,
            ),
        ],
        true,
    );
    let plan = plan(&host, Api::JsonSourceFile, "/");
    use tsc_program::content_mapper::host::OptionPathSegment;
    let span = plan
        .content_mapper_option_diagnostic_location(
            &plan.content_mappers()[0],
            &[
                OptionPathSegment::Property("plugins".to_owned()),
                OptionPathSegment::Index(0),
                OptionPathSegment::Property("name".to_owned()),
            ],
        )
        .unwrap();
    let units = text.encode_utf16().collect::<Vec<_>>();
    let start = span.start() as usize;
    assert_eq!(
        String::from_utf16(&units[start..start + span.length() as usize]).unwrap(),
        "1"
    );
    // A JSON object config has no syntax to point at.
    let plan = self::plan(&host, Api::Json, "/");
    assert!(plan
        .content_mapper_option_diagnostic_location(&plan.content_mappers()[0], &[])
        .is_none());
}

/// tsgo `TestContentMappersAreInheritedFromExtendedConfig`.
#[test]
fn content_mappers_are_inherited_from_an_extended_config() {
    let host = host(
        &[
            ("/project/tsconfig.json", r#"{ "extends": "./base.json" }"#),
            (
                "/project/base.json",
                r#"{ "contentMappers": [{ "package": "vue-mapper", "extensions": [".vue"] }], "include": ["src"] }"#,
            ),
            ("/project/src/index.ts", "export {};"),
            ("/project/src/component.vue", "<template></template>"),
            (
                "/project/node_modules/vue-mapper/package.json",
                r#"{ "name": "vue-mapper", "version": "1.2.3", "typescript": { "contentMapper": { "exec": ["node", "./mapper.js"] } } }"#,
            ),
        ],
        true,
    );
    for api in APIS {
        let plan = plan(&host, api, "/project");
        assert!(plan.errors().is_empty(), "{api:?}: {:?}", plan.errors());
        assert_eq!(plan.content_mappers().len(), 1, "{api:?}");
        assert_eq!(plan.content_mappers()[0].definition.package, "vue-mapper");
        assert_eq!(extensions(&plan), [".vue"]);
        assert!(plan
            .file_names()
            .iter()
            .any(|name| name.as_str() == Some("/project/src/component.vue")));
    }
}

/// tsgo `TestContentMappersRequireFlag` and
/// `TestUnresolvedContentMapperDoesNotRegisterExtensions`.
#[test]
fn mappers_without_the_flag_or_a_package_register_nothing() {
    let host = host(
        &[
            (
                "/tsconfig.json",
                r#"{ "contentMappers": [{ "package": "vue-mapper", "extensions": [".vue"] }], "include": ["src"] }"#,
            ),
            ("/src/app.ts", "export {}"),
            ("/src/Component.vue", "<template />"),
        ],
        true,
    );
    for api in APIS {
        let without_flag = plan_with(&host, api, "/", false);
        assert_eq!(codes(&without_flag), [18068], "{api:?}");
        assert!(without_flag.content_mappers().is_empty());

        let unresolved = plan(&host, api, "/");
        assert_eq!(codes(&unresolved), [18075], "{api:?}");
        assert!(unresolved.content_mappers().is_empty());
        assert_eq!(extensions(&unresolved), [] as [&str; 0]);
        let file_names = unresolved
            .file_names()
            .iter()
            .map(|name| name.as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(file_names, ["/src/app.ts"], "{api:?}");
    }
}

/// tsgo `TestContentMappersValidation`.
#[test]
fn invalid_entries_are_reported_and_dropped() {
    let cases = [
        (
            "extension without leading dot",
            r#"[{ "package": "vue-mapper", "extensions": ["vue"] }]"#,
            18065,
        ),
        (
            "built-in extension",
            r#"[{ "package": "x", "extensions": [".ts"] }]"#,
            18066,
        ),
        ("missing extensions", r#"[{ "package": "x" }]"#, 5024),
        (
            "duplicate extension across mappers",
            r#"[{ "package": "a", "extensions": [".vue"] }, { "package": "b", "extensions": [".vue"] }]"#,
            18067,
        ),
        (
            "extensions is not an array",
            r#"[{ "package": "x", "extensions": ".vue" }]"#,
            5024,
        ),
        (
            "extensions contains a non-string",
            r#"[{ "package": "x", "extensions": [".vue", 1] }]"#,
            5024,
        ),
        (
            "package is not a string",
            r#"[{ "package": ["x"], "extensions": [".vue"] }]"#,
            5024,
        ),
        ("missing package", r#"[{ "extensions": [".vue"] }]"#, 5024),
        (
            "options is not an object",
            r#"[{ "package": "x", "extensions": [".vue"], "options": ["strict"] }]"#,
            5024,
        ),
    ];
    for (name, content_mappers, code) in cases {
        let config = format!(r#"{{ "contentMappers": {content_mappers} }}"#);
        let host = host(
            &[
                ("/tsconfig.json", &config),
                ("/app.ts", "export {}"),
                (
                    "/node_modules/a/package.json",
                    r#"{ "name": "a", "version": "1.0.0", "typescript": { "contentMapper": { "exec": ["a"] } } }"#,
                ),
                (
                    "/node_modules/b/package.json",
                    r#"{ "name": "b", "version": "1.0.0", "typescript": { "contentMapper": { "exec": ["b"] } } }"#,
                ),
            ],
            true,
        );
        for api in APIS {
            let plan = plan(&host, api, "/");
            let diagnostic = plan
                .errors()
                .iter()
                .find(|error| error.code() == code)
                .unwrap_or_else(|| panic!("{name} ({api:?}): {:?}", plan.errors()));
            match name {
                "built-in extension" => assert_eq!(extensions(&plan), [] as [&str; 0]),
                "duplicate extension across mappers" => {
                    let mappers = plan.content_mappers();
                    assert_eq!(mappers.len(), 2);
                    assert_eq!(
                        mappers[0].definition.extensions.as_deref(),
                        Some(&[".vue".to_owned()][..])
                    );
                    assert_eq!(mappers[1].definition.extensions.as_deref(), Some(&[][..]));
                    assert_eq!(extensions(&plan), [".vue"]);
                }
                "extension without leading dot" => {}
                _ => assert!(plan.content_mappers().is_empty(), "{name}"),
            }
            // From the config's source file a diagnostic points at the
            // offending syntax.
            if let Api::JsonSourceFile = api {
                assert!(diagnostic.file_name.is_some(), "{name}");
                assert!(diagnostic.length.is_some_and(|length| length > 0), "{name}");
            }
        }
    }
}

/// tsgo `TestContentMapperExtensionValidationUsesHostCaseSensitivity`.
#[test]
fn extension_checks_follow_the_host_case_sensitivity() {
    let cases = [
        (
            false,
            r#"[{ "package": "mapper", "extensions": [".TS"] }]"#,
            Some(18066),
        ),
        (
            false,
            r#"[{ "package": "a", "extensions": [".vue"] }, { "package": "b", "extensions": [".VUE"] }]"#,
            Some(18067),
        ),
        (
            true,
            r#"[{ "package": "mapper", "extensions": [".TS"] }]"#,
            Some(18066),
        ),
        (
            true,
            r#"[{ "package": "a", "extensions": [".vue"] }, { "package": "b", "extensions": [".VUE"] }]"#,
            None,
        ),
    ];
    for (case_sensitive, content_mappers, code) in cases {
        let config = format!(r#"{{ "contentMappers": {content_mappers} }}"#);
        let host = host(
            &[
                ("/tsconfig.json", &config),
                ("/app.ts", "export {};"),
                (
                    "/node_modules/mapper/package.json",
                    r#"{ "name": "mapper", "version": "1.0.0", "typescript": { "contentMapper": { "exec": ["mapper"] } } }"#,
                ),
                (
                    "/node_modules/a/package.json",
                    r#"{ "name": "a", "version": "1.0.0", "typescript": { "contentMapper": { "exec": ["a"] } } }"#,
                ),
                (
                    "/node_modules/b/package.json",
                    r#"{ "name": "b", "version": "1.0.0", "typescript": { "contentMapper": { "exec": ["b"] } } }"#,
                ),
            ],
            case_sensitive,
        );
        let plan = plan(&host, Api::JsonSourceFile, "/");
        assert_eq!(
            codes(&plan),
            code.into_iter().collect::<Vec<_>>(),
            "{content_mappers}"
        );
    }
}

/// tsgo `TestGetContentMapperForFileNameUsesLongestExtension` and
/// `TestGetContentMapperForFileNameUsesHostCaseSensitivity`.
#[test]
fn a_file_s_mapper_is_found_by_its_longest_extension() {
    let host = host(
        &[
            (
                "/tsconfig.json",
                r#"{ "contentMappers": [{ "package": "z", "extensions": [".z"] }, { "package": "yz", "extensions": [".y.z"] }], "files": [] }"#,
            ),
            (
                "/node_modules/z/package.json",
                r#"{ "name": "z", "typescript": { "contentMapper": { "exec": ["z"] } } }"#,
            ),
            (
                "/node_modules/yz/package.json",
                r#"{ "name": "yz", "typescript": { "contentMapper": { "exec": ["yz"] } } }"#,
            ),
        ],
        true,
    );
    let plan = plan(&host, Api::JsonSourceFile, "/");
    let mapper = |file_name: &str, case_sensitive| {
        plan.content_mapper_for_file_name(JsStr::from(file_name), case_sensitive)
            .map(|mapper| mapper.definition.package.as_str())
    };
    assert_eq!(mapper("/src/Component.y.z", true), Some("yz"));
    assert_eq!(mapper("/src/Component.z", true), Some("z"));
    assert_eq!(mapper("/src/Component.ts", true), None);
    assert_eq!(mapper("/src/Component.Y.Z", false), Some("yz"));
    assert_eq!(mapper("/src/Component.Y.Z", true), None);
}

/// A located diagnostic as tsgo prints it: code, 1-based line and column
/// (UTF-16), message; `(0, 0)` without a location.
type Row = (u32, u32, u32, &'static str);

fn rows(plan: &ConfigRootPlan, text: &str) -> Vec<(u32, u32, u32, String)> {
    let units = text.encode_utf16().collect::<Vec<_>>();
    let mut rows = plan
        .root_parse_diagnostics()
        .iter()
        .chain(plan.errors())
        .map(|diagnostic| {
            let (line, column) = diagnostic.start.map_or((0, 0), |start| {
                let before = &units[..start as usize];
                let line = before
                    .iter()
                    .filter(|unit| **unit == u16::from(b'\n'))
                    .count();
                let line_start = before
                    .iter()
                    .rposition(|unit| *unit == u16::from(b'\n'))
                    .map_or(0, |index| index + 1);
                (line as u32 + 1, (before.len() - line_start) as u32 + 1)
            });
            (
                diagnostic.code(),
                line,
                column,
                diagnostic.message_text().as_str().unwrap().to_owned(),
            )
        })
        .collect::<Vec<_>>();
    rows.sort();
    rows.dedup();
    rows
}

fn expected(rows: &[Row]) -> Vec<(u32, u32, u32, String)> {
    let mut rows = rows
        .iter()
        .map(|(code, line, column, message)| (*code, *line, *column, (*message).to_owned()))
        .collect::<Vec<_>>();
    rows.sort();
    rows
}

const INVALID_ENTRIES: &str = r#"{
  "compilerOptions": { "noEmit": true },
  "contentMappers": [
    { "package": "a", "extensions": ["vue", ".ts", ".x"] },
    { "package": "b", "extensions": [".X", ".TS", ".tſ", ".y"] },
    { "extensions": [".z"] },
    { "package": "missing", "extensions": [".m"] },
    { "package": "noname", "extensions": [".n"] },
    { "package": "noexec", "extensions": [".e"] },
    { "package": "badjson", "extensions": [".j"] },
    { "package": "nomanifest", "extensions": [".k"], "options": ["x"] },
    { "package": "nomanifest", "extensions": [".k2"] },
    { "package": "badexec", "extensions": [".q"], "options": { "a": 1 } },
    42,
    { "package": "", "extensions": [".w"], "options": null }
  ]
}
"#;

/// tsgo on a case-insensitive host (macOS), with and without
/// `--runExternalCode`.
#[test]
fn invalid_entries_are_reported_as_tsgo_reports_them() {
    let host = host(
        &[
            ("/p/tsconfig.json", INVALID_ENTRIES),
            ("/p/src/index.ts", "export const x = 1;\n"),
            (
                "/p/node_modules/a/package.json",
                r#"{"name":"a","version":"1.0.0","typescript":{"contentMapper":{"exec":["node","a.js"]}}}"#,
            ),
            (
                "/p/node_modules/b/package.json",
                r#"{"name":"b","typescript":{"contentMapper":{"exec":["b"]}}}"#,
            ),
            (
                "/p/node_modules/noname/package.json",
                r#"{"version":"1.0.0","typescript":{"contentMapper":{"exec":["n"]}}}"#,
            ),
            (
                "/p/node_modules/noexec/package.json",
                r#"{"name":"noexec","typescript":{"contentMapper":{}}}"#,
            ),
            (
                "/p/node_modules/badjson/package.json",
                r#"{"name":"badjson",}"#,
            ),
            (
                "/p/node_modules/nomanifest/package.json",
                r#"{"name":"nomanifest"}"#,
            ),
            (
                "/p/node_modules/badexec/package.json",
                r#"{"name":"badexec","typescript":{"contentMapper":{"exec":"node x.js"}}}"#,
            ),
        ],
        false,
    );
    let entries: &[Row] = &[
        (18065, 4, 38, "Content mapper file extension 'vue' must begin with a '.'."),
        (18066, 4, 45, "Content mapper file extension '.ts' is a built-in extension and cannot be registered by a content mapper."),
        (18067, 5, 38, "Content mapper file extension '.X' is registered by more than one content mapper."),
        (18066, 5, 44, "Content mapper file extension '.TS' is a built-in extension and cannot be registered by a content mapper."),
        (18066, 5, 51, "Content mapper file extension '.tſ' is a built-in extension and cannot be registered by a content mapper."),
        (5024, 6, 5, "Compiler option 'contentMapper.package' requires a value of type string."),
        (5024, 11, 5, "Compiler option 'contentMapper.options' requires a value of type object."),
        (5024, 14, 5, "Compiler option 'contentMappers' requires a value of type object."),
        (5024, 15, 5, "Compiler option 'contentMapper.options' requires a value of type object."),
        (5024, 15, 5, "Compiler option 'contentMapper.package' requires a value of type string."),
    ];
    let resolution: &[Row] = &[
        (18075, 7, 18, "The content mapper package 'missing' could not be resolved."),
        (18077, 8, 18, "The 'package.json' of the content mapper package 'noname' does not specify a 'name'."),
        (18079, 9, 18, "The 'typescript.contentMapper.exec' of the content mapper package 'noexec' must be a non-empty array of strings."),
        (18076, 10, 18, "The 'package.json' of the content mapper package 'badjson' could not be parsed."),
        (18078, 12, 18, "The 'package.json' of the content mapper package 'nomanifest' does not declare a 'typescript.contentMapper' object."),
        (18079, 13, 18, "The 'typescript.contentMapper.exec' of the content mapper package 'badexec' must be a non-empty array of strings."),
    ];
    let with_flag = plan_with(&host, Api::JsonSourceFile, "/p", true);
    assert_eq!(
        rows(&with_flag, INVALID_ENTRIES),
        expected(&[entries, resolution].concat())
    );
    let packages = with_flag
        .content_mappers()
        .iter()
        .map(|mapper| mapper.definition.package.as_str())
        .collect::<Vec<_>>();
    assert_eq!(packages, ["a", "b"]);
    assert_eq!(extensions(&with_flag), [".x", ".y"]);

    let without_flag = plan_with(&host, Api::JsonSourceFile, "/p", false);
    assert_eq!(
        rows(&without_flag, INVALID_ENTRIES),
        expected(
            &[
                entries,
                &[(18068, 3, 3, "Content mappers require the '--runExternalCode' command line flag to be enabled.")],
            ]
            .concat()
        )
    );
    assert!(without_flag.content_mappers().is_empty());
}

const SHIFTED_ELEMENTS: &str = r#"{
  "compilerOptions": { "noEmit": true },
  "references": [null, 42],
  "files": ["a.ts"],
  "contentMappers": [null, 7, { "package": "q", "extensions": [".q"] }, "s"]
}
"#;

/// tsgo checks a list's elements by their index among the values its
/// conversion kept, so a dropped null shifts the syntax they point at.
#[test]
fn list_elements_are_located_by_their_index_among_the_kept_values() {
    let host = host(
        &[
            ("/p/tsconfig.json", SHIFTED_ELEMENTS),
            ("/p/a.ts", "export const x = 1;\n"),
        ],
        false,
    );
    let plan = plan(&host, Api::JsonSourceFile, "/p");
    assert_eq!(
        rows(&plan, SHIFTED_ELEMENTS),
        expected(&[
            (
                5024,
                3,
                18,
                "Compiler option 'references' requires a value of type object."
            ),
            (
                5024,
                5,
                22,
                "Compiler option 'contentMappers' requires a value of type object."
            ),
            (
                18075,
                5,
                28,
                "The content mapper package 'q' could not be resolved."
            ),
            (
                5024,
                5,
                31,
                "Compiler option 'contentMappers' requires a value of type object."
            ),
        ])
    );
}

/// Mappers inherited from a base config are reported without a location:
/// the root config has no `contentMappers` syntax.
#[test]
fn inherited_mappers_are_reported_without_a_location() {
    let host = host(
        &[
            (
                "/p/tsconfig.json",
                r#"{ "extends": "./base/tsconfig.base.json", "compilerOptions": { "noEmit": true }, "include": ["src"] }"#,
            ),
            (
                "/p/base/tsconfig.base.json",
                r#"{ "contentMappers": [{ "package": "nope", "extensions": [".vue", "vue2"] }] }"#,
            ),
            ("/p/src/a.ts", "export {};\n"),
            ("/p/src/c.vue", "<template/>\n"),
        ],
        false,
    );
    let with_flag = plan_with(&host, Api::JsonSourceFile, "/p", true);
    assert_eq!(
        rows(&with_flag, ""),
        expected(&[
            (
                18065,
                0,
                0,
                "Content mapper file extension 'vue2' must begin with a '.'."
            ),
            (
                18075,
                0,
                0,
                "The content mapper package 'nope' could not be resolved."
            ),
        ])
    );
    let without_flag = plan_with(&host, Api::JsonSourceFile, "/p", false);
    assert_eq!(
        rows(&without_flag, ""),
        expected(&[
            (
                18065,
                0,
                0,
                "Content mapper file extension 'vue2' must begin with a '.'."
            ),
            (
                18068,
                0,
                0,
                "Content mappers require the '--runExternalCode' command line flag to be enabled."
            ),
        ])
    );
    for plan in [with_flag, without_flag] {
        let file_names = plan
            .file_names()
            .iter()
            .map(|name| name.as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(file_names, ["/p/src/a.ts"]);
    }
}

/// The options a mapper's project sees are tsgo's `CompilerOptions` as its
/// JSON encoder writes them, the command line's `project` among them: the
/// bytes tsgo sent soybean-admin's Vue mapper (`-p tsconfig.tee.json
/// --runExternalCode --pretty false`), with its directory as `/p`.
#[test]
fn a_mapper_s_project_options_are_tsgo_s_compiler_options() {
    let config = r#"{
        "compilerOptions": {
            "target": "ESNext", "jsx": "preserve", "jsxImportSource": "vue", "lib": ["DOM", "ESNext"],
            "module": "ESNext", "moduleResolution": "bundler",
            "paths": { "@/*": ["./src/*"], "~/*": ["./*"] },
            "resolveJsonModule": true,
            "types": ["vite/client", "node", "unplugin-icons/types/vue", "naive-ui/volar"],
            "strict": true, "strictNullChecks": true, "noUnusedLocals": false,
            "allowSyntheticDefaultImports": true, "esModuleInterop": true,
            "forceConsistentCasingInFileNames": true, "isolatedModules": true, "skipLibCheck": true
        },
        "contentMappers": [{ "package": "vue-mapper", "extensions": [".vue"], "options": { "languageFeatures": false } }]
    }"#;
    let host = host(
        &[
            ("/p/tsconfig.tee.json", config),
            (
                "/p/node_modules/vue-mapper/package.json",
                r#"{ "name": "vue-mapper", "version": "0.0.1", "typescript": { "contentMapper": { "exec": ["vue-mapper"], "dynamicConfig": true } } }"#,
            ),
        ],
        true,
    );
    let options = command_line_option_bag(
        &[
            (
                "project".to_owned(),
                JsonValue::String("/p/tsconfig.tee.json".into()),
            ),
            ("runExternalCode".to_owned(), JsonValue::Bool(true)),
            ("pretty".to_owned(), JsonValue::Bool(false)),
        ],
        JsStr::from("/p"),
    );
    let plan = parse_config_root_plan_with_command_line(
        &CompilerConfigHost::new(&host),
        ConfigRootPlanRequest {
            file_name: JsString::from("/p/tsconfig.tee.json"),
            text: config.to_owned(),
            base_path: JsString::from("/p"),
        },
        &options,
        &mut ConfigExtendedCache::default(),
    )
    .unwrap();
    let spec = plan
        .program_options()
        .content_mapper_spec()
        .expect("the config has a mapper");
    assert_eq!(spec.config_file_name, "/p/tsconfig.tee.json");
    assert_eq!(
        spec.compiler_options.compact(),
        r#"{"forceConsistentCasingInFileNames":true,"isolatedModules":true,"jsx":1,"jsxImportSource":"vue","lib":["lib.dom.d.ts","lib.esnext.d.ts"],"module":99,"moduleResolution":100,"noUnusedLocals":false,"paths":{"@/*":["./src/*"],"~/*":["./*"]},"project":"/p/tsconfig.tee.json","resolveJsonModule":true,"skipLibCheck":true,"strict":true,"strictNullChecks":true,"target":99,"types":["vite/client","node","unplugin-icons/types/vue","naive-ui/volar"],"allowSyntheticDefaultImports":true,"esModuleInterop":true,"configFilePath":"/p/tsconfig.tee.json","pathsBasePath":"/p","pretty":false,"runExternalCode":true}"#
    );
}
