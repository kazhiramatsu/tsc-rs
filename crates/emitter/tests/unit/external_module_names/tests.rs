use std::path::{Path, PathBuf};

use serde_json::Value;
use tsc_program::SourceFileId;
use tsc_types::CompilerOptions;

use super::*;

struct Host {
    cwd: PathBuf,
    common: PathBuf,
    case_sensitive: bool,
    options: CompilerOptions,
}

impl EmitHost for Host {
    fn compiler_options(&self) -> &CompilerOptions {
        &self.options
    }
    fn current_directory(&self) -> &Path {
        &self.cwd
    }
    fn common_source_directory(&self) -> &Path {
        &self.common
    }
    fn config_file_path(&self) -> Option<&Path> {
        None
    }
    fn use_case_sensitive_file_names(&self) -> bool {
        self.case_sensitive
    }
    fn source_file_ids(&self) -> &[SourceFileId] {
        &[]
    }
    fn source_file(&self, _: SourceFileId) -> Option<crate::EmitSource<'_>> {
        None
    }
}

fn observation() -> Value {
    let value: Value = serde_json::from_slice(include_bytes!(
        "../../fixtures/bundle-module-identities.json"
    ))
    .unwrap();
    assert_eq!(value["typescript"], "6.0.3");
    assert_eq!(value["repetitions"], 2);
    assert_eq!(
        value["compiler_sha256"],
        "569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39"
    );
    value
}

#[test]
fn h2_7d_module_paths_match_all_typescript_helper_observations_twice() {
    let value = observation();
    let cases = value["path_cases"].as_array().unwrap();
    assert_eq!(cases.len(), 14);
    for case in cases {
        let host = Host {
            cwd: case["cwd"].as_str().unwrap().into(),
            common: case["common"].as_str().unwrap().into(),
            case_sensitive: case["case_sensitive"].as_bool().unwrap(),
            options: CompilerOptions::default(),
        };
        for _ in 0..2 {
            assert_eq!(
                external_module_name_from_path(
                    &host,
                    case["file"].as_str().unwrap(),
                    case["reference"].as_str(),
                ),
                case["observation"],
                "{}",
                case["case_id"]
            );
        }
    }
}

#[test]
fn h2_7d_module_identities_match_typescript_source_facts_twice() {
    let value = observation();
    let cases = value["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 24);
    for case in cases {
        let host = Host {
            cwd: case["current_directory"].as_str().unwrap().into(),
            common: case["observation"]["common_source_directory"]
                .as_str()
                .unwrap()
                .into(),
            case_sensitive: true,
            options: CompilerOptions {
                out_file: case["options"]["outFile"].as_str().map(str::to_owned),
                ..CompilerOptions::default()
            },
        };
        for identity in case["observation"]["identities"].as_array().unwrap() {
            let input = case["files"]
                .as_array()
                .unwrap()
                .iter()
                .find(|input| input["path"] == identity["path"])
                .unwrap();
            for _ in 0..2 {
                let source = tsc_syntax::parse_source_file(
                    input["path"].as_str().unwrap(),
                    input["text"].as_str().unwrap(),
                    Default::default(),
                    None,
                );
                assert_eq!(
                    source.module_name.as_deref(),
                    identity["explicit_module_name"].as_str()
                );
                assert_eq!(
                    source.is_declaration_file,
                    identity["declaration_file"].as_bool().unwrap()
                );
                assert_eq!(
                    get_resolved_external_module_name(&host, &source, None),
                    identity["resolved_name"],
                    "{} {}",
                    case["case_id"],
                    identity["path"]
                );
                assert_eq!(
                    try_get_module_name_from_file(Some(&host), &source).as_deref(),
                    identity["emitted_module_name"].as_str(),
                    "{} {}",
                    case["case_id"],
                    identity["path"]
                );
            }
        }
    }
}

#[test]
fn h2_7d_hostless_bundle_module_names_are_refused() {
    for module in [2, 4] {
        let options = CompilerOptions {
            module: Some(module),
            out_file: Some("bundle.js".into()),
            ..CompilerOptions::default()
        };
        let error = crate::get_script_transformers(&options, &crate::UnavailableEmitResolver)
            .err()
            .expect("hostless bundle transform must be refused");
        assert!(matches!(
            error,
            TransformError::UnsupportedCompilerOption {
                option: "outFile",
                ..
            }
        ));
    }
}
