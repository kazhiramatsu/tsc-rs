//! Declaration-map observations through the printer, whole Program, and real CLI.
//! The CLI checks status/exit for every fixture; the Program checks the original
//! absolute-path command diagnostics, callback artifacts, and raw sourceMaps.
use std::path::{Path, PathBuf};
use tsc_compiler::{MemoryOutputSink, ProgramSession};
use tsc_diagnostics::{Diagnostic, MessageChain};
use tsc_host::MemoryCompilerHost;

use base64::Engine;
use serde_json::{json, Value};
use tsc_checker::{
    check_program_with_authoritative_modules_at_for_emit, AuthoritativeModuleLookupFailure,
    AuthoritativeModuleProvider, AuthoritativeModuleRequest, AuthoritativeModuleResolution,
    AuthoritativeSourceMetadata, AuthoritativeSourceToken, InputFile, ProgramSnapshot,
};
use tsc_emitter::{
    create_printer, declaration_map_recording_inputs_for, finish_declaration_map, preflight_emit,
    transform_declaration_unit_with_observer_for_harness, DeclarationPrintHandlers, EmitArtifact,
    EmitHost, EmitResolver, EmitSource, EmitWriteMetadata, GlobalNameOracle, MapLaneInputs,
    NewLineKind, PlanDeclarationPaths, PrinterOptions, SourceFileId, SourceFileTextMode,
    TransformRoot,
};
use tsc_program::{
    load_emitting_program, CompilerOptions, LibraryCatalog, ProgramLoadLimits, ProgramOptions,
};

