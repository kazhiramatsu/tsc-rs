use std::cell::RefCell;
use std::collections::BTreeMap;

use serde_json::json;
use tsc_program::{
    parse_config_root_plan, ConfigHostError, ConfigHostOperation, ConfigOptionValueState,
    ConfigParseErrorKind, ConfigParseHost, ConfigRootPlanRequest,
};

#[derive(Default)]
struct MemoryConfigHost {
    files: BTreeMap<String, String>,
    directory_files: Vec<String>,
    case_sensitive: Option<bool>,
    requested_extensions: RefCell<Vec<Vec<String>>>,
    requested_includes: RefCell<Vec<Option<Vec<String>>>>,
    requested_excludes: RefCell<Vec<Option<Vec<String>>>>,
    requested_file_exists: RefCell<Vec<String>>,
    requested_reads: RefCell<Vec<String>>,
    directory_error: Option<ConfigHostError>,
}

impl MemoryConfigHost {
    fn with_file(mut self, path: &str, text: &str) -> Self {
        self.files.insert(path.to_owned(), text.to_owned());
        self
    }

    fn with_directory_files(mut self, files: &[&str]) -> Self {
        self.directory_files = files.iter().map(|file| (*file).to_owned()).collect();
        self
    }

    fn case_insensitive(mut self) -> Self {
        self.case_sensitive = Some(false);
        self
    }

    fn with_directory_error(mut self, path: &str) -> Self {
        self.directory_error = Some(ConfigHostError::new(
            ConfigHostOperation::ReadDirectory,
            path,
            "synthetic directory failure",
        ));
        self
    }

    fn stored_file(&self, path: &str) -> Option<&String> {
        if self.case_sensitive.unwrap_or(true) {
            self.files.get(path)
        } else {
            self.files
                .iter()
                .find_map(|(name, text)| name.eq_ignore_ascii_case(path).then_some(text))
        }
    }
}

impl ConfigParseHost for MemoryConfigHost {
    fn use_case_sensitive_file_names(&self) -> bool {
        self.case_sensitive.unwrap_or(true)
    }

    fn file_exists(&self, path: &str) -> Result<bool, ConfigHostError> {
        self.requested_file_exists
            .borrow_mut()
            .push(path.to_owned());
        Ok(self.stored_file(path).is_some())
    }

    fn read_file(&self, path: &str) -> Result<Option<String>, ConfigHostError> {
        self.requested_reads.borrow_mut().push(path.to_owned());
        Ok(self.stored_file(path).cloned())
    }

    fn read_directory(
        &self,
        _directory: &str,
        extensions: &[&str],
        excludes: Option<&[String]>,
        includes: Option<&[String]>,
        _depth: Option<usize>,
    ) -> Result<Vec<String>, ConfigHostError> {
        if let Some(error) = &self.directory_error {
            return Err(error.clone());
        }
        self.requested_extensions.borrow_mut().push(
            extensions
                .iter()
                .map(|extension| (*extension).to_owned())
                .collect(),
        );
        self.requested_includes
            .borrow_mut()
            .push(includes.map(<[String]>::to_vec));
        self.requested_excludes
            .borrow_mut()
            .push(excludes.map(<[String]>::to_vec));
        Ok(self
            .directory_files
            .iter()
            .filter(|file| extensions.iter().any(|extension| file.ends_with(extension)))
            .cloned()
            .collect())
    }
}

fn request(file_name: &str, text: &str) -> ConfigRootPlanRequest {
    ConfigRootPlanRequest {
        file_name: file_name.to_owned(),
        text: text.to_owned(),
        base_path: "/".to_owned(),
    }
}

#[test]
fn jsconfig_defaults_are_effective_before_root_discovery() {
    let host = MemoryConfigHost::default()
        .with_directory_files(&["/project/main.ts", "/project/helper.js"]);

    let plan = parse_config_root_plan(&host, request("/project/jsconfig.json", "{}"))
        .expect("jsconfig root plan");

    assert_eq!(
        plan.file_names(),
        ["/project/main.ts", "/project/helper.js"]
    );
    assert_eq!(plan.options().get("allowJs").unwrap().value, json!(true));
    assert_eq!(
        plan.options().get("maxNodeModuleJsDepth").unwrap().value,
        json!(2)
    );
    assert_eq!(plan.options().get("noEmit").unwrap().value, json!(true));
    assert_eq!(
        plan.module_resolution_options()
            .compiler_options()
            .max_node_module_js_depth_effective(),
        2.0
    );
    assert!(host.requested_extensions.borrow()[0]
        .iter()
        .any(|extension| extension == ".js"));
}

#[test]
fn explicit_jsconfig_options_override_defaults() {
    let host = MemoryConfigHost::default()
        .with_directory_files(&["/project/main.ts", "/project/helper.js"]);

    let plan = parse_config_root_plan(
        &host,
        request(
            "/project/jsconfig.json",
            r#"{"compilerOptions":{"allowJs":false,"maxNodeModuleJsDepth":null}}"#,
        ),
    )
    .expect("overridden jsconfig root plan");

    assert_eq!(plan.file_names(), ["/project/main.ts"]);
    assert_eq!(plan.options().get("allowJs").unwrap().value, json!(false));
    assert_eq!(
        plan.module_resolution_options()
            .compiler_options()
            .max_node_module_js_depth_effective(),
        0.0,
        "an own null masks the jsconfig default with JavaScript undefined"
    );
    assert!(!host.requested_extensions.borrow()[0]
        .iter()
        .any(|extension| extension == ".js"));
}

#[test]
fn parsed_commandline_project_references_and_wildcard_directories_are_retained() {
    let host = MemoryConfigHost::default()
        .with_directory_files(&["/project/src/main.ts", "/project/src/nested/helper.ts"]);
    let plan = parse_config_root_plan(
        &host,
        request(
            "/project/tsconfig.json",
            r#"{
                "references":[{"path":"../other","prepend":true,"circular":false}],
                "include":["src/**/*.ts"]
            }"#,
        ),
    )
    .expect("partial parsed command line keeps project metadata");

    assert_eq!(
        plan.project_references(),
        Some(
            &[tsc_program::ConfigProjectReference {
                path: "/other".to_owned(),
                original_path: "../other".to_owned(),
                prepend: Some(true),
                circular: Some(false),
            }][..]
        )
    );
    assert_eq!(
        plan.wildcard_directories(),
        &[tsc_program::ConfigWildcardDirectory {
            path: "/project/src".to_owned(),
            recursive: true,
        }]
    );
}

#[test]
fn wildcard_directories_apply_explicit_and_default_output_excludes() {
    let host = MemoryConfigHost::default();
    let excluded = parse_config_root_plan(
        &host,
        request(
            "/project/tsconfig.json",
            r#"{"include":["src/**/*.ts"],"exclude":["src"]}"#,
        ),
    )
    .expect("explicit exclude plan");
    assert!(excluded.wildcard_directories().is_empty());

    let output_excluded = parse_config_root_plan(
        &host,
        request(
            "/project/tsconfig.json",
            r#"{"compilerOptions":{"outDir":"out"},"include":["**/*"]}"#,
        ),
    )
    .expect("default output exclude plan");
    assert_eq!(
        output_excluded.wildcard_directories(),
        &[tsc_program::ConfigWildcardDirectory {
            path: "/project".to_owned(),
            recursive: true,
        }]
    );
}

#[test]
fn compiler_option_names_are_case_sensitive() {
    let host = MemoryConfigHost::default()
        .with_directory_files(&["/project/main.ts", "/project/helper.js"]);

    let plan = parse_config_root_plan(
        &host,
        request(
            "/project/tsconfig.json",
            r#"{"compilerOptions":{"ALLOWJS":true}}"#,
        ),
    )
    .expect("case-sensitive option root plan");

    assert_eq!(plan.file_names(), ["/project/main.ts"]);
    assert!(plan.options().get("allowJs").is_none());
    assert_eq!(plan.options().get("ALLOWJS").unwrap().value, json!(true));
}

#[test]
fn command_line_only_compiler_option_is_present_undefined() {
    let plan = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request(
            "/project/tsconfig.json",
            r#"{"compilerOptions":{"help":true},"files":["main.ts"]}"#,
        ),
    )
    .expect("command-line-only option returns a partial plan");

    assert_eq!(plan.errors()[0].code(), 6266);
    assert_eq!(
        plan.options().typed_value_state("help"),
        tsc_program::ConfigOptionValueState::Undefined
    );

    let missing = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request(
            "/project/tsconfig.json",
            r#"{"compilerOptions":{"help":},"files":["main.ts"]}"#,
        ),
    )
    .expect("a missing command-line-only option keeps both notifier diagnostics");
    assert_eq!(
        missing
            .errors()
            .iter()
            .map(|error| error.code())
            .collect::<Vec<_>>(),
        [5024, 6266]
    );
    assert_eq!(
        missing.options().typed_value_state("help"),
        tsc_program::ConfigOptionValueState::Undefined
    );
    assert_eq!(missing.raw()["compilerOptions"], json!({}));

    let distinct_properties = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request(
            "/project/tsconfig.json",
            r#"{"compilerOptions":{"help":true,"strict":},"files":["main.ts"]}"#,
        ),
    )
    .expect("notifier order correction stays within one property assignment");
    assert_eq!(
        distinct_properties
            .errors()
            .iter()
            .map(|error| error.code())
            .collect::<Vec<_>>(),
        [6266, 5024]
    );
}

#[test]
fn duplicate_compiler_options_notify_in_source_order_before_raw_collapse() {
    let plan = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request(
            "/project/tsconfig.json",
            r#"{"compilerOptions":{"strict":true,"allowJs":"bad"},"compilerOptions":{"allowJs":true},"files":["x.ts"]}"#,
        ),
    )
    .expect("duplicate compilerOptions return a partial plan");

    assert_eq!(
        plan.errors()
            .iter()
            .map(|error| error.code())
            .collect::<Vec<_>>(),
        [5024]
    );
    assert!(plan.errors()[0].start.is_some());
    assert_eq!(plan.raw()["compilerOptions"], json!({"allowJs": true}));
    assert_eq!(
        plan.options().typed_value_state("strict"),
        tsc_program::ConfigOptionValueState::Value(&json!(true))
    );
    assert_eq!(
        plan.options().typed_value_state("allowJs"),
        tsc_program::ConfigOptionValueState::Value(&json!(true))
    );
}

