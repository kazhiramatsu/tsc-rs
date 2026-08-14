use super::{
    comment_cursor::{CommentCursor, CommentResume, CommentResumeError},
    create_printer,
    token_cursor::{cursor_work, reset_cursor_work, CursorWork},
    transform_nodes, DisabledSourceMapRecorder, NewLineKind, PrintRequest, PrinterOptions,
    SourceBytePosition, SourceFileTextMode, TransformArena, TransformRoot,
};
use tsc_syntax::parse_source_file;

fn canonical_cursor_work(statement_count: usize) -> CursorWork {
    let source_text = "if (items[0]) items[0];\n".repeat(statement_count);
    let parsed = parse_source_file("linear.ts", &source_text, Default::default(), None);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        Vec::new(),
        false,
    )
    .expect("identity transform");

    reset_cursor_work();
    let printed = create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    )
    .print(
        &mut result,
        PrintRequest::SourceFile(source),
        &mut DisabledSourceMapRecorder,
    )
    .expect("canonical cursor print");
    assert_eq!(
        printed.text().matches("if (items[0])").count(),
        statement_count
    );
    cursor_work()
}

#[test]
fn position_cursor_2727_statement_work_is_linear_and_scan_free() {
    const SMALL: usize = 909;
    const LARGE: usize = 2_727;

    let small = canonical_cursor_work(SMALL);
    let large = canonical_cursor_work(LARGE);

    assert_eq!(large.emissions, small.emissions * 3);
    // The first statement has no preceding newline. Apart from that constant
    // boundary term, local trivia plus fixed-token advance scales exactly
    // with the number of structurally identical statements.
    assert!(
        large.source_bytes <= small.source_bytes * 3 + 64,
        "cursor source work grew faster than linearly: {small:?} -> {large:?}",
    );
    assert!(
        large.source_bytes >= small.source_bytes * 3 - 64,
        "cursor source accounting unexpectedly changed shape: {small:?} -> {large:?}",
    );

    let printer_source = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/printer.rs"));
    assert!(
        !printer_source.contains("scan_tokens("),
        "the structured printer must advance typed positions, not search the source token stream",
    );
}

#[test]
fn comment_resume_can_only_merge_progress_for_the_same_owner_boundary() {
    let parsed = parse_source_file("resume.ts", "/* a */ value;\n", Default::default(), None);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    let position = |value| {
        CommentCursor::new(
            source,
            SourceBytePosition::new(value, parsed.positions()).expect("source position"),
        )
    };
    let near = CommentResume::new(position(0), position(3)).expect("near resume");
    let far = CommentResume::new(position(0), position(7)).expect("far resume");

    assert_eq!(near.furthest(far), Ok(far));
    assert!(matches!(
        near.furthest(CommentResume::new(position(1), position(7)).expect("other owner")),
        Err(CommentResumeError::OwnerMismatch { .. })
    ));
}
