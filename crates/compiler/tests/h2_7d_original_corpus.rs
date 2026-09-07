//! Original D283 command comparator; the hosted rung calls the same shared assertion.
use std::path::Path;

#[path = "integration/h2_7d_original_corpus_shared.rs"]
mod h2_7d_original_corpus_shared;

#[test]
fn h2_7d_original_corpus_matches_production_command_tuples() {
    assert_eq!(
        std::env::current_dir().unwrap().canonicalize().unwrap(),
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .canonicalize()
            .unwrap(),
        "run via cargo in the compiled worktree; never reuse a different worktree's target"
    );
    h2_7d_original_corpus_shared::assert_original_corpus(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
    );
}