#[test]
fn compiler_options_arrays_follow_javascripts_empty_object_compatibility() {
    for text in [
        r#"{"compilerOptions":[],"files":["x.ts"]}"#,
        r#"{"compilerOptions":[true,{"strict":true},"strict"],"files":["x.ts"]}"#,
    ] {
        let plan = parse_config_root_plan(
            &MemoryConfigHost::default(),
            request("/project/tsconfig.json", text),
        )
        .expect("compilerOptions arrays are accepted as empty option objects");
        assert!(plan.errors().is_empty());
        assert!(plan.options().entries().is_empty());
        assert_eq!(plan.file_names(), ["/project/x.ts"]);
    }
}

#[test]
fn invalid_module_diagnostic_omits_typescripts_deprecated_named_values() {
    let plan = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request(
            "/project/tsconfig.json",
            r#"{"compilerOptions":{"module":"wat"},"files":["x.ts"]}"#,
        ),
    )
    .expect("an invalid module value returns a partial plan");
    assert_eq!(plan.errors().len(), 1);
    assert_eq!(plan.errors()[0].code(), 6046);
    assert_eq!(
        plan.errors()[0].message_text(),
        "Argument for '--module' option must be: 'commonjs', 'es6', 'es2015', 'es2020', 'es2022', 'esnext', 'node16', 'node18', 'node20', 'nodenext', 'preserve'."
    );
}

#[test]
fn non_finite_numeric_options_keep_javascript_number_identity() {
    let plan = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request(
            "/project/tsconfig.json",
            r#"{"compilerOptions":{"maxNodeModuleJsDepth":1e309},"files":["x.ts"]}"#,
        ),
    )
    .expect("positive infinity is a valid JavaScript numeric option");
    assert!(plan.errors().is_empty());
    assert_eq!(
        plan.options().typed_value_state("maxNodeModuleJsDepth"),
        tsc_program::ConfigOptionValueState::PositiveInfinity
    );
    assert_eq!(
        plan.module_resolution_options()
            .compiler_options()
            .max_node_module_js_depth_effective(),
        f64::INFINITY
    );

    let negative = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request(
            "/project/tsconfig.json",
            r#"{"compilerOptions":{"maxNodeModuleJsDepth":-1e309},"files":["x.ts"]}"#,
        ),
    )
    .expect("negative infinity is a valid JavaScript numeric option");
    assert!(negative.errors().is_empty());
    assert_eq!(
        negative.options().typed_value_state("maxNodeModuleJsDepth"),
        tsc_program::ConfigOptionValueState::NegativeInfinity
    );
    assert_eq!(
        negative
            .module_resolution_options()
            .compiler_options()
            .max_node_module_js_depth_effective(),
        f64::NEG_INFINITY
    );
}

#[test]
fn missing_compiler_option_value_remains_a_partial_plan() {
    let plan = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request(
            "/project/tsconfig.json",
            r#"{"compilerOptions":{"allowJs":},"files":["x.ts"]}"#,
        ),
    )
    .expect("a missing option value is recoverable config syntax");

    assert_eq!(
        plan.root_parse_diagnostics()
            .iter()
            .map(|error| error.code())
            .collect::<Vec<_>>(),
        [1109]
    );
    assert_eq!(
        plan.errors()
            .iter()
            .map(|error| error.code())
            .collect::<Vec<_>>(),
        [5024]
    );
    assert_eq!(
        plan.options().typed_value_state("allowJs"),
        tsc_program::ConfigOptionValueState::Undefined
    );
    assert_eq!(plan.file_names(), ["/project/x.ts"]);
}

#[test]
fn undefined_duplicate_overwrites_the_previous_object_projection() {
    let plan = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request(
            "/project/tsconfig.json",
            r#"{"compilerOptions":{"allowJs":true,"allowJs":},"files":["x.ts"]}"#,
        ),
    )
    .expect("a later undefined value overwrites the previous assignment");

    assert_eq!(plan.raw()["compilerOptions"], json!({}));
    assert_eq!(
        plan.options().typed_value_state("allowJs"),
        tsc_program::ConfigOptionValueState::Undefined
    );
    assert_eq!(plan.errors().last().unwrap().code(), 5024);

    let restored = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request(
            "/project/tsconfig.json",
            r#"{"compilerOptions":{"allowJs":,"strict":true,"allowJs":true},"files":["x.ts"]}"#,
        ),
    )
    .expect("a later value reuses the insertion slot created by undefined");
    assert_eq!(
        restored
            .options()
            .entries()
            .iter()
            .map(|option| option.name.as_str())
            .collect::<Vec<_>>(),
        ["allowJs", "strict"]
    );
}

#[test]
fn missing_unknown_compiler_option_value_reports_conversion_then_unknown() {
    let plan = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request(
            "/project/tsconfig.json",
            r#"{"compilerOptions":{"mystery":},"files":["x.ts"]}"#,
        ),
    )
    .expect("an unknown missing option value remains recoverable");

    assert_eq!(
        plan.errors()
            .iter()
            .map(|error| error.code())
            .collect::<Vec<_>>(),
        [1328, 5023]
    );
}

#[test]
fn undefined_option_in_a_later_compiler_options_object_removes_the_raw_entry() {
    let plan = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request(
            "/project/tsconfig.json",
            r#"{"compilerOptions":{"allowJs":true},"compilerOptions":{"allowJs":},"files":["x.ts"]}"#,
        ),
    )
    .expect("a later compilerOptions notifier overwrites the raw entry");

    assert_eq!(plan.raw()["compilerOptions"], json!({}));
    assert!(plan.options().get("allowJs").is_none());
    assert_eq!(
        plan.options().typed_value_state("allowJs"),
        tsc_program::ConfigOptionValueState::Undefined
    );
}

#[test]
fn missing_unknown_root_value_reports_the_conversion_diagnostic() {
    let plan = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request("/project/tsconfig.json", r#"{"mystery":}"#),
    )
    .expect("an unknown missing root value remains recoverable");

    assert_eq!(
        plan.errors()
            .iter()
            .map(|error| error.code())
            .collect::<Vec<_>>(),
        [1328, 18003]
    );
}

#[test]
fn missing_excludes_value_preserves_conversion_then_notifier_order() {
    let plan = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request("/project/tsconfig.json", r#"{"excludes":}"#),
    )
    .expect("the misspelled missing root value remains recoverable");

    assert_eq!(
        plan.errors()
            .iter()
            .map(|error| error.code())
            .collect::<Vec<_>>(),
        [1328, 6114, 18003]
    );
}

#[test]
fn typed_file_options_are_normalized_without_changing_raw_values() {
    let plan = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request(
            "/project/tsconfig.json",
            r#"{"compilerOptions":{"outDir":"./out","rootDir":"src"},"files":["x.ts"]}"#,
        ),
    )
    .expect("file-path options normalize in the typed projection");

    assert_eq!(plan.options().get("outDir").unwrap().value, json!("./out"));
    assert_eq!(
        plan.options().typed_value_state("outDir"),
        tsc_program::ConfigOptionValueState::Value(&json!("/project/out"))
    );
    assert_eq!(
        plan.options().typed_value_state("rootDir"),
        tsc_program::ConfigOptionValueState::Value(&json!("/project/src"))
    );

    let rooted = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request(
            "/project/tsconfig.json",
            r#"{"compilerOptions":{"outDir":"//server/share/out","rootDir":"file:///tmp/src"},"files":["x.ts"]}"#,
        ),
    )
    .expect("UNC and URL file-option roots preserve their volumes");
    assert_eq!(
        rooted.options().typed_value_state("outDir"),
        tsc_program::ConfigOptionValueState::Value(&json!("//server/share/out"))
    );
    assert_eq!(
        rooted.options().typed_value_state("rootDir"),
        tsc_program::ConfigOptionValueState::Value(&json!("file:///tmp/src"))
    );
    assert_eq!(
        rooted.discovery_options().out_dir(),
        Some("//server/share/out")
    );

    let edge = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request(
            "/project/tsconfig.json",
            r#"{"compilerOptions":{"outDir":"/a//","rootDir":"/a/.//","declarationDir":"c:","mapRoot":"foo_bar://h/a"},"files":["x.ts"]}"#,
        ),
    )
    .expect("TypeScript path roots and trailing separators remain observable");
    assert!(edge.errors().is_empty());
    assert_eq!(
        edge.options().typed_value_state("outDir"),
        tsc_program::ConfigOptionValueState::Value(&json!("/a/"))
    );
    assert_eq!(
        edge.options().typed_value_state("rootDir"),
        tsc_program::ConfigOptionValueState::Value(&json!("/a//"))
    );
    assert_eq!(
        edge.options().typed_value_state("declarationDir"),
        tsc_program::ConfigOptionValueState::Value(&json!("c:"))
    );
    assert_eq!(
        edge.options().typed_value_state("mapRoot"),
        tsc_program::ConfigOptionValueState::Value(&json!("foo_bar://h/a"))
    );
}

#[test]
fn config_paths_normalize_the_base_spelling_and_preserve_lexical_nul() {
    let windows = parse_config_root_plan(
        &MemoryConfigHost::default(),
        ConfigRootPlanRequest {
            file_name: "tsconfig.json".to_owned(),
            text: r#"{"compilerOptions":{"outDir":"out"},"files":["x.ts"]}"#.to_owned(),
            base_path: r"C:\Project".to_owned(),
        },
    )
    .expect("a backslash base path is normalized before config parsing");
    assert_eq!(windows.config_file_name(), "C:/Project/tsconfig.json");
    assert_eq!(windows.file_names(), ["C:/Project/x.ts"]);
    assert_eq!(
        windows.options().typed_value_state("outDir"),
        tsc_program::ConfigOptionValueState::Value(&json!("C:/Project/out"))
    );

    let nul = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request(
            "/project/tsconfig.json",
            r#"{"files":["a\u0000.ts"],"compilerOptions":{"outDir":"a\u0000b"}}"#,
        ),
    )
    .expect("config paths remain lexical until a filesystem host boundary");
    assert!(nul.errors().is_empty());
    assert_eq!(nul.file_names(), ["/project/a\0.ts"]);
    assert_eq!(
        nul.options().typed_value_state("outDir"),
        tsc_program::ConfigOptionValueState::Value(&json!("/project/a\0b"))
    );
}

