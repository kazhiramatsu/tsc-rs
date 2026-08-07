use std::path::PathBuf;

use super::{EmitOutcome, SourceMapObservation};

#[test]
fn outcome_retains_optional_presence_and_independent_emitted_file_order() {
    let source_map = SourceMapObservation::new(
        vec![PathBuf::from("/project/input.ts")],
        "{\"version\":3}".into(),
    );
    let absent = EmitOutcome::new(Vec::new(), true, None, None);
    let present = EmitOutcome::new(
        Vec::new(),
        false,
        Some(vec![
            PathBuf::from("/project/out.js"),
            PathBuf::from("/project/out.js.map"),
        ]),
        Some(vec![source_map]),
    );

    assert!(absent.emit_skipped());
    assert_eq!(absent.emitted_files(), None);
    assert_eq!(absent.source_maps(), None);
    assert_eq!(
        present.emitted_files(),
        Some(
            [
                PathBuf::from("/project/out.js"),
                PathBuf::from("/project/out.js.map"),
            ]
            .as_slice()
        )
    );
    let maps = present.source_maps().expect("present map observations");
    assert_eq!(
        maps[0].input_source_files(),
        [PathBuf::from("/project/input.ts")]
    );
    assert_eq!(maps[0].canonical_json(), "{\"version\":3}");
}
