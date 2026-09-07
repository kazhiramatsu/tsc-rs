//! Join the unchanged H2.7e census to original inputs and complete TS observations.
//! Shared by compiler contracts and the hosted qualification rung; CLI checks stay in the test wrapper.
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tsc_compiler::{MemoryOutputSink, ProgramSession};
use tsc_diagnostics::{Diagnostic, MessageChain};
use tsc_emitter::{EmitArtifact, EmitArtifactKind, EmitWriteMetadata, H2RuntimeSlice};
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_emitting_program, CompilerOptions, LibraryCatalog, PreparedProgram, ProgramLoadLimits,
    ProgramOptions,
};

pub(super) fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub(super) fn frozen(path: &str, hash: &str) -> Value {
    let bytes = std::fs::read(workspace().join(path)).unwrap();
    assert_eq!(format!("{:x}", Sha256::digest(&bytes)), hash, "{path}");
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["typescript"], "6.0.3");
    assert_eq!(
        value["source_commit"],
        "050880ce59e30b356b686bd3144efe24f875ebc8"
    );
    value
}

pub(super) fn indexed(artifact: &Value) -> BTreeMap<&str, &Value> {
    let rows = artifact["cases"].as_array().unwrap();
    let index = rows
        .iter()
        .map(|row| (row["case_id"].as_str().unwrap(), row))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(index.len(), rows.len(), "duplicate original case IDs");
    index
}

fn options(value: &Value) -> CompilerOptions {
    let mut options = CompilerOptions::default();
    for (key, value) in value.as_object().unwrap() {
        match key.as_str() {
            "target" => options.target = Some(value.as_i64().unwrap() as i32),
            "module" => options.module = Some(value.as_i64().unwrap() as i32),
            "newLine" => options.new_line = Some(value.as_i64().unwrap() as i32),
            "declaration" => options.declaration = value.as_bool(),
            "declarationMap" => options.declaration_map = value.as_bool(),
            "sourceMap" => options.source_map = value.as_bool(),
            "noResolve" => options.no_resolve = value.as_bool(),
            "noErrorTruncation" => options.no_error_truncation = value.as_bool(),
            "skipDefaultLibCheck" => options.skip_default_lib_check = value.as_bool(),
            "strict" => options.strict = value.as_bool(),
            "outFile" => options.out_file = value.as_str().map(str::to_owned),
            other => panic!("unprojected original option {other}"),
        }
    }
    options
}

fn prepared(case: &Value) -> PreparedProgram {
    let input = &case["input"];
    assert_eq!(input["route"], "whole-program");
    assert_eq!(input["current_directory"], "/.src");
    assert_eq!(input["use_case_sensitive_file_names"], true);
    assert_eq!(input["vfs_symlinks"], json!([]));
    for field in ["config", "shared_mount", "default_library_file_name"] {
        assert!(input[field].is_null(), "unhandled original {field}");
    }
    let mut host = MemoryCompilerHost::builder("/.src").case_sensitive(true);
    for file in input["files"].as_array().unwrap() {
        host = host.file(
            file["path"].as_str().unwrap(),
            file["text"].as_str().unwrap().as_bytes(),
        );
    }
    for entry in std::fs::read_dir(workspace().join("vendor/typescript-6.0.3/lib")).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("lib.") && name.ends_with(".d.ts") {
            host = host.file(format!("/lib/{name}"), std::fs::read(entry.path()).unwrap());
        }
    }
    let roots = input["roots"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| PathBuf::from(p.as_str().unwrap()))
        .collect::<Vec<_>>();
    load_emitting_program(
        &host.build().unwrap(),
        &roots,
        options(&case["effective_options"]),
        ProgramOptions::default(),
        &LibraryCatalog::typescript_6_0_3("/lib"),
        ProgramLoadLimits::new(256, 2048, 64, 16 * 1024 * 1024, 128 * 1024 * 1024),
    )
    .unwrap()
}

fn message(chain: &MessageChain, indent: usize, text: &mut String) {
    if indent != 0 {
        text.push('\n');
        text.push_str(&"  ".repeat(indent));
    }
    text.push_str(&chain.text);
    for next in &chain.next {
        message(next, indent + 1, text);
    }
}

fn diagnostics(diagnostics: &[Diagnostic]) -> Value {
    json!(diagnostics.iter().map(|d| {
        let mut text = String::new(); message(&d.message, 0, &mut text);
        let related = (d.related_information_present || !d.related.is_empty()).then(|| d.related.iter().map(|r| {
            let mut text = String::new(); message(&r.message, 0, &mut text);
            json!({"code":r.message.code,"category":format!("{:?}",r.message.category),
                "file":r.file_name,"start":r.start,"length":r.length,"message":text,"related_information":null})
        }).collect::<Vec<_>>());
        json!({"code":d.code(),"category":format!("{:?}",d.category()),"file":d.file_name,
            "start":d.start,"length":d.length,"message":text,"related_information":related})
    }).collect::<Vec<_>>())
}