#[test]
fn first_misplaced_root_compiler_option_is_reported_after_conversion() {
    let plan = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request(
            "/project/tsconfig.json",
            r#"{"strict":true,"target":"es2020","files":["x.ts"]}"#,
        ),
    )
    .expect("misplaced options return a partial plan");

    assert_eq!(
        plan.errors()
            .iter()
            .map(|error| error.code())
            .collect::<Vec<_>>(),
        [6258]
    );
    assert!(plan.errors()[0]
        .message_text()
        .starts_with("'strict' should"));

    let suppressed = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request(
            "/project/tsconfig.json",
            r#"{"strict":true,"compilerOptions":null,"files":["x.ts"]}"#,
        ),
    )
    .expect("an explicit compilerOptions property suppresses the placement hint");
    assert!(suppressed.errors().is_empty());

    let own_undefined = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request(
            "/project/tsconfig.json",
            r#"{"__proto__":{"compilerOptions":{}},"strict":true,"compilerOptions":,"files":["x.ts"]}"#,
        ),
    )
    .expect("an own undefined compilerOptions property shadows its JSONC prototype");
    assert_eq!(
        own_undefined
            .root_parse_diagnostics()
            .iter()
            .map(|error| error.code())
            .collect::<Vec<_>>(),
        [1109]
    );
    assert_eq!(
        own_undefined
            .errors()
            .iter()
            .map(|error| error.code())
            .collect::<Vec<_>>(),
        [5024, 6258]
    );

    let common_build_option = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request(
            "/project/tsconfig.json",
            r#"{"help":true,"incremental":true,"files":["x.ts"]}"#,
        ),
    )
    .expect("common build options are outside the placement hint catalog");
    assert!(common_build_option.errors().is_empty());
}

