//! Direct declaration transform/printer comparisons. Whole-Program command and
//! sink outcomes stay in the fixture for the subsequent runtime integration.
use std::path::Path;

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
use tsc_program::CompilerOptions;

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
            "emitDeclarationOnly" => options.emit_declaration_only = value.as_bool(),
            "listEmittedFiles" => options.list_emitted_files = value.as_bool(),
            "newLine" => options.new_line = Some(value.as_i64().unwrap() as i32),
            "strict" => options.strict = value.as_bool(),
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
        if expected["kind"] == "declaration-map" {
            tsc_emitter::EmitArtifactKind::DeclarationMap
        } else {
            tsc_emitter::EmitArtifactKind::Declaration
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
    match actual.metadata() {
        Some(EmitWriteMetadata::Text(data)) => {
            assert_eq!(expected["data_present"], true);
            assert!(data.diagnostics().is_empty());
            assert_eq!(expected["data_diagnostics"], json!([]));
            assert_eq!(
                data.source_map_url_position()
                    .map(|pos| u64::from(pos.value())),
                expected["data_source_map_url_pos"].as_u64()
            );
        }
        None => assert_eq!(expected["data_present"], false),
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
