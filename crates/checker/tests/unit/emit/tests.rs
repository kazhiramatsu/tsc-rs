use super::*;

use std::{path::Path, sync::Arc};

use serde_json::Value;
use tsc_binder::BinderWorker;
use tsc_diagnostics::{DocumentVersion, TextSnapshot};
use tsc_emitter::{
    create_printer, get_script_transformers, get_script_transformers_for_source, transform_nodes,
    DisabledSourceMapRecorder, EmitHost, EmitResolverError, EmitResolverMethod, EmitResolverNode,
    EmitSource, NewLineKind, PrintRequest, PrinterOptions, SourceFileId, TransformArena,
    TransformRoot,
};
use tsc_syntax::{LanguageVariant, NodeId, ParseOptions, SyntaxKind};
use tsc_types::{CompilerOptions, IdentityDomain, ModuleKind, ScriptTarget};

use crate::{BoundDocument, ParsedDocument, ProgramSnapshot};

const ACTIVE_TRANSFORM_ORACLE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ratchets/h1-active-transform.v1.json"
));

fn active_transform_oracle() -> Value {
    serde_json::from_slice(ACTIVE_TRANSFORM_ORACLE)
        .expect("H1.3 active-transform oracle is valid JSON")
}

struct CheckerEmitHost<'a> {
    options: &'a CompilerOptions,
    syntax: &'a tsc_syntax::SourceFile,
    source_ids: [SourceFileId; 1],
}

impl EmitHost for CheckerEmitHost<'_> {
    fn compiler_options(&self) -> &CompilerOptions {
        self.options
    }

    fn current_directory(&self) -> &Path {
        Path::new("/project")
    }

    fn common_source_directory(&self) -> &Path {
        Path::new("/project")
    }

    fn config_file_path(&self) -> Option<&Path> {
        None
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        true
    }

    fn source_file_ids(&self) -> &[SourceFileId] {
        &self.source_ids
    }

    fn source_file(&self, id: SourceFileId) -> Option<EmitSource<'_>> {
        (id == self.source_ids[0]).then(|| {
            let path = Path::new(&self.syntax.file_name);
            EmitSource::new(id, path, path, true, None, Some(self.syntax))
        })
    }
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
fn scoped_emit_resolver_reads_live_alias_and_constant_links_and_fails_closed_elsewhere() {
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

        assert_eq!(
            resolver
                .get_constant_value(node(export_aliases[0]))
                .expect("non-enum value query"),
            None
        );
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

#[test]
fn classic_jsx_fragment_retains_only_the_fragment_factory_import() {
    let domain = IdentityDomain::reclaiming();
    let dependency = tsc_syntax::parse_source_file_from_snapshot_in_identity_domain(
        "/project/jsx.ts".to_owned(),
        TextSnapshot::new(
            "export function element() {}\nexport function fragment() {}\n".to_owned(),
            DocumentVersion::new("1"),
        ),
        ParseOptions::default(),
        None,
        &domain,
    )
    .expect("JSX factory dependency identity allocation");
    let source = tsc_syntax::parse_source_file_from_snapshot_in_identity_domain(
        "/project/index.tsx".to_owned(),
        TextSnapshot::new(
            concat!(
                "import { element, fragment } from \"./jsx\";\n",
                "export const a = <>fragment text</>;\n",
            )
            .to_owned(),
            DocumentVersion::new("1"),
        ),
        ParseOptions {
            language_variant: LanguageVariant::Jsx,
            ..ParseOptions::default()
        },
        None,
        &domain,
    )
    .expect("classic JSX source identity allocation");
    let dependency = Arc::new(dependency);
    let source = Arc::new(source);
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::COMMON_JS.bits()),
        jsx: Some(2),
        jsx_factory: Some("element".to_owned()),
        jsx_fragment_factory: Some("fragment".to_owned()),
        no_unused_locals: Some(true),
        ..CompilerOptions::default()
    };
    let dependency_worker = BinderWorker::bind_in_identity_domain(&dependency, &options, &domain)
        .expect("JSX dependency bind identity allocation");
    let worker = BinderWorker::bind_in_identity_domain(&source, &options, &domain)
        .expect("classic JSX bind identity allocation");
    let dependency_document = Arc::new(BoundDocument::new(
        Arc::new(ParsedDocument::new(Arc::clone(&dependency))),
        dependency_worker.into_bind_data(),
    ));
    let document = Arc::new(BoundDocument::new(
        Arc::new(ParsedDocument::new(Arc::clone(&source))),
        worker.into_bind_data(),
    ));
    let snapshot =
        ProgramSnapshot::new(vec![dependency_document, document], 0).expect("Program snapshot");
    let import_specifiers = source
        .arena
        .node_ids()
        .filter(|id| source.arena.node(*id).kind == SyntaxKind::ImportSpecifier)
        .collect::<Vec<_>>();
    assert_eq!(import_specifiers.len(), 2);

    let mut state = CheckerState::from_snapshot(&snapshot, &options);
    state.check_source_file(0);
    state.check_source_file(1);
    let session = CheckerSession::from_checked_state(state);
    let printed = session.with_emit_resolver(|resolver| {
        assert!(
            !resolver
                .is_referenced_alias_declaration(EmitResolverNode::from_raw_source(
                    1,
                    import_specifiers[0],
                ))
                .expect("ordinary classic JSX factory reachability"),
            "the fragment's semantic factory lookup is not import reachability",
        );
        assert!(
            resolver
                .is_referenced_alias_declaration(EmitResolverNode::from_raw_source(
                    1,
                    import_specifiers[1],
                ))
                .expect("classic JSX fragment factory reachability"),
            "the fragment factory value must survive import elision",
        );

        let program_source = SourceFileId::from_raw(1);
        let host = CheckerEmitHost {
            options: &options,
            syntax: &source,
            source_ids: [program_source],
        };
        let mut arena = TransformArena::new();
        let source_id = arena.add_source(&source, Some(program_source));
        let mut transformed = transform_nodes(
            arena,
            vec![TransformRoot::SourceFile(source_id)],
            get_script_transformers_for_source(&options, resolver, &host, program_source)
                .expect("classic JSX transformers"),
            false,
        )
        .expect("classic JSX transform");
        create_printer(PrinterOptions::new(NewLineKind::LineFeed))
            .print(
                &mut transformed,
                PrintRequest::SourceFile(source_id),
                &mut DisabledSourceMapRecorder,
            )
            .expect("classic JSX CommonJS print")
            .text()
            .to_owned()
    });

    assert!(printed.contains("require(\"./jsx\")"), "{printed}",);
    assert!(
        printed.contains("jsx_1.element") && printed.contains("jsx_1.fragment"),
        "{printed}",
    );
}

