use super::*;
use tsc_program::ProgramPath;

#[test]
fn prepared_to_checker_projection_preserves_the_exact_snapshot_arc() {
    let path = ProgramPath::from_trusted_parts("main.ts", "main.ts").expect("test path");
    let source = PreparedSourceFile::new(path, "export const value = 1;");
    let (input, _) = project_source(&source, SourceFileId::from_raw(0)).expect("projection");

    assert!(Arc::ptr_eq(source.snapshot(), input.snapshot()));
    assert_eq!(input.text(), source.text());
    assert_eq!(
        input.snapshot().positions().kind(),
        tsc_diagnostics::PositionIndexKind::StaticDense
    );
}
