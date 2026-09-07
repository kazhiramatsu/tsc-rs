//! Ordinary bundle/standalone module transform/print facets with the real checker.
//! This does not execute public Program.emit or claim its complete tuple/admission.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tsc_compiler::ProgramSession;
use tsc_emitter::{
    create_printer, get_script_transformers_for_source, preflight_emit, transform_nodes, EmitHost,
    EmitResolver, EmitRoot, EmitSelection, NewLineKind, PrintRequest, PrinterOptions,
    SourceFileTextMode, TransformArena, TransformBundle, TransformRoot,
};
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_emitting_program, CompilerOptions, LibraryCatalog, PathMapping, PreparedProgram,
    ProgramLoadLimits, ProgramOptions,
};

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(bytes: &[u8], expected_sha256: &str) -> Value {
    assert_eq!(format!("{:x}", Sha256::digest(bytes)), expected_sha256);
    let fixture: Value = serde_json::from_slice(bytes).unwrap();
    assert_eq!(fixture["typescript"], "6.0.3");
    assert_eq!(fixture["repetitions"], 2);
    assert_eq!(
        fixture["source_commit"],
        "050880ce59e30b356b686bd3144efe24f875ebc8"
    );
    assert_eq!(
        format!(
            "{:x}",
            Sha256::digest(
                std::fs::read(workspace().join("vendor/typescript-6.0.3/lib/typescript.js"))
                    .unwrap()
            )
        ),
        fixture["compiler_sha256"].as_str().unwrap()
    );
    for dependency in fixture["dependencies"].as_array().unwrap() {
        let path = dependency["path"].as_str().unwrap();
        assert_eq!(
            format!(
                "{:x}",
                Sha256::digest(std::fs::read(workspace().join(path)).unwrap())
            ),
            dependency["sha256"].as_str().unwrap(),
            "{path}"
        );
    }
    fixture
}