struct NoModuleRequests;
impl AuthoritativeModuleProvider for NoModuleRequests {
    fn resolve_module(
        &self,
        _: AuthoritativeModuleRequest<'_>,
    ) -> Result<AuthoritativeModuleResolution, AuthoritativeModuleLookupFailure> {
        Err(AuthoritativeModuleLookupFailure::Missing)
    }
}
struct Host<'a> {
    snapshot: &'a ProgramSnapshot,
    options: &'a CompilerOptions,
    ids: Vec<SourceFileId>,
    common: &'a Path,
}
impl EmitHost for Host<'_> {
    fn compiler_options(&self) -> &CompilerOptions {
        self.options
    }
    fn current_directory(&self) -> &Path {
        Path::new("/project")
    }
    fn common_source_directory(&self) -> &Path {
        self.common
    }
    fn config_file_path(&self) -> Option<&Path> {
        None
    }
    fn use_case_sensitive_file_names(&self) -> bool {
        true
    }
    fn source_file_ids(&self) -> &[SourceFileId] {
        &self.ids
    }
    fn source_file(&self, id: SourceFileId) -> Option<EmitSource<'_>> {
        let syntax = self.snapshot.documents().get(id.index())?.source();
        let path = Path::new(&syntax.file_name);
        Some(EmitSource::new(
            id,
            path,
            path,
            !syntax.is_declaration_file,
            None,
            Some(syntax),
        ))
    }
}
struct GlobalNames<'a>(&'a dyn EmitResolver);
impl GlobalNameOracle for GlobalNames<'_> {
    fn has_global_name(&self, name: &str) -> Result<bool, tsc_emitter::EmitResolverError> {
        self.0.has_global_name(name)
    }
}
fn project_options(value: &Value) -> CompilerOptions {
    let mut options = CompilerOptions::default();
    for (key, value) in value.as_object().unwrap() {
        match key.as_str() {
            "target" => options.target = Some(value.as_i64().unwrap() as i32),
            "module" => options.module = Some(value.as_i64().unwrap() as i32),
            "declaration" => options.declaration = value.as_bool(),
            "declarationMap" => options.declaration_map = value.as_bool(),
            "sourceMap" => options.source_map = value.as_bool(),
            "noEmitOnError" => options.no_emit_on_error = value.as_bool(),
            "isolatedDeclarations" => options.isolated_declarations = value.as_bool(),
            "outFile" => options.out_file = value.as_str().map(str::to_owned),
            "emitDeclarationOnly" => options.emit_declaration_only = value.as_bool(),
            "listEmittedFiles" => options.list_emitted_files = value.as_bool(),
            "newLine" => options.new_line = Some(value.as_i64().unwrap() as i32),
            "strict" => options.strict = value.as_bool(),
            "noResolve" => options.no_resolve = value.as_bool(),
            "skipDefaultLibCheck" => options.skip_default_lib_check = value.as_bool(),
            "noErrorTruncation" => options.no_error_truncation = value.as_bool(),
            "stripInternal" => options.strip_internal = value.as_bool(),
            "declarationDir" => options.declaration_dir = value.as_str().map(str::to_owned),
            "sourceRoot" => options.source_root = value.as_str().map(str::to_owned),
            "mapRoot" => options.map_root = value.as_str().map(str::to_owned),
            "inlineSources" => options.inline_sources = value.as_bool(),
            "inlineSourceMap" => options.inline_source_map = value.as_bool(),
            "emitBOM" => options.emit_bom = value.as_bool(),
            "removeComments" => options.remove_comments = value.as_bool(),
            "allowJs" => options.allow_js = value.as_bool().unwrap(),
            other => panic!("unprojected option {other}"),
        }
    }
    options
}
fn assert_artifact(actual: &EmitArtifact, expected: &Value) {
    assert_eq!(
        actual.path().to_string_lossy(),
        expected["path"].as_str().unwrap()
    );
    let text = base64::engine::general_purpose::STANDARD
        .decode(expected["callback_utf8_base64"].as_str().unwrap())
        .unwrap();
    assert_eq!(actual.callback_text(), String::from_utf8(text).unwrap());
    assert_eq!(
        actual.callback_text().len() as u64,
        expected["callback_utf8_bytes"].as_u64().unwrap()
    );
    assert_eq!(
        actual.write_byte_order_mark(),
        expected["write_byte_order_mark"].as_bool().unwrap()
    );
    assert_eq!(
        actual.materialized_bytes().as_ref(),
        base64::engine::general_purpose::STANDARD
            .decode(expected["materialized_utf8_base64"].as_str().unwrap())
            .unwrap()
    );
    assert_eq!(
        actual.materialized_bytes().len() as u64,
        expected["materialized_utf8_bytes"].as_u64().unwrap()
    );
    assert_eq!(
        actual.kind(),
        match expected["kind"].as_str().unwrap() {
            "declaration-map" => tsc_emitter::EmitArtifactKind::DeclarationMap,
            "declaration" => tsc_emitter::EmitArtifactKind::Declaration,
            "javascript-map" => tsc_emitter::EmitArtifactKind::JavaScriptMap,
            "javascript" => tsc_emitter::EmitArtifactKind::JavaScript,
            other => panic!("unexpected artifact kind {other}"),
        }
    );
    assert_eq!(
        actual
            .source_files()
            .unwrap()
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        serde_json::from_value::<Vec<String>>(expected["source_files"].clone()).unwrap()
    );
    if expected.get("data_keys").is_some() {
        assert_eq!(
            expected["data_keys"],
            if actual.metadata().is_some() {
                json!(["sourceMapUrlPos", "diagnostics"])
            } else {
                Value::Null
            }
        );
        assert_eq!(expected["data_build_info"], Value::Null);
    }
    match actual.metadata() {
        Some(EmitWriteMetadata::Text(data)) => {
            assert_eq!(expected["data_present"], true);
            assert_eq!(
                diagnostics_json(data.diagnostics()),
                expected["data_diagnostics"]
            );
            assert_eq!(
                data.source_map_url_position()
                    .map(|pos| u64::from(pos.value())),
                expected["data_source_map_url_pos"].as_u64()
            );
        }
        None => {
            assert_eq!(expected["data_present"], false);
            assert_eq!(expected["data_diagnostics"], Value::Null);
            assert_eq!(expected["data_source_map_url_pos"], Value::Null);
        }
        _ => panic!("unexpected build-info metadata"),
    }
}
#[test]
fn h2_7e_nonbundle_declaration_maps_match_typescript() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/declaration-maps.json")).unwrap();
    assert_eq!(fixture["repetitions"], 2);
    let mut failures = Vec::new();
    for case in fixture["cases"].as_array().unwrap() {
        let id = case["case_id"].as_str().unwrap();
        let checked = std::panic::catch_unwind(|| {
            let options = project_options(&case["options"]);
            let inputs = case["files"]
                .as_array()
                .unwrap()
                .iter()
                .map(|file| {
                    InputFile::new(
                        file["path"].as_str().unwrap(),
                        file["text"].as_str().unwrap(),
                    )
                })
                .collect::<Vec<_>>();
            let metadata = inputs
                .iter()
                .enumerate()
                .map(|(index, input)| AuthoritativeSourceMetadata {
                    token: AuthoritativeSourceToken(index as u32),
                    file_name: input.name.clone(),
                    may_be_emitted: true,
                    implied_node_format: None,
                    implied_node_format_for_emit: None,
                })
                .collect::<Vec<_>>();
            let expected = &case["typescript_observation"];
            for _ in 0..2 {
                check_program_with_authoritative_modules_at_for_emit(
                    &[],
                    &inputs,
                    &[],
                    &metadata,
                    &options,
                    "/project",
                    &NoModuleRequests,
                    |snapshot, checker, checked| {
                        assert!(checked.partial_checks.is_empty());
                        let host = Host {
                            snapshot,
                            options: &options,
                            ids: (0..inputs.len())
                                .map(|id| SourceFileId::from_raw(id as u32))
                                .collect(),
                            common: Path::new(
                                expected["common_source_directory"].as_str().unwrap(),
                            ),
                        };
                        let preflight =
                            preflight_emit(&host, tsc_emitter::EmitSelection::WholeProgram)
                                .unwrap();
                        let paths = PlanDeclarationPaths::new(&host, &preflight);
                        let lane = MapLaneInputs {
                            common_source_directory: expected["common_source_directory"]
                                .as_str()
                                .unwrap()
                                .to_owned(),
                            current_directory: "/project".to_owned(),
                            use_case_sensitive_source_keys: true,
                        };
                        let newline = if options.new_line == Some(0) {
                            NewLineKind::CarriageReturnLineFeed
                        } else {
                            NewLineKind::LineFeed
                        };
                        let mut writes = Vec::new();
                        let mut observations = Vec::new();
                        checker.with_emit_resolver(|resolver| {
                            for unit in preflight.plan().units() {
                                let tsc_emitter::EmitRoot::SourceFile(source) = unit.root() else {
                                    panic!("non-bundle fixture")
                                };
                                let mut observer = |_| {};
                                let (outcome, mut transformed) =
                                    transform_declaration_unit_with_observer_for_harness(
                                        resolver,
                                        &host,
                                        &preflight,
                                        &paths,
                                        *source,
                                        &mut observer,
                                    )
                                    .unwrap();
                                assert!(!outcome.decl_blocked, "{id}: {:?}", outcome.diagnostics);
                                assert!(outcome.diagnostics.is_empty());
                                let TransformRoot::SourceFile(root) = transformed.roots()[0] else {
                                    panic!("source root")
                                };
                                let declaration_path = unit.paths().declaration_path().unwrap();
                                let map_path = unit.paths().declaration_map_path().unwrap();
                                let source_path = host.source_file(*source).unwrap().path();
                                let recording = declaration_map_recording_inputs_for(
                                    &lane,
                                    &options,
                                    declaration_path,
                                    source_path,
                                );
                                assert!(!recording.inline_sources);
                                let printer_options = PrinterOptions::new(newline)
                                    .with_remove_comments(options.remove_comments == Some(true))
                                    .with_no_emit_helpers(true)
                                    .with_declaration_syntax(true)
                                    .with_only_print_js_doc_style(true)
                                    .with_omit_brace_source_map_positions(true)
                                    .with_target(options.emit_script_target())
                                    .with_source_file_text_mode(SourceFileTextMode::Canonical);
                                let names = GlobalNames(resolver);
                                let printed = create_printer(printer_options)
                                    .print_declaration_with_recording(
                                        &mut transformed,
                                        root,
                                        DeclarationPrintHandlers::new(&names),
                                        Some(recording),
                                    )
                                    .unwrap();
                                let mapped = finish_declaration_map(
                                    &lane,
                                    &options,
                                    declaration_path,
                                    map_path,
                                    source_path,
                                    &printed,
                                    outcome.diagnostics,
                                    newline,
                                )
                                .unwrap();
                                observations.push(json!({
                                    "inputSourceFileNames": mapped.observation.input_source_files(),
                                    "sourceMap": serde_json::from_str::<Value>(
                                        mapped.observation.canonical_json()
                                    ).unwrap()
                                }));
                                writes.push(mapped.map);
                                writes.push(mapped.declaration);
                                transformed.dispose();
                            }
                        });
                        let expected_writes = expected["writes"].as_array().unwrap();
                        assert_eq!(writes.len(), expected_writes.len());
                        for (actual, expected) in writes.iter().zip(expected_writes) {
                            assert_artifact(actual, expected);
                        }
                        assert_eq!(json!(observations), expected["emit_result"]["source_maps"]);
                    },
                )
                .unwrap();
            }
        });
        if let Err(error) = checked {
            let message = error
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| error.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            failures.push(format!("{id}: {message}"));
        } else {
            eprintln!("H2.7e PASS {id}");
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

fn flatten_message(chain: &MessageChain, indent: usize, text: &mut String) {
    if indent != 0 {
        text.push('\n');
        text.push_str(&"  ".repeat(indent));
    }
    text.push_str(&chain.text);
    for child in &chain.next {
        flatten_message(child, indent + 1, text);
    }
}

fn diagnostics_json(diagnostics: &[Diagnostic]) -> Value {
    Value::Array(diagnostics.iter().map(|diagnostic| {
        let mut message = String::new();
        flatten_message(&diagnostic.message, 0, &mut message);
        let related = if diagnostic.related_information_present || !diagnostic.related.is_empty() {
            Value::Array(diagnostic.related.iter().map(|related| {
                let mut message = String::new();
                flatten_message(&related.message, 0, &mut message);
                json!({"code":related.message.code,"category":format!("{:?}", related.message.category),
                    "file":related.file_name,"start":related.start,"length":related.length,
                    "message":message,"related_information":null})
            }).collect())
        } else {Value::Null};
        json!({"code":diagnostic.code(),"category":format!("{:?}", diagnostic.category()),
            "file":diagnostic.file_name,"start":diagnostic.start,"length":diagnostic.length,
            "message":message,"related_information":related})
    }).collect())
}

fn memory_host(case: &Value) -> MemoryCompilerHost {
    let mut builder = MemoryCompilerHost::builder(case["current_directory"].as_str().unwrap());
    for file in case["files"].as_array().unwrap() {
        builder = builder.file(
            file["path"].as_str().unwrap(),
            file["text"].as_str().unwrap().as_bytes(),
        );
    }
    let libraries = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/typescript-6.0.3/lib");
    for entry in std::fs::read_dir(libraries).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("lib.") && name.ends_with(".d.ts") {
            builder = builder.file(format!("/lib/{name}"), std::fs::read(entry.path()).unwrap());
        }
    }
    builder.build().unwrap()
}

fn prepared(case: &Value, host: &MemoryCompilerHost) -> tsc_program::PreparedProgram {
    let roots = case["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| PathBuf::from(file["path"].as_str().unwrap()))
        .collect::<Vec<_>>();
    load_emitting_program(
        host,
        &roots,
        project_options(&case["options"]),
        ProgramOptions::default(),
        &LibraryCatalog::typescript_6_0_3("/lib"),
        ProgramLoadLimits::new(256, 2048, 64, 16 * 1024 * 1024, 128 * 1024 * 1024),
    )
    .unwrap()
}

fn compare_cases(compare: impl FnMut(&Value)) {
    compare_fixture(include_str!("fixtures/declaration-maps.json"), 24, compare);
}

fn compare_fixture(source: &str, count: usize, mut compare: impl FnMut(&Value)) {
    let fixture: Value = serde_json::from_str(source).unwrap();
    assert_eq!(fixture["typescript"], "6.0.3");
    assert_eq!(fixture["repetitions"], 2);
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), count);
    let mut failures = Vec::new();
    for case in cases {
        let checked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| compare(case)));
        if let Err(error) = checked {
            let detail = error
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| error.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            failures.push(format!("{}: {detail}", case["case_id"]));
        } else {
            eprintln!("H2.7e PASS {}", case["case_id"]);
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn h2_7e_ordinary_program_matches_complete_typescript_observations() {
    compare_cases(assert_program_case);
}

fn assert_program_case(case: &Value) {
    assert_program_case_with_session(case, false);
}

fn assert_program_case_with_session(case: &Value, scoped: bool) {
    let expected = &case["typescript_observation"];
    let host = memory_host(case);
    for _ in 0..2 {
        let mut sink = MemoryOutputSink::new();
        let program = prepared(case, &host);
        if expected.get("program_source_order").is_some() {
            let mut sources = Vec::new();
            let mut libraries = Vec::new();
            for source in program.source_files() {
                let name = source.path().display().to_string_lossy();
                if let Some(name) = name.strip_prefix("/lib/") {
                    libraries.push(name.to_owned());
                } else {
                    sources.push(name.into_owned());
                }
            }
            assert_eq!(json!(sources), expected["program_source_order"]);
            assert_eq!(json!(libraries), expected["standard_libraries"]);
        }
        let (outcome, reported) = if scoped {
            ProgramSession::new(program)
                .with_declarations(|session| {
                    let report = session.emit_with_reported_diagnostics(&mut sink)?;
                    assert_eq!(json!(report.status_writes()), expected["status_writes"]);
                    assert_eq!(json!(report.exit_code()), expected["exit_code"]);
                    Ok((report.emit().clone(), report.diagnostics().to_vec()))
                })
                .unwrap()
        } else {
            ProgramSession::new(program)
                .emit_with_reported_diagnostics_for_harness(&mut sink)
                .unwrap()
        };
        assert_eq!(
            diagnostics_json(&reported),
            expected["reported_diagnostics"]
        );
        assert_eq!(
            diagnostics_json(outcome.diagnostics()),
            expected["emit_result"]["diagnostics"]
        );
        assert_eq!(
            json!(outcome.emit_skipped()),
            expected["emit_result"]["emit_skipped"]
        );
        assert_eq!(json!(outcome.emit_skipped()), expected["emit_refused"]);
        assert_eq!(
            json!(outcome.emitted_files()),
            expected["emit_result"]["emitted_files"]
        );
        let maps = outcome.source_maps().map(|maps| {
            maps.iter()
                .map(|map| {
                    json!({"inputSourceFileNames":map.input_source_files(),
                    "sourceMap":serde_json::from_str::<Value>(map.canonical_json()).unwrap()})
                })
                .collect::<Vec<_>>()
        });
        assert_eq!(json!(maps), expected["emit_result"]["source_maps"]);
        let writes = expected["writes"].as_array().unwrap();
        assert_eq!(sink.writes().len(), writes.len());
        for (index, (actual, expected)) in sink.writes().iter().zip(writes).enumerate() {
            assert_eq!(json!(index), expected["index"]);
            // Rust's OutputSink result is the writeFile onError channel.
            assert_eq!(expected["on_error_callback_present"], true);
            assert_artifact(actual, expected);
        }
    }
}

struct CliTree(PathBuf);
impl CliTree {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "tsc-rs-h2-7e-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        // Resolve macOS /var -> /private/var once, before constructing expected
        // CLI paths, so the relocation is an input transformation, not an output fix.
        Self(root.canonicalize().unwrap())
    }
}
impl Drop for CliTree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn h2_7e_cli_status_and_exit_match_every_typescript_observation() {
    compare_cases(assert_cli_case);
}

