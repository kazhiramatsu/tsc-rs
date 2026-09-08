//! Unchanged original output-directory intersections, freshly reobserved on TS6.
use std::path::Path;

#[allow(dead_code)] // The shared source also contains the separate D283 entry.
#[path = "integration/h2_7d_original_corpus_shared.rs"]
mod h2_7d_original_corpus_shared;

#[test]
fn original_output_directory_corpus_matches_production_commands() {
    let ids = h2_7d_original_corpus_shared::assert_output_directory_references(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
    );
    assert_eq!(ids.len(), 23);
}

#[test]
fn original_output_matrix_candidates_match_complete_production_commands() {
    let ids = h2_7d_original_corpus_shared::assert_output_matrix_candidates(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
    );
    assert_eq!(ids.len(), 769);
}

#[test]
fn original_export_assignment_annotations_match_complete_commands() {
    let ids = (1..=8)
        .map(|index| {
            format!(
                "typescript-6.0.3/compiler/checkJsdocTypeTagOnExportAssignment{index}.ts#default"
            )
        })
        .collect::<Vec<_>>();
    let names = ids.iter().map(String::as_str).collect::<Vec<_>>();
    let exact = h2_7d_original_corpus_shared::assert_output_matrix_projection(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
        &names,
    );
    assert_eq!(exact.len(), 8);
}
