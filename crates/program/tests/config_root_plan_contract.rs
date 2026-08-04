use std::cell::RefCell;
use std::collections::BTreeMap;

use serde_json::json;
use tsc_program::{
    parse_config_root_plan, ConfigHostError, ConfigHostOperation, ConfigParseErrorKind,
    ConfigParseHost, ConfigRootPlanRequest,
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
            r#"{"compilerOptions":{"allowJs":false}}"#,
        ),
    )
    .expect("overridden jsconfig root plan");

    assert_eq!(plan.file_names(), ["/project/main.ts"]);
    assert_eq!(plan.options().get("allowJs").unwrap().value, json!(false));
    assert!(!host.requested_extensions.borrow()[0]
        .iter()
        .any(|extension| extension == ".js"));
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
fn invalid_recursive_specs_fail_before_directory_observation() {
    for include in ["src/**", "**/../src/*.ts"] {
        let host = MemoryConfigHost::default().with_directory_files(&["/project/src/main.ts"]);
        let text = format!(r#"{{"include":["{include}"]}}"#);

        let error = parse_config_root_plan(&host, request("/project/tsconfig.json", &text))
            .expect_err("invalid recursive spec must fail closed");

        assert_eq!(error.kind(), ConfigParseErrorKind::InvalidConfig);
        assert!(host.requested_extensions.borrow().is_empty());
    }
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
fn circular_extends_fails_with_a_typed_error() {
    let host = MemoryConfigHost::default()
        .with_file("/project/tsconfig.json", r#"{"extends":"./base.json"}"#)
        .with_file("/project/base.json", r#"{"extends":"./tsconfig.json"}"#);

    let error = parse_config_root_plan(
        &host,
        request("/project/tsconfig.json", r#"{"extends":"./base.json"}"#),
    )
    .expect_err("cycle must fail closed");

    assert_eq!(error.kind(), ConfigParseErrorKind::CircularExtends);
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
}

#[test]
fn explicit_missing_json_extends_observes_the_read_boundary() {
    let host = MemoryConfigHost::default();
    let error = parse_config_root_plan(
        &host,
        request("/project/tsconfig.json", r#"{"extends":"./missing.json"}"#),
    )
    .expect_err("missing explicit JSON config");

    assert_eq!(error.kind(), ConfigParseErrorKind::MissingExtends);
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
        let error = parse_config_root_plan(&host, request("/project/tsconfig.json", &config))
            .expect_err("JSON config lookup must not replace unrelated written families");
        assert_eq!(error.kind(), ConfigParseErrorKind::MissingExtends);
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
    let error = parse_config_root_plan(
        &imports,
        request("/project/tsconfig.json", r##"{"extends":"#base"}"##),
    )
    .expect_err("bare imports re-entry is not a config lookup");
    assert_eq!(error.kind(), ConfigParseErrorKind::MissingExtends);
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
    let error = parse_config_root_plan(
        &exports,
        request("/project/tsconfig.json", r#"{"extends":"foo/../bar"}"#),
    )
    .expect_err("an exports map still owns and blocks the lexical package request");
    assert_eq!(error.kind(), ConfigParseErrorKind::MissingExtends);
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
    let error = parse_config_root_plan(&cycle_host, request("/project/tsconfig.json", cycle_text))
        .expect_err("exact current-directory config is a self cycle");
    assert_eq!(error.kind(), ConfigParseErrorKind::CircularExtends);
}

#[test]
fn config_dir_templates_use_the_root_config_directory() {
    let host = MemoryConfigHost::default()
        .with_file(
            "/base/base.json",
            r#"{
                "compilerOptions":{"outDir":"${configDir}/dist"},
                "include":["${configDir}/src/**/*.ts"]
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

    let error = parse_config_root_plan(
        &host,
        request(
            "/project/tsconfig.json",
            r#"{"extends":"foo/private.json"}"#,
        ),
    )
    .expect_err("null export blocks the physical subpath");
    assert_eq!(error.kind(), ConfigParseErrorKind::MissingExtends);
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
    let error = parse_config_root_plan(
        &blocked_import,
        request("/project/tsconfig.json", r##"{"extends":"#base"}"##),
    )
    .expect_err("a null imports target blocks node_modules fallback");
    assert_eq!(error.kind(), ConfigParseErrorKind::MissingExtends);

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
    let error = parse_config_root_plan(
        &bare_target,
        request("/project/tsconfig.json", r##"{"extends":"#base"}"##),
    )
    .expect_err("bare imports re-entry does not use config package fields");
    assert_eq!(error.kind(), ConfigParseErrorKind::MissingExtends);

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
        let error = parse_config_root_plan(
            &host,
            request("/project/tsconfig.json", r##"{"extends":"#base"}"##),
        )
        .expect_err("imports targets do not gain an implicit JSON extension");
        assert_eq!(error.kind(), ConfigParseErrorKind::MissingExtends);
    }
}