fn assert_cli_case(case: &Value) {
    let prefix = format!("{}/", case["current_directory"].as_str().unwrap());
    let tree = CliTree::new();
    let roots = case["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| {
            let name = file["path"]
                .as_str()
                .unwrap()
                .strip_prefix(prefix.as_str())
                .unwrap();
            let destination = tree.0.join(name);
            std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
            std::fs::write(destination, file["text"].as_str().unwrap()).unwrap();
            name.to_owned()
        })
        .collect::<Vec<_>>();
    let mut options = case["options"].clone();
    assert_eq!(options["target"], 2);
    assert_eq!(options["module"], 1);
    options["target"] = json!("es2015");
    options["module"] = json!("commonjs");
    options["newLine"] = json!(if options["newLine"] == 0 {
        "crlf"
    } else {
        "lf"
    });
    if let Some(directory) = options["declarationDir"]
        .as_str()
        .and_then(|s| s.strip_prefix(prefix.as_str()))
    {
        options["declarationDir"] = json!(tree.0.join(directory));
    }
    std::fs::write(
        tree.0.join("tsconfig.json"),
        serde_json::to_vec(&json!({"compilerOptions":options,"files":roots})).unwrap(),
    )
    .unwrap();
    if let Some(rules) = case["sink_rules"].as_array() {
        // A directory occupying an output filename exercises the real CLI's
        // filesystem failure path. The exact controlled diagnostic text is
        // compared by the Program sink test; CLI status/exit are invariant.
        for rule in rules {
            assert!(matches!(
                rule["action"].as_str().unwrap(),
                "on-error" | "system-throw"
            ));
            let relative = rule["path"]
                .as_str()
                .unwrap()
                .strip_prefix(prefix.as_str())
                .unwrap();
            std::fs::create_dir_all(tree.0.join(relative)).unwrap();
        }
    }
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_tsc-rs"))
        .current_dir(&tree.0)
        .args(["-p", "tsconfig.json", "--pretty", "false"])
        .output()
        .unwrap();
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let status = stdout
        .lines()
        .filter(|line| line.starts_with("TSFILE: "))
        .collect::<Vec<_>>();
    let observation = &case["typescript_observation"];
    let expected = observation
        .get("calls")
        .map_or(observation, |calls| &calls[0]);
    let expected_status = expected["status_writes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|line| {
            let relative = line
                .as_str()
                .unwrap()
                .strip_prefix(format!("TSFILE: {prefix}").as_str())
                .unwrap();
            format!("TSFILE: {}", tree.0.join(relative).display())
        })
        .collect::<Vec<_>>();
    assert_eq!(status, expected_status, "actual CLI output: {stdout}");
    assert_eq!(
        json!(output.status.code()),
        expected["exit_code"],
        "actual CLI output: {stdout}"
    );
    if case["options"]["declarationMap"] == true && case["options"]["declaration"] != true {
        // Preserve the original Program tuple above; additionally compare the
        // relocated CLI's config-diagnostic rendering and exact JS bytes.
        for write in expected["writes"].as_array().unwrap() {
            let relative = write["path"]
                .as_str()
                .unwrap()
                .strip_prefix(prefix.as_str())
                .unwrap();
            assert_eq!(
                std::fs::read(tree.0.join(relative)).unwrap(),
                base64::engine::general_purpose::STANDARD
                    .decode(write["materialized_utf8_base64"].as_str().unwrap())
                    .unwrap()
            );
        }
        let reference = std::process::Command::new("node")
            .arg(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../vendor/typescript-6.0.3/lib/_tsc.js"),
            )
            .current_dir(&tree.0)
            .args(["-p", "tsconfig.json", "--pretty", "false"])
            .output()
            .unwrap();
        assert_eq!(reference.stdout, stdout.as_bytes());
        assert_eq!(reference.stderr, output.stderr);
        assert_eq!(reference.status.code(), output.status.code());
        let actual_outputs = std::fs::read_dir(&tree.0)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name != "tsconfig.json" && !roots.contains(name))
            .collect::<std::collections::BTreeSet<_>>();
        let expected_outputs = expected["writes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|write| {
                write["path"]
                    .as_str()
                    .unwrap()
                    .strip_prefix(prefix.as_str())
                    .unwrap()
                    .to_owned()
            })
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(actual_outputs, expected_outputs);
    }
}

