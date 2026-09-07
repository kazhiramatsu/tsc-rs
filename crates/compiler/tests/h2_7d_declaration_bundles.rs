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
    let library_directory =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/typescript-6.0.3/lib");
    let libraries = std::fs::read_dir(library_directory)
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
        .collect::<Vec<_>>();
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
