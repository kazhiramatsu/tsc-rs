use super::*;

#[test]
fn frozen_h1_qualification_executes_exactly() {
    let workspace = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    run(&workspace).expect("H1 compatible upstream acceptance");
}