#[test]
fn h2_7e_javascript_and_declaration_map_order_matches_typescript() {
    compare_fixture(
        include_str!("fixtures/declaration-maps-runtime.json"),
        3,
        |case| {
            assert_program_case(case);
            assert_cli_case(case);
        },
    );
}

#[test]
fn h2_7e_bundle_maps_keep_typed_boundary() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/declaration-maps.json")).unwrap();
    let mut case = fixture["cases"][0].clone();
    let host = memory_host(&case);
    case["options"]["declaration"] = json!(true);
    case["options"]["outFile"] = json!("/project/bundle.js");
    let mut sink = MemoryOutputSink::new();
    let error = ProgramSession::new(prepared(&case, &host))
        .emit(&mut sink)
        .unwrap_err();
    assert!(matches!(
        error,
        tsc_compiler::DriverError::Emit(tsc_emitter::EmitFailure::UnsupportedCompilerOption {
            option: "outFile"
        })
    ));
    assert!(sink.writes().is_empty());
}

struct ControlledFileSystem<'a> {
    rules: &'a Value,
    attempts: Vec<Value>,
}
impl tsc_emitter::EmitFileSystem for ControlledFileSystem<'_> {
    fn write_file(&mut self, path: &Path, bytes: &[u8]) -> Result<(), String> {
        self.attempts.push(json!({"path":path,
            "callback_utf8_base64":base64::engine::general_purpose::STANDARD.encode(bytes),
            "write_byte_order_mark":false}));
        if self
            .rules
            .as_array()
            .unwrap()
            .iter()
            .any(|rule| rule["path"].as_str().unwrap() == path.to_string_lossy())
        {
            Err("H2.7e controlled system failure".to_owned())
        } else {
            Ok(())
        }
    }
    fn create_directory(&mut self, _: &Path) -> Result<(), String> {
        Ok(())
    }
    fn directory_exists(&mut self, _: &Path) -> bool {
        true
    }
}

