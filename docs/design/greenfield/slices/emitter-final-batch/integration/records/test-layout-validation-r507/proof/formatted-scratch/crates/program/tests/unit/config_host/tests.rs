
use super::*;
use serde_json::{json, Value};

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
fn config_discovery_base_paths_match_typescript() {
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
            let exact = json!(actual) == case["base_paths"];
            eprintln!(
                "H2.8b-CFG1b-path {}",
                json!({
                    "case_id": case["case_id"], "repetition": repetition, "actual": actual, "exact": exact,
                })
            );
            if !exact {
                failures.push(format!(
                    "{}: {:?} != {}",
                    case["case_id"], actual, case["base_paths"]
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
