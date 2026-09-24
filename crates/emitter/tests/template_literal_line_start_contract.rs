//! A template literal whose text ends with a line break closes at column 0:
//! tsc writes the whole token text, delimiters included, in one
//! `writeLiteral` call, so the pending indentation of the new line is never
//! applied to the closing backtick (hono's `html\`...\n\`` redirect page).
use tsc_emitter::{
    create_printer, transform_nodes, NewLineKind, PrintRequest, PrinterOptions, SourceFileTextMode,
    TransformArena, TransformRoot,
};
use tsc_syntax::parse_source_file;

fn print(source_text: &str) -> String {
    let parsed = parse_source_file("main.js", source_text, Default::default(), None);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![],
        false,
    )
    .unwrap();
    create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    )
    .print(&mut result, PrintRequest::SourceFile(source), None)
    .unwrap()
    .text()
    .to_owned()
}

#[test]
fn tagged_template_tail_ending_with_a_line_break_closes_at_column_zero() {
    let output = print(
        "const generate = (location) => {\n\
         \x20   const content = html`<!DOCTYPE html>\n\
         <title>${location}</title>\n\
         <body>${location}</body>\n\
         `;\n\
         \x20   return content;\n\
         };\n",
    );
    assert!(
        output.contains("<body>${location}</body>\n`;\n"),
        "closing delimiter is indented:\n{output}"
    );
    assert!(!output.contains("\n    `;"), "{output}");
    assert!(output.contains("return content;"), "{output}");
}

#[test]
fn no_substitution_template_ending_with_a_line_break_closes_at_column_zero() {
    let output = print(
        "function f() {\n\
         \x20   const text = `line\n\
         `;\n\
         \x20   return text;\n\
         }\n",
    );
    assert!(output.contains("`line\n`;\n"), "{output}");
    assert!(!output.contains("\n    `;"), "{output}");
}
