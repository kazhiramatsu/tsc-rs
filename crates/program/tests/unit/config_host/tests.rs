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