struct ControlledSink<'a> {
    rules: &'a Value,
    system: Option<ControlledFileSystem<'a>>,
    writes: Vec<EmitArtifact>,
    feedback: Vec<(String, Option<String>, bool)>,
    materialized: Vec<usize>,
}
impl tsc_emitter::OutputSink for ControlledSink<'_> {
    fn write(
        &mut self,
        artifact: EmitArtifact,
    ) -> Result<tsc_emitter::EmitWriteDisposition, tsc_emitter::EmitIoError> {
        use tsc_emitter::{EmitIoError, EmitIoOperation, EmitWriteDisposition, FsOutputSink};
        let action = self
            .rules
            .as_array()
            .unwrap()
            .iter()
            .find(|rule| rule["path"].as_str().unwrap() == artifact.path().to_string_lossy())
            .map_or("write", |rule| rule["action"].as_str().unwrap());
        let is_declaration = artifact.kind() == tsc_emitter::EmitArtifactKind::Declaration;
        self.writes.push(artifact.clone());
        let result = if let Some(system) = &mut self.system {
            FsOutputSink::new(system).write(artifact)
        } else {
            match action {
                "on-error" => Err(EmitIoError::new(
                    EmitIoOperation::WriteFile,
                    artifact.path(),
                    "H2.7e controlled callback failure",
                )),
                "skip-unchanged" => Ok(EmitWriteDisposition::SkippedUnchanged),
                "write" => Ok(EmitWriteDisposition::Written),
                other => panic!("unselected sink action {other}"),
            }
        };
        let message = match &result {
            Err(error) => Some(error.message().to_owned()),
            _ => None,
        };
        let skipped_dts = result == Ok(EmitWriteDisposition::SkippedUnchanged) && is_declaration;
        self.feedback
            .push((action.to_owned(), message, skipped_dts));
        if result == Ok(EmitWriteDisposition::Written) {
            self.materialized.push(self.writes.len() - 1);
        }
        result
    }
}