#[test]
fn null_jsx_fragment_factory_does_not_retain_the_ordinary_factory_import_or_its_pragmas() {
    let domain = IdentityDomain::reclaiming();
    let dependency = tsc_syntax::parse_source_file_from_snapshot_in_identity_domain(
        "/project/renderer.ts".to_owned(),
        TextSnapshot::new(
            "export function jsx() {}\n".to_owned(),
            DocumentVersion::new("1"),
        ),
        ParseOptions::default(),
        None,
        &domain,
    )
    .expect("JSX factory dependency identity allocation");
    let source = tsc_syntax::parse_source_file_from_snapshot_in_identity_domain(
        "/project/index.tsx".to_owned(),
        TextSnapshot::new(
            concat!(
                "/* @jsx jsx */\n",
                "/* @jsxfrag null */\n",
                "import { jsx } from \"./renderer\";\n",
                "<></>;\n",
            )
            .to_owned(),
            DocumentVersion::new("1"),
        ),
        ParseOptions {
            language_variant: LanguageVariant::Jsx,
            ..ParseOptions::default()
        },
        None,
        &domain,
    )
    .expect("null JSX fragment source identity allocation");
    let dependency = Arc::new(dependency);
    let source = Arc::new(source);
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::COMMON_JS.bits()),
        jsx: Some(2),
        no_unused_locals: Some(true),
        ..CompilerOptions::default()
    };
    let dependency_worker = BinderWorker::bind_in_identity_domain(&dependency, &options, &domain)
        .expect("JSX dependency bind identity allocation");
    let worker = BinderWorker::bind_in_identity_domain(&source, &options, &domain)
        .expect("null JSX fragment bind identity allocation");
    let dependency_document = Arc::new(BoundDocument::new(
        Arc::new(ParsedDocument::new(Arc::clone(&dependency))),
        dependency_worker.into_bind_data(),
    ));
    let document = Arc::new(BoundDocument::new(
        Arc::new(ParsedDocument::new(Arc::clone(&source))),
        worker.into_bind_data(),
    ));
    let snapshot =
        ProgramSnapshot::new(vec![dependency_document, document], 0).expect("Program snapshot");
    let import_specifier = source
        .arena
        .node_ids()
        .find(|id| source.arena.node(*id).kind == SyntaxKind::ImportSpecifier)
        .expect("ordinary JSX factory import specifier");

    let mut state = CheckerState::from_snapshot(&snapshot, &options);
    state.check_source_file(0);
    state.check_source_file(1);
    let session = CheckerSession::from_checked_state(state);
    let printed = session.with_emit_resolver(|resolver| {
        assert!(
            !resolver
                .is_referenced_alias_declaration(EmitResolverNode::from_raw_source(
                    1,
                    import_specifier,
                ))
                .expect("null-fragment ordinary JSX factory reachability"),
            "the semantic factory lookup must not become runtime import reachability",
        );

        let program_source = SourceFileId::from_raw(1);
        let host = CheckerEmitHost {
            options: &options,
            syntax: &source,
            source_ids: [program_source],
        };
        let mut arena = TransformArena::new();
        let source_id = arena.add_source(&source, Some(program_source));
        let mut transformed = transform_nodes(
            arena,
            vec![TransformRoot::SourceFile(source_id)],
            get_script_transformers_for_source(&options, resolver, &host, program_source)
                .expect("classic JSX transformers"),
            false,
        )
        .expect("null JSX fragment transform");
        create_printer(PrinterOptions::new(NewLineKind::LineFeed))
            .print(
                &mut transformed,
                PrintRequest::SourceFile(source_id),
                &mut DisabledSourceMapRecorder,
            )
            .expect("print null JSX fragment")
            .text()
            .to_owned()
    });

    assert!(
        printed.contains("(0, renderer_1.jsx)(null, null);"),
        "{printed}",
    );
    assert!(!printed.contains("require(\"./renderer\")"), "{printed}");
    assert!(!printed.contains("@jsx"), "{printed}");
}

