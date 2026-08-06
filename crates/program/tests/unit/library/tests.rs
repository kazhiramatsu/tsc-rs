use std::path::Path;

use super::LibraryCatalog;
use tsc_types::CompilerOptions;

#[test]
fn typescript_6_0_3_catalog_pins_aliases_counts_and_target_defaults() {
    let catalog = LibraryCatalog::typescript_6_0_3("/vendor/lib");
    assert_eq!(catalog.logical_entry_count(), 107);
    assert_eq!(catalog.distinct_file_count(), 95);
    assert_eq!(catalog.option_file_name("es6"), Some("lib.es2015.d.ts"));
    assert_eq!(
        catalog.option_file_name("esnext.object"),
        Some("lib.es2024.object.d.ts")
    );
    assert_eq!(catalog.option_file_name("DOM"), None);
    assert_eq!(catalog.option_file_name("lib.dom.d.ts"), None);
    assert_eq!(catalog.reference_file_name("lib.dom.d.ts"), None);
    assert_eq!(
        catalog.default_file_name(&CompilerOptions::default()),
        "lib.es2025.full.d.ts"
    );
    assert_eq!(
        catalog.default_file_name(&CompilerOptions {
            target: Some(2),
            ..CompilerOptions::default()
        }),
        "lib.es6.d.ts"
    );
}

#[test]
fn priorities_and_spelling_suggestions_match_the_pinned_order() {
    let catalog = LibraryCatalog::typescript_6_0_3("/vendor/lib");
    let directory = Path::new("/vendor/lib");
    assert_eq!(
        catalog.priority(directory, Path::new("/vendor/lib/lib.es6.d.ts")),
        0
    );
    assert!(
        catalog.priority(directory, Path::new("/vendor/lib/lib.es5.d.ts"))
            < catalog.priority(directory, Path::new("/vendor/lib/lib.dom.d.ts"))
    );
    assert_eq!(
        catalog.priority(directory, Path::new("/outside/lib.es5.d.ts")),
        catalog.logical_entry_count() + 2
    );
    assert_eq!(catalog.spelling_suggestion("es2050"), Some("es2015"));
    assert_eq!(catalog.spelling_suggestion("not-a-library"), None);
}