fn prepared(case: &Value, libraries: &[(String, Vec<u8>)]) -> Result<PreparedProgram, String> {
    let mut options = CompilerOptions::default();
    let mut program_options = ProgramOptions::default();
    for (key, value) in case["options"].as_object().unwrap() {
        match key.as_str() {
            "target" => options.target = Some(value.as_i64().unwrap() as i32),
            "module" => options.module = Some(value.as_i64().unwrap() as i32),
            "moduleResolution" => options.module_resolution = Some(value.as_i64().unwrap() as i32),
            "outFile" => options.out_file = value.as_str().map(str::to_owned),
            "outDir" => options.out_dir = value.as_str().map(str::to_owned),
            "declaration" => options.declaration = value.as_bool(),
            "listEmittedFiles" => options.list_emitted_files = value.as_bool(),
            "strict" => options.strict = value.as_bool(),
            "skipDefaultLibCheck" => options.skip_default_lib_check = value.as_bool(),
            "noErrorTruncation" => options.no_error_truncation = value.as_bool(),
            "newLine" => options.new_line = Some(value.as_i64().unwrap() as i32),
            "baseUrl" => options.base_url = value.as_str().map(str::to_owned),
            "paths" => {
                program_options = program_options.with_paths(
                    value
                        .as_object()
                        .unwrap()
                        .iter()
                        .map(|(pattern, substitutions)| {
                            PathMapping::new(
                                pattern.clone(),
                                substitutions
                                    .as_array()
                                    .unwrap()
                                    .iter()
                                    .map(|value| value.as_str().unwrap().to_owned())
                                    .collect(),
                            )
                        })
                        .collect(),
                );
            }
            other => return Err(format!("unprojected original option {other}")),
        }
    }
    let mut host = MemoryCompilerHost::builder(case["current_directory"].as_str().unwrap())
        .case_sensitive(true); // The pinned observer's host is case-sensitive.
    let files = case["files"].as_array().unwrap();
    for file in files {
        host = host.file(
            file["path"].as_str().unwrap(),
            file["text"].as_str().unwrap().as_bytes(),
        );
    }
    for (name, bytes) in libraries {
        host = host.file(format!("/lib/{name}"), bytes.clone());
    }
    let roots = case
        .get("roots")
        .map(|roots| {
            roots
                .as_array()
                .unwrap()
                .iter()
                .map(|path| PathBuf::from(path.as_str().unwrap()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| {
            files
                .iter()
                .map(|file| PathBuf::from(file["path"].as_str().unwrap()))
                .collect()
        });
    load_emitting_program(
        &host.build().map_err(|error| format!("host: {error}"))?,
        &roots,
        options,
        program_options,
        &LibraryCatalog::typescript_6_0_3("/lib"),
        ProgramLoadLimits::new(256, 2048, 64, 16 * 1024 * 1024, 128 * 1024 * 1024),
    )
    .map_err(|error| format!("prepare: {error}"))
}

fn paths_equal(label: &str, actual: &[String], expected: &Value) -> Result<(), String> {
    let expected = expected
        .as_array()
        .unwrap()
        .iter()
        .map(|path| path.as_str().unwrap())
        .collect::<Vec<_>>();
    let mismatch = actual
        .iter()
        .map(String::as_str)
        .zip(expected.iter().copied())
        .position(|(actual, expected)| actual != expected);
    if mismatch.is_some() || actual.len() != expected.len() {
        let index = mismatch.unwrap_or(actual.len().min(expected.len()));
        return Err(format!(
            "{label}[{index}]: actual={:?}, expected={:?}; lengths {}/{}",
            actual.get(index),
            expected.get(index),
            actual.len(),
            expected.len()
        ));
    }
    Ok(())
}

fn bytes_equal(label: &str, actual: &[u8], expected: &[u8]) -> Result<(), String> {
    if actual == expected {
        return Ok(());
    }
    let offset = actual
        .iter()
        .zip(expected)
        .position(|(a, e)| a != e)
        .unwrap_or(actual.len().min(expected.len()));
    let prefix = &expected[..offset];
    let line = prefix.iter().filter(|byte| **byte == b'\n').count() + 1;
    let column = prefix
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(offset + 1, |newline| offset - newline);
    let around = |bytes: &[u8]| {
        String::from_utf8_lossy(
            &bytes[offset.saturating_sub(20).min(bytes.len())..(offset + 40).min(bytes.len())],
        )
        .into_owned()
    };
    Err(format!("{label}: byte {offset}, line {line}, byte-column {column}, lengths {}/{}; actual={:?}; expected={:?}",
        actual.len(), expected.len(), around(actual), around(expected)))
}

fn compare_facet(
    host: &dyn EmitHost,
    resolver: &dyn EmitResolver,
    case: &Value,
) -> Result<(), String> {
    let expected = &case["observation"];
    if host.current_directory() != Path::new(case["current_directory"].as_str().unwrap()) {
        return Err(format!(
            "actual current directory {}",
            host.current_directory().display()
        ));
    }
    if !host.use_case_sensitive_file_names() {
        return Err("host lost case sensitivity".to_owned());
    }
    // These are assertions after production host construction, never oracle
    // values supplied to its source IDs, common directory or module provider.
    let source_order = host
        .source_file_ids()
        .iter()
        .map(|id| {
            host.source_file(*id)
                .map(|source| source.path().display().to_string())
                .ok_or_else(|| format!("missing host source {id:?}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    paths_equal(
        "Program source order",
        &source_order,
        &expected["source_order"],
    )?;
    if let Some(libraries) = expected.get("libraries") {
        let actual = host
            .source_file_ids()
            .iter()
            .filter_map(|id| host.source_file(*id))
            .filter(|source| source.path().starts_with("/lib"))
            .map(|source| {
                let syntax = source.syntax().ok_or("loaded library syntax unavailable")?;
                Ok(serde_json::json!({
                    "path": source.path().display().to_string(),
                    "sha256": format!("{:x}", Sha256::digest(syntax.text().as_bytes())),
                }))
            })
            .collect::<Result<Vec<Value>, String>>()?;
        if Value::Array(actual) != *libraries {
            return Err("loaded standard-library bytes/order differ".to_owned());
        }
    }
    if host.common_source_directory()
        != Path::new(expected["common_source_directory"].as_str().unwrap())
    {
        return Err(format!(
            "common directory actual={}, expected={}",
            host.common_source_directory().display(),
            expected["common_source_directory"]
        ));
    }
    let preflight = preflight_emit(host, EmitSelection::WholeProgram)
        .map_err(|error| format!("plan: {error}"))?;
    let expected_writes = expected["writes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|write| write["path"].as_str().unwrap().ends_with(".js"))
        .collect::<Vec<_>>();
    let units = preflight.plan().units();
    if units.len() != expected_writes.len() {
        return Err(format!(
            "JavaScript output count: planned={}, expected={}",
            units.len(),
            expected_writes.len()
        ));
    }
    let options = host.compiler_options();
    let newline = match options.new_line {
        Some(0) => NewLineKind::CarriageReturnLineFeed,
        None | Some(1) => NewLineKind::LineFeed,
        _ => return Err("unhandled original newLine".to_owned()),
    };
    let printer_options = PrinterOptions::new(newline)
        .with_target(options.emit_script_target())
        .with_module_kind(options.emit_module_kind())
        .with_remove_comments(options.remove_comments == Some(true))
        .with_no_emit_helpers(options.no_emit_helpers == Some(true))
        .with_import_helpers(options.import_helpers == Some(true))
        .with_source_file_text_mode(SourceFileTextMode::Canonical);
    // Keep one real Program/resolver and one printer. Bundle sources share one
    // transform/print; standalone units use distinct source transformations and
    // print requests, exercising the per-file reset in the same printer.
    let mut printer = create_printer(printer_options);
    let mut failures = Vec::new();
    for (output_index, (unit, expected_write)) in units.iter().zip(expected_writes).enumerate() {
        let comparison = (|| -> Result<(), String> {
            let is_bundle = options.out_file.is_some();
            let source_ids = match (unit.root(), is_bundle) {
                (EmitRoot::Bundle(bundle), true) => bundle.source_files().to_vec(),
                (EmitRoot::SourceFile(source), false) => vec![*source],
                _ => return Err("plan root disagrees with original outFile presence".to_owned()),
            };
            let javascript_path = unit
                .paths()
                .javascript_path()
                .ok_or("plan has no JavaScript path")?;
            if javascript_path != Path::new(expected_write["path"].as_str().unwrap()) {
                return Err(format!(
                    "JavaScript plan path actual={}, expected={}",
                    javascript_path.display(),
                    expected_write["path"]
                ));
            }
            // Compare planned source associations with the frozen callbacks;
            // this internal facet does not fabricate or invoke an OutputSink.
            let planned_sources = source_ids
                .iter()
                .map(|id| {
                    host.source_file(*id)
                        .map(|source| source.path().display().to_string())
                        .ok_or_else(|| format!("missing planned source {id:?}"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            paths_equal(
                "output source_files/order",
                &planned_sources,
                &expected_write["source_files"],
            )?;
            let first = *source_ids
                .first()
                .ok_or("ordinary module output has no source")?;
            let mut arena = TransformArena::new();
            let sources = source_ids
                .iter()
                .map(|id| {
                    let syntax = host
                        .source_file(*id)
                        .and_then(|source| source.syntax())
                        .ok_or_else(|| format!("checked syntax unavailable for {id:?}"))?;
                    Ok(arena.add_source(syntax, Some(*id)))
                })
                .collect::<Result<Vec<_>, String>>()?;
            let root = if is_bundle {
                TransformRoot::Bundle(TransformBundle::new(sources))
            } else {
                let [source] = sources.as_slice() else {
                    return Err("standalone source count".to_owned());
                };
                TransformRoot::SourceFile(*source)
            };
            let transformers = get_script_transformers_for_source(options, resolver, host, first)
                .map_err(|error| format!("transform selection: {error}"))?;
            let mut transformed = transform_nodes(arena, vec![root], transformers, false)
                .map_err(|error| format!("transform: {error}"))?;
            let request = match (transformed.roots(), is_bundle) {
                ([TransformRoot::Bundle(bundle)], true) => PrintRequest::Bundle(bundle.clone()),
                ([TransformRoot::SourceFile(source)], false) => PrintRequest::SourceFile(*source),
                _ => return Err("transform changed the output root kind/count".to_owned()),
            };
            let printed = printer
                .print(&mut transformed, request, None)
                .map_err(|error| format!("printer: {error}"))?;
            if printed.source_map().is_some() {
                return Err("unexpected source-map recording".to_owned());
            }
            let decode = |field: &str| {
                base64::engine::general_purpose::STANDARD
                    .decode(expected_write[field].as_str().unwrap())
                    .map_err(|error| error.to_string())
            };
            let callback = decode("callback_utf8_base64")?;
            if callback.len() as u64 != expected_write["callback_utf8_bytes"].as_u64().unwrap() {
                return Err("frozen callback byte length differs".to_owned());
            }
            bytes_equal("JavaScript callback", printed.text().as_bytes(), &callback)?;
            // BOM materialization is a host/printer facet, not a sink observation.
            let bom = options.emit_bom == Some(true);
            if Some(bom) != expected_write["write_byte_order_mark"].as_bool() {
                return Err(format!(
                    "BOM policy actual={bom}, expected={}",
                    expected_write["write_byte_order_mark"]
                ));
            }
            let mut materialized = Vec::new();
            if bom {
                materialized.extend_from_slice(&[239, 187, 191]);
            }
            materialized.extend_from_slice(printed.text().as_bytes());
            bytes_equal(
                "materialized JavaScript",
                &materialized,
                &decode("materialized_utf8_base64")?,
            )?;
            transformed.dispose();
            Ok(())
        })();
        if let Err(error) = comparison {
            failures.push(format!(
                "output {output_index} {}: {error}",
                expected_write["path"]
            ));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n"))
    }
}

fn compare_case(case: &Value, libraries: &[(String, Vec<u8>)]) -> Result<(), String> {
    let (result, checked) = ProgramSession::new(prepared(case, libraries)?)
        .with_checked_emit_resolver_for_harness(|host, resolver, checked| {
            if checked.program_semantic_diagnostics.is_none() {
                return Ok(Err("whole-Program semantic bucket unavailable".to_owned()));
            }
            Ok(compare_facet(host, resolver, case))
        })
        .map_err(|error| format!("live checker/provider: {error}"))?;
    if checked.program_semantic_diagnostics.is_none() {
        return Err("returned whole-Program semantic bucket unavailable".to_owned());
    }
    result.ok_or("ordinary source Program had no checked callback")?
}

fn assert_compiled_worktree() {
    assert_eq!(
        std::env::current_dir().unwrap().canonicalize().unwrap(),
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .canonicalize()
            .unwrap(),
        "run from the compiled worktree; build targets cannot be shared across worktrees"
    );
}

#[test]
fn h2_7d_ordinary_module_bundles_match_complete_javascript_twice() {
    assert_compiled_worktree();
    let fixture = fixture(
        include_bytes!("../../emitter/tests/fixtures/bundle-module-identities.json"),
        "965c6ab4b187ac8488b63e572544ea0924afd230af43077092ce57ed69bb91bb",
    );
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 24);
    assert_eq!(
        cases
            .iter()
            .map(|case| case["case_id"].as_str().unwrap())
            .collect::<BTreeSet<_>>()
            .len(),
        24
    );
    assert_eq!(fixture["path_cases"].as_array().unwrap().len(), 14); // Separate helper references.
    let references = cases
        .iter()
        .filter(|case| case["api_reference"] == true)
        .map(|case| case["case_id"].as_str().unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        references,
        BTreeSet::from([
            "amd/resolved-before-rename",
            "amd/unresolved-rename",
            "system/resolved-before-rename",
            "system/unresolved-rename"
        ])
    );
    let ordinary = cases
        .iter()
        .filter(|case| case["api_reference"] == false)
        .collect::<Vec<_>>();
    assert_eq!(ordinary.len(), 20);
    compare_cases(&ordinary);
}

#[test]
fn h2_7d_system_generated_names_match_bundle_and_standalone_javascript_twice() {
    assert_compiled_worktree();
    let fixture = fixture(
        include_bytes!("../../emitter/tests/fixtures/system-generated-names.json"),
        "ee524599b64d4d01287ee8ff1a1ff5bfa42ddefadfc0bccd9f1f8b07ae513112",
    );
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 12);
    assert_eq!(
        cases
            .iter()
            .map(|case| case["case_id"].as_str().unwrap())
            .collect::<BTreeSet<_>>()
            .len(),
        12
    );
    assert!(cases.iter().all(|case| case["api_reference"] == false));
    assert_eq!(
        cases
            .iter()
            .filter(|case| case["options"].get("outFile").is_some())
            .count(),
        10
    );
    assert_eq!(
        cases
            .iter()
            .filter(|case| case["options"].get("outDir").is_some())
            .count(),
        2
    );
    // Full TS tuples remain in the artifact; this harness compares actual JS
    // paths/order/source associations/bytes/BOM, not declaration output or the
    // public Program.emit result, diagnostics, status, maps or activity.
    compare_cases(&cases.iter().collect::<Vec<_>>());
}

fn compare_cases(cases: &[&Value]) {
    let mut libraries = Vec::new();
    for entry in std::fs::read_dir(workspace().join("vendor/typescript-6.0.3/lib")).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("lib.") && name.ends_with(".d.ts") {
            libraries.push((name, std::fs::read(entry.path()).unwrap()));
        }
    }
    libraries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut failures = Vec::new();
    for case in cases {
        let id = case["case_id"].as_str().unwrap();
        let before = failures.len();
        for repetition in 1..=2 {
            let result = std::panic::catch_unwind(|| compare_case(case, &libraries));
            let error = match result {
                Ok(Ok(())) => None,
                Ok(Err(error)) => Some(error),
                Err(panic) => Some(format!(
                    "panic: {}",
                    panic
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| panic.downcast_ref::<&str>().map(|value| value.to_string()))
                        .unwrap_or_else(|| "non-string payload".to_owned())
                )),
            };
            if let Some(error) = error {
                failures.push(format!("{id} repetition {repetition}: {error}"));
            }
        }
        if failures.len() == before {
            eprintln!("H2.7d JS facet PASS {id} (2 repetitions)");
        }
    }
    assert!(
        failures.is_empty(),
        "{} failed ordinary JS facet repetitions:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