#[test]
fn emit_import_resolution_filters_only_value_bearing_type_only_origins() {
    let cases = [
        (
            "named type provenance merged with a value",
            vec![
                ("/project/a.ts", "interface A {}\nexport type { A };\n"),
                (
                    "/project/b.ts",
                    "import { A } from \"./a\";\nconst A = 0;\nexport { A };\n",
                ),
                ("/project/c.ts", "import { A } from \"./b\";\nA;\n"),
            ],
            true,
        ),
        (
            "type-star provenance merged with a value",
            vec![
                ("/project/a.ts", "export type A = number;\n"),
                ("/project/b.ts", "export type * from \"./a\";\n"),
                (
                    "/project/c.ts",
                    "import { A } from \"./b\";\nconst A = 1;\nexport { A };\n",
                ),
                ("/project/d.ts", "import { A } from \"./c\";\nA;\n"),
            ],
            true,
        ),
        (
            "a runtime value explicitly exported as type-only",
            vec![
                ("/project/a.ts", "export class A {}\n"),
                ("/project/b.ts", "export type { A } from \"./a\";\n"),
                ("/project/c.ts", "import { A } from \"./b\";\nA;\n"),
            ],
            false,
        ),
    ];

    for (label, inputs, retains_runtime_import) in cases {
        let domain = IdentityDomain::reclaiming();
        let options = CompilerOptions {
            target: Some(ScriptTarget::ES2015.bits()),
            module: Some(ModuleKind::COMMON_JS.bits()),
            ..CompilerOptions::default()
        };
        let sources = inputs
            .iter()
            .map(|(path, text)| {
                Arc::new(
                    tsc_syntax::parse_source_file_from_snapshot_in_identity_domain(
                        (*path).to_owned(),
                        TextSnapshot::new((*text).to_owned(), DocumentVersion::new("1")),
                        ParseOptions::default(),
                        None,
                        &domain,
                    )
                    .expect("emit alias source identity allocation"),
                )
            })
            .collect::<Vec<_>>();
        let documents = sources
            .iter()
            .map(|source| {
                let worker = BinderWorker::bind_in_identity_domain(source, &options, &domain)
                    .expect("emit alias bind identity allocation");
                Arc::new(BoundDocument::new(
                    Arc::new(ParsedDocument::new(Arc::clone(source))),
                    worker.into_bind_data(),
                ))
            })
            .collect();
        let snapshot = ProgramSnapshot::new(documents, 0).expect("Program snapshot");
        let final_index = sources.len() - 1;
        let final_source = &sources[final_index];
        let import_specifier = final_source
            .arena
            .node_ids()
            .find(|id| final_source.arena.node(*id).kind == SyntaxKind::ImportSpecifier)
            .expect("downstream import specifier");
        let reference = final_source
            .arena
            .node_ids()
            .find(|id| {
                let node = final_source.arena.node(*id);
                node.kind == SyntaxKind::Identifier
                    && node.parent.is_some_and(|parent| {
                        final_source.arena.node(parent).kind == SyntaxKind::ExpressionStatement
                    })
            })
            .expect("downstream value reference");

        let mut state = CheckerState::from_snapshot(&snapshot, &options);
        for index in 0..sources.len() {
            state.check_source_file(index);
        }
        let session = CheckerSession::from_checked_state(state);
        let source_id = u32::try_from(final_index).expect("test source index fits SourceFileId");
        let reference = EmitResolverNode::from_raw_source(source_id, reference);
        let expected = retains_runtime_import
            .then(|| EmitResolverNode::from_raw_source(source_id, import_specifier));
        session.with_emit_resolver(|resolver| {
            assert_eq!(
                resolver
                    .get_referenced_import_declaration(reference)
                    .expect("parsed import reference resolution"),
                expected,
                "{label}",
            );
            assert_eq!(
                resolver
                    .get_jsx_factory_import_declaration(reference, "A")
                    .expect("classic JSX factory import resolution"),
                expected,
                "{label}",
            );
        });
    }
}