#[test]
fn h2_7e_ordinary_api_gates_and_sink_feedback_match_typescript() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/declaration-map-apis.json")).unwrap();
    assert_eq!(fixture["typescript"], "6.0.3");
    assert_eq!(fixture["repetitions"], 2);
    let ids = [
        "sink/ordinary-command#map-on-error",
        "sink/ordinary-command#declaration-on-error",
        "sink/ordinary-command#both-on-error",
        "sink/ordinary-command#map-skip-unchanged",
        "sink/ordinary-command#declaration-skip-unchanged",
        "sink/compiler-host-system#map-throw",
        "sink/compiler-host-system#declaration-throw",
        "getter/gate#noEmitOnError",
        "getter/gate#isolatedDeclarations",
        "getter/empty",
        "getter/declaration-only-input",
    ];
    for id in ids {
        let case = fixture["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["case_id"] == id)
            .unwrap();
        let expected = case["typescript_observation"]["calls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|call| call["kind"] == "ordinary-command")
            .unwrap();
        let host = memory_host(case);
        for _ in 0..2 {
            let mut sink = ControlledSink {
                rules: &case["sink_rules"],
                system: id.starts_with("sink/compiler-host-system").then_some(
                    ControlledFileSystem {
                        rules: &case["sink_rules"],
                        attempts: Vec::new(),
                    },
                ),
                writes: Vec::new(),
                feedback: Vec::new(),
                materialized: Vec::new(),
            };
            let (outcome, reported) = ProgramSession::new(prepared(case, &host))
                .emit_with_reported_diagnostics_for_harness(&mut sink)
                .unwrap();
            assert_eq!(expected["exception"], Value::Null, "{id}");
            assert_eq!(
                diagnostics_json(&reported),
                expected["reported_diagnostics"],
                "{id}"
            );
            assert_eq!(
                diagnostics_json(outcome.diagnostics()),
                expected["emit_result"]["diagnostics"],
                "{id}"
            );
            assert_eq!(
                json!(outcome.emit_skipped()),
                expected["emit_result"]["emit_skipped"],
                "{id}"
            );
            assert_eq!(
                json!(outcome.emitted_files()),
                expected["emit_result"]["emitted_files"],
                "{id}"
            );
            assert_eq!(json!(outcome.source_maps().map(|maps| maps.iter().map(|map| json!({
                "input_source_file_names":map.input_source_files(),"source_map_json":map.canonical_json()
            })).collect::<Vec<_>>())), expected["emit_result"]["source_maps"], "{id}");
            assert_eq!(
                json!(sink.materialized),
                expected["materialized_write_indices"],
                "{id}"
            );
            assert_eq!(
                json!(sink
                    .system
                    .as_ref()
                    .map(|system| system.attempts.as_slice())
                    .unwrap_or(&[])),
                expected["system_write_attempts"],
                "{id}"
            );
            let writes = expected["writes"].as_array().unwrap();
            assert_eq!(sink.writes.len(), writes.len(), "{id}");
            for (index, ((actual, feedback), expected)) in sink
                .writes
                .iter()
                .zip(&sink.feedback)
                .zip(writes)
                .enumerate()
            {
                assert_eq!(json!(index), expected["index"]);
                assert_eq!(expected["on_error_callback_present"], true);
                // The API fixture records the original data object before and
                // after sink feedback; the immutable Rust artifact represents
                // the former, and SkippedUnchanged carries the latter mutation.
                let mut before = expected.clone();
                let (api_kind, flat_kind) = match actual.kind() {
                    tsc_emitter::EmitArtifactKind::DeclarationMap => {
                        ("source-map", "declaration-map")
                    }
                    tsc_emitter::EmitArtifactKind::Declaration => ("declaration", "declaration"),
                    tsc_emitter::EmitArtifactKind::JavaScriptMap => {
                        ("source-map", "javascript-map")
                    }
                    tsc_emitter::EmitArtifactKind::JavaScript => ("javascript", "javascript"),
                    _ => panic!("unexpected build info"),
                };
                before["kind"] = json!(flat_kind);
                assert_eq!(expected["kind"], api_kind);
                before["data_present"] = expected["data_before"]["present"].clone();
                before["data_diagnostics"] = expected["data_before"]["diagnostics"].clone();
                before["data_source_map_url_pos"] =
                    expected["data_before"]["source_map_url_pos"].clone();
                assert_artifact(actual, &before);
                let (keys, diagnostics, position) = match actual.metadata() {
                    Some(EmitWriteMetadata::Text(data)) => (
                        json!(["sourceMapUrlPos", "diagnostics"]),
                        diagnostics_json(data.diagnostics()),
                        json!(data.source_map_url_position().map(|p| p.value())),
                    ),
                    None => (Value::Null, Value::Null, Value::Null),
                    _ => panic!("unexpected build info"),
                };
                let mut data = json!({"present":actual.metadata().is_some(), "keys":keys,"diagnostics":diagnostics,"source_map_url_pos":position,"skipped_dts_write":null});
                assert_eq!(data, expected["data_before"], "{id}");
                if feedback.2 {
                    data["keys"]
                        .as_array_mut()
                        .unwrap()
                        .push(json!("skippedDtsWrite"));
                    data["skipped_dts_write"] = json!(true);
                }
                assert_eq!(data, expected["data_after"], "{id}");
                assert_eq!(json!(feedback.0), expected["sink_action"], "{id}");
                assert_eq!(
                    json!(feedback.1.iter().collect::<Vec<_>>()),
                    expected["on_error_messages"],
                    "{id}"
                );
                assert_eq!(
                    json!(sink.materialized.contains(&index)),
                    expected["sink_materialized"],
                    "{id}"
                );
            }
        }
        if id.starts_with("sink/") && !id.ends_with("skip-unchanged") {
            assert_cli_case(case);
        }
        eprintln!("H2.7e sink PASS {id}");
    }
}

#[test]
fn h2_7e_ordinary_maps_without_declaration_preserve_ts5069_and_javascript() {
    const ORIGINAL: &str = "typescript-6.0.3/compiler/declarationMapsWithoutDeclaration.ts#default";
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    use sha2::{Digest, Sha256};
    let read = |path: &str, hash: &str| -> Value {
        let bytes = std::fs::read(root.join(path)).unwrap();
        assert_eq!(format!("{:x}", Sha256::digest(&bytes)), hash);
        serde_json::from_slice(&bytes).unwrap()
    };
    let inputs = read(
        "ratchets/h2-7de-candidate-inputs.v1.json",
        "f2e078a6b6d10cd3c6df833584924c18e8f78fe98c1e41621c70e10e215a073a",
    );
    let observations = read(
        "ratchets/h2-7de-observations.v1.json",
        "1a1681b2375d27d9012b06e29808aca72aa3e39d1dbc1536b80ba2aadf9e8ce2",
    );
    let input = inputs["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["case_id"] == ORIGINAL)
        .unwrap();
    let original = observations["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["case_id"] == ORIGINAL)
        .unwrap();
    assert_eq!(
        original["input_sha256"],
        "7849a59f11919c9201e9bf1b227ad6f2d4defd63aa7b484703404946c215b4b3"
    );
    let joined = json!({"case_id":ORIGINAL,"current_directory":input["input"]["current_directory"],
        "files":input["input"]["files"],"options":input["effective_options"],"typescript_observation":original["typescript_observation"]});
    assert_eq!(
        input["input"]["roots"],
        json!(["/.src/declarationMapsWithoutDeclaration.ts"])
    );
    assert!(joined["options"].get("declaration").is_none());
    assert_program_case(&joined);
    assert_cli_case(&joined);
    compare_fixture(
        include_str!("fixtures/declaration-maps-disabled-declaration.json"),
        2,
        |case| {
            assert_eq!(case["files"], input["input"]["files"]);
            assert_program_case(case);
            assert_cli_case(case);
        },
    );
}

#[test]
fn h2_7e_scoped_ordinary_commands_preserve_existing_complete_observations() {
    for fixture in [
        include_str!("fixtures/declaration-maps.json"),
        include_str!("fixtures/declaration-maps-runtime.json"),
        include_str!("fixtures/declaration-maps-disabled-declaration.json"),
    ] {
        let fixture: Value = serde_json::from_str(fixture).unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            assert_program_case_with_session(case, true);
        }
    }
}
