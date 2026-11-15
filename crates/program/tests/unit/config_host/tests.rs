use super::*;
use serde_json::{json, Value};

#[test]
fn directory_exclusions_follow_each_include_base() {
    let host = tsc_host::MemoryCompilerHost::builder("/work")
        .file("/work/src/keep.ts", Vec::new())
        .file("/work/src/deep/drop.test.ts", Vec::new())
        .file("/work/generated/drop.ts", Vec::new())
        .file("/outside/src/keep.ts", Vec::new())
        .file("/outside/src/drop.ts", Vec::new())
        .build()
        .unwrap();
    let includes = [
        JsString::from("src/**/*.ts"),
        JsString::from("/outside/src/**/*.ts"),
    ];
    let excludes = [
        JsString::from("generated/**/*"),
        JsString::from("src/**/*.test.ts"),
        JsString::from("/outside/src/drop.ts"),
    ];
    let files = CompilerConfigHost::new(&host)
        .read_directory(
            "/work".into(),
            &[".ts"],
            Some(&excludes),
            Some(&includes),
            None,
        )
        .unwrap();
    assert_eq!(
        files,
        [
            JsString::from("/work/src/keep.ts"),
            JsString::from("/outside/src/keep.ts"),
        ]
    );
}

#[test]
fn config_host_keeps_js_directory_keys_patterns_and_read_names() {
    let base = JsString::from_code_units(&[0x2f, 0x77, 0x6f, 0x72, 0x6b, 0x2f, 0xd800]);
    let leaves = [0xd800, 0xd801, 0xfffd].map(|unit| {
        let mut path = base.clone();
        path.push('/');
        path.push_code_unit(unit);
        path.push_str("/a.ts");
        path
    });
    let mut builder = tsc_host::MemoryCompilerHost::builder_js("/work");
    for (index, path) in leaves.iter().enumerate() {
        builder = builder.file_js(path, format!("source {index}").into_bytes());
    }
    let host = builder.build().unwrap();
    let config = CompilerConfigHost::new(&host);
    let mut first_pattern = JsString::from_code_units(&[0xd801]);
    first_pattern.push_str("/**/*.ts");
    let includes = [first_pattern.clone(), JsString::from("**/*.ts")];
    assert_eq!(
        config
            .read_directory(base.as_js(), &[".ts"], None, Some(&includes), None)
            .unwrap(),
        [leaves[1].clone(), leaves[0].clone(), leaves[2].clone()]
    );
    assert_eq!(
        config
            .read_directory(
                base.as_js(),
                &[".ts"],
                Some(&[first_pattern]),
                Some(&includes),
                None
            )
            .unwrap(),
        [leaves[0].clone(), leaves[2].clone()]
    );
    for (index, path) in leaves.iter().enumerate() {
        assert!(config.file_exists(path.as_js()).unwrap());
        assert_eq!(
            config.read_file(path.as_js()).unwrap(),
            Some(format!("source {index}"))
        );
    }
}

#[test]
fn config_discovery_base_paths_match_native_reference() {
    let oracle: Value = serde_json::from_slice(include_bytes!(
        "../../fixtures/h2-8b-config-discovery-paths.json"
    ))
    .expect("frozen base path observations");
    assert_path_cases(&oracle, 16);
}

#[test]
fn config_discovery_spelling_matches_typescript() {
    let oracle: Value = serde_json::from_slice(include_bytes!(
        "../../fixtures/h2-8b-config-discovery-spelling.json"
    ))
    .expect("frozen spelling observations");
    assert_path_cases(&oracle, 6);
}