#[test]
fn emit_resolver_reanchors_decorator_metadata_imports_to_the_class_scope() {
    let domain = IdentityDomain::reclaiming();
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::COMMON_JS.bits()),
        experimental_decorators: true,
        emit_decorator_metadata: Some(true),
        ..CompilerOptions::default()
    };
    let inputs = [
        ("/service.ts", "export default class Service {}\n"),
        (
            "/component.ts",
            concat!(
                "import Service from './service';\n",
                "declare const decorator: any;\n",
                "@decorator class Component {\n",
                "    constructor(Service: Service) {}\n",
                "}\n",
            ),
        ),
    ];
    let sources = inputs
        .into_iter()
        .map(|(path, text)| {
            Arc::new(
                tsc_syntax::parse_source_file_from_snapshot_in_identity_domain(
                    path.to_owned(),
                    TextSnapshot::new(text.to_owned(), DocumentVersion::new("1")),
                    ParseOptions::default(),
                    None,
                    &domain,
                )
                .expect("decorator metadata source identity allocation"),
            )
        })
        .collect::<Vec<_>>();
    let documents = sources
        .iter()
        .map(|source| {
            let worker = BinderWorker::bind_in_identity_domain(source, &options, &domain)
                .expect("decorator metadata bind identity allocation");
            Arc::new(BoundDocument::new(
                Arc::new(ParsedDocument::new(Arc::clone(source))),
                worker.into_bind_data(),
            ))
        })
        .collect();
    let snapshot = ProgramSnapshot::new(documents, 0).expect("Program snapshot");
    let component = &sources[1];
    let import_clause = component
        .arena
        .node_ids()
        .find(|node| component.arena.node(*node).kind == SyntaxKind::ImportClause)
        .expect("default import clause");
    let class_scope = component
        .arena
        .node_ids()
        .find(|node| component.arena.node(*node).kind == SyntaxKind::ClassDeclaration)
        .expect("decorated class scope");
    let type_name = component
        .arena
        .node_ids()
        .find(|node| {
            let record = component.arena.node(*node);
            record.kind == SyntaxKind::Identifier
                && record.parent.is_some_and(|parent| {
                    component.arena.node(parent).kind == SyntaxKind::TypeReference
                })
        })
        .expect("constructor parameter type name");

    let mut state = CheckerState::from_snapshot(&snapshot, &options);
    state.check_source_file(0);
    state.check_source_file(1);
    let session = CheckerSession::from_checked_state(state);
    let type_name = EmitResolverNode::from_raw_source(1, type_name);
    let class_scope = EmitResolverNode::from_raw_source(1, class_scope);
    session.with_emit_resolver(|resolver| {
        assert_eq!(
            resolver
                .get_referenced_import_declaration_at_location(type_name, class_scope)
                .expect("class-scoped metadata import resolution"),
            Some(EmitResolverNode::from_raw_source(1, import_clause)),
        );
    });
}