fn write(index: usize, artifact: &EmitArtifact) -> Value {
    let path = artifact.path().to_string_lossy();
    let kind = match artifact.kind() {
        EmitArtifactKind::JavaScript => "javascript",
        EmitArtifactKind::Declaration => "declaration",
        EmitArtifactKind::JavaScriptMap => {
            assert!(path.ends_with(".js.map"));
            "source-map"
        }
        EmitArtifactKind::DeclarationMap => {
            assert!(path.ends_with(".d.ts.map"));
            "source-map"
        }
        EmitArtifactKind::BuildInfo => panic!("build-info remains outside this packet"),
    };
    let (keys, position, data_diagnostics) = match artifact.metadata() {
        Some(EmitWriteMetadata::Text(data)) => (
            json!(["sourceMapUrlPos", "diagnostics"]),
            json!(data.source_map_url_position().map(|p| p.value())),
            diagnostics(data.diagnostics()),
        ),
        None => (Value::Null, Value::Null, Value::Null),
        _ => panic!("unexpected original callback metadata"),
    };
    json!({"index":index,"path":path,"kind":kind,
        "callback_utf8_base64":base64::engine::general_purpose::STANDARD.encode(artifact.callback_bytes()),
        "callback_utf8_bytes":artifact.callback_bytes().len(),"write_byte_order_mark":artifact.write_byte_order_mark(),
        "materialized_utf8_base64":base64::engine::general_purpose::STANDARD.encode(artifact.materialized_bytes()),
        "materialized_utf8_bytes":artifact.materialized_bytes().len(),
        // OutputSink::write's Result is the typed equivalent of onError.
        "on_error_callback_present":true,"source_files":artifact.source_files(),
        "data_present":artifact.metadata().is_some(),"data_keys":keys,
        "data_source_map_url_pos":position,"data_diagnostics":data_diagnostics,"data_build_info":null})
}

fn assert_program(case: &Value, expected: &Value) {
    let prepared = prepared(case);
    let mut sources = Vec::new();
    let mut libraries = Vec::new();
    for source in prepared.source_files() {
        let name = source.path().display().to_string_lossy();
        if let Some(name) = name.strip_prefix("/lib/") {
            libraries.push(name.to_owned());
        } else {
            sources.push(name.into_owned());
        }
    }
    let mut sink = MemoryOutputSink::new();
    let command = ProgramSession::new(prepared)
        .emit_command_for_harness(&mut sink)
        .unwrap();
    let outcome = command.emit();
    let activity = outcome.h2_activity();
    assert_eq!(activity.runtime_slice(H2RuntimeSlice::H2_7d), 0);
    assert_eq!(case["effective_options"]["declarationMap"], true);
    assert_eq!(activity.runtime_slice(H2RuntimeSlice::H2_7e), 1);
    for slice in H2RuntimeSlice::ALL {
        if slice == H2RuntimeSlice::H2_7a || slice > H2RuntimeSlice::H2_7e {
            assert_eq!(
                activity.runtime_slice(slice),
                0,
                "{}: inactive {}",
                case["case_id"],
                slice.name(),
            );
        }
    }
    let maps = outcome.source_maps().map(|maps| {
        maps.iter().map(|map| json!({
        "input_source_file_names":map.input_source_files(),"source_map_json":map.canonical_json()
    })).collect::<Vec<_>>()
    });
    let actual = json!({"program_source_order":sources,"standard_libraries":libraries,
        "writes":sink.writes().iter().enumerate().map(|(i,a)| write(i,a)).collect::<Vec<_>>(),
        "reported_diagnostics":diagnostics(command.diagnostics()),"emit_refused":outcome.emit_skipped(),
        "emit_result":{"emit_skipped":outcome.emit_skipped(),"diagnostics":diagnostics(outcome.diagnostics()),
            "emitted_files":outcome.emitted_files(),"source_maps":maps},"status_writes":command.status_writes(),"exit_code":command.exit_code()});
    assert_eq!(
        actual.as_object().unwrap().keys().collect::<Vec<_>>(),
        expected.as_object().unwrap().keys().collect::<Vec<_>>()
    );
    for (field, expected) in expected.as_object().unwrap() {
        assert_eq!(
            actual[field], *expected,
            "{}: original {field}",
            case["case_id"]
        );
    }
}

