//! Internal declaration Bundle comparisons using the production Program's
//! authoritative host/resolver. Runtime outFile admission stays separate.
use std::path::{Path, PathBuf};

use base64::Engine;
use serde_json::{json, Value};
use tsc_emitter::{
    create_printer, preflight_emit, transform_nodes, DeclarationPathResolver,
    DeclarationPrintHandlers, DeclarationTransformer, EmitHost, EmitResolver, EmitRoot,
    EmitSelection, GlobalNameOracle, NewLineKind, PlanDeclarationPaths, PrinterOptions,
    SourceFileTextMode, TransformArena, TransformBundle, TransformNode, TransformNodeArray,
    TransformRoot, TransformationResult,
};
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_emitting_program, CompilerOptions, LibraryCatalog, PreparedProgram, ProgramLoadLimits,
    ProgramOptions,
};
use tsc_syntax::{FileReference, NodeData, SyntaxKind, TypeReferenceDirective};

fn options(value: &Value) -> CompilerOptions {
    let mut options = CompilerOptions::default();
    for (key, value) in value.as_object().unwrap() {
        match key.as_str() {
            "target" => options.target = Some(value.as_i64().unwrap() as i32),
            "module" => options.module = Some(value.as_i64().unwrap() as i32),
            "moduleResolution" => options.module_resolution = Some(value.as_i64().unwrap() as i32),
            "declaration" => options.declaration = value.as_bool(),
            "outFile" => options.out_file = value.as_str().map(str::to_owned),
            "listEmittedFiles" => options.list_emitted_files = value.as_bool(),
            "ignoreDeprecations" => options.ignore_deprecations = value.as_str().map(str::to_owned),
            "strict" => options.strict = value.as_bool(),
            "skipDefaultLibCheck" => options.skip_default_lib_check = value.as_bool(),
            "noErrorTruncation" => options.no_error_truncation = value.as_bool(),
            "newLine" => options.new_line = Some(value.as_i64().unwrap() as i32),
            "declarationDir" => options.declaration_dir = value.as_str().map(str::to_owned),
            "emitBOM" => options.emit_bom = value.as_bool(),
            "allowJs" => options.allow_js = value.as_bool().unwrap(),
            "checkJs" => options.check_js = value.as_bool(),
            "resolveJsonModule" => options.resolve_json_module = value.as_bool(),
            "stripInternal" => options.strip_internal = value.as_bool(),
            "noEmitOnError" => options.no_emit_on_error = value.as_bool(),
            "sourceMap" => options.source_map = value.as_bool(),
            "declarationMap" => options.declaration_map = value.as_bool(),
            "inlineSourceMap" => options.inline_source_map = value.as_bool(),
            "inlineSources" => options.inline_sources = value.as_bool(),
            "emitDeclarationOnly" => options.emit_declaration_only = value.as_bool(),
            "alwaysStrict" => options.always_strict = value.as_bool(),
            "noResolve" => options.no_resolve = value.as_bool(),
            other => panic!("unprojected option {other}"),
        }
    }
    options
}

