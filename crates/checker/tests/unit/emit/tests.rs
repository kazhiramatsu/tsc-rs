use super::*;

use std::sync::Arc;

use serde_json::Value;
use tsc_binder::BinderWorker;
use tsc_diagnostics::{DocumentVersion, TextSnapshot};
use tsc_emitter::{
    create_printer, get_script_transformers, transform_nodes, DisabledSourceMapRecorder,
    EmitResolverError, EmitResolverMethod, EmitResolverNode, NewLineKind, PrintRequest,
    PrinterOptions, SourceFileId, TransformArena, TransformRoot,
};
use tsc_syntax::{NodeId, ParseOptions, SyntaxKind};
use tsc_types::{CompilerOptions, IdentityDomain, ScriptTarget};

use crate::{BoundDocument, ParsedDocument, ProgramSnapshot};

const ACTIVE_TRANSFORM_ORACLE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ratchets/h1-active-transform.v1.json"
));

fn active_transform_oracle() -> Value {
    serde_json::from_slice(ACTIVE_TRANSFORM_ORACLE)
        .expect("H1.3 active-transform oracle is valid JSON")
}

fn checked_alias_session() -> (ProgramSnapshot, CompilerOptions, Vec<NodeId>, Vec<NodeId>) {
    let oracle = active_transform_oracle();
    let inputs = oracle["inputs"].as_array().expect("oracle inputs");
    let dependency_path = inputs[0]["path"].as_str().expect("dependency path");
    let dependency_text = inputs[0]["text"].as_str().expect("dependency text");
    let main_path = inputs[1]["path"].as_str().expect("main path");
    let main_text = inputs[1]["text"].as_str().expect("main text");
    let domain = IdentityDomain::reclaiming();
    let dependency = tsc_syntax::parse_source_file_from_snapshot_in_identity_domain(
        dependency_path.to_owned(),
        TextSnapshot::new(dependency_text.to_owned(), DocumentVersion::new("1")),
        ParseOptions::default(),
        None,
        &domain,
    )
    .expect("dependency source identity allocation");
    let source = tsc_syntax::parse_source_file_from_snapshot_in_identity_domain(
        main_path.to_owned(),
        TextSnapshot::new(main_text.to_owned(), DocumentVersion::new("1")),
        ParseOptions::default(),
        None,
        &domain,
    )
    .expect("source identity allocation");
    let dependency = Arc::new(dependency);
    let source = Arc::new(source);
    let options = CompilerOptions {
        target: Some(
            i32::try_from(
                oracle["compiler_options"]["target"]
                    .as_i64()
                    .expect("target"),
            )
            .expect("target fits i32"),
        ),
        module: Some(
            i32::try_from(
                oracle["compiler_options"]["module"]
                    .as_i64()
                    .expect("module"),
            )
            .expect("module fits i32"),
        ),
        use_define_for_class_fields: oracle["compiler_options"]["useDefineForClassFields"]
            .as_bool(),
        ..CompilerOptions::default()
    };
    assert_eq!(options.target, Some(ScriptTarget::ES_NEXT.bits()));
    let dependency_worker = BinderWorker::bind_in_identity_domain(&dependency, &options, &domain)
        .expect("dependency bind identity allocation");
    let worker = BinderWorker::bind_in_identity_domain(&source, &options, &domain)
        .expect("bind identity allocation");
    let dependency_document = Arc::new(BoundDocument::new(
        Arc::new(ParsedDocument::new(Arc::clone(&dependency))),
        dependency_worker.into_bind_data(),
    ));
    let document = Arc::new(BoundDocument::new(
        Arc::new(ParsedDocument::new(Arc::clone(&source))),
        worker.into_bind_data(),
    ));
    let program =
        ProgramSnapshot::new(vec![dependency_document, document], 0).expect("Program snapshot");
    let import_aliases = source
        .arena
        .node_ids()
        .filter(|id| {
            matches!(
                source.arena.node(*id).kind,
                SyntaxKind::ImportClause | SyntaxKind::ImportSpecifier
            )
        })
        .collect();
    let export_aliases = source
        .arena
        .node_ids()
        .filter(|id| source.arena.node(*id).kind == SyntaxKind::ExportSpecifier)
        .collect();
    (program, options, import_aliases, export_aliases)
}

