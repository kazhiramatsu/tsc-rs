use tsc_emitter::{
    create_printer, transform_ecmascript_module, transform_nodes, transform_type_script,
    EmitConstantValue, EmitExportContainerMode, EmitResolver, EmitResolverError, EmitResolverNode,
    NewLineKind, PrintRequest, PrinterOptions, SourceFileId, TransformArena, TransformRoot,
};
use tsc_syntax::parse_source_file;
use tsc_types::{CompilerOptions, ScriptTarget};

struct RecoveryResolver;
impl EmitResolver for RecoveryResolver {
    fn get_constant_value(
        &self,
        _: EmitResolverNode,
    ) -> Result<Option<EmitConstantValue>, EmitResolverError> {
        Ok(None)
    }
    fn get_referenced_export_container(
        &self,
        _: EmitResolverNode,
        _: EmitExportContainerMode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }
    fn get_referenced_import_declaration(
        &self,
        _: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Ok(None)
    }
    fn has_node_check_flag(&self, _: EmitResolverNode, _: u32) -> Result<bool, EmitResolverError> {
        Ok(false)
    }
}

#[test]
fn recovered_clause_comments_match_pinned_tsgo() {
    // These expected files are the pinned tsgo binary's output, byte for byte.
    macro_rules! case {
        ($stem:literal) => {{
            let parsed = parse_source_file(
                concat!($stem, ".ts"),
                include_str!(concat!(
                    "fixtures/tsgo-recovery-trailing-comments/",
                    $stem,
                    ".ts"
                )),
                Default::default(),
                None,
            );
            let options = CompilerOptions {
                target: Some(11),
                module: Some(99),
                strict: Some(true),
                always_strict: Some(true),
                ..CompilerOptions::default()
            };
            let mut arena = TransformArena::new();
            let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
            let mut transformed = transform_nodes(
                arena,
                vec![TransformRoot::SourceFile(source)],
                vec![
                    transform_type_script(&options, &RecoveryResolver),
                    transform_ecmascript_module(&options),
                ],
                false,
            )
            .expect("transform");
            let printed = create_printer(
                PrinterOptions::new(NewLineKind::LineFeed).with_target(ScriptTarget::ES2024),
            )
            .print(&mut transformed, PrintRequest::SourceFile(source), None)
            .expect("print");
            assert_eq!(
                printed.text(),
                include_str!(concat!(
                    "fixtures/tsgo-recovery-trailing-comments/",
                    $stem,
                    ".js"
                )),
                $stem
            );
        }};
    }
    case!("a-recovery-comment-min");
    case!("h-recovery-do");
    case!("d-recovery-catch");
    case!("e-recovery-finally");
    case!("f-valid-try");
    case!("g-valid-own-line");
}