fn prepare(case: &Value, libraries: &[(String, Vec<u8>)]) -> PreparedProgram {
    let mut host = MemoryCompilerHost::builder(case["current_directory"].as_str().unwrap())
        .case_sensitive(case["use_case_sensitive_file_names"].as_bool().unwrap());
    for file in case["files"].as_array().unwrap() {
        host = host.file(
            file["path"].as_str().unwrap(),
            file["text"].as_str().unwrap().as_bytes(),
        );
    }
    for (name, bytes) in libraries {
        host = host.file(format!("/lib/{name}"), bytes.clone());
    }
    let roots = case["roots"]
        .as_array()
        .map(|roots| {
            roots
                .iter()
                .map(|root| PathBuf::from(root.as_str().unwrap()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| {
            case["files"]
                .as_array()
                .unwrap()
                .iter()
                .map(|file| PathBuf::from(file["path"].as_str().unwrap()))
                .collect()
        });
    load_emitting_program(
        &host.build().unwrap(),
        &roots,
        options(&case["options"]),
        ProgramOptions::default(),
        &LibraryCatalog::typescript_6_0_3("/lib"),
        ProgramLoadLimits::new(256, 2048, 64, 16 * 1024 * 1024, 128 * 1024 * 1024),
    )
    .unwrap()
}

struct GlobalNames<'a>(&'a dyn EmitResolver);
impl GlobalNameOracle for GlobalNames<'_> {
    fn has_global_name(&self, name: &str) -> Result<bool, tsc_emitter::EmitResolverError> {
        self.0.has_global_name(name)
    }
}

fn reference(reference: &FileReference) -> Value {
    let mut value = json!({"pos":reference.pos as i32,"end":reference.end as i32,"fileName":reference.file_name});
    if reference.preserve {
        value["preserve"] = json!(true);
    }
    value
}
fn type_reference(reference: &TypeReferenceDirective) -> Value {
    let mut value = json!({"pos":reference.pos as i32,"end":reference.end as i32,"fileName":reference.file_name});
    if reference.preserve {
        value["preserve"] = json!(true);
    }
    if let Some(mode) = reference.resolution_mode {
        value["resolutionMode"] = json!(match mode {
            tsc_syntax::TypeReferenceDirectiveResolutionMode::Import => 99,
            tsc_syntax::TypeReferenceDirectiveResolutionMode::Require => 1,
        });
    }
    value
}

fn node_name(arena: &TransformArena, node: Option<TransformNode>) -> Option<String> {
    match &arena.node(node?).ok()?.data {
        NodeData::Identifier(data) => Some(data.text.clone()),
        NodeData::StringLiteral(data) => Some(data.text.clone()),
        _ => None,
    }
}

fn statement_shape(arena: &TransformArena, node: TransformNode) -> Value {
    let record = arena.node(node).unwrap();
    let child = |id: Option<tsc_syntax::NodeId>| id.map(|id| TransformNode::new(node.source(), id));
    let (name, modifiers) = match &record.data {
        NodeData::VariableStatement(data) => (None, data.modifiers),
        NodeData::FunctionDeclaration(data) => (data.name, data.modifiers),
        NodeData::ClassDeclaration(data) => (data.name, data.modifiers),
        NodeData::InterfaceDeclaration(data) => (data.name, data.modifiers),
        NodeData::TypeAliasDeclaration(data) => (data.name, data.modifiers),
        NodeData::EnumDeclaration(data) => (data.name, data.modifiers),
        NodeData::ModuleDeclaration(data) => (data.name, data.modifiers),
        NodeData::ImportDeclaration(data) => (None, data.modifiers),
        NodeData::ImportEqualsDeclaration(data) => (data.name, data.modifiers),
        NodeData::ExportDeclaration(data) => (None, data.modifiers),
        NodeData::ExportAssignment(data) => (None, data.modifiers),
        _ => panic!("unobserved declaration statement {:?}", record.kind),
    };
    let modifiers = modifiers.map(|array| {
        arena
            .node_array(TransformNodeArray::new(node.source(), array))
            .unwrap()
            .nodes
            .iter()
            .map(|&id| {
                format!(
                    "{:?}",
                    arena
                        .node(TransformNode::new(node.source(), id))
                        .unwrap()
                        .kind
                )
            })
            .collect::<Vec<_>>()
    });
    let declaration_names = match &record.data {
        NodeData::VariableStatement(data) => {
            let NodeData::VariableDeclarationList(list) = &arena
                .node(child(data.declaration_list).unwrap())
                .unwrap()
                .data
            else {
                panic!("declaration list")
            };
            Some(
                arena
                    .node_array(TransformNodeArray::new(
                        node.source(),
                        list.declarations.unwrap(),
                    ))
                    .unwrap()
                    .nodes
                    .iter()
                    .map(|&declaration| {
                        let NodeData::VariableDeclaration(data) = &arena
                            .node(TransformNode::new(node.source(), declaration))
                            .unwrap()
                            .data
                        else {
                            panic!("declaration")
                        };
                        let name = child(data.name).unwrap();
                        node_name(arena, Some(name))
                            .unwrap_or_else(|| format!("{:?}", arena.node(name).unwrap().kind))
                    })
                    .collect::<Vec<_>>(),
            )
        }
        _ => None,
    };
    let module_specifier = match &record.data {
        NodeData::ImportDeclaration(data) => node_name(arena, child(data.module_specifier)),
        NodeData::ExportDeclaration(data) => node_name(arena, child(data.module_specifier)),
        _ => None,
    };
    let empty_export = match &record.data {
        NodeData::ExportDeclaration(data) => child(data.export_clause).is_some_and(|clause| {
            let NodeData::NamedExports(data) = &arena.node(clause).unwrap().data else {
                return false;
            };
            data.elements.is_none_or(|array| {
                arena
                    .node_array(TransformNodeArray::new(node.source(), array))
                    .unwrap()
                    .nodes
                    .is_empty()
            })
        }),
        _ => false,
    };
    let body = match &record.data {
        NodeData::ModuleDeclaration(data) => child(data.body).and_then(|body| {
            let NodeData::ModuleBlock(data) = &arena.node(body).unwrap().data else {return None};
            let array = arena.node_array(TransformNodeArray::new(node.source(), data.statements.unwrap())).unwrap();
            Some(json!({"pos":array.pos as i32,"end":array.end as i32,"statements":array.nodes.iter().map(|&id| statement_shape(arena, TransformNode::new(node.source(), id))).collect::<Vec<_>>()}))
        }),
        _ => None,
    };
    let kind = if record.kind == SyntaxKind::VariableStatement {
        "FirstStatement".to_owned()
    } else {
        format!("{:?}", record.kind)
    };
    json!({"kind":kind,"pos":record.pos as i32,"end":record.end as i32,"name":node_name(arena, child(name)),
        "modifiers":modifiers,"declaration_names":declaration_names,"module_specifier":module_specifier,
        "empty_export":empty_export,"body":body})
}

fn bundle_shape(result: &TransformationResult<'_>, bundle: &TransformBundle) -> Value {
    let arena = result.arena();
    let sources = bundle.sources().iter().map(|&source| {
        let syntax = arena.source(source).unwrap().syntax();
        let root = arena.root(source).unwrap();
        let NodeData::SourceFile(data) = &arena.node(root).unwrap().data else {panic!("source")};
        let array = arena.node_array(TransformNodeArray::new(source, data.statements.unwrap())).unwrap();
        json!({"file_name":syntax.file_name,"is_declaration_file":syntax.is_declaration_file,
            "module_name":syntax.module_name,"has_no_default_lib":arena.source(source).unwrap().updated_has_no_default_lib(),
            "referenced_files":syntax.referenced_files.iter().map(reference).collect::<Vec<_>>(),
            "type_reference_directives":syntax.type_reference_directives.iter().map(type_reference).collect::<Vec<_>>(),
            "lib_reference_directives":syntax.lib_reference_directives.iter().map(reference).collect::<Vec<_>>(),
            "statements":{"pos":array.pos as i32,"end":array.end as i32,"nodes":array.nodes.iter().map(|&id|statement_shape(arena,TransformNode::new(source,id))).collect::<Vec<_>>()}})
    }).collect::<Vec<_>>();
    json!({"kind":"Bundle","synthetic_file_references":bundle.synthetic_file_references().map(|refs|refs.iter().map(reference).collect::<Vec<_>>()),
        "synthetic_type_references":bundle.synthetic_type_references().map(|refs|refs.iter().map(type_reference).collect::<Vec<_>>()),
        "synthetic_lib_references":bundle.synthetic_lib_references().map(|refs|refs.iter().map(reference).collect::<Vec<_>>()),
        "source_files":sources})
}

fn diagnostic(value: &tsc_diagnostics::Diagnostic) -> Value {
    json!({"code":value.code(),"category":format!("{:?}",value.category()),
        "file":value.file_name,"start":value.start,"length":value.length,
        "message":value.message_text(),"related_information":(value.related_information_present || !value.related.is_empty()).then(||value.related.iter().map(|related| {
            assert!(related.message.next.is_empty());
            json!({"code":related.message.code,"category":format!("{:?}",related.message.category),
                "file":related.file_name,"start":related.start,"length":related.length,"message":related.message.text,"related_information":null})
        }).collect::<Vec<_>>())})
}

fn compare_visitor(case: &Value, host: &dyn EmitHost, resolver: &dyn EmitResolver) {
    let expected = &case["ordinary_declaration_tree_reference"];
    let files = case["files"].as_array().unwrap();
    let input_order = host
        .source_file_ids()
        .iter()
        .map(|&id| host.source_file(id).unwrap())
        .map(|source| source.path().to_string_lossy().into_owned())
        .filter(|path| files.iter().any(|file| file["path"] == *path))
        .collect::<Vec<_>>();
    assert_eq!(
        json!(input_order),
        case["typescript_observation"]["program_source_order"]
    );
    let preflight = preflight_emit(host, EmitSelection::WholeProgram).unwrap();
    let declaration_writes = expected["writes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|write| write["kind"] == "declaration")
        .collect::<Vec<_>>();
    if preflight.plan().units().is_empty() {
        assert_eq!(expected["roots"], json!([]));
        assert!(declaration_writes.is_empty());
        return;
    }
    assert_eq!(preflight.plan().units().len(), 1);
    let unit = &preflight.plan().units()[0];
    let EmitRoot::Bundle(root) = unit.root() else {
        panic!("bundle output unit")
    };
    let mut arena = TransformArena::new();
    let source_ids = host
        .source_file_ids()
        .iter()
        .map(|&id| {
            let source = host.source_file(id).unwrap();
            (id, arena.add_source(source.syntax().unwrap(), Some(id)))
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let selected = root
        .source_files()
        .iter()
        .copied()
        .filter(|&id| {
            !host
                .source_file(id)
                .unwrap()
                .path()
                .to_string_lossy()
                .to_ascii_lowercase()
                .ends_with(".json")
        })
        .map(|id| source_ids[&id])
        .collect();
    let paths = PlanDeclarationPaths::new(host, &preflight);
    let options = host.compiler_options();
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::Bundle(TransformBundle::new(selected))],
        vec![Box::new(DeclarationTransformer::new(
            options, resolver, host, &paths,
        ))],
        false,
    )
    .unwrap();
    assert_eq!(result.roots().len(), 1);
    let TransformRoot::Bundle(bundle) = result.roots()[0].clone() else {
        panic!("declaration Bundle")
    };
    assert_eq!(
        json!([bundle_shape(&result, &bundle)]),
        expected["roots"],
        "{} tree",
        case["case_id"]
    );
    let mut diagnostics = result.diagnostics().to_vec();
    tsc_diagnostics::sort_and_dedupe_diagnostics(&mut diagnostics);
    assert_eq!(
        json!(diagnostics.iter().map(diagnostic).collect::<Vec<_>>()),
        expected["emit_result"]["diagnostics"]
    );
    let path = paths.bundle_declaration_file_path().unwrap();
    if !diagnostics.is_empty() || preflight.is_emit_blocked(host, &path) {
        assert!(declaration_writes.is_empty());
        assert_eq!(expected["emit_result"]["emit_skipped"], true);
        result.dispose();
        return;
    }
    assert_eq!(declaration_writes.len(), 1);
    let write = declaration_writes[0];
    assert_eq!(path.to_string_lossy(), write["path"].as_str().unwrap());
    assert_eq!(
        json!(bundle
            .sources()
            .iter()
            .map(|&source| result
                .arena()
                .source(source)
                .unwrap()
                .syntax()
                .file_name
                .clone())
            .collect::<Vec<_>>()),
        write["source_files"]
    );
    let newline = if options.new_line == Some(0) {
        NewLineKind::CarriageReturnLineFeed
    } else {
        NewLineKind::LineFeed
    };
    let printer_options = PrinterOptions::new(newline)
        .with_remove_comments(options.remove_comments == Some(true))
        .with_no_emit_helpers(true)
        .with_declaration_syntax(true)
        .with_only_print_js_doc_style(true)
        .with_omit_brace_source_map_positions(true)
        .with_target(options.emit_script_target())
        .with_module_kind(options.emit_module_kind())
        .with_source_file_text_mode(SourceFileTextMode::Canonical);
    let names = GlobalNames(resolver);
    let printed = create_printer(printer_options)
        .print_declaration_bundle(
            &mut result,
            &bundle,
            DeclarationPrintHandlers::new(&names),
            None,
        )
        .unwrap();
    let expected_bytes = base64::engine::general_purpose::STANDARD
        .decode(write["callback_utf8_base64"].as_str().unwrap())
        .unwrap();
    assert_eq!(
        printed.text().as_bytes(),
        expected_bytes,
        "{} bytes",
        case["case_id"]
    );
    assert_eq!(
        printed.text().len() as u64,
        write["callback_utf8_bytes"].as_u64().unwrap()
    );
    assert!(printed.source_map().is_none());
    result.dispose();
}

#[test]
fn ordinary_declaration_bundles_match_typescript_visitor_and_printer_twice() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../emitter/tests/fixtures/bundle-declarations.json"
    ))
    .unwrap();
    let libraries = libraries();
    let deferred = [
        "diagnostics/no-emit-on-error",
        "diagnostics/semantic-gate",
        "sequence/adjacent/declaration-collision#true",
    ];
    let mut compared = 0;
    let mut failures = Vec::new();
    for case in fixture["cases"].as_array().unwrap() {
        let id = case["case_id"].as_str().unwrap();
        if case["owners"]
            .as_array()
            .unwrap()
            .iter()
            .any(|owner| owner == "H2.7e")
        {
            continue; // Complete compound map bytes belong to the next D/E packet.
        }
        if deferred.contains(&id) {
            assert_eq!(
                case["ordinary_declaration_tree_reference"]["roots"],
                json!([])
            );
            continue; // These Program gates precede this internal visitor seam.
        }
        compared += 1;
        for repetition in 0..2 {
            let checked = std::panic::catch_unwind(|| {
                let prepared = prepare(case, &libraries);
                let (observation, check) = tsc_compiler::ProgramSession::new(prepared)
                    .with_checked_emit_resolver_for_harness(|host, resolver, checked| {
                        assert!(checked.partial_checks.is_empty());
                        compare_visitor(case, host, resolver);
                        Ok(())
                    })
                    .unwrap();
                assert!(check.partial_checks.is_empty());
                if observation.is_none() {
                    assert_eq!(
                        case["ordinary_declaration_tree_reference"]["roots"],
                        json!([])
                    );
                    assert_eq!(
                        case["ordinary_declaration_tree_reference"]["writes"],
                        json!([])
                    );
                }
            });
            if checked.is_err() {
                failures.push(format!("{id} #{repetition}"));
            }
        }
    }
    assert_eq!(compared, 19);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn libraries() -> Vec<(String, Vec<u8>)> {
    let library_directory =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/typescript-6.0.3/lib");
    std::fs::read_dir(library_directory)
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with("lib.") && name.ends_with(".d.ts")
        })
        .map(|entry| {
            (
                entry.file_name().to_string_lossy().into_owned(),
                std::fs::read(entry.path()).unwrap(),
            )
        })
        .collect::<Vec<_>>()
}