#[test]
fn resolve_json_module_uses_typescript_six_computed_defaults() {
    let default_host = MemoryConfigHost::default().with_directory_files(&["/project/data.json"]);
    let default_plan = parse_config_root_plan(
        &default_host,
        request("/project/tsconfig.json", r#"{"include":["*.json"]}"#),
    )
    .expect("default JSON root plan");
    assert_eq!(default_plan.file_names(), ["/project/data.json"]);
    assert!(default_plan.discovery_options().resolve_json_module());

    let node_host = MemoryConfigHost::default().with_directory_files(&["/project/data.json"]);
    let node_plan = parse_config_root_plan(
        &node_host,
        request(
            "/project/tsconfig.json",
            r#"{"compilerOptions":{"module":"node16"},"include":["*.json"]}"#,
        ),
    )
    .expect("Node16 JSON root plan");
    assert!(node_plan.file_names().is_empty());
    assert!(!node_plan.discovery_options().resolve_json_module());
}

#[test]
fn json_wildcards_use_the_first_matching_include_pattern() {
    let host = MemoryConfigHost::default()
        .with_directory_files(&["/project/src/a.json", "/project/other/b.json"]);

    let plan = parse_config_root_plan(
        &host,
        request(
            "/project/tsconfig.json",
            r#"{"include":["src/*.json","other/**/*"]}"#,
        ),
    )
    .expect("mixed JSON include plan");

    assert_eq!(plan.file_names(), ["/project/src/a.json"]);
}

#[test]
fn json_wildcards_test_every_json_ending_include_against_the_root_base() {
    let host = MemoryConfigHost::default().with_directory_files(&["/project/data.json"]);

    let plan = parse_config_root_plan(
        &host,
        request(
            "/project/tsconfig.json",
            r#"{"include":["**/*","**/*.json"]}"#,
        ),
    )
    .expect("later JSON include plan");

    assert_eq!(plan.file_names(), ["/project/data.json"]);
    assert_eq!(
        host.requested_includes.borrow()[0],
        Some(vec!["**/*".to_owned(), "**/*.json".to_owned()])
    );

    let upper = MemoryConfigHost::default().with_directory_files(&["/project/data.json"]);
    let plan = parse_config_root_plan(
        &upper,
        request("/project/tsconfig.json", r#"{"include":["*.JSON"]}"#),
    )
    .expect("case-sensitive JSON suffix plan");
    assert!(plan.file_names().is_empty());
}

#[test]
fn rooted_json_wildcard_prefixes_preserve_their_disk_roots() {
    for (config, include, candidate) in [
        ("/project/tsconfig.json", "/*.json", "/data.json"),
        ("C:/Project/tsconfig.json", "C:/*.json", "C:/data.json"),
    ] {
        let host = MemoryConfigHost::default().with_directory_files(&[candidate]);
        let text = format!(r#"{{"include":["{include}"]}}"#);
        let plan = parse_config_root_plan(&host, request(config, &text))
            .expect("rooted JSON wildcard plan");
        assert_eq!(plan.file_names(), [candidate]);
    }
}

#[test]
fn json_only_patterns_apply_files_matcher_implicit_exclusions() {
    let hidden = "/project/.hidden/data.json";
    let host = MemoryConfigHost::default().with_directory_files(&[hidden]);
    let plan = parse_config_root_plan(
        &host,
        request(
            "/project/tsconfig.json",
            r#"{"include":[".hidden/**/*","**/*.json"]}"#,
        ),
    )
    .expect("implicit hidden-directory JSON filter");
    assert!(plan.file_names().is_empty());

    let explicit = MemoryConfigHost::default().with_directory_files(&[hidden]);
    let plan = parse_config_root_plan(
        &explicit,
        request(
            "/project/tsconfig.json",
            r#"{"include":[".hidden/**/*.json"]}"#,
        ),
    )
    .expect("explicit hidden-directory JSON filter");
    assert_eq!(plan.file_names(), [hidden]);
}

#[test]
fn invalid_recursive_specs_report_diagnostics_before_directory_observation() {
    for include in ["src/**", "**/../src/*.ts"] {
        let host = MemoryConfigHost::default().with_directory_files(&["/project/src/main.ts"]);
        let text = format!(r#"{{"include":["{include}"]}}"#);

        let plan = parse_config_root_plan(&host, request("/project/tsconfig.json", &text))
            .expect("invalid recursive spec still returns a partial plan");

        assert!(matches!(plan.errors()[0].code(), 5010 | 5065));
        assert!(host.requested_extensions.borrow().is_empty());
    }
}

#[test]
fn recursive_spec_diagnostics_use_the_written_separator_spelling() {
    let host = MemoryConfigHost::default();
    let plan = parse_config_root_plan(
        &host,
        request(
            "/project/tsconfig.json",
            r#"{"include":["src\\**","src/**//","src\\**\\..\\x"]}"#,
        ),
    )
    .expect("backslashes and repeated trailing separators are validated verbatim");

    assert_eq!(
        plan.errors()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [18003]
    );
    assert_eq!(
        host.requested_includes.borrow()[0],
        Some(vec![
            r"src\**".to_owned(),
            "src/**//".to_owned(),
            r"src\**\..\x".to_owned(),
        ])
    );
}

#[test]
fn repeated_spec_values_reuse_typescripts_first_source_location() {
    let plan = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request("/project/tsconfig.json", r#"{"include":["**","**"]}"#),
    )
    .expect("invalid duplicate include patterns return a partial plan");

    assert_eq!(
        plan.errors()
            .iter()
            .map(|error| error.code())
            .collect::<Vec<_>>(),
        [5010, 5010, 18003]
    );
    assert_eq!(plan.errors()[0].start, Some(12));
    assert_eq!(plan.errors()[0].length, Some(4));
    assert_eq!(plan.errors()[1].start, Some(12));
    assert_eq!(plan.errors()[1].length, Some(4));
}

#[test]
fn duplicate_files_validate_every_assignment_but_use_the_first_array_location() {
    let plan = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request("/project/tsconfig.json", r#"{"files":[1],"files":[]}"#),
    )
    .expect("duplicate files return a partial plan");

    assert_eq!(
        plan.errors()
            .iter()
            .map(|error| error.code())
            .collect::<Vec<_>>(),
        [5024, 18002]
    );
    assert_eq!(
        (plan.errors()[0].start, plan.errors()[0].length),
        (Some(10), Some(1))
    );
    assert_eq!(
        (plan.errors()[1].start, plan.errors()[1].length),
        (Some(9), Some(3))
    );
}

#[test]
fn omitted_spec_element_is_diagnosed_then_filtered() {
    let plan = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request("/project/tsconfig.json", r#"{"files":[,"x.ts"]}"#),
    )
    .expect("an array hole is recoverable config syntax");

    assert!(plan.root_parse_diagnostics().is_empty());
    assert_eq!(
        plan.errors()
            .iter()
            .map(|error| error.code())
            .collect::<Vec<_>>(),
        [5024]
    );
    assert_eq!(plan.file_names(), ["/project/x.ts"]);
}

#[test]
fn undefined_files_presence_suppresses_no_input_diagnostics() {
    let plan = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request("/project/tsconfig.json", r#"{"files":}"#),
    )
    .expect("an undefined files property remains an own property");

    assert_eq!(
        plan.errors()
            .iter()
            .map(|error| error.code())
            .collect::<Vec<_>>(),
        [5024]
    );
    assert!(plan.file_names().is_empty());
}

#[test]
fn undefined_duplicate_include_does_not_block_inheritance() {
    let host = MemoryConfigHost::default()
        .with_file("/project/base.json", r#"{"include":["base/**/*.ts"]}"#)
        .with_directory_files(&["/project/base/main.ts"]);
    let plan = parse_config_root_plan(
        &host,
        request(
            "/project/tsconfig.json",
            r#"{"extends":"./base.json","include":["own/**/*.ts"],"include":}"#,
        ),
    )
    .expect("undefined include allows the inherited spec through");

    assert_eq!(plan.raw()["include"], json!(["base/**/*.ts"]));
    assert_eq!(plan.errors().last().unwrap().code(), 5024);
}

#[test]
fn array_extends_uses_later_option_precedence() {
    let host = MemoryConfigHost::default()
        .with_file(
            "/project/base-a.json",
            r#"{"compilerOptions":{"allowJs":true}}"#,
        )
        .with_file(
            "/project/base-b.json",
            r#"{"compilerOptions":{"allowJs":false}}"#,
        )
        .with_directory_files(&["/project/main.ts", "/project/helper.js"]);

    let plan = parse_config_root_plan(
        &host,
        request(
            "/project/tsconfig.json",
            r#"{"extends":["./base-a.json","./base-b.json"]}"#,
        ),
    )
    .expect("array extends root plan");

    assert_eq!(plan.file_names(), ["/project/main.ts"]);
    assert_eq!(plan.options().get("allowJs").unwrap().value, json!(false));
    assert_eq!(plan.extended_sources().len(), 2);
}

#[test]
fn invalid_own_option_masks_an_inherited_typed_value() {
    let host = MemoryConfigHost::default()
        .with_file(
            "/project/base.json",
            r#"{"compilerOptions":{"allowJs":true}}"#,
        )
        .with_directory_files(&["/project/main.ts", "/project/helper.js"]);
    let plan = parse_config_root_plan(
        &host,
        request(
            "/project/tsconfig.json",
            r#"{"extends":"./base.json","compilerOptions":{"allowJs":"yes"}}"#,
        ),
    )
    .expect("invalid own value still returns a partial plan");

    assert_eq!(plan.errors()[0].code(), 5024);
    assert_eq!(
        plan.options().typed_value_state("allowJs"),
        tsc_program::ConfigOptionValueState::Undefined
    );
    assert_eq!(plan.file_names(), ["/project/main.ts"]);

    let missing_host = MemoryConfigHost::default().with_file(
        "/project/base.json",
        r#"{"compilerOptions":{"allowJs":true,"mystery":true}}"#,
    );
    let missing = parse_config_root_plan(
        &missing_host,
        request(
            "/project/tsconfig.json",
            r#"{"extends":"./base.json","compilerOptions":{"allowJs":,"mystery":},"files":["x.ts"]}"#,
        ),
    )
    .expect("own undefined values delete inherited raw option projections");
    assert_eq!(
        missing
            .errors()
            .iter()
            .map(|error| error.code())
            .collect::<Vec<_>>(),
        [5024, 1328, 5023, 5023]
    );
    assert_eq!(
        missing.options().typed_value_state("allowJs"),
        tsc_program::ConfigOptionValueState::Undefined
    );
    assert!(missing.options().get("allowJs").is_none());
    assert!(missing.options().get("mystery").is_none());

    let jsconfig = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request(
            "/project/jsconfig.json",
            r#"{"compilerOptions":{"allowJs":},"files":["x.js"]}"#,
        ),
    )
    .expect("own undefined values delete filename defaults from the raw bag");
    assert!(jsconfig.options().get("allowJs").is_none());
    assert_eq!(
        jsconfig.options().typed_value_state("allowJs"),
        tsc_program::ConfigOptionValueState::Undefined
    );
}

#[test]
fn circular_extends_reports_a_partial_plan_diagnostic() {
    let host = MemoryConfigHost::default()
        .with_file("/project/tsconfig.json", r#"{"extends":"./base.json"}"#)
        .with_file("/project/base.json", r#"{"extends":"./tsconfig.json"}"#);

    let plan = parse_config_root_plan(
        &host,
        request("/project/tsconfig.json", r#"{"extends":"./base.json"}"#),
    )
    .expect("cycle returns a partial config plan");

    assert_eq!(plan.errors()[0].code(), 18000);
    assert_eq!(
        plan.extended_source_files(),
        ["/project/base.json", "/project/tsconfig.json"]
    );

    let quoted_cycle =
        MemoryConfigHost::default().with_file("/project/a.json", r#"{'extends':'./a.json'}"#);
    let plan = parse_config_root_plan(
        &quoted_cycle,
        request(
            "/project/tsconfig.json",
            r#"{"extends":"./a.json","files":["x.ts"]}"#,
        ),
    )
    .expect("cycle conversion diagnostics remain observable after TS18000");
    assert_eq!(
        plan.errors()
            .iter()
            .map(|error| error.code())
            .collect::<Vec<_>>(),
        [1327, 1327, 18000, 1327, 1327]
    );

    let array_cycle = MemoryConfigHost::default().with_file(
        "/project/a.json",
        r#"[{'extends':'./a.json'},{'note':'x'}]"#,
    );
    let plan = parse_config_root_plan(
        &array_cycle,
        request(
            "/project/tsconfig.json",
            r#"{"extends":"./a.json","files":["x.ts"]}"#,
        ),
    )
    .expect("cycle conversion walks the complete root expression after TS18000");
    assert_eq!(
        plan.errors()
            .iter()
            .map(|error| error.code())
            .collect::<Vec<_>>(),
        [5092, 1327, 1327, 18000, 1327, 1327, 1327, 1327]
    );

    let invalid_cycle = MemoryConfigHost::default()
        .with_file("/project/a.json", r#"{"extends":"./a.json","note":foo}"#);
    let plan = parse_config_root_plan(
        &invalid_cycle,
        request(
            "/project/tsconfig.json",
            r#"{"extends":"./a.json","files":["x.ts"]}"#,
        ),
    )
    .expect("cycle conversion replays invalid-value diagnostics after TS18000");
    assert_eq!(
        plan.errors()
            .iter()
            .map(|error| error.code())
            .collect::<Vec<_>>(),
        [1328, 18000, 1328]
    );
}

#[test]
fn extended_parse_diagnostic_precedes_cycle_detection() {
    let host = MemoryConfigHost::default()
        .with_file("/project/tsconfig.json", r#"{"extends":"./base.json""#)
        .with_file("/project/base.json", r#"{"extends":"./tsconfig.json"}"#);

    let plan = parse_config_root_plan(
        &host,
        request("/project/tsconfig.json", r#"{"extends":"./base.json"}"#),
    )
    .expect("a malformed cyclic target skips only that branch");

    assert_eq!(plan.errors()[0].code(), 1005);
    assert!(!plan
        .errors()
        .iter()
        .any(|diagnostic| diagnostic.code() == 18000));
}

#[test]
fn empty_source_is_still_a_valid_cycle_target() {
    let host = MemoryConfigHost::default()
        .with_file("/project/base.json", r#"{"extends":"./tsconfig.json"}"#)
        .with_file("/project/tsconfig.json", "");

    let plan = parse_config_root_plan(
        &host,
        request(
            "/project/tsconfig.json",
            r#"{"extends":"./base.json","files":["x.ts"]}"#,
        ),
    )
    .expect("an empty re-read cycle target reports circularity");

    assert_eq!(plan.errors()[0].code(), 18000);
}

#[test]
fn duplicate_extends_probe_every_assignment_but_read_only_the_last() {
    let host = MemoryConfigHost::default()
        .with_file("/project/ok.json", r#"{"compilerOptions":{"strict":true}}"#);
    let plan = parse_config_root_plan(
        &host,
        request(
            "/project/tsconfig.json",
            r#"{"extends":"./first-missing","extends":"./ok.json","files":["x.ts"]}"#,
        ),
    )
    .expect("the final extends assignment supplies the effective branch");

    assert_eq!(
        plan.errors()
            .iter()
            .map(|error| error.code())
            .collect::<Vec<_>>(),
        [6053]
    );
    assert_eq!(
        host.requested_file_exists.borrow().as_slice(),
        [
            "/project/first-missing",
            "/project/first-missing.json",
            "/project/ok.json"
        ]
    );
    assert_eq!(
        host.requested_reads.borrow().as_slice(),
        ["/project/ok.json"]
    );
    assert_eq!(plan.extended_source_files(), ["/project/ok.json"]);
    assert_eq!(
        plan.options().typed_value_state("strict"),
        tsc_program::ConfigOptionValueState::Value(&json!(true))
    );
}

#[test]
fn missing_extends_value_keeps_both_conversion_diagnostics() {
    let plan = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request("/project/tsconfig.json", r#"{"extends":,"files":[]}"#),
    )
    .expect("a missing extends value remains recoverable");

    assert_eq!(
        plan.root_parse_diagnostics()
            .iter()
            .map(|error| error.code())
            .collect::<Vec<_>>(),
        [1109]
    );
    assert_eq!(
        plan.errors()
            .iter()
            .map(|error| error.code())
            .collect::<Vec<_>>(),
        [5024, 5024]
    );
    assert_eq!(plan.errors()[0].start, plan.errors()[1].start);
    assert_eq!(plan.errors()[0].length, plan.errors()[1].length);
}

#[test]
fn config_parser_and_extends_graph_have_typed_depth_limits() {
    let nested = format!("{{\"value\":{}0{}}}", "[".repeat(256), "]".repeat(256));
    let error = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request("/project/tsconfig.json", &nested),
    )
    .expect_err("recursive JSON parser boundary must fail before parsing");
    assert_eq!(error.kind(), ConfigParseErrorKind::ResourceLimit);

    let mut host = MemoryConfigHost::default();
    for index in 1..=256 {
        host.files.insert(
            format!("/project/base-{index}.json"),
            format!(r#"{{"extends":"./base-{}.json"}}"#, index + 1),
        );
    }
    let error = parse_config_root_plan(
        &host,
        request("/project/tsconfig.json", r#"{"extends":"./base-1.json"}"#),
    )
    .expect_err("extends graph must stop at its typed resource boundary");
    assert_eq!(error.kind(), ConfigParseErrorKind::ResourceLimit);
}

#[test]
fn empty_config_is_the_default_config_object() {
    let host = MemoryConfigHost::default().with_directory_files(&["/project/main.ts"]);

    let plan = parse_config_root_plan(&host, request("/project/tsconfig.json", ""))
        .expect("empty config root plan");

    assert_eq!(plan.raw(), &json!({}));
    assert_eq!(plan.file_names(), ["/project/main.ts"]);
}

#[test]
fn no_input_diagnostic_reports_default_output_exclusions() {
    let host = MemoryConfigHost::default();
    let plan = parse_config_root_plan(
        &host,
        request(
            "/project/tsconfig.json",
            r#"{"compilerOptions":{"outDir":"dist","declarationDir":"types"}}"#,
        ),
    )
    .expect("empty output-only config plan");

    assert_eq!(plan.errors().last().unwrap().code(), 18003);
    assert!(plan
        .errors()
        .last()
        .unwrap()
        .message_text()
        .contains("exclude' paths were '[\"/project/dist\",\"/project/types\"]'"));
}

#[test]
fn no_input_diagnostic_uses_javascript_json_number_rendering() {
    let plan = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request(
            "/project/tsconfig.json",
            r#"{"include":[1e309,-0,1.0,1e2,{"2":"x","1":"y"}]}"#,
        ),
    )
    .expect("invalid numeric specs still return a no-input diagnostic");

    assert_eq!(
        plan.errors()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [5024, 5024, 5024, 5024, 5024, 18003]
    );
    assert!(plan
        .errors()
        .last()
        .unwrap()
        .message_text()
        .contains("include' paths were '[null,0,1,100,{\"1\":\"y\",\"2\":\"x\"}]'"));
}

#[test]
fn non_object_root_reports_ts5092_and_recovers_the_first_object() {
    let host = MemoryConfigHost::default();
    let plan = parse_config_root_plan(
        &host,
        request(
            "/project/custom.json",
            r#"[1,{"compilerOptions":{"strict":true},"files":["a.ts"]},{"files":["b.ts"]}]"#,
        ),
    )
    .expect("array root returns a recoverable partial plan");

    assert_eq!(plan.errors()[0].code(), 5092);
    assert_eq!(
        plan.errors()[0].message_text(),
        "The root value of a 'tsconfig.json' file must be an object."
    );
    assert_eq!(plan.file_names(), ["/project/a.ts"]);
    assert_eq!(plan.options().get("strict").unwrap().value, json!(true));

    let ignored_tail = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request(
            "/project/tsconfig.json",
            r#"[{"files":["x.ts"]},{'ignored':'value'}]"#,
        ),
    )
    .expect("root-array recovery does not convert later object elements");
    assert_eq!(
        ignored_tail
            .errors()
            .iter()
            .map(|error| error.code())
            .collect::<Vec<_>>(),
        [5092]
    );
    assert_eq!(ignored_tail.file_names(), ["/project/x.ts"]);
}

#[test]
fn jsonc_conversion_is_exercised_through_the_public_config_planner() {
    let host = MemoryConfigHost::default();
    let plan = parse_config_root_plan(
        &host,
        request(
            "/project/tsconfig.json",
            r#"{
                // comments and trailing commas are config syntax
                "compilerOptions": {"strict": false, "strict": true,},
                "include": [],
            }"#,
        ),
    )
    .expect("JSONC root plan");

    assert_eq!(
        plan.raw(),
        &json!({"compilerOptions": {"strict": true}, "include": []})
    );
    assert_eq!(plan.options().get("strict").unwrap().value, json!(true));
}

#[test]
fn non_json_string_spellings_recover_with_typescripts_conversion_diagnostics() {
    let unquoted = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request(
            "/project/tsconfig.json",
            r#"{compilerOptions:{strict:true},files:["x.ts"]}"#,
        ),
    )
    .expect("unquoted JSONC names remain a recoverable config");
    assert_eq!(
        unquoted
            .errors()
            .iter()
            .map(|error| error.code())
            .collect::<Vec<_>>(),
        [1327, 1327, 1327]
    );
    assert_eq!(
        unquoted.options().typed_value_state("strict"),
        tsc_program::ConfigOptionValueState::Value(&json!(true))
    );
    assert_eq!(unquoted.file_names(), ["/project/x.ts"]);

    let keyword_name = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request(
            "/project/tsconfig.json",
            r#"{compilerOptions:{module:"esnext"},files:["x.ts"]}"#,
        ),
    )
    .expect("keyword property names remain bounded recoverable JSONC");
    assert_eq!(
        keyword_name
            .errors()
            .iter()
            .map(|error| error.code())
            .collect::<Vec<_>>(),
        [1327, 1327, 1327]
    );
    assert_eq!(
        keyword_name.options().typed_value_state("module"),
        tsc_program::ConfigOptionValueState::Value(&json!(99))
    );

    let single_quoted = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request(
            "/project/tsconfig.json",
            r#"{'compilerOptions':{'strict':true},'files':['x.ts']}"#,
        ),
    )
    .expect("single-quoted JSONC names and values remain recoverable");
    assert_eq!(
        single_quoted
            .errors()
            .iter()
            .map(|error| error.code())
            .collect::<Vec<_>>(),
        [1327, 1327, 1327, 1327]
    );
    assert_eq!(
        single_quoted.options().typed_value_state("strict"),
        tsc_program::ConfigOptionValueState::Value(&json!(true))
    );
    assert_eq!(single_quoted.file_names(), ["/project/x.ts"]);

    let identifier_value = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request(
            "/project/tsconfig.json",
            r#"{"compilerOptions":{"strict":foo},"files":["x.ts"]}"#,
        ),
    )
    .expect("identifier option values recover as own undefined");
    assert_eq!(
        identifier_value
            .errors()
            .iter()
            .map(|error| error.code())
            .collect::<Vec<_>>(),
        [5024]
    );
    assert_eq!(
        identifier_value.options().typed_value_state("strict"),
        tsc_program::ConfigOptionValueState::Undefined
    );
}

#[test]
fn nested_conversion_diagnostics_precede_parent_option_notifiers() {
    for (text, expected) in [
        (
            r#"{"compilerOptions":{"strict":[foo]},"files":["x.ts"]}"#,
            [(1328, Some(30)), (5024, Some(29))],
        ),
        (
            r#"{"compilerOptions":{"wat":'x'},"files":["x.ts"]}"#,
            [(1327, Some(26)), (5023, Some(20))],
        ),
        (
            r#"{"compilerOptions":{"help":'x'},"files":["x.ts"]}"#,
            [(1327, Some(27)), (6266, Some(20))],
        ),
        (
            r#"{"compilerOptions":{"strict":{"x":'y'}},"files":["x.ts"]}"#,
            [(1327, Some(34)), (5024, Some(29))],
        ),
    ] {
        let plan = parse_config_root_plan(
            &MemoryConfigHost::default(),
            request("/project/tsconfig.json", text),
        )
        .expect("nested JSON conversion diagnostics remain recoverable");
        assert_eq!(
            plan.errors()
                .iter()
                .map(|diagnostic| (diagnostic.code(), diagnostic.start))
                .collect::<Vec<_>>(),
            expected,
            "diagnostics for {text}"
        );
    }

    let filtered = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request("/project/tsconfig.json", r#"{"files":[foo,bar,{"x":'y'}]}"#),
    )
    .expect("filtered list values retain conversion-before-notifier ordering");
    assert_eq!(
        filtered
            .errors()
            .iter()
            .map(|diagnostic| (diagnostic.code(), diagnostic.start))
            .collect::<Vec<_>>(),
        [
            (5024, Some(10)),
            (5024, Some(14)),
            (1327, Some(23)),
            (5024, Some(10)),
        ]
    );
}

#[test]
fn published_raw_config_contains_only_own_enumerable_json_properties() {
    let host = MemoryConfigHost::default();
    let plan = parse_config_root_plan(
        &host,
        request(
            "/project/tsconfig.json",
            r#"{"__proto__":{"inherited":"hidden"},"own":true}"#,
        ),
    )
    .expect("prototype-bearing config");
    assert_eq!(plan.raw(), &json!({"own": true}));
    assert!(plan
        .raw()
        .as_object()
        .unwrap()
        .keys()
        .all(|name| !name.contains('\0')));

    let plan = parse_config_root_plan(
        &host,
        request(
            "/project/tsconfig.json",
            r#"{"__proto__":null,"__proto__":{"own":true}}"#,
        ),
    )
    .expect("null prototype followed by own property");
    assert_eq!(plan.raw(), &json!({"__proto__": {"own": true}}));

    let plan = parse_config_root_plan(
        &host,
        request(
            "/project/tsconfig.json",
            r#"{"\u0000user":true,"include":[]}"#,
        ),
    )
    .expect("leading-NUL user property");
    assert_eq!(plan.raw(), &json!({"\0user": true, "include": []}));
}

#[test]
fn published_compiler_options_strip_jsonc_prototype_state_recursively() {
    let host = MemoryConfigHost::default();
    let plan = parse_config_root_plan(
        &host,
        request(
            "/project/tsconfig.json",
            r#"{
                "compilerOptions": {
                    "__proto__": {"inherited": true},
                    "strict": true,
                    "\u0000custom": {"__proto__": {"hidden": true}, "own": true}
                },
                "include": []
            }"#,
        ),
    )
    .expect("prototype-bearing compiler options");

    assert_eq!(plan.options().get("strict").unwrap().value, json!(true));
    assert!(plan.options().get("inherited").is_none());
    assert_eq!(
        plan.options().get("\0custom").unwrap().value,
        json!({"own": true})
    );
    assert!(plan
        .options()
        .entries()
        .iter()
        .all(|option| !option.name.contains("tsc-rs:jsonc-prototype")));
}

#[test]
fn jsonc_prototype_specs_follow_apply_extended_config_without_becoming_public() {
    let inherited = MemoryConfigHost::default()
        .with_file(
            "/project/base.json",
            r#"{"__proto__":{"files":["inherited.ts"]}}"#,
        )
        .with_directory_files(&["/project/default.ts"]);
    let plan = parse_config_root_plan(
        &inherited,
        request("/project/tsconfig.json", r#"{"extends":"./base.json"}"#),
    )
    .expect("prototype files inherited through applyExtendedConfig");
    assert_eq!(plan.file_names(), ["/project/inherited.ts"]);
    assert_eq!(
        plan.raw(),
        &json!({"extends": "./base.json", "files": ["inherited.ts"]})
    );

    let blocked = MemoryConfigHost::default()
        .with_file("/project/base.json", r#"{"files":["base.ts"]}"#)
        .with_directory_files(&["/project/default.ts"]);
    let plan = parse_config_root_plan(
        &blocked,
        request(
            "/project/tsconfig.json",
            r#"{"__proto__":{"files":[]},"extends":"./base.json"}"#,
        ),
    )
    .expect("prototype files block inherited files without becoming own raw");
    assert_eq!(plan.file_names(), ["/project/default.ts"]);
    assert_eq!(plan.raw(), &json!({"extends": "./base.json"}));

    let own_undefined_files =
        MemoryConfigHost::default().with_file("/project/base.json", r#"{"files":["base.ts"]}"#);
    let plan = parse_config_root_plan(
        &own_undefined_files,
        request(
            "/project/tsconfig.json",
            r#"{"extends":"./base.json","__proto__":{"files":["proto.ts"]},"files":}"#,
        ),
    )
    .expect("own undefined files shadow the prototype without blocking the base config");
    assert_eq!(
        plan.root_parse_diagnostics()
            .iter()
            .map(|error| error.code())
            .collect::<Vec<_>>(),
        [1109]
    );
    assert_eq!(
        plan.errors()
            .iter()
            .map(|error| error.code())
            .collect::<Vec<_>>(),
        [5024]
    );
    assert_eq!(plan.raw()["files"], json!(["base.ts"]));
    assert_eq!(plan.file_names(), ["/project/base.ts"]);

    let own_undefined = MemoryConfigHost::default()
        .with_file("/project/base.json", r#"{"exclude":["base"]}"#)
        .with_directory_files(&["/project/default.ts"]);
    let plan = parse_config_root_plan(
        &own_undefined,
        request(
            "/project/tsconfig.json",
            r#"{"extends":"./base.json","__proto__":{"exclude":["proto"]},"exclude":,"include":["**/*"]}"#,
        ),
    )
    .expect("own undefined excludes shadow the prototype without blocking the base config");
    assert_eq!(
        plan.root_parse_diagnostics()
            .iter()
            .map(|error| error.code())
            .collect::<Vec<_>>(),
        [1109]
    );
    assert_eq!(
        plan.errors()
            .iter()
            .map(|error| error.code())
            .collect::<Vec<_>>(),
        [5024]
    );
    assert_eq!(plan.raw()["exclude"], json!(["base"]));
    assert_eq!(
        own_undefined.requested_excludes.borrow().as_slice(),
        [Some(vec!["base".to_owned()])]
    );
}

#[test]
fn explicit_missing_json_extends_observes_the_read_boundary() {
    let host = MemoryConfigHost::default();
    let plan = parse_config_root_plan(
        &host,
        request("/project/tsconfig.json", r#"{"extends":"./missing.json"}"#),
    )
    .expect("missing explicit JSON config returns a partial plan");

    assert_eq!(plan.errors()[0].code(), 5083);
    assert_eq!(plan.extended_source_files(), ["/project/missing.json"]);
    assert_eq!(
        host.requested_file_exists.borrow().as_slice(),
        ["/project/missing.json"]
    );
    assert_eq!(
        host.requested_reads.borrow().as_slice(),
        ["/project/missing.json"]
    );
}

#[test]
fn package_extends_uses_manifestless_subpaths_and_tsconfig_fields() {
    let manifestless = MemoryConfigHost::default()
        .with_file(
            "/project/node_modules/foo/base.json",
            r#"{"compilerOptions":{"strict":true}}"#,
        )
        .with_directory_files(&["/project/main.ts"]);
    let plan = parse_config_root_plan(
        &manifestless,
        request("/project/tsconfig.json", r#"{"extends":"foo/base"}"#),
    )
    .expect("manifestless package subpath");
    assert_eq!(plan.options().get("strict").unwrap().value, json!(true));
    assert_eq!(
        plan.extended_sources()[0].file_name,
        "/project/node_modules/foo/base.json"
    );

    let package_field = MemoryConfigHost::default()
        .with_file(
            "/project/node_modules/foo/package.json",
            r#"{"tsconfig":"./config/base.json"}"#,
        )
        .with_file(
            "/project/node_modules/foo/config/base.json",
            r#"{"compilerOptions":{"noImplicitAny":true}}"#,
        )
        .with_directory_files(&["/project/main.ts"]);
    let plan = parse_config_root_plan(
        &package_field,
        request("/project/tsconfig.json", r#"{"extends":"foo"}"#),
    )
    .expect("package tsconfig field");
    assert_eq!(
        plan.options().get("noImplicitAny").unwrap().value,
        json!(true)
    );
    assert_eq!(
        plan.extended_sources()[0].file_name,
        "/project/node_modules/foo/config/base.json"
    );
}

#[test]
fn json_config_resolution_preserves_written_extension_families() {
    for written in ["base.tsx", "base.mts"] {
        let host = MemoryConfigHost::default().with_file(
            "/project/node_modules/pkg/base.json",
            r#"{"compilerOptions":{"strict":true}}"#,
        );
        let config = format!(r#"{{"extends":"pkg/{written}"}}"#);
        let plan = parse_config_root_plan(&host, request("/project/tsconfig.json", &config))
            .expect("failed JSON config lookup returns a partial plan");
        assert_eq!(plan.errors()[0].code(), 6053);
        assert!(!host
            .requested_file_exists
            .borrow()
            .iter()
            .any(|path| path == "/project/node_modules/pkg/base.json"));
    }

    let imports = MemoryConfigHost::default()
        .with_file(
            "/project/package.json",
            r##"{"imports":{"#base":"pkg/base.ts"}}"##,
        )
        .with_file(
            "/project/node_modules/pkg/base.json",
            r#"{"compilerOptions":{"strict":true}}"#,
        );
    let plan = parse_config_root_plan(
        &imports,
        request("/project/tsconfig.json", r##"{"extends":"#base"}"##),
    )
    .expect("bare imports re-entry failure returns a partial plan");
    assert_eq!(plan.errors()[0].code(), 6053);
    assert!(!imports
        .requested_file_exists
        .borrow()
        .iter()
        .any(|path| path == "/project/node_modules/pkg/base.json"));
}

#[test]
fn package_extends_normalizes_lexical_parent_components_before_legacy_loading() {
    for with_manifest in [false, true] {
        let mut host = MemoryConfigHost::default().with_file(
            "/project/node_modules/bar.json",
            r#"{"compilerOptions":{"strict":true}}"#,
        );
        if with_manifest {
            host = host.with_file("/project/node_modules/foo/package.json", "{}");
        }
        let plan = parse_config_root_plan(
            &host,
            request("/project/tsconfig.json", r#"{"extends":"foo/../bar"}"#),
        )
        .expect("normalized legacy package path");
        assert_eq!(plan.options().get("strict").unwrap().value, json!(true));
        assert_eq!(
            plan.extended_sources()[0].file_name,
            "/project/node_modules/bar.json"
        );
    }

    let exports = MemoryConfigHost::default()
        .with_file(
            "/project/node_modules/foo/package.json",
            r#"{"exports":{}}"#,
        )
        .with_file(
            "/project/node_modules/bar.json",
            r#"{"compilerOptions":{"strict":true}}"#,
        );
    let plan = parse_config_root_plan(
        &exports,
        request("/project/tsconfig.json", r#"{"extends":"foo/../bar"}"#),
    )
    .expect("an exports block returns a partial plan");
    assert_eq!(plan.errors()[0].code(), 6053);
}

#[test]
fn exact_parent_extends_uses_the_json_config_relative_loader() {
    let host = MemoryConfigHost::default().with_file(
        "/project/tsconfig.json",
        r#"{"compilerOptions":{"strict":true}}"#,
    );

    let plan = parse_config_root_plan(
        &host,
        request("/project/sub/tsconfig.json", r#"{"extends":".."}"#),
    )
    .expect("exact parent config extends");

    assert_eq!(plan.options().get("strict").unwrap().value, json!(true));
    assert_eq!(
        plan.extended_sources()[0].file_name,
        "/project/tsconfig.json"
    );

    let cycle_text = r#"{"extends":"."}"#;
    let cycle_host = MemoryConfigHost::default().with_file("/project/tsconfig.json", cycle_text);
    let plan = parse_config_root_plan(&cycle_host, request("/project/tsconfig.json", cycle_text))
        .expect("exact current-directory cycle returns a partial plan");
    assert_eq!(plan.errors()[0].code(), 18000);
}

#[test]
fn config_dir_templates_use_the_root_config_directory() {
    let host = MemoryConfigHost::default()
        .with_file(
            "/base/base.json",
            r#"{
                "compilerOptions":{"outDir":"${configDir}/dist"},
                "include":["${configDir}/a*/../src/**/*.ts"]
            }"#,
        )
        .with_directory_files(&["/project/src/main.ts"]);

    let plan = parse_config_root_plan(
        &host,
        request(
            "/project/tsconfig.json",
            r#"{"extends":"../base/base.json"}"#,
        ),
    )
    .expect("configDir root plan");

    assert_eq!(plan.discovery_options().out_dir(), Some("/project/dist"));
    assert_eq!(
        host.requested_includes.borrow()[0],
        Some(vec!["/project/src/**/*.ts".to_owned()])
    );
    assert_eq!(
        host.requested_excludes.borrow()[0],
        Some(vec!["/project/dist".to_owned()])
    );
}

#[test]
fn inherited_paths_keep_their_defining_base_while_templates_use_the_root_base() {
    let host = MemoryConfigHost::default().with_file(
        "/base/base.json",
        r#"{
            "compilerOptions": {
                "rootDir": "${configDir}/src",
                "paths": {
                    "@plain/*": ["relative/*"],
                    "@generated/*": ["${configDir}/generated/*"]
                }
            }
        }"#,
    );

    let plan = parse_config_root_plan(
        &host,
        request(
            "/project/tsconfig.json",
            r#"{"extends":"../base/base.json","files":["root.ts"]}"#,
        ),
    )
    .expect("inherited paths plan");

    assert_eq!(plan.options().stored_paths_base_path(), Some("/base"));
    assert_eq!(
        plan.options().typed_value_state("pathsBasePath"),
        ConfigOptionValueState::Value(&json!("/base"))
    );
    assert_eq!(
        plan.options().typed_value_state("rootDir"),
        ConfigOptionValueState::Value(&json!("/project/src"))
    );
    let ConfigOptionValueState::Object(paths) = plan.options().typed_value_state("paths") else {
        panic!("paths is a converted object value")
    };
    assert_eq!(
        paths.json_projection(),
        json!({
            "@plain/*": ["relative/*"],
            "@generated/*": ["/project/generated/*"],
        })
    );

    let raw_paths = plan.options().get("paths").expect("effective raw paths");
    assert_eq!(raw_paths.base_path, "/base");
    assert_eq!(
        raw_paths.value,
        json!({
            "@plain/*": ["relative/*"],
            "@generated/*": ["${configDir}/generated/*"],
        })
    );
}

#[test]
fn paths_own_property_view_keeps_javascript_order_and_undefined_values() {
    let plan = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request(
            "/project/tsconfig.json",
            r#"{
                "compilerOptions": {
                    "paths": {
                        "z": [foo, "ok"],
                        10: [],
                        "drop": foo,
                        2: [],
                        1e2: [],
                        0x10: [],
                        1: [],
                        "a": true
                    }
                },
                "files": ["root.ts"]
            }"#,
        ),
    )
    .expect("paths own-property plan");

    let error_codes = plan
        .errors()
        .iter()
        .map(|diagnostic| diagnostic.code())
        .collect::<Vec<_>>();
    assert_eq!(error_codes.iter().filter(|code| **code == 1327).count(), 5);
    assert_eq!(error_codes.iter().filter(|code| **code == 1328).count(), 2);
    let properties = plan
        .options()
        .typed_object_properties("paths")
        .expect("typed paths own properties");
    assert_eq!(
        properties
            .iter()
            .map(|property| property.name())
            .collect::<Vec<_>>(),
        ["1", "2", "10", "16", "100", "z", "drop", "a"]
    );
    assert_eq!(
        properties[5]
            .value()
            .map(tsc_program::ConfigTypedJsonValue::json_projection),
        Some(json!(["ok"]))
    );
    assert_eq!(properties[6].value(), None);
    assert_eq!(
        properties[7]
            .value()
            .map(tsc_program::ConfigTypedJsonValue::json_projection),
        Some(json!(true))
    );
    assert_eq!(
        plan.options()
            .typed_object_value("paths")
            .expect("typed paths object")
            .json_projection(),
        json!({
            "1": [],
            "2": [],
            "10": [],
            "16": [],
            "100": [],
            "z": ["ok"],
            "a": true,
        })
    );
}

#[test]
fn nested_paths_objects_keep_undefined_identity_for_compiler_option_cache_keys() {
    let plan = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request(
            "/project/tsconfig.json",
            r#"{
                "compilerOptions": {
                    "paths": {
                        "ordered": {"2": foo, "1": true, "drop": bar},
                        "undefinedOnly": {"drop": foo},
                        "empty": {}
                    }
                },
                "files": ["root.ts"]
            }"#,
        ),
    )
    .expect("nested paths object plan");

    let paths = plan
        .options()
        .typed_object_value("paths")
        .expect("typed paths object");
    let properties = paths.properties();
    let Some(tsc_program::ConfigTypedJsonValue::Object(ordered)) = properties[0].value() else {
        panic!("ordered path value is a typed nested object")
    };
    assert_eq!(
        ordered
            .properties()
            .iter()
            .map(|property| property.name())
            .collect::<Vec<_>>(),
        ["1", "2", "drop"]
    );
    assert_eq!(
        ordered.properties()[0]
            .value()
            .map(tsc_program::ConfigTypedJsonValue::json_projection),
        Some(json!(true))
    );
    assert_eq!(ordered.properties()[1].value(), None);
    assert_eq!(ordered.properties()[2].value(), None);

    let undefined_only = properties[1].value().expect("undefined-only nested object");
    let empty = properties[2].value().expect("empty nested object");
    assert_eq!(undefined_only.json_projection(), empty.json_projection());
    assert_ne!(undefined_only, empty);
    assert_eq!(
        paths.compiler_option_cache_identity(),
        "{ordered: {1: true2: undefineddrop: undefined}undefinedOnly: {drop: undefined}empty: {}}"
    );
}

#[test]
fn paths_json_projection_uses_javascript_number_stringification() {
    let plan = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request(
            "/project/tsconfig.json",
            r#"{
                "compilerOptions": {
                    "paths": {
                        "array": [1, -0, 1e999],
                        "positive": 1e999,
                        "negative": -1e999
                    }
                },
                "files": ["root.ts"]
            }"#,
        ),
    )
    .expect("non-finite paths values retain JavaScript identity");

    assert!(plan.errors().is_empty());
    let paths = plan
        .options()
        .typed_object_value("paths")
        .expect("typed paths object");
    assert_eq!(
        paths.json_projection(),
        json!({"array": [1, 0, null], "positive": null, "negative": null})
    );
    assert_eq!(
        paths.compiler_option_cache_identity(),
        "{array: [1,0,Infinity]positive: Infinitynegative: -Infinity}"
    );
}

#[test]
fn changed_paths_clone_routes_proto_through_the_fresh_object_setter() {
    let plan = parse_config_root_plan(
        &MemoryConfigHost::default(),
        request(
            "/project/tsconfig.json",
            r#"{
                "compilerOptions": {
                    "paths": {
                        "__proto__": null,
                        "__proto__": ["${configDir}/prototype/*"],
                        "x": []
                    }
                },
                "files": ["root.ts"]
            }"#,
        ),
    )
    .expect("changed paths proto plan");

    assert!(plan.errors().is_empty());
    let properties = plan
        .options()
        .typed_object_properties("paths")
        .expect("typed paths own properties");
    assert_eq!(
        properties
            .iter()
            .map(|property| property.name())
            .collect::<Vec<_>>(),
        ["x"]
    );
    assert_eq!(
        plan.options()
            .typed_object_value("paths")
            .expect("typed paths object")
            .json_projection(),
        json!({"x": []})
    );
}

#[test]
fn inherited_specs_are_rebased_into_raw_with_root_path_casing() {
    let host = MemoryConfigHost::default().case_insensitive().with_file(
        "c:/project/sub/base.json",
        r#"{"files":["Foo.ts"],"include":["generated/**/*.ts"]}"#,
    );

    let plan = parse_config_root_plan(
        &host,
        request(
            "C:/Project/tsconfig.json",
            r#"{"extends":"c:/project/sub/base.json"}"#,
        ),
    )
    .expect("case-insensitive inherited root plan");

    assert_eq!(plan.file_names(), ["C:/Project/sub/Foo.ts"]);
    assert_eq!(plan.raw()["files"], json!(["sub/Foo.ts"]));
    assert_eq!(plan.raw()["include"], json!(["sub/generated/**/*.ts"]));

    let drive_host =
        MemoryConfigHost::default().with_file("c:/B/base.json", r#"{"files":["x.ts"]}"#);
    let drive = parse_config_root_plan(
        &drive_host,
        request("C:/A/tsconfig.json", r#"{"extends":"c:/B/base.json"}"#),
    )
    .expect("drive roots compare without case on a case-sensitive host");
    assert_eq!(drive.raw()["files"], json!(["../B/x.ts"]));
    assert_eq!(drive.file_names(), ["C:/B/x.ts"]);

    let unc_host =
        MemoryConfigHost::default().with_file("//two/B/base.json", r#"{"files":["x.ts"]}"#);
    let unc = parse_config_root_plan(
        &unc_host,
        request(
            "//one/A/tsconfig.json",
            r#"{"extends":"//two/B/base.json"}"#,
        ),
    )
    .expect("different UNC server roots preserve the extended absolute path");
    assert_eq!(unc.raw()["files"], json!(["//two/B/x.ts"]));
    assert_eq!(unc.file_names(), ["//two/B/x.ts"]);

    let unicode_root_host =
        MemoryConfigHost::default().with_file("//ß/B/base.json", r#"{"files":["x.ts"]}"#);
    let unicode_root = parse_config_root_plan(
        &unicode_root_host,
        request("//ss/A/tsconfig.json", r#"{"extends":"//ß/B/base.json"}"#),
    )
    .expect("config roots use TypeScript's uppercase case-insensitive comparison");
    assert_eq!(unicode_root.raw()["files"], json!(["../B/x.ts"]));
    assert_eq!(unicode_root.file_names(), ["//ss/B/x.ts"]);
}

#[test]
fn inherited_url_specs_preserve_the_extended_directory_in_raw() {
    let host = MemoryConfigHost::default()
        .with_file("file:///root/sub/base.json", r#"{"files":["src.ts"]}"#);

    let plan = parse_config_root_plan(
        &host,
        request(
            "file:///root/tsconfig.json",
            r#"{"extends":"./sub/base.json"}"#,
        ),
    )
    .expect("a file-URL config inherits file specs without disk-path relativization");

    assert!(plan.errors().is_empty());
    assert_eq!(plan.raw()["files"], json!(["file:///root/sub/src.ts"]));
    assert_eq!(plan.file_names(), ["file:///root/sub/src.ts"]);
}

#[test]
fn exact_drive_root_extends_is_resolved_as_a_disk_path() {
    let host = MemoryConfigHost::default().with_file("c:", r#"{"files":["x.ts"]}"#);
    let plan = parse_config_root_plan(
        &host,
        request("/project/tsconfig.json", r#"{"extends":"c:"}"#),
    )
    .expect("an exact drive root is not a package specifier");

    assert!(plan.errors().is_empty());
    assert_eq!(host.requested_file_exists.borrow().as_slice(), ["c:"]);
    assert_eq!(host.requested_reads.borrow().as_slice(), ["c:"]);
    assert_eq!(plan.raw()["files"], json!(["c:/x.ts"]));
    assert_eq!(plan.file_names(), ["c:/x.ts"]);
}

#[test]
fn inherited_invalid_specs_follow_typescripts_raw_array_like_recovery() {
    let scalar = MemoryConfigHost::default().with_file("/project/base.json", r#"{"files":true}"#);
    let plan = parse_config_root_plan(
        &scalar,
        request("/project/tsconfig.json", r#"{"extends":"./base.json"}"#),
    )
    .expect("a truthy scalar extended files value maps to an empty raw array");
    assert_eq!(
        plan.errors()
            .iter()
            .map(|error| error.code())
            .collect::<Vec<_>>(),
        [5024]
    );
    assert_eq!(plan.raw()["files"], json!([]));
    assert!(plan.file_names().is_empty());
    assert!(scalar.requested_includes.borrow().is_empty());

    let string =
        MemoryConfigHost::default().with_file("/project/base.json", r#"{"files":"foo.ts"}"#);
    let plan = parse_config_root_plan(
        &string,
        request("/project/tsconfig.json", r#"{"extends":"./base.json"}"#),
    )
    .expect("a string extended files value maps through its array-like characters");
    assert_eq!(
        plan.errors()
            .iter()
            .map(|error| error.code())
            .collect::<Vec<_>>(),
        [5024]
    );
    assert_eq!(plan.raw()["files"], json!(["f", "o", "o", ".", "t", "s"]));
    assert_eq!(
        plan.file_names(),
        [
            "/project/f",
            "/project/o",
            "/project",
            "/project/t",
            "/project/s"
        ]
    );

    let null_element =
        MemoryConfigHost::default().with_file("/project/base.json", r#"{"files":[null]}"#);
    let plan = parse_config_root_plan(
        &null_element,
        request("/project/tsconfig.json", r#"{"extends":"./base.json"}"#),
    )
    .expect("a falsey raw array element maps through combinePaths as an empty path");
    assert!(plan.errors().is_empty());
    assert_eq!(plan.raw()["files"], json!([""]));
    assert_eq!(plan.file_names(), ["/project"]);
    assert!(null_element.requested_includes.borrow().is_empty());
}

#[test]
fn empty_path_specs_keep_each_typescript_host_boundary_distinct() {
    let literal = MemoryConfigHost::default();
    let plan = parse_config_root_plan(
        &literal,
        request("/project/tsconfig.json", r#"{"files":[""]}"#),
    )
    .expect("empty literal path");
    assert_eq!(plan.file_names(), ["/project"]);

    let output = MemoryConfigHost::default();
    let plan = parse_config_root_plan(
        &output,
        request(
            "/project/tsconfig.json",
            r#"{"compilerOptions":{"outDir":""}}"#,
        ),
    )
    .expect("empty output directory");
    assert_eq!(plan.discovery_options().out_dir(), Some("/project"));
    assert_eq!(
        output.requested_excludes.borrow()[0],
        Some(vec!["/project".to_owned()])
    );

    let include = MemoryConfigHost::default();
    parse_config_root_plan(
        &include,
        request("/project/tsconfig.json", r#"{"include":[""]}"#),
    )
    .expect("empty include host observation");
    assert_eq!(
        include.requested_includes.borrow()[0],
        Some(vec![String::new()])
    );
}

#[test]
fn explicit_empty_exclude_remains_distinct_from_an_absent_exclude() {
    let host = MemoryConfigHost::default();
    parse_config_root_plan(
        &host,
        request(
            "/project/tsconfig.json",
            r#"{"include":["src/**/*.ts"],"exclude":[]}"#,
        ),
    )
    .expect("explicit empty exclude");

    assert_eq!(host.requested_excludes.borrow()[0], Some(Vec::new()));

    let absent = MemoryConfigHost::default();
    parse_config_root_plan(
        &absent,
        request("/project/tsconfig.json", r#"{"include":["src/**/*.ts"]}"#),
    )
    .expect("absent exclude");
    assert_eq!(absent.requested_excludes.borrow()[0], None);
}

#[test]
fn root_plan_exposes_effective_file_include_and_exclude_specs() {
    let host = MemoryConfigHost::default().with_directory_files(&["/project/src/main.ts"]);
    let plan = parse_config_root_plan(
        &host,
        request(
            "/project/tsconfig.json",
            r#"{"files":["src/main.ts"],"include":["src/**/*.ts"],"exclude":["src/generated"]}"#,
        ),
    )
    .expect("root spec projection");

    assert_eq!(plan.files(), Some(["src/main.ts".to_owned()].as_slice()));
    assert_eq!(plan.include(), Some(["src/**/*.ts".to_owned()].as_slice()));
    assert_eq!(
        plan.exclude(),
        Some(["src/generated".to_owned()].as_slice())
    );
}

#[test]
fn root_plan_retains_noninherited_references_and_inherited_root_settings() {
    let host = MemoryConfigHost::default()
        .with_file(
            "/project/base.json",
            r#"{"references":[{"path":"base"}],"watchOptions":{"watchFile":"useFsEvents"},"typeAcquisition":{"include":["jest"]},"compileOnSave":true}"#,
        )
        .with_file("/project/main.ts", "const value = 1;");
    let plan = parse_config_root_plan(
        &host,
        request(
            "/project/tsconfig.json",
            r#"{"extends":"./base.json","files":["main.ts"]}"#,
        ),
    )
    .expect("root metadata projection");

    assert!(plan.references().is_none());
    assert_eq!(
        plan.watch_options(),
        Some(&json!({"watchFile": "useFsEvents"}))
    );
    assert_eq!(plan.type_acquisition(), Some(&json!({"include": ["jest"]})));
    assert_eq!(plan.compile_on_save(), Some(&json!(true)));
    assert_eq!(plan.raw()["compileOnSave"], json!(true));
}

#[test]
fn own_falsey_root_settings_mask_inherited_unsupported_scopes() {
    let host = MemoryConfigHost::default().with_file(
        "/project/base.json",
        r#"{"watchOptions":{"watchFile":"useFsEvents"},"typeAcquisition":{"include":["jest"]},"compileOnSave":true}"#,
    );
    let plan = parse_config_root_plan(
        &host,
        request(
            "/project/tsconfig.json",
            r#"{"extends":"./base.json","watchOptions":false,"typeAcquisition":null,"compileOnSave":false,"files":["main.ts"]}"#,
        ),
    )
    .expect("falsey own root settings remain observable");

    assert_eq!(plan.watch_options(), Some(&json!(false)));
    assert_eq!(plan.type_acquisition(), Some(&json!(null)));
    assert_eq!(plan.compile_on_save(), Some(&json!(false)));
    assert_eq!(plan.unsupported_root_scopes().next(), None);
}

#[test]
fn case_only_extended_source_spellings_remain_observable_without_a_cache() {
    let host = MemoryConfigHost::default().case_insensitive().with_file(
        "/project/base.json",
        r#"{"compilerOptions":{"strict":true}}"#,
    );

    let plan = parse_config_root_plan(
        &host,
        request(
            "/project/tsconfig.json",
            r#"{"extends":["./BASE.json","./base.json"]}"#,
        ),
    )
    .expect("case-only extended source identities");

    assert_eq!(
        plan.extended_sources()
            .iter()
            .map(|source| source.file_name.as_str())
            .collect::<Vec<_>>(),
        ["/project/BASE.json", "/project/base.json"]
    );
}

#[test]
fn host_failures_retain_the_typed_operation_and_error_source() {
    let host = MemoryConfigHost::default().with_directory_error("/project");
    let error = parse_config_root_plan(&host, request("/project/tsconfig.json", "{}"))
        .expect_err("directory failure must remain typed");

    assert_eq!(error.kind(), ConfigParseErrorKind::Host);
    let host_error = error.host_error().expect("typed host error");
    assert_eq!(host_error.operation(), ConfigHostOperation::ReadDirectory);
    assert_eq!(host_error.path(), "/project");
    assert!(std::error::Error::source(&error).is_some());
}

#[test]
fn package_exports_use_source_condition_order_and_block_legacy_fallback() {
    let host = MemoryConfigHost::default()
        .with_file(
            "/project/node_modules/foo/package.json",
            r#"{"exports":{".":{"default":"./default.json","types":"./types.json"},"./private.json":null}}"#,
        )
        .with_file(
            "/project/node_modules/foo/default.json",
            r#"{"compilerOptions":{"strict":true}}"#,
        )
        .with_file(
            "/project/node_modules/foo/types.json",
            r#"{"compilerOptions":{"strict":false}}"#,
        )
        .with_file(
            "/project/node_modules/foo/private.json",
            r#"{"compilerOptions":{"allowJs":true}}"#,
        )
        .with_directory_files(&["/project/main.ts"]);

    let plan = parse_config_root_plan(
        &host,
        request("/project/tsconfig.json", r#"{"extends":"foo"}"#),
    )
    .expect("ordered package condition");
    assert_eq!(plan.options().get("strict").unwrap().value, json!(true));
    assert_eq!(
        plan.extended_sources()[0].file_name,
        "/project/node_modules/foo/default.json"
    );

    let blocked = parse_config_root_plan(
        &host,
        request(
            "/project/tsconfig.json",
            r#"{"extends":"foo/private.json"}"#,
        ),
    )
    .expect("null export block returns a partial plan");
    assert_eq!(blocked.errors()[0].code(), 6053);
}

#[test]
fn package_imports_resolve_config_targets() {
    let host = MemoryConfigHost::default()
        .with_file(
            "/project/package.json",
            r##"{"imports":{"#base":"./configs/base.json"}}"##,
        )
        .with_file(
            "/project/configs/base.json",
            r#"{"compilerOptions":{"strict":true}}"#,
        )
        .with_directory_files(&["/project/main.ts"]);

    let plan = parse_config_root_plan(
        &host,
        request("/project/tsconfig.json", r##"{"extends":"#base"}"##),
    )
    .expect("package imports config target");

    assert_eq!(plan.options().get("strict").unwrap().value, json!(true));
    assert_eq!(
        plan.extended_sources()[0].file_name,
        "/project/configs/base.json"
    );

    let missing_import = MemoryConfigHost::default().with_file(
        "/project/node_modules/#missing.json",
        r#"{"compilerOptions":{"allowJs":true}}"#,
    );
    let plan = parse_config_root_plan(
        &missing_import,
        request("/project/tsconfig.json", r##"{"extends":"#missing"}"##),
    )
    .expect("an inapplicable imports lookup falls through to node_modules");
    assert_eq!(plan.options().get("allowJs").unwrap().value, json!(true));

    let missing_target = MemoryConfigHost::default()
        .with_file(
            "/project/package.json",
            r##"{"imports":{"#base":"./missing.json"}}"##,
        )
        .with_file(
            "/project/node_modules/#base.json",
            r#"{"compilerOptions":{"allowJs":true}}"#,
        );
    let plan = parse_config_root_plan(
        &missing_target,
        request("/project/tsconfig.json", r##"{"extends":"#base"}"##),
    )
    .expect("a missing imports target falls through to node_modules");
    assert_eq!(plan.options().get("allowJs").unwrap().value, json!(true));
    assert_eq!(
        missing_target
            .requested_file_exists
            .borrow()
            .iter()
            .filter(|path| path.as_str() == "/project/package.json")
            .count(),
        2
    );
    assert_eq!(
        missing_target
            .requested_reads
            .borrow()
            .iter()
            .filter(|path| path.as_str() == "/project/package.json")
            .count(),
        2
    );

    let blocked_import = MemoryConfigHost::default()
        .with_file("/project/package.json", r##"{"imports":{"#base":null}}"##)
        .with_file(
            "/project/node_modules/#base.json",
            r#"{"compilerOptions":{"allowJs":true}}"#,
        );
    let plan = parse_config_root_plan(
        &blocked_import,
        request("/project/tsconfig.json", r##"{"extends":"#base"}"##),
    )
    .expect("a null imports target block returns a partial plan");
    assert_eq!(plan.errors()[0].code(), 6053);

    let bare_target = MemoryConfigHost::default()
        .with_file("/project/package.json", r##"{"imports":{"#base":"cfg"}}"##)
        .with_file(
            "/project/node_modules/cfg/package.json",
            r#"{"tsconfig":"./base.json"}"#,
        )
        .with_file(
            "/project/node_modules/cfg/base.json",
            r#"{"compilerOptions":{"strict":true}}"#,
        );
    let plan = parse_config_root_plan(
        &bare_target,
        request("/project/tsconfig.json", r##"{"extends":"#base"}"##),
    )
    .expect("bare imports package-field refusal returns a partial plan");
    assert_eq!(plan.errors()[0].code(), 6053);

    let bare_export = MemoryConfigHost::default()
        .with_file("/project/package.json", r##"{"imports":{"#base":"cfg"}}"##)
        .with_file(
            "/project/node_modules/cfg/package.json",
            r#"{"exports":"./base.json"}"#,
        )
        .with_file(
            "/project/node_modules/cfg/base.json",
            r#"{"compilerOptions":{"strict":true}}"#,
        )
        .with_directory_files(&["/project/main.ts"]);
    let plan = parse_config_root_plan(
        &bare_export,
        request("/project/tsconfig.json", r##"{"extends":"#base"}"##),
    )
    .expect("bare imports re-entry may use JSON exports");
    assert_eq!(plan.options().get("strict").unwrap().value, json!(true));

    let lexical_bare = MemoryConfigHost::default()
        .with_file(
            "/project/package.json",
            r##"{"imports":{"#base":"foo/../bar.json"}}"##,
        )
        .with_file(
            "/project/node_modules/bar.json",
            r#"{"compilerOptions":{"strict":true}}"#,
        );
    let plan = parse_config_root_plan(
        &lexical_bare,
        request("/project/tsconfig.json", r##"{"extends":"#base"}"##),
    )
    .expect("bare imports targets normalize before their JSON module load");
    assert_eq!(plan.options().get("strict").unwrap().value, json!(true));

    for target in ["./configs/base", "cfg/base"] {
        let package = format!(r##"{{"imports":{{"#base":"{target}"}}}}"##);
        let host = MemoryConfigHost::default()
            .with_file("/project/package.json", &package)
            .with_file(
                if target.starts_with('.') {
                    "/project/configs/base.json"
                } else {
                    "/project/node_modules/cfg/base.json"
                },
                r#"{"compilerOptions":{"strict":true}}"#,
            );
        let plan = parse_config_root_plan(
            &host,
            request("/project/tsconfig.json", r##"{"extends":"#base"}"##),
        )
        .expect("implicit-extension refusal returns a partial plan");
        assert_eq!(plan.errors()[0].code(), 6053);
    }
}
