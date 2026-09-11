use super::{replacement_package_name, LibraryCatalog};
use tsc_types::CompilerOptions;

#[test]
fn physical_library_priority_accepts_optional_filename_affixes() {
    let catalog = LibraryCatalog::typescript_6_0_3("/lib");
    let directory = crate::ProgramPath::from_trusted_parts("/lib", "/lib").unwrap();
    // Direct replay of getDefaultLibFilePriority returns 1 for both paths:
    // removePrefix/removeSuffix leave a nonmatching spelling unchanged.
    for name in ["/lib/es5.d.ts", "/lib/lib.es5"] {
        let source = crate::ProgramPath::from_trusted_parts(name, name).unwrap();
        assert_eq!(
            catalog.source_file_priority(&source, &directory),
            1,
            "{name}"
        );
    }
}

#[test]
fn resolved_source_priorities_match_typescript_path_boundaries() {
    let artifact: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../fixtures/h2-8b-library-priority.json"))
            .expect("frozen upstream library priorities");
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    let cases = artifact["cases"].as_array().expect("priority cases");
    assert_eq!(cases.len(), 15);
    let path = |text: &str| {
        crate::ProgramPath::from_trusted_parts(text, tsc_host::to_file_name_lower_case(text))
            .expect("normalized witness path")
    };
    for case in cases {
        let directory = path(case["directory"].as_str().unwrap());
        let source = path(case["file"].as_str().unwrap());
        let catalog = LibraryCatalog::typescript_6_0_3(directory.display());
        for _ in 0..2 {
            assert_eq!(
                catalog.source_file_priority(&source, &directory),
                case["priority"].as_u64().unwrap() as usize,
                "{}",
                case["case_id"],
            );
        }
    }
}

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