fn compare_map_artifact(actual: &tsc_emitter::EmitArtifact, expected: &Value) {
    assert_eq!(
        actual.path().to_string_lossy(),
        expected["path"].as_str().unwrap()
    );
    assert_eq!(
        actual.callback_bytes(),
        base64::engine::general_purpose::STANDARD
            .decode(expected["callback_utf8_base64"].as_str().unwrap())
            .unwrap()
    );
    assert_eq!(
        json!(actual.callback_bytes().len()),
        expected["callback_utf8_bytes"]
    );
    assert_eq!(
        json!(actual.write_byte_order_mark()),
        expected["write_byte_order_mark"]
    );
    assert_eq!(
        actual.materialized_bytes().as_ref(),
        base64::engine::general_purpose::STANDARD
            .decode(expected["materialized_utf8_base64"].as_str().unwrap())
            .unwrap()
    );
    assert_eq!(
        json!(actual.materialized_bytes().len()),
        expected["materialized_utf8_bytes"]
    );
    assert_eq!(
        json!(actual.source_files().map(|files| files
            .iter()
            .map(|file| file.to_string_lossy().into_owned())
            .collect::<Vec<_>>())),
        expected["source_files"]
    );
    assert_eq!(expected["data_build_info"], Value::Null);
    match actual.metadata() {
        Some(tsc_emitter::EmitWriteMetadata::Text(data)) => {
            assert_eq!(actual.kind(), tsc_emitter::EmitArtifactKind::Declaration);
            assert_eq!(expected["data_present"], true);
            assert_eq!(
                expected["data_keys"],
                json!(["sourceMapUrlPos", "diagnostics"])
            );
            assert!(data.diagnostics().is_empty());
            assert_eq!(expected["data_diagnostics"], json!([]));
            assert_eq!(
                json!(data.source_map_url_position().map(|pos| pos.value())),
                expected["data_source_map_url_pos"]
            );
        }
        None => {
            assert_eq!(actual.kind(), tsc_emitter::EmitArtifactKind::DeclarationMap);
            for key in ["data_keys", "data_diagnostics", "data_source_map_url_pos"] {
                assert_eq!(expected[key], Value::Null);
            }
            assert_eq!(expected["data_present"], false);
        }
        _ => panic!("unexpected build info"),
    }
}

