use std::path::{Path, PathBuf};

use tsc_program::SourceFileId;
use tsc_syntax::{parse_source_file, FileReference, NodeData, ParseOptions, SourceFile};
use tsc_types::CompilerOptions;

use crate::{
    transform_nodes, DeclarationCustomTransformers, DeclarationPathResolver, EmitHost, EmitSource,
    GeneratedIdentifierFlags, NodeFactory, TransformArena, TransformNode, TransformNodeArray,
    TransformRoot, UnavailableEmitResolver,
};

struct TestHost {
    options: CompilerOptions,
    paths: Vec<PathBuf>,
    sources: Vec<SourceFile>,
    ids: Vec<SourceFileId>,
}

impl EmitHost for TestHost {
    fn compiler_options(&self) -> &CompilerOptions {
        &self.options
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
        &self.ids
    }

    fn source_file(&self, id: SourceFileId) -> Option<EmitSource<'_>> {
        let index = id.raw() as usize;
        Some(EmitSource::new(
            id,
            self.paths.get(index)?,
            self.paths.get(index)?,
            true,
            None,
            self.sources.get(index),
        ))
    }
}

struct TestPaths;

impl DeclarationPathResolver for TestPaths {
    fn declaration_file_path(&self, source: SourceFileId) -> Option<PathBuf> {
        Some(PathBuf::from(format!("/project/out/{}.d.ts", source.raw())))
    }

    fn reference_target_path(&self, source: SourceFileId) -> Option<PathBuf> {
        Some(PathBuf::from(format!("/project/{}.d.ts", source.raw())))
    }
}

#[test]
fn update_faces_preserve_identity_and_attach_original_on_change() {
    let parsed = parse_source_file("main.ts", "let value = 1;", ParseOptions::default(), None);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let root = arena.root(source).expect("source root");
    let statement = match &arena.node(root).expect("root node").data {
        NodeData::SourceFile(data) => {
            let array = data.statements.expect("statements");
            TransformNode::new(
                source,
                arena
                    .node_array(TransformNodeArray::new(source, array))
                    .expect("array")
                    .nodes[0],
            )
        }
        _ => panic!("expected source file"),
    };
    let declaration_list = match &arena.node(statement).expect("statement").data {
        NodeData::VariableStatement(data) => data.declaration_list.expect("declaration list"),
        _ => panic!("expected variable statement"),
    };
    let declaration_list = TransformNode::new(source, declaration_list);
    let declaration = match &arena.node(declaration_list).expect("declaration list").data {
        NodeData::VariableDeclarationList(data) => {
            let declarations = data.declarations.expect("declarations");
            TransformNode::new(
                source,
                arena
                    .node_array(TransformNodeArray::new(source, declarations))
                    .expect("declarations array")
                    .nodes[0],
            )
        }
        _ => panic!("expected variable declaration list"),
    };
    let declaration_name = match &arena.node(declaration).expect("declaration").data {
        NodeData::VariableDeclaration(data) => {
            TransformNode::new(source, data.name.expect("declaration name"))
        }
        _ => panic!("expected variable declaration"),
    };
    let mut factory = NodeFactory::new(&mut arena);
    let unchanged = factory
        .update_variable_statement(statement, None, declaration_list)
        .expect("identity update");
    assert_eq!(unchanged, statement);

    let declare = factory
        .create_modifier(source, tsc_syntax::SyntaxKind::DeclareKeyword)
        .expect("declare modifier");
    let modifiers = factory
        .create_node_array(source, vec![declare])
        .expect("modifier array");
    let changed = factory
        .update_variable_statement(statement, Some(modifiers), declaration_list)
        .expect("changed update");
    assert_ne!(changed, statement);
    assert_eq!(factory.arena().get_original_node(changed), statement);

    let any = factory
        .create_keyword_type_node(source, tsc_syntax::SyntaxKind::AnyKeyword)
        .expect("any type");
    let changed_declaration = factory
        .update_variable_declaration(declaration, declaration_name, None, Some(any), None)
        .expect("changed declaration update");
    assert_ne!(
        factory.arena().transform_flags(changed_declaration),
        factory.arena().transform_flags(declaration)
    );
    assert_eq!(
        factory.arena().get_original_node(changed_declaration),
        declaration
    );

    let generated = factory
        .get_generated_name_for_node(
            statement,
            GeneratedIdentifierFlags::OPTIMISTIC,
            Some("__"),
            None,
        )
        .expect("generated name");
    assert_eq!(factory.arena().get_original_node(generated), statement);
    assert!(factory
        .arena()
        .metadata(generated)
        .and_then(|metadata| metadata.generated_binding_preferred_base())
        .is_some_and(|base| base.starts_with("__generated@")));
}

#[test]
fn preserved_reference_is_synthesized_with_relative_target_and_sentinel_range() {
    let mut main = parse_source_file("main.ts", "", ParseOptions::default(), None);
    main.referenced_files.push(FileReference {
        file_name: "dep.ts".into(),
        pos: 4,
        end: 11,
        preserve: true,
    });
    let dep = parse_source_file(
        "dep.ts",
        "export const dep = 1;",
        ParseOptions::default(),
        None,
    );
    let host = TestHost {
        options: CompilerOptions::default(),
        paths: vec!["/project/main.ts".into(), "/project/dep.ts".into()],
        sources: vec![main.clone(), dep],
        ids: vec![SourceFileId::from_raw(0), SourceFileId::from_raw(1)],
    };
    let mut arena = TransformArena::new();
    let source = arena.add_source(&main, Some(SourceFileId::from_raw(0)));
    let paths = TestPaths;
    let transformers = crate::declarations::get_declaration_transformers(
        host.compiler_options(),
        &UnavailableEmitResolver,
        &host,
        &paths,
        &DeclarationCustomTransformers::none(),
    )
    .expect("declaration transformer selection");
    let result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        transformers,
        false,
    )
    .expect("declaration transform");
    let root = match result.roots().first().expect("transformed root") {
        TransformRoot::SourceFile(root) => *root,
        TransformRoot::Bundle(_) => panic!("bundle root"),
    };
    let references = &result
        .arena()
        .source(root)
        .expect("transformed source")
        .syntax()
        .referenced_files;
    assert_eq!(references.len(), 1);
    assert_eq!(references[0].file_name, "../1.d.ts");
    assert_eq!(references[0].pos, u32::MAX);
    assert_eq!(references[0].end, u32::MAX);
}

#[test]
fn custom_declaration_transformers_are_a_typed_refusal() {
    let host = TestHost {
        options: CompilerOptions::default(),
        paths: Vec::new(),
        sources: Vec::new(),
        ids: Vec::new(),
    };
    let custom = DeclarationCustomTransformers {
        after_declarations: vec![()],
    };
    let result = crate::declarations::get_declaration_transformers(
        host.compiler_options(),
        &UnavailableEmitResolver,
        &host,
        &TestPaths,
        &custom,
    );
    assert!(matches!(
        result,
        Err(crate::TransformError::Unsupported(
            crate::UnsupportedEmitFeature::CustomTransformers
        ))
    ));
}