fn assert_path_cases(oracle: &Value, count: usize) {
    assert_eq!(oracle["typescript"], "6.0.3");
    let cases = oracle["cases"].as_array().expect("path cases");
    assert_eq!(cases.len(), count);
    let mut failures = Vec::new();
    for case in cases {
        let includes = case["includes"].as_array().map(|values| {
            values
                .iter()
                .map(|value| JsString::from(value.as_str().expect("include")))
                .collect::<Vec<_>>()
        });
        for repetition in 1..=2 {
            let actual = discovery_base_paths(
                case["directory"].as_str().expect("directory").into(),
                includes.as_deref(),
                case["case_sensitive"].as_bool().expect("case policy"),
            )
            .expect("valid discovery paths");
            let actual = actual
                .iter()
                .map(|path| {
                    path.as_str()
                        .expect("these frozen controls contain scalar path values")
                })
                .collect::<Vec<_>>();
            // Preserve the frozen JS oracle's provenance. This native
            // reference intentionally uses byte order for this one case.
            let expected = if case["case_id"] == "utf16-order" {
                json!(["/project/app", "/project/\u{e000}", "/project/\u{10000}"])
            } else {
                case["base_paths"].clone()
            };
            let exact = json!(actual) == expected;
            eprintln!(
                "H2.8b-CFG1b-path {}",
                json!({
                    "case_id": case["case_id"], "repetition": repetition, "actual": actual, "exact": exact,
                })
            );
            if !exact {
                failures.push(format!("{}: {:?} != {}", case["case_id"], actual, expected));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn native_include_base_order_separates_roots_case_profiles_and_dynamic_names() {
    use std::cmp::Ordering::{Equal, Greater, Less};
    for (left, right, sensitive, expected) in [
        ("/x/\u{e000}", "/x/\u{10000}", true, Less),
        ("/x/\u{e000}", "/x/\u{10000}", false, Less),
        ("/x/_", "/x/a", false, Less),
        ("/x/Z", "/x/a", false, Greater),
        ("/x/Z", "/x/a", true, Less),
        ("c:/Z", "C:/a", true, Less),
        ("/x/\u{130}", "/x/i", false, Equal),
        ("/x/e\u{301}", "/x/\u{e9}", false, Less),
        (
            "^/~ts-uri~/untitled/B",
            "^/~ts-uri~/untitled/a",
            false,
            Less,
        ),
        ("^/untitled/B", "^/untitled/a", false, Greater),
    ] {
        assert_eq!(
            discovery_base_order(left.into(), right.into(), sensitive),
            expected,
            "{left:?}, {right:?}, case-sensitive={sensitive}"
        );
    }
}

#[test]
fn native_config_root_order_matches_pinned_cli_fixtures() {
    let manifest: Value = serde_json::from_slice(include_bytes!(
        "../../fixtures/native-name-order/cases.json"
    ))
    .unwrap();
    assert_eq!(manifest["reference"], "pinned tsgo 7.1");
    for case in manifest["cases"].as_array().unwrap() {
        let mut builder = tsc_host::MemoryCompilerHost::builder("/fixture")
            .case_sensitive(case["case_sensitive"].as_bool().unwrap());
        for (name, text) in case["files"].as_object().unwrap() {
            builder = builder.file(
                format!("/fixture/{name}"),
                text.as_str().unwrap().as_bytes(),
            );
        }
        let text = serde_json::to_string(&case["config"]).unwrap();
        let host = builder
            .file("/fixture/tsconfig.json", text.as_bytes())
            .build()
            .unwrap();
        let plan = crate::parse_config_root_plan(
            &CompilerConfigHost::new(&host),
            crate::ConfigRootPlanRequest {
                file_name: "/fixture/tsconfig.json".into(),
                text,
                base_path: "/fixture".into(),
            },
        )
        .unwrap();
        let names = plan
            .file_names()
            .iter()
            .map(|name| {
                format!(
                    "./{}",
                    name.as_str().unwrap().strip_prefix("/fixture/").unwrap()
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(json!(names), case["expected_roots"], "{}", case["name"]);
    }
}
#[test]
fn config_discovery_reports_directory_failures_in_walk_order_not_include_order() {
    use tsc_host::{HostErrorKind, HostOperation, MemoryCompilerHost};

    let mut builder = MemoryCompilerHost::builder("/repo")
        .file("/repo/a/input.ts", b"export {};".to_vec())
        .file("/repo/b/input.ts", b"export {};".to_vec());
    for directory in ["/repo/a", "/repo/b"] {
        builder = builder.failure(HostError::new_js(
            HostErrorKind::PermissionDenied,
            HostOperation::ReadDirectory,
            Some(JsStr::from_str(directory)),
            "unreadable directory",
        ));
    }
    let host = builder.build().unwrap();
    let includes = [JsString::from("b/**/*.ts"), JsString::from("a/**/*.ts")];
    let error = CompilerConfigHost::new(&host)
        .read_directory("/repo".into(), &[".ts"], None, Some(&includes), None)
        .unwrap_err();
    assert_eq!(error.operation(), ConfigHostOperation::ReadDirectory);
    assert_eq!(error.path(), JsStr::from_str("/repo/a"));
}

macro_rules! config_host_observations {
    () => {
        fn use_case_sensitive_file_names(&self) -> bool {
            self.inner.use_case_sensitive_file_names()
        }
        fn file_exists(&self, path: JsStr<'_>) -> Result<bool, ConfigHostError> {
            self.inner.file_exists(path)
        }
        fn directory_exists(&self, path: JsStr<'_>) -> Result<bool, ConfigHostError> {
            self.inner.directory_exists(path)
        }
        fn read_file(&self, path: JsStr<'_>) -> Result<Option<String>, ConfigHostError> {
            self.inner.read_file(path)
        }
        fn realpath(&self, path: JsStr<'_>) -> Result<Option<JsString>, ConfigHostError> {
            self.inner.realpath(path)
        }
    };
}

struct LegacyDiscovery<'a> {
    inner: CompilerConfigHost<'a>,
    reads: std::cell::Cell<usize>,
}

impl ConfigParseHost for LegacyDiscovery<'_> {
    config_host_observations!();
    fn read_directory(
        &self,
        directory: JsStr<'_>,
        extensions: &[&str],
        excludes: Option<&[JsString]>,
        includes: Option<&[JsString]>,
        depth: Option<usize>,
    ) -> Result<Vec<JsString>, ConfigHostError> {
        self.reads.set(self.reads.get() + 1);
        self.inner
            .read_directory(directory, extensions, excludes, includes, depth)
    }
}

struct BrokenDiscoveryProof<'a> {
    inner: CompilerConfigHost<'a>,
    wrong_length: bool,
}

impl ConfigParseHost for BrokenDiscoveryProof<'_> {
    config_host_observations!();
    fn read_directory(
        &self,
        directory: JsStr<'_>,
        extensions: &[&str],
        excludes: Option<&[JsString]>,
        includes: Option<&[JsString]>,
        depth: Option<usize>,
    ) -> Result<Vec<JsString>, ConfigHostError> {
        self.inner
            .read_directory(directory, extensions, excludes, includes, depth)
    }
    fn read_directory_with_include_matches(
        &self,
        directory: JsStr<'_>,
        extensions: &[&str],
        excludes: Option<&[JsString]>,
        includes: Option<&[JsString]>,
        depth: Option<usize>,
    ) -> Result<ConfigDirectoryListing, ConfigHostError> {
        let mut listing = self.inner.read_directory_with_include_matches(
            directory, extensions, excludes, includes, depth,
        )?;
        listing.include_spec_indices = Some(if self.wrong_length {
            vec![0; listing.file_names.len() + 1]
        } else {
            vec![usize::MAX; listing.file_names.len()]
        });
        Ok(listing)
    }
}

#[test]
fn discovery_provenance_preserves_reasons_and_original_host_callbacks() {
    let cases = [
        (
            json!({"compilerOptions":{"allowJs":true}, "include":["*.js", "*.ts"]}),
            1,
        ),
        (
            json!({"compilerOptions":{"resolveJsonModule":true}, "include":["**/*", "*.json"]}),
            1,
        ),
        (json!({"include":["**", "*.ts"]}), 1),
        (json!({"include":["missing/**/*", "*.ts"]}), 1),
        (
            json!({"files":["ALIAS.ts", "alias.ts"], "include":["*.ts"]}),
            1,
        ),
        (json!({"compilerOptions":{}}), 1),
        (json!({"files":["choice.ts"]}), 0),
        (json!({"extends":"./base/parent.json"}), 1),
    ];
    for sensitive in [true, false] {
        for (config, expected_callbacks) in &cases {
            let text = serde_json::to_string(config).unwrap();
            let host = tsc_host::MemoryCompilerHost::builder("/fixture")
                .case_sensitive(sensitive)
                .file("/fixture/tsconfig.json", text.as_bytes())
                .file("/fixture/choice.js", b"export {};".to_vec())
                .file("/fixture/choice.ts", b"export {};".to_vec())
                .file("/fixture/alias.ts", b"export {};".to_vec())
                .file("/fixture/data.json", b"{}".to_vec())
                .file(
                    "/fixture/base/parent.json",
                    br#"{"include":["src/**/*.ts"]}"#.to_vec(),
                )
                .file("/fixture/base/src/inherited.ts", b"export {};".to_vec())
                .build()
                .unwrap();
            let native = CompilerConfigHost::new(&host);
            let legacy = LegacyDiscovery {
                inner: native,
                reads: std::cell::Cell::new(0),
            };
            let request = || crate::ConfigRootPlanRequest {
                file_name: "/fixture/tsconfig.json".into(),
                text: text.clone(),
                base_path: "/fixture".into(),
            };
            let expected = crate::parse_config_root_plan(&legacy, request()).unwrap();
            assert_eq!(legacy.reads.get(), *expected_callbacks, "{config}");
            let bad_length = BrokenDiscoveryProof {
                inner: native,
                wrong_length: true,
            };
            let bad_index = BrokenDiscoveryProof {
                inner: native,
                wrong_length: false,
            };
            for provider in [&native as &dyn ConfigParseHost, &bad_length, &bad_index] {
                let actual = crate::parse_config_root_plan(provider, request()).unwrap();
                assert_eq!(
                    actual.file_names(),
                    expected.file_names(),
                    "{config}, case={sensitive}"
                );
                assert_eq!(
                    actual.root_reasons(),
                    expected.root_reasons(),
                    "{config}, case={sensitive}"
                );
                assert_eq!(
                    actual
                        .errors()
                        .iter()
                        .map(|d| (d.code(), d.start, d.length))
                        .collect::<Vec<_>>(),
                    expected
                        .errors()
                        .iter()
                        .map(|d| (d.code(), d.start, d.length))
                        .collect::<Vec<_>>()
                );
            }
        }
    }
}

#[test]
fn discovery_provenance_keeps_unpaired_include_and_file_spellings() {
    let text = r#"{"include":["\ud800/**/*.ts","**/*.ts"]}"#.to_owned();
    let mut file = JsString::from("/fixture/");
    file.push_code_unit(0xd800);
    file.push_str("/source.ts");
    for sensitive in [true, false] {
        let host = tsc_host::MemoryCompilerHost::builder_js("/fixture")
            .case_sensitive(sensitive)
            .file_js("/fixture/tsconfig.json", text.as_bytes().to_vec())
            .file_js(file.clone(), b"export {};".to_vec())
            .file_js("/fixture/other.ts", b"export {};".to_vec())
            .build()
            .unwrap();
        let native = CompilerConfigHost::new(&host);
        let legacy = LegacyDiscovery {
            inner: native,
            reads: std::cell::Cell::new(0),
        };
        let request = || crate::ConfigRootPlanRequest {
            file_name: "/fixture/tsconfig.json".into(),
            text: text.clone(),
            base_path: "/fixture".into(),
        };
        let expected = crate::parse_config_root_plan(&legacy, request()).unwrap();
        let actual = crate::parse_config_root_plan(&native, request()).unwrap();
        assert_eq!(legacy.reads.get(), 1);
        assert!(actual.file_names().contains(&file));
        assert_eq!(actual.file_names(), expected.file_names());
        assert_eq!(actual.root_reasons(), expected.root_reasons());
        assert_eq!(
            actual
                .errors()
                .iter()
                .map(|d| (d.code(), d.start, d.length))
                .collect::<Vec<_>>(),
            expected
                .errors()
                .iter()
                .map(|d| (d.code(), d.start, d.length))
                .collect::<Vec<_>>()
        );
    }
}

struct ObservedDiscoveryProof<'a> {
    inner: CompilerConfigHost<'a>,
    reads: std::cell::Cell<usize>,
    proofs: std::cell::Cell<usize>,
}

impl ConfigParseHost for ObservedDiscoveryProof<'_> {
    config_host_observations!();
    fn read_directory(
        &self,
        directory: JsStr<'_>,
        extensions: &[&str],
        excludes: Option<&[JsString]>,
        includes: Option<&[JsString]>,
        depth: Option<usize>,
    ) -> Result<Vec<JsString>, ConfigHostError> {
        self.reads.set(self.reads.get() + 1);
        self.inner
            .read_directory(directory, extensions, excludes, includes, depth)
    }
    fn read_directory_with_include_matches(
        &self,
        directory: JsStr<'_>,
        extensions: &[&str],
        excludes: Option<&[JsString]>,
        includes: Option<&[JsString]>,
        depth: Option<usize>,
    ) -> Result<ConfigDirectoryListing, ConfigHostError> {
        self.proofs.set(self.proofs.get() + 1);
        self.inner
            .read_directory_with_include_matches(directory, extensions, excludes, includes, depth)
    }
}

#[test]
fn implicit_default_include_does_not_request_match_provenance() {
    for (text, proofs) in [(r#"{}"#, 0), (r#"{"include":["**/*"]}"#, 1)] {
        let host = tsc_host::MemoryCompilerHost::builder("/fixture")
            .file("/fixture/tsconfig.json", text.as_bytes())
            .file("/fixture/source.ts", b"export {};".to_vec())
            .build()
            .unwrap();
        let observed = ObservedDiscoveryProof {
            inner: CompilerConfigHost::new(&host),
            reads: std::cell::Cell::new(0),
            proofs: std::cell::Cell::new(0),
        };
        let plan = crate::parse_config_root_plan(
            &observed,
            crate::ConfigRootPlanRequest {
                file_name: "/fixture/tsconfig.json".into(),
                text: text.to_owned(),
                base_path: "/fixture".into(),
            },
        )
        .unwrap();
        assert_eq!(plan.file_names(), [JsString::from("/fixture/source.ts")]);
        assert_eq!(observed.proofs.get(), proofs);
        assert_eq!(observed.reads.get(), 1 - proofs);
    }
}

struct SpelledDiscoveryProof<'a> {
    inner: CompilerConfigHost<'a>,
    spelling: &'a str,
    proof: bool,
}

impl ConfigParseHost for SpelledDiscoveryProof<'_> {
    config_host_observations!();
    fn read_directory(
        &self,
        _directory: JsStr<'_>,
        _extensions: &[&str],
        _excludes: Option<&[JsString]>,
        _includes: Option<&[JsString]>,
        _depth: Option<usize>,
    ) -> Result<Vec<JsString>, ConfigHostError> {
        Ok(vec![self.spelling.into()])
    }
    fn read_directory_with_include_matches(
        &self,
        directory: JsStr<'_>,
        extensions: &[&str],
        excludes: Option<&[JsString]>,
        includes: Option<&[JsString]>,
        depth: Option<usize>,
    ) -> Result<ConfigDirectoryListing, ConfigHostError> {
        Ok(ConfigDirectoryListing {
            file_names: self.read_directory(directory, extensions, excludes, includes, depth)?,
            include_spec_indices: self.proof.then_some(vec![0]),
        })
    }
}