pub(super) fn assert_original_corpus(workspace_root: &Path) -> BTreeSet<String> {
    assert_eq!(
        workspace_root.canonicalize().unwrap(),
        workspace().canonicalize().unwrap(),
        "H2.7e caller must select the compiled workspace",
    );
    let census = frozen(
        "ratchets/h2-7de-candidates.v1.json",
        "1af6d75acf8212135a0850c5ff09487a5589de4d0f825ff1f0e9bc8e3f0f141d",
    );
    let inputs = frozen(
        "ratchets/h2-7de-candidate-inputs.v1.json",
        "f2e078a6b6d10cd3c6df833584924c18e8f78fe98c1e41621c70e10e215a073a",
    );
    let observations = frozen(
        "ratchets/h2-7de-observations.v1.json",
        "1a1681b2375d27d9012b06e29808aca72aa3e39d1dbc1536b80ba2aadf9e8ce2",
    );
    assert_eq!(observations["repetitions"], 2);
    for pin in observations["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .chain(std::iter::once(&observations["generator"]))
    {
        let bytes = std::fs::read(workspace().join(pin["path"].as_str().unwrap())).unwrap();
        assert_eq!(
            json!(format!("{:x}", Sha256::digest(bytes))),
            pin["sha256"],
            "{}",
            pin["path"]
        );
    }
    let census = indexed(&census);
    let inputs = indexed(&inputs);
    let observations = indexed(&observations);
    assert_eq!(census.len(), 325, "unchanged D/E candidate union");
    assert_eq!(inputs.len(), 325);
    let cases = census
        .iter()
        .filter(|(_, row)| {
            row["required_slices"]
                .as_array()
                .unwrap()
                .contains(&json!("H2.7e"))
        })
        .collect::<Vec<_>>();
    assert_eq!(cases.len(), 13);
    let (mut ordinary, mut declaration_absent, mut bundles, mut transpile) = (0, 0, 0, 0);
    let (mut historical_total, mut historical_exact) = (0, 0);
    let mut failures = Vec::new();
    let mut compared_ids = BTreeSet::new();
    for (id, row) in cases {
        let input = inputs[id];
        assert_eq!(row["source"], input["source"]);
        assert_eq!(row["runtime_admitted"], false);
        let in_h2_6c = row["parent_membership"]
            .as_array()
            .unwrap()
            .iter()
            .any(|membership| membership["parent"] == "H2.6c");
        historical_total += usize::from(in_h2_6c);
        let owners = &row["required_slices"];
        if input["input"]["route"] == "transpile-api" {
            assert_eq!(*owners, json!(["H2.7e", "H2.8c"]));
            assert!(!observations.contains_key(id));
            assert_eq!(input["input"]["units"].as_array().unwrap().len(), 5);
            transpile += 1;
            continue;
        }
        let reference = observations[id];
        assert_eq!(row["input_sha256"], reference["input_sha256"]);
        assert_eq!(*owners, reference["required_slices"]);
        assert_eq!(reference["repetitions"], 2);
        if *owners == json!(["H2.7d", "H2.7e"]) {
            // Compound cases are compared completely by the D283 shared entry.
            // Keep their original identity here without double-counting the union.
            bundles += 1;
            continue;
        }
        assert_eq!(*owners, json!(["H2.7e"]));
        if input["effective_options"].get("declaration").is_none() {
            assert_eq!(
                *id,
                "typescript-6.0.3/compiler/declarationMapsWithoutDeclaration.ts#default"
            );
            declaration_absent += 1;
        } else {
            assert_eq!(input["effective_options"]["declaration"], true);
        }
        ordinary += 1;
        historical_exact += usize::from(in_h2_6c);
        let before = failures.len();
        for repetition in 0..2 {
            let compared = std::panic::catch_unwind(|| {
                assert_program(input, &reference["typescript_observation"]);
            });
            if let Err(error) = compared {
                failures.push(format!(
                    "{id} repetition {}: {}",
                    repetition + 1,
                    error
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| error.downcast_ref::<&str>().map(|s| s.to_string()))
                        .unwrap_or_default()
                ));
            }
        }
        if failures.len() == before {
            assert!(compared_ids.insert((*id).to_owned()));
            eprintln!("H2.7e original EXACT x2 {id}");
        }
    }
    assert_eq!(
        (ordinary, declaration_absent, bundles, transpile),
        (8, 1, 3, 2)
    );
    assert_eq!((historical_total, historical_exact), (6, 5));
    assert_eq!(
        ordinary - historical_exact,
        3,
        "new case IDs relative to H2.6c"
    );
    assert!(
        failures.is_empty(),
        "{} original residuals:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert_eq!(compared_ids.len(), 8);
    compared_ids
}
