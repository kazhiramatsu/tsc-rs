use tsc_emitter::{
    create_printer, get_script_transformers, transform_nodes, NewLineKind, PrintRequest,
    PrinterOptions, SourceFileTextMode, TransformArena, TransformRoot, UnavailableEmitResolver,
};
use tsc_program::SourceFileId;
use tsc_syntax::{parse_source_file, LanguageVariant, ParseOptions};
use tsc_types::{CompilerOptions, ModuleKind, ScriptTarget};

#[test]
fn preserved_jsx_erases_type_arguments_without_consuming_the_attribute_boundary() {
    let parsed = parse_source_file(
        "type-arguments.tsx",
        concat!(
            "function SFC<T>(props: Record<string, T>) { return ''; }\n",
            "<SFC<string> prop={1}></SFC>;\n",
            "<SFC<number> prop={2} />;\n",
        ),
        ParseOptions {
            script_target: ScriptTarget::ES_NEXT,
            language_variant: LanguageVariant::Jsx,
            ..ParseOptions::default()
        },
        None,
    );
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let options = CompilerOptions {
        target: Some(ScriptTarget::ES2015.bits()),
        module: Some(ModuleKind::PRESERVE.bits()),
        jsx: Some(1),
        always_strict: Some(false),
        ..CompilerOptions::default()
    };
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        get_script_transformers(&options, &UnavailableEmitResolver)
            .expect("create preserve-JSX transformers"),
        false,
    )
    .expect("erase TypeScript syntax from preserved JSX");
    let output = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_target(ScriptTarget::ES2015)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    )
    .print(&mut result, PrintRequest::SourceFile(source), None)
    .expect("print preserved JSX")
    .text()
    .to_owned();

    // tsc-port: transformTypeScript visitJsxSelfClosingElement and
    // visitJsxOpeningElement @6.0.3 (_tsc.js:95159-95176). The type argument
    // list is erased as a typed field; the JSX printer then owns the single
    // boundary between the tag name and its attributes.
    assert_eq!(output.matches("<SFC prop=").count(), 2, "{output}");
    assert!(!output.contains("<SFC<"), "{output}");
    assert!(!output.contains("<SFCstring"), "{output}");
    assert!(!output.contains("<SFCnumber"), "{output}");
}