#[test]
fn noncanonical_discovery_spellings_keep_the_original_reason_matcher() {
    let text = r#"{"include":["first/*.ts","second/*.ts"]}"#;
    let host = tsc_host::MemoryCompilerHost::builder("/fixture")
        .file("/fixture/tsconfig.json", text.as_bytes())
        .file("/fixture/second/source.ts", b"export {};".to_vec())
        .build()
        .unwrap();
    for spelling in [
        "/fixture/first/../second/source.ts",
        r"/fixture\second\source.ts",
    ] {
        let request = || crate::ConfigRootPlanRequest {
            file_name: "/fixture/tsconfig.json".into(),
            text: text.to_owned(),
            base_path: "/fixture".into(),
        };
        let legacy = SpelledDiscoveryProof {
            inner: CompilerConfigHost::new(&host),
            spelling,
            proof: false,
        };
        let claimed = SpelledDiscoveryProof {
            inner: CompilerConfigHost::new(&host),
            spelling,
            proof: true,
        };
        let expected = crate::parse_config_root_plan(&legacy, request()).unwrap();
        let actual = crate::parse_config_root_plan(&claimed, request()).unwrap();
        assert_eq!(actual.file_names(), expected.file_names());
        assert_eq!(actual.root_reasons(), expected.root_reasons());
    }
}