#[test]
fn scoped_emit_resolver_reads_live_alias_links_and_fails_closed_elsewhere() {
    let (snapshot, options, import_aliases, export_aliases) = checked_alias_session();
    assert_eq!(import_aliases.len(), 3);
    assert_eq!(export_aliases.len(), 2);

    let mut state = CheckerState::from_snapshot(&snapshot, &options);
    state.check_source_file(0);
    state.check_source_file(1);
    let session = CheckerSession::from_checked_state(state);

    let printed = session.with_emit_resolver(|resolver| {
        let node = |id| EmitResolverNode::from_raw_source(1, id);
        assert!(resolver
            .is_referenced_alias_declaration(node(import_aliases[2]))
            .expect("used default import reference"));
        assert!(!resolver
            .is_referenced_alias_declaration(node(import_aliases[0]))
            .expect("unused named import reference"));
        assert!(!resolver
            .is_referenced_alias_declaration(node(import_aliases[1]))
            .expect("type-only named import reference"));

        assert!(resolver
            .is_value_alias_declaration(node(export_aliases[0]))
            .expect("runtime export value"));
        assert!(!resolver
            .is_value_alias_declaration(node(export_aliases[1]))
            .expect("type export value"));

        assert!(matches!(
            resolver.get_constant_value(node(export_aliases[0])),
            Err(EmitResolverError::Unavailable {
                method: EmitResolverMethod::GetConstantValue,
                ..
            })
        ));
        assert!(matches!(
            resolver.is_value_alias_declaration(EmitResolverNode::from_raw_source(
                99,
                export_aliases[0]
            )),
            Err(EmitResolverError::UnknownSource {
                method: EmitResolverMethod::IsValueAliasDeclaration,
                ..
            })
        ));
        assert!(matches!(
            resolver.is_value_alias_declaration(EmitResolverNode::from_raw_source(
                0,
                export_aliases[0]
            )),
            Err(EmitResolverError::SourceNodeMismatch {
                method: EmitResolverMethod::IsValueAliasDeclaration,
                actual_program_index: 1,
                ..
            })
        ));
        assert!(matches!(
            resolver
                .is_value_alias_declaration(EmitResolverNode::from_raw_source(1, NodeId(u32::MAX))),
            Err(EmitResolverError::UnknownNode {
                method: EmitResolverMethod::IsValueAliasDeclaration,
                ..
            })
        ));

        let mut arena = TransformArena::new();
        let source = arena.add_source(
            snapshot.document(1).source(),
            Some(SourceFileId::from_raw(1)),
        );
        let mut transformed = transform_nodes(
            arena,
            vec![TransformRoot::SourceFile(source)],
            get_script_transformers(&options, resolver).expect("bootstrap transformer list"),
            false,
        )
        .expect("transform with live checker resolver");
        create_printer(PrinterOptions::new(NewLineKind::LineFeed))
            .print(
                &mut transformed,
                PrintRequest::SourceFile(source),
                &mut DisabledSourceMapRecorder,
            )
            .expect("print while checker resolver remains live")
            .text()
            .to_owned()
    });
    let oracle = active_transform_oracle();
    let expected = oracle["observation"]["writes"]
        .as_array()
        .expect("oracle writes")
        .iter()
        .find(|write| write["path"] == "/project/main.js")
        .and_then(|write| write["text"].as_str())
        .expect("main JavaScript oracle write");
    assert_eq!(printed, expected);

    let state = session.into_state();
    assert_eq!(state.binder.file_count(), 2);
}