#[allow(clippy::too_many_arguments)]
fn compare_recorded_output(
    case: &Value,
    printed: &tsc_emitter::PrintedText,
    path: &Path,
    map_path: Option<&Path>,
    map_options: &CompilerOptions,
    lane: &tsc_emitter::MapLaneInputs,
    first_source_path: &Path,
    declaration: bool,
    source_files: &[PathBuf],
    map_observations: &mut Vec<Value>,
) {
    let expected = &case["typescript_observation"];
    let write = expected["writes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|write| write["path"] == path.to_string_lossy().as_ref())
        .unwrap();
    let newline = if case["options"]["newLine"] == 0 {
        "\r\n"
    } else {
        "\n"
    };
    if declaration && printed.source_map().is_some() {
        let artifacts = tsc_emitter::finish_declaration_bundle_map(
            lane,
            map_options,
            path,
            map_path.unwrap(),
            source_files,
            printed,
            vec![],
            if newline == "\r\n" {
                NewLineKind::CarriageReturnLineFeed
            } else {
                NewLineKind::LineFeed
            },
        )
        .unwrap();
        for artifact in [&artifacts.map, &artifacts.declaration] {
            let expected_write = expected["writes"]
                .as_array()
                .unwrap()
                .iter()
                .find(|write| write["path"] == artifact.path().to_string_lossy().as_ref())
                .unwrap();
            compare_map_artifact(artifact, expected_write);
        }
        map_observations.push(
            json!({ "input_source_file_names": artifacts.observation.input_source_files(),
            "source_map_json": artifacts.observation.canonical_json() }),
        );
        return;
    }
    let mut text = printed.text().to_owned();
    if let Some(generator) = printed.source_map() {
        let mut generator = generator.clone();
        let map_json = generator.to_json_string();
        map_observations.push(json!({
            "input_source_file_names": generator.raw_sources(), "source_map_json": map_json,
        }));
        if let Some(map_path) = map_path {
            let map_write = expected["writes"]
                .as_array()
                .unwrap()
                .iter()
                .find(|write| write["path"] == map_path.to_string_lossy().as_ref())
                .unwrap();
            let expected_map = base64::engine::general_purpose::STANDARD
                .decode(map_write["callback_utf8_base64"].as_str().unwrap())
                .unwrap();
            assert_eq!(
                map_json.as_bytes(),
                expected_map,
                "{} map bytes",
                case["case_id"]
            );
            assert_eq!(map_write["write_byte_order_mark"], false);
            assert_eq!(map_write["data_present"], false);
            assert_eq!(map_write["source_files"], write["source_files"]);
        }
        // These unchanged inputs have no root options. The existing URL
        // worker therefore does not consult its standalone source argument.
        assert!(map_options.map_root.is_none() && map_options.source_root.is_none());
        let url = tsc_emitter::source_mapping_url(
            lane,
            map_options,
            &map_json,
            path,
            map_path,
            first_source_path,
        )
        .unwrap();
        if printed.end().column() != 0 {
            text.push_str(newline);
        }
        assert_eq!(
            json!(text.encode_utf16().count()),
            write["data_source_map_url_pos"]
        );
        text.push_str("//# sourceMappingURL=");
        text.push_str(&url);
    } else {
        assert_eq!(write["data_source_map_url_pos"], Value::Null);
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(write["callback_utf8_base64"].as_str().unwrap())
        .unwrap();
    assert_eq!(
        text.as_bytes(),
        bytes,
        "{} {} text",
        case["case_id"],
        if declaration {
            "declaration"
        } else {
            "JavaScript"
        }
    );
    assert_eq!(json!(text.len()), write["callback_utf8_bytes"]);
    let bom = case["options"]["emitBOM"] == true;
    assert_eq!(write["write_byte_order_mark"], bom);
    let materialized = if bom {
        [&[239, 187, 191][..], text.as_bytes()].concat()
    } else {
        text.into_bytes()
    };
    assert_eq!(
        materialized,
        base64::engine::general_purpose::STANDARD
            .decode(write["materialized_utf8_base64"].as_str().unwrap())
            .unwrap()
    );
}

fn compare_bundle_recording(
    case: &Value,
    host: &dyn EmitHost,
    resolver: &dyn EmitResolver,
    forced: bool,
) {
    use tsc_emitter::{
        get_script_transformers_for_source, source_map_recording_inputs_for, MapLaneInputs,
        PrintRequest,
    };
    let options = host.compiler_options();
    assert!(options.source_root.is_none() && options.map_root.is_none());
    let expected = &case["typescript_observation"];
    let preflight = preflight_emit(host, EmitSelection::WholeProgram).unwrap();
    assert_eq!(preflight.plan().units().len(), 1);
    let unit = &preflight.plan().units()[0];
    let EmitRoot::Bundle(root) = unit.root() else {
        panic!("bundle unit")
    };
    let source_names = root
        .source_files()
        .iter()
        .map(|&id| {
            host.source_file(id)
                .unwrap()
                .path()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        json!(source_names),
        expected["program_source_order"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|name| !name.as_str().unwrap().ends_with(".d.ts"))
            .cloned()
            .collect::<Value>()
    );
    for write in expected["writes"].as_array().unwrap() {
        let declaration = write["path"].as_str().unwrap().contains(".d.ts");
        let associated = source_names
            .iter()
            .filter(|name| !declaration || forced || !name.to_ascii_lowercase().ends_with(".json"))
            .collect::<Vec<_>>();
        assert_eq!(write["source_files"], json!(associated));
    }
    let first = root.source_files()[0];
    let first_source_path = host.source_file(first).unwrap().path().to_path_buf();
    let common = host
        .common_source_directory()
        .to_string_lossy()
        .replace('\\', "/");
    let lane = MapLaneInputs {
        common_source_directory: if common.ends_with('/') {
            common
        } else {
            format!("{common}/")
        },
        current_directory: host
            .current_directory()
            .to_string_lossy()
            .replace('\\', "/"),
        use_case_sensitive_source_keys: host.use_case_sensitive_file_names(),
    };
    let new_line = if options.new_line == Some(0) {
        NewLineKind::CarriageReturnLineFeed
    } else {
        NewLineKind::LineFeed
    };
    let mut maps = Vec::new();
    let mut emit_diagnostics = Vec::new();
    let mut parsed_metadata = None;
    for declaration in [false, true] {
        if forced && !declaration {
            continue;
        }
        let Some(path) = (if declaration {
            unit.paths().declaration_path()
        } else {
            unit.paths().javascript_path()
        }) else {
            continue;
        };
        assert!(!preflight.is_emit_blocked(host, path));
        let mut arena = TransformArena::new();
        let ids = host
            .source_file_ids()
            .iter()
            .map(|&id| {
                (
                    id,
                    arena.add_source(host.source_file(id).unwrap().syntax().unwrap(), Some(id)),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let selected = root
            .source_files()
            .iter()
            .filter(|&&id| {
                !declaration
                    || forced
                    || !host
                        .source_file(id)
                        .unwrap()
                        .path()
                        .to_string_lossy()
                        .to_ascii_lowercase()
                        .ends_with(".json")
            })
            .map(|id| ids[id])
            .collect();
        if declaration {
            if let Some(snapshot) = &parsed_metadata {
                arena.restore_parsed_emit_metadata(snapshot, host).unwrap();
            }
        }
        let paths = PlanDeclarationPaths::new(host, &preflight);
        let transformers: Vec<Box<dyn tsc_emitter::Transformer + '_>> = if declaration {
            vec![Box::new(DeclarationTransformer::new(
                options, resolver, host, &paths,
            ))]
        } else {
            get_script_transformers_for_source(options, resolver, host, first).unwrap()
        };
        let mut result = transform_nodes(
            arena,
            vec![TransformRoot::Bundle(TransformBundle::new(selected))],
            transformers,
            false,
        )
        .unwrap();
        if !result.diagnostics().is_empty() {
            assert!(declaration);
            emit_diagnostics.extend(result.diagnostics().iter().cloned());
            if !forced {
                assert!(expected["writes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|write| write["path"] != path.to_string_lossy().as_ref()));
                result.dispose();
                continue;
            }
        }
        let TransformRoot::Bundle(bundle) = result.roots()[0].clone() else {
            panic!("bundle result")
        };
        let map_options = if declaration {
            CompilerOptions {
                source_map: options.declaration_map,
                source_root: options.source_root.clone(),
                map_root: options.map_root.clone(),
                ..CompilerOptions::default()
            }
        } else {
            options.clone()
        };
        let recording = (map_options.source_map == Some(true)
            || map_options.inline_source_map == Some(true))
        .then(|| {
            if declaration {
                tsc_emitter::declaration_bundle_map_recording_inputs_for(&lane, options, path)
            } else {
                source_map_recording_inputs_for(&lane, &map_options, path, &first_source_path)
            }
        });
        let printer_options = PrinterOptions::new(new_line)
            .with_target(options.emit_script_target())
            .with_module_kind(options.emit_module_kind())
            .with_remove_comments(options.remove_comments == Some(true))
            .with_no_emit_helpers(declaration || options.no_emit_helpers == Some(true))
            .with_import_helpers(options.import_helpers == Some(true))
            .with_source_file_text_mode(SourceFileTextMode::Canonical)
            .with_declaration_syntax(declaration)
            .with_only_print_js_doc_style(declaration)
            .with_omit_brace_source_map_positions(declaration);
        let mut printer = create_printer(printer_options);
        let names = GlobalNames(resolver);
        let printed = if declaration {
            printer
                .print_declaration_bundle(
                    &mut result,
                    &bundle,
                    DeclarationPrintHandlers::new(&names),
                    recording,
                )
                .unwrap()
        } else {
            printer
                .print(&mut result, PrintRequest::Bundle(bundle.clone()), recording)
                .unwrap()
        };
        if !declaration {
            parsed_metadata = Some(result.arena().snapshot_parsed_emit_metadata(host).unwrap());
            if let Some(phases) = expected["phases"].as_array() {
                let last = phases
                    .iter()
                    .rev()
                    .find(|phase| phase["phase"] == "after-js")
                    .unwrap();
                let project = |value: &Value| {
                    json!({ "file": value["parsed_identity"]["file"],
                    "kind": value["kind"], "pos": value["pos"], "end": value["end"], "flags": value["flags"],
                    "type_kind": value["type_node"]["kind"] })
                };
                let mut expected_metadata = last["parsed_metadata"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(project)
                    .map(|value| value.to_string())
                    .collect::<Vec<_>>();
                let mut actual_metadata = Vec::new();
                for (&program, &source) in &ids {
                    let parsed = host.source_file(program).unwrap().syntax().unwrap();
                    for (offset, record) in parsed.arena.nodes().iter().enumerate() {
                        let node = TransformNode::new(
                            source,
                            tsc_syntax::NodeId(parsed.arena.node_base() + offset as u32),
                        );
                        let Some(metadata) = result.arena().metadata(node) else {
                            continue;
                        };
                        if metadata.flags().is_empty() && metadata.type_node().is_none() {
                            continue;
                        }
                        actual_metadata.push(json!({ "file": parsed.file_name, "kind": format!("{:?}", record.kind),
                            "pos": parsed.positions().byte_to_utf16(record.pos).unwrap(),
                            "end": parsed.positions().byte_to_utf16(record.end).unwrap(), "flags": metadata.flags().bits(),
                            "type_kind": metadata.type_node().map(|node| format!("{:?}", result.arena().node(node).unwrap().kind)),
                        }).to_string());
                    }
                }
                actual_metadata.sort();
                expected_metadata.sort();
                assert_eq!(
                    actual_metadata, expected_metadata,
                    "{} direct parse metadata",
                    case["case_id"]
                );
            }
        }
        if (!declaration || forced) && expected["parsed_constant_values"].is_array() {
            let mut actual_constants = Vec::new();
            for (&program, &source) in &ids {
                let parsed = host.source_file(program).unwrap().syntax().unwrap();
                for (offset, record) in parsed.arena.nodes().iter().enumerate() {
                    let node = TransformNode::new(
                        source,
                        tsc_syntax::NodeId(parsed.arena.node_base() + offset as u32),
                    );
                    let Some(value) = result
                        .arena()
                        .metadata(node)
                        .and_then(tsc_emitter::EmitMetadata::constant_value)
                    else {
                        continue;
                    };
                    let value = match value {
                        tsc_emitter::EmitConstantValue::Number(value) => {
                            json!({ "number_bits": format!("{:016x}", value.bits()) })
                        }
                        tsc_emitter::EmitConstantValue::String(value) => {
                            json!({ "string_utf16": value.code_units() })
                        }
                        other => panic!("unobserved constant {other:?}"),
                    };
                    actual_constants.push(json!({ "file": parsed.file_name, "kind": format!("{:?}", record.kind),
                        "pos": parsed.positions().byte_to_utf16(record.pos).unwrap(),
                        "end": parsed.positions().byte_to_utf16(record.end).unwrap(), "value": value }).to_string());
                }
            }
            let mut expected_constants = expected["parsed_constant_values"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| {
                    json!({ "file": value["file"], "kind": value["kind"], "pos": value["pos"],
                        "end": value["end"], "value": value["value"] })
                    .to_string()
                })
                .collect::<Vec<_>>();
            actual_constants.sort();
            expected_constants.sort();
            assert_eq!(
                actual_constants, expected_constants,
                "{} direct constantValue metadata",
                case["case_id"]
            );
        }
        let map_path = if declaration {
            unit.paths().declaration_map_path()
        } else {
            unit.paths().javascript_map_path()
        };
        let source_files = bundle
            .sources()
            .iter()
            .map(|&id| PathBuf::from(&result.arena().source(id).unwrap().syntax().file_name))
            .collect::<Vec<_>>();
        compare_recorded_output(
            case,
            &printed,
            path,
            map_path,
            options,
            &lane,
            &first_source_path,
            declaration,
            &source_files,
            &mut maps,
        );
        result.dispose();
    }
    assert_eq!(
        json!((options.source_map == Some(true)
            || options.inline_source_map == Some(true)
            || options.declaration_map == Some(true))
        .then_some(maps)),
        expected["emit_result"]["source_maps"],
        "{} complete sourceMaps",
        case["case_id"]
    );
    tsc_diagnostics::sort_and_dedupe_diagnostics(&mut emit_diagnostics);
    assert_eq!(
        expected["emit_result"]["diagnostics"],
        json!(emit_diagnostics.iter().map(diagnostic).collect::<Vec<_>>())
    );
    assert_eq!(
        expected["emit_result"]["emit_skipped"],
        !emit_diagnostics.is_empty()
    );
}

#[test]
fn ordinary_bundle_source_maps_match_complete_typescript_maps_twice() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../emitter/tests/fixtures/bundle-maps.json"
    ))
    .unwrap();
    assert_eq!(fixture["cases"].as_array().unwrap().len(), 12);
    let libraries = libraries();
    let mut failures = Vec::new();
    let json_references = fixture["json_bundle_references"].as_array().unwrap();
    assert_eq!(json_references.len(), 8);
    for case in fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .chain(json_references)
    {
        for repetition in 0..2 {
            let outcome = std::panic::catch_unwind(|| {
                let prepared = prepare(case, &libraries);
                let (observed, checked) = tsc_compiler::ProgramSession::new(prepared)
                    .with_checked_emit_resolver_for_harness(|host, resolver, checked| {
                        assert!(checked.partial_checks.is_empty());
                        compare_bundle_recording(case, host, resolver, false);
                        Ok(())
                    })
                    .unwrap();
                assert!(observed.is_some());
                assert!(checked.partial_checks.is_empty());
            });
            if outcome.is_err() {
                failures.push(format!("{} #{repetition}", case["case_id"]));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn ordinary_and_fresh_forced_bundle_metadata_lifetimes_match_typescript_twice() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../emitter/tests/fixtures/bundle-maps.json"
    ))
    .unwrap();
    let references = fixture["metadata_lifetime_references"].as_array().unwrap();
    assert_eq!(references.len(), 6);
    let constant_references = fixture["constant_value_references"].as_array().unwrap();
    assert_eq!(constant_references.len(), 2);
    let runtime_references = fixture["runtime_comment_owner_references"]
        .as_array()
        .unwrap();
    assert_eq!(runtime_references.len(), 3);
    let libraries = libraries();
    let mut failures = Vec::new();
    for reference in references
        .iter()
        .chain(constant_references)
        .chain(runtime_references)
    {
        for mode in ["ordinary", "fresh-forced"] {
            let mut case = reference.clone();
            case["typescript_observation"] = reference["modes"][mode].clone();
            for repetition in 0..2 {
                let outcome = std::panic::catch_unwind(|| {
                    let prepared = prepare(&case, &libraries);
                    let (observed, checked) = tsc_compiler::ProgramSession::new(prepared)
                        .with_checked_emit_resolver_for_harness(|host, resolver, checked| {
                            assert!(checked.partial_checks.is_empty());
                            compare_bundle_recording(&case, host, resolver, mode == "fresh-forced");
                            Ok(())
                        })
                        .unwrap();
                    assert!(observed.is_some());
                    assert!(checked.partial_checks.is_empty());
                });
                if outcome.is_err() {
                    failures.push(format!("{} {mode} #{repetition}", case["case_id"]));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
