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
