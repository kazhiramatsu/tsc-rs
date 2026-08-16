use super::{replacement_package_name, LibraryCatalog};
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
    assert_eq!(catalog.file_name_priority("lib.es6.d.ts"), 0);
    assert!(
        catalog.file_name_priority("lib.es5.d.ts") < catalog.file_name_priority("lib.dom.d.ts")
    );
    assert_eq!(
        catalog.file_name_priority("outside-lib.es5.d.ts"),
        catalog.logical_entry_count() + 2
    );
    assert_eq!(catalog.spelling_suggestion("es2050"), Some("es2015"));
    assert_eq!(catalog.spelling_suggestion("not-a-library"), None);
}

#[test]
fn replacement_package_names_preserve_tsc_package_and_subpath_shape() {
    assert_eq!(
        replacement_package_name("lib.dom.d.ts"),
        "@typescript/lib-dom"
    );
    assert_eq!(
        replacement_package_name("lib.dom.iterable.d.ts"),
        "@typescript/lib-dom/iterable"
    );
    assert_eq!(
        replacement_package_name("lib.esnext.array.extra.d.ts"),
        "@typescript/lib-esnext/array-extra"
    );
}
